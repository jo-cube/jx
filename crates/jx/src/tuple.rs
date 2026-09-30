mod reduce;
mod stages;
use crate::{
    Error, Value,
    evaluate::Operand,
    expression::{Kind, Node, Step},
    sequence::{Context, Halt, Output, View, Walk},
};
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
    match &node.kind {
        Kind::Tuples(..) => true,
        Kind::Sort(base, _) | Kind::Keep(base, _) | Kind::Filter(base, _) => active(base),
        Kind::Group(base) => crate::provenance::captured(base),
        Kind::Block(items) => items.last().is_some_and(crate::provenance::captured),
        _ => false,
    }
}

// The reference loses its tuple marker after sorting an existing tuple stream.
// A following map restores it; direct grouping would expose internal tuple objects.
pub(crate) fn ends_sorted(node: &Node) -> bool {
    match &node.kind {
        Kind::Sort(base, _) => active(base),
        Kind::Tuples(steps, _) => steps.last().is_some_and(|s| ends_sorted(&s.node)),
        Kind::Keep(base, _) | Kind::Filter(base, _) => ends_sorted(base),
        _ => false,
    }
}

// A stage finishes its input even after its own evaluation fails, preserving
// earlier-stage error precedence. Consumer cancellation always stops immediately.
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

fn walk<'e, 'i>(node: &'e Node, context: &Context<'e, 'i>, output: &mut Emit<'_, 'e, 'i>) -> Walk {
    match &node.kind {
        Kind::Tuples(steps, focus) => route(steps, *focus, context, output),
        Kind::Keep(base, _) => walk(base, context, output),
        Kind::Group(base) => block(std::slice::from_ref(base), context, output),
        Kind::Block(items) => block(items, context, output),
        Kind::Filter(base, predicates) => {
            let mut input = |emit: &mut Emit<'_, 'e, 'i>| walk(base, context, emit);
            stages::postfilters(&mut input, predicates, context, output)
        }
        Kind::Sort(base, terms) => ordered(base, terms, context, node.offset, output),
        _ => unreachable!("tuple path boundary"),
    }
}
fn block<'e, 'i>(
    nodes: &'e [Node],
    context: &Context<'e, 'i>,
    output: &mut Emit<'_, 'e, 'i>,
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
            crate::retain::materialize(node, &local)?;
        }
        walk(last, &local, &mut |mut row| {
            if row.bindings.iter().any(|(name, _)| !name.starts_with('!')) {
                Rc::make_mut(&mut row.bindings).retain(|(name, _)| name.starts_with('!'));
            }
            output(row)
        })
    })();
    child.release();
    result
}

fn source<'e, 'i>(
    node: &'e Node,
    context: &Context<'e, 'i>,
    output: &mut Emit<'_, 'e, 'i>,
) -> Walk {
    if active(node) {
        return walk(node, context, output);
    }
    let bindings = Rc::new(Vec::new());
    View::Operand(&node.run(context)?).candidates(false, &mut |value| {
        output(Row {
            value,
            bindings: bindings.clone(),
        })
    })
}

fn route<'e, 'i>(
    steps: &'e [Step],
    focus: bool,
    context: &Context<'e, 'i>,
    output: &mut Emit<'_, 'e, 'i>,
) -> Walk {
    let start = steps
        .iter()
        .position(|s| s.bindings.is_some() || active(&s.node))
        .unwrap();
    let start = if focus && start == 0 { 1 } else { start };
    let bindings = Rc::new(Vec::new());
    let mut seed = |emit: &mut Emit<'_, 'e, 'i>| {
        if focus && start == 1 {
            crate::filter::with_filters(
                &steps[0].node,
                &steps[0].predicates,
                context,
                &mut |view| {
                    view.candidates(false, &mut |value| {
                        emit(Row {
                            value,
                            bindings: bindings.clone(),
                        })
                    })
                },
            )
        } else if start > 0 {
            let mut pending = None;
            let mut multiple = false;
            crate::route::walk(&steps[..start], focus, false, context, &mut |view| {
                view.walk(&mut |value| {
                    if multiple {
                        return emit(Row {
                            value,
                            bindings: bindings.clone(),
                        });
                    }
                    if let Some(first) = pending.take() {
                        multiple = true;
                        emit(Row {
                            value: first,
                            bindings: bindings.clone(),
                        })?;
                        emit(Row {
                            value,
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
                        bindings: bindings.clone(),
                    })
                })?;
            }
            Ok(())
        } else {
            let absolute = matches!(&steps[0].node.kind, Kind::Variable(_) | Kind::Sort(..))
                || matches!(&steps[0].node.kind, Kind::Path(p) if p.rooted && p.fields.is_empty());
            if context.wrapped || absolute || focus {
                emit(Row {
                    value: context.value.clone(),
                    bindings: bindings.clone(),
                })
            } else {
                View::Operand(&Operand::One(context.value.clone())).candidates(
                    false,
                    &mut |value| {
                        emit(Row {
                            value,
                            bindings: bindings.clone(),
                        })
                    },
                )
            }
        }
    };
    stages::pipeline(&mut seed, &steps[start..], context, output)
}

pub(crate) fn group<'e, 'i>(
    base: &'e Node,
    pairs: &'e [(Node, Node)],
    context: &Context<'e, 'i>,
    offset: usize,
) -> Result<Value<'e, 'i>, Error> {
    reduce::evaluate(base, pairs, context, offset)
}

pub(crate) fn route_values<'e, 'i>(
    steps: &'e [Step],
    focus: bool,
    context: &Context<'e, 'i>,
    output: &mut Output<'_, 'e, 'i>,
) -> Walk {
    route(steps, focus, context, &mut |row| output(row.value))
}
fn ordered<'e, 'i>(
    base: &'e Node,
    terms: &'e [(Node, bool)],
    context: &Context<'e, 'i>,
    offset: usize,
    output: &mut Emit<'_, 'e, 'i>,
) -> Walk {
    let mut rows = Vec::new();
    source(base, context, &mut |row| {
        rows.push(row);
        Ok(())
    })?;
    let indices = crate::ordering::indices(rows.len(), |a, b| {
        rows[a].scoped(context, |left| {
            rows[b].scoped(context, |right| {
                crate::ordering::contexts(left, right, terms, offset)
            })
        })
    })?;
    for index in indices {
        output(rows[index].clone())?;
    }
    Ok(())
}
pub(crate) fn sorted<'e, 'i>(
    base: &'e Node,
    terms: &'e [(Node, bool)],
    context: &Context<'e, 'i>,
    offset: usize,
) -> Result<Operand<'e, 'i>, Error> {
    crate::retain::collect(|emit| {
        match ordered(base, terms, context, offset, &mut |row| {
            emit(row.value);
            Ok(())
        }) {
            Ok(()) => Ok(()),
            Err(Halt::Evaluation(error)) => Err(error),
            Err(Halt::Stop) => unreachable!("sorting is fully consumed"),
        }
    })
    .map(|value| value.map_or(Operand::Missing, Operand::One))
}

pub(crate) fn filtered_values<'e, 'i>(
    base: &'e Node,
    predicates: &'e [Node],
    context: &Context<'e, 'i>,
    output: &mut Output<'_, 'e, 'i>,
) -> Walk {
    let mut source = |emit: &mut Emit<'_, 'e, 'i>| walk(base, context, emit);
    stages::postfilters(&mut source, predicates, context, &mut |row| {
        output(row.value)
    })
}
