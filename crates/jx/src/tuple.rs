mod model;
mod reduce;
mod stages;
use crate::{
    Error, Value,
    evaluate::Operand,
    expression::{Kind, Node, Step},
    sequence::{Context, Halt, Output, View, Walk},
};
use model::materialize;
pub(crate) use model::{Evaluator, Expression, Form};
use std::rc::Rc;

#[derive(Clone, Debug, Default)]
pub(crate) struct Bindings {
    pub focus: Option<Box<str>>,
    pub ancestors: Vec<Box<str>>,
    pub index: Option<Box<str>>,
    // Position bindings after a filter apply to the combined stage sequence.
    pub indices: Vec<(usize, Box<str>)>,
}
impl Bindings {
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.focus
            .iter()
            .chain(&self.index)
            .map(AsRef::as_ref)
            .chain(self.indices.iter().map(|(_, name)| name.as_ref()))
    }
}

#[derive(Clone)]
struct Row<'e, 'i> {
    value: Value<'e, 'i>,
    object_context: bool,
    bindings: Rc<crate::runtime::BindingValues<'e, 'i>>,
}
type Emit<'a, 'e, 'i> = dyn FnMut(Row<'e, 'i>) -> Walk + 'a;
type Source<'a, 'e, 'i> = dyn FnMut(&mut Emit<'_, 'e, 'i>) -> Walk + 'a;
impl<'e, 'i> Row<'e, 'i> {
    fn bind(&mut self, name: &'e str, value: Value<'e, 'i>) {
        let bindings = Rc::make_mut(&mut self.bindings);
        if let Some((_, previous)) = bindings.iter_mut().find(|(key, _)| *key == name) {
            *previous = value;
        } else {
            bindings.push((name, value));
        }
    }
    fn object(&self) -> Value<'e, 'i> {
        let mut members = Vec::with_capacity(self.bindings.len() + 1);
        members.push((
            Value::StringLiteral(crate::RawJson("\"@\"")),
            self.value.clone(),
        ));
        members.extend(
            self.bindings
                .iter()
                .map(|(name, value)| (Value::from_string(*name), value.clone())),
        );
        Value::object(members)
    }
    fn stage<T>(&self, outer: &Context<'e, 'i>, run: impl FnOnce(&Context<'e, 'i>) -> T) -> T {
        if self.object_context {
            run(&Context {
                value: self.object(),
                wrapped: false,
                scope: outer.scope.clone(),
            })
        } else {
            self.scoped(outer, run)
        }
    }
    fn scoped<T>(&self, outer: &Context<'e, 'i>, run: impl FnOnce(&Context<'e, 'i>) -> T) -> T {
        let scope = outer
            .scope
            .as_ref()
            .expect("tuple path has lexical runtime");
        let child = scope.shared(self.bindings.clone());
        let result = run(&Context {
            value: self.value.clone(),
            wrapped: false,
            scope: Some(child.clone()),
        });
        child.release();
        result
    }
}

pub(crate) fn active(node: &Node) -> bool {
    correlated(node)
}
pub(crate) fn correlated<B: Expression>(node: &B) -> bool {
    !matches!(node.form(), Form::Value)
}
// A stage finishes its input even after its own evaluation fails, preserving
// earlier-stage error precedence. Consumer cancellation always stops immediately.

fn walk<'e, 'i, B: Expression + 'e>(
    node: &'e B,
    context: &Context<'e, 'i>,
    output: &mut Emit<'_, 'e, 'i>,
    evaluator: &impl Evaluator<'e, 'i, B>,
) -> Walk {
    match node.form() {
        Form::Route(steps, focus) => route(steps, focus, context, output, evaluator),
        Form::Keep(base) => walk(base, context, output, evaluator),
        Form::Scope(items) => block(items, context, output, evaluator),
        Form::Filter(base, predicates) => {
            let mut input = |emit: &mut Emit<'_, 'e, 'i>| walk(base, context, emit, evaluator);
            stages::postfilters(&mut input, predicates, context, output, evaluator)
        }
        Form::Order(base, terms) => ordered(base, terms, context, node.offset(), output, evaluator),
        Form::Value => unreachable!("tuple path boundary"),
    }
}
fn block<'e, 'i, B: Expression + 'e>(
    nodes: &'e [B],
    context: &Context<'e, 'i>,
    output: &mut Emit<'_, 'e, 'i>,
    evaluator: &impl Evaluator<'e, 'i, B>,
) -> Walk {
    let scope = context.scope.as_ref().expect("scoped path runtime");
    let child = scope.child(scope.frame);
    let local = Context {
        scope: Some(child.clone()),
        ..context.clone()
    };
    let result = (|| {
        let Some((last, prefix)) = nodes.split_last() else {
            return Ok(());
        };
        for node in prefix {
            materialize(node, &local, evaluator)?;
        }
        walk(
            last,
            &local,
            &mut |mut row| {
                if row.bindings.iter().any(|(name, _)| !name.starts_with('!')) {
                    Rc::make_mut(&mut row.bindings).retain(|(name, _)| name.starts_with('!'));
                }
                output(row)
            },
            evaluator,
        )
    })();
    child.release();
    result
}

fn source<'e, 'i, B: Expression + 'e>(
    node: &'e B,
    context: &Context<'e, 'i>,
    output: &mut Emit<'_, 'e, 'i>,
    evaluator: &impl Evaluator<'e, 'i, B>,
) -> Walk {
    if correlated(node) {
        return walk(node, context, output, evaluator);
    }
    let bindings = Rc::new(Vec::new());
    View::Operand(&evaluator.run(node, context)?).candidates(false, &mut |value| {
        output(Row {
            value,
            object_context: false,
            bindings: bindings.clone(),
        })
    })
}

fn route<'e, 'i, B: Expression + 'e>(
    steps: &'e [Step<B>],
    focus: bool,
    context: &Context<'e, 'i>,
    output: &mut Emit<'_, 'e, 'i>,
    evaluator: &impl Evaluator<'e, 'i, B>,
) -> Walk {
    let start = steps
        .iter()
        .position(|s| s.bindings.is_some() || correlated(&s.node))
        .unwrap();
    let start = if focus && start == 0 { 1 } else { start };
    let bindings = Rc::new(Vec::new());
    let mut seed = |emit: &mut Emit<'_, 'e, 'i>| {
        if focus && start == 1 {
            evaluator.filtered(&steps[0].node, &steps[0].predicates, context, &mut |view| {
                view.candidates(false, &mut |value| {
                    emit(Row {
                        value,
                        object_context: false,
                        bindings: bindings.clone(),
                    })
                })
            })
        } else if start > 0 {
            let mut pending = None;
            let mut multiple = false;
            evaluator.prefix(&steps[..start], focus, context, &mut |view| {
                view.walk(&mut |value| {
                    if multiple {
                        return emit(Row {
                            value,
                            object_context: false,
                            bindings: bindings.clone(),
                        });
                    }
                    if let Some(first) = pending.take() {
                        multiple = true;
                        emit(Row {
                            value: first,
                            object_context: false,
                            bindings: bindings.clone(),
                        })?;
                        emit(Row {
                            value,
                            object_context: false,
                            bindings: bindings.clone(),
                        })
                    } else {
                        pending = Some(value);
                        Ok(())
                    }
                })
            })?;
            if let Some(value) = pending {
                View::Operand(&Operand::One(value)).candidates(false, &mut |value| {
                    emit(Row {
                        value,
                        object_context: false,
                        bindings: bindings.clone(),
                    })
                })?;
            }
            Ok(())
        } else {
            let absolute = steps[0].node.absolute();
            if context.wrapped || absolute || focus {
                emit(Row {
                    value: context.value.clone(),
                    object_context: false,
                    bindings: bindings.clone(),
                })
            } else {
                View::Operand(&Operand::One(context.value.clone())).candidates(
                    false,
                    &mut |value| {
                        emit(Row {
                            value,
                            object_context: false,
                            bindings: bindings.clone(),
                        })
                    },
                )
            }
        }
    };
    stages::pipeline(&mut seed, &steps[start..], context, output, evaluator)
}

pub(crate) fn group_with<'e, 'i, B: Expression + 'e>(
    base: &'e B,
    pairs: &'e [(B, B)],
    context: &Context<'e, 'i>,
    offset: usize,
    evaluator: &impl Evaluator<'e, 'i, B>,
) -> Result<Value<'e, 'i>, Error> {
    reduce::evaluate(base, pairs, context, offset, evaluator)
}

pub(crate) fn values<'e, 'i, B: Expression + 'e>(
    node: &'e B,
    context: &Context<'e, 'i>,
    output: &mut Output<'_, 'e, 'i>,
    evaluator: &impl Evaluator<'e, 'i, B>,
) -> Walk {
    walk(node, context, &mut |row| output(row.value), evaluator)
}

fn ordered<'e, 'i, B: Expression + 'e>(
    base: &'e B,
    terms: &'e [(B, bool)],
    context: &Context<'e, 'i>,
    offset: usize,
    output: &mut Emit<'_, 'e, 'i>,
    evaluator: &impl Evaluator<'e, 'i, B>,
) -> Walk {
    let mut rows = Vec::new();
    source(
        base,
        context,
        &mut |row| {
            rows.push(row);
            Ok(())
        },
        evaluator,
    )?;
    let mut keys = crate::ordering::Keys::with_terms(
        rows.len(),
        terms.len(),
        terms.iter().any(|(t, _)| !t.effects()),
    );
    let indices = crate::ordering::indices(rows.len(), |a, b| {
        rows[a].stage(context, |left| {
            rows[b].stage(context, |right| {
                keys.compare_with(
                    a,
                    b,
                    terms.iter().map(|(term, desc)| (term.effects(), *desc)),
                    offset,
                    |index, term| {
                        materialize(
                            &terms[term].0,
                            if index == a { left } else { right },
                            evaluator,
                        )
                    },
                )
            })
        })
    })?;
    // Multi-item sorting drops the reference tuple marker; postfix stages see
    // objects, while a later map resumes tuple focus. Singleton sorting is inert.
    let expose = rows.len() > 1;
    for index in indices {
        let mut row = rows[index].clone();
        row.object_context |= expose;
        output(row)?;
    }
    Ok(())
}

fn transform<'e, 'i>(
    input: &mut Source<'_, 'e, 'i>,
    mut apply: impl FnMut(Row<'e, 'i>) -> Walk,
) -> Walk {
    let mut error = None;
    input(&mut |row| {
        if error.is_some() {
            return Ok(());
        }
        match apply(row) {
            Err(Halt::Evaluation(e)) => {
                error = Some(e);
                Ok(())
            }
            result => result,
        }
    })?;
    error.map_or(Ok(()), |e| Err(Halt::Evaluation(e)))
}
