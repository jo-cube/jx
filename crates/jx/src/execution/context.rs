//! Demand-driven correlated sequence regions. Only annotated paths use row
//! bindings. Ordinary activations and cells remain unchanged. The shared tuple
//! kernels own correlation; all bodies execute the common IR backend.
use super::{
    run::{Activation, Cell},
    *,
};
use crate::{
    evaluate::Operand,
    expression::Step,
    sequence::{Context, Halt, Output, View, Walk},
    tuple::{Evaluator, Expression, Form},
};
#[derive(Clone, Debug)]
pub(super) struct Region {
    pub(super) kind: Kind,
    offset: usize,
    boolean: bool,
    position: Option<f64>,
    absolute: bool,
    effects: bool,
    pub(super) parent_schema: Option<Arc<acquisition::Schema>>,
}
#[derive(Clone, Debug)]
pub(super) enum Kind {
    Value(Box<Program>),
    Route {
        steps: Box<[Step<Region>]>,
        focus: bool,
    },
    Scope(Box<[Region]>),
    Keep(Box<Region>),
    Filter(Box<Region>, Box<[Region]>),
    Order(Box<Region>, Box<[(Region, bool)]>),
}
impl Region {
    pub(super) fn compile(node: &crate::expression::Node) -> Result<Self, Unsupported> {
        use crate::expression::Kind as N;
        let kind = match node.form() {
            Form::Route(steps, focus) => {
                let start = steps
                    .iter()
                    .position(|s| s.bindings.is_some() || crate::tuple::active(&s.node))
                    .unwrap();
                let start = if focus && start == 0 { 1 } else { start };
                // An uncorrelated prefix is one ordinary IR body. Retain no
                // duplicate programs for stages that correlation never visits.
                let collapsed = start > 0 && !(focus && start == 1);
                let mut compiled = Vec::new();
                if collapsed {
                    let prefix = crate::expression::Node {
                        kind: N::Route(steps[..start].into(), focus),
                        offset: node.offset,
                        depth: node.depth,
                        effects: node.effects,
                        clock: node.clock,
                        tail_call: false,
                    };
                    compiled.push(Step {
                        node: Self::value(&prefix)?,
                        predicates: Box::new([]),
                        bindings: None,
                        lookup: false,
                        effects: node.effects,
                    });
                }
                compiled.extend(
                    steps
                        .iter()
                        .skip(if collapsed { start } else { 0 })
                        .map(|s| {
                            Ok(Step {
                                node: Self::compile(&s.node)?,
                                predicates: s
                                    .predicates
                                    .iter()
                                    .map(Self::value)
                                    .collect::<Result<_, _>>()?,
                                bindings: s.bindings.clone(),
                                lookup: s.lookup,
                                effects: s.effects,
                            })
                        })
                        .collect::<Result<Vec<_>, Unsupported>>()?,
                );
                Kind::Route {
                    steps: compiled.into(),
                    focus: focus && !collapsed,
                }
            }
            Form::Scope(nodes) => {
                Kind::Scope(nodes.iter().map(Self::compile).collect::<Result<_, _>>()?)
            }
            Form::Keep(n) => Kind::Keep(Box::new(Self::compile(n)?)),
            Form::Filter(n, tests) => Kind::Filter(
                Box::new(Self::compile(n)?),
                tests.iter().map(Self::value).collect::<Result<_, _>>()?,
            ),
            Form::Order(n, terms) => Kind::Order(
                Box::new(Self::compile(n)?),
                terms
                    .iter()
                    .map(|(n, desc)| Ok((Self::value(n)?, *desc)))
                    .collect::<Result<_, Unsupported>>()?,
            ),
            Form::Value => return Self::value(node),
        };
        Ok(Self::new(node, kind))
    }
    pub(super) fn value(node: &crate::expression::Node) -> Result<Self, Unsupported> {
        Ok(Self::new(
            node,
            Kind::Value(Box::new(lower::context_body(node)?)),
        ))
    }
    fn new(node: &crate::expression::Node, kind: Kind) -> Self {
        Self {
            kind,
            offset: node.offset,
            boolean: node.boolean(),
            position: node.position(),
            absolute: node.absolute(),
            effects: node.effects,
            parent_schema: None,
        }
    }
    pub(super) fn children(&self, visit: &mut impl FnMut(&Program)) {
        match &self.kind {
            Kind::Value(p) => visit(p),
            Kind::Route { steps, .. } => {
                for s in steps {
                    s.node.children(visit);
                    for p in &s.predicates {
                        p.children(visit)
                    }
                }
            }
            Kind::Scope(nodes) => {
                for n in nodes {
                    n.children(visit)
                }
            }
            Kind::Keep(n) => n.children(visit),
            Kind::Filter(n, tests) => {
                n.children(visit);
                for p in tests {
                    p.children(visit)
                }
            }
            Kind::Order(n, terms) => {
                n.children(visit);
                for (p, _) in terms {
                    p.children(visit)
                }
            }
        }
    }
    pub(super) fn children_mut(&mut self, visit: &mut impl FnMut(&mut Program)) {
        match &mut self.kind {
            Kind::Value(p) => visit(p),
            Kind::Route { steps, .. } => {
                for s in steps {
                    s.node.children_mut(visit);
                    for p in &mut s.predicates {
                        p.children_mut(visit)
                    }
                }
            }
            Kind::Scope(nodes) => {
                for n in nodes {
                    n.children_mut(visit)
                }
            }
            Kind::Keep(n) => n.children_mut(visit),
            Kind::Filter(n, tests) => {
                n.children_mut(visit);
                for p in tests {
                    p.children_mut(visit)
                }
            }
            Kind::Order(n, terms) => {
                n.children_mut(visit);
                for (p, _) in terms {
                    p.children_mut(visit)
                }
            }
        }
    }
}
impl Expression for Region {
    fn form(&self) -> Form<'_, Self> {
        match &self.kind {
            Kind::Value(_) => Form::Value,
            Kind::Route { steps, focus, .. } => Form::Route(steps, *focus),
            Kind::Scope(n) => Form::Scope(n),
            Kind::Keep(n) => Form::Keep(n),
            Kind::Filter(n, p) => Form::Filter(n, p),
            Kind::Order(n, t) => Form::Order(n, t),
        }
    }
    fn offset(&self) -> usize {
        self.offset
    }
    fn boolean(&self) -> bool {
        self.boolean
    }
    fn position(&self) -> Option<f64> {
        self.position
    }
    fn absolute(&self) -> bool {
        self.absolute
    }
    fn effects(&self) -> bool {
        self.effects
    }
}
struct Execute<'a, 'e, 'i, 's> {
    activation: Activation<'a, 'e, 'i, 's>,
    frame: std::cell::RefCell<frame::Frame<'e, 'i>>,
}
impl<'a, 'e, 'i, 's> Execute<'a, 'e, 'i, 's> {
    fn new(activation: Activation<'a, 'e, 'i, 's>) -> Self {
        Self {
            activation,
            frame: std::cell::RefCell::new(frame::Frame::repeated()),
        }
    }
}
impl<'e, 'i> Evaluator<'e, 'i, Region> for Execute<'_, 'e, 'i, '_> {
    fn parent(&self, body: &'e Region, value: &Value<'e, 'i>) -> Option<Value<'e, 'i>> {
        body.parent_schema
            .as_ref()
            .map(|schema| self.activation.input.retain(value.clone(), schema))
    }

    fn run(
        &self,
        body: &'e Region,
        context: &Context<'e, 'i>,
    ) -> Result<Operand<'e, 'i>, crate::Error> {
        if let Kind::Value(program) = &body.kind {
            return self.body(program, context);
        }
        crate::retain::collect(|emit| {
            finish(crate::tuple::values(
                body,
                context,
                &mut |v| {
                    emit(v);
                    Ok(())
                },
                self,
            ))
        })
        .map(|v| v.map_or(Operand::Missing, Operand::One))
    }
    fn filtered(
        &self,
        body: &'e Region,
        tests: &'e [Region],
        context: &Context<'e, 'i>,
        output: &mut dyn FnMut(View<'_, 'e, 'i>) -> Walk,
    ) -> Walk {
        let value = self.run(body, context)?;
        self.filters(View::Operand(&value), tests, context, output)
    }
    fn prefix(
        &self,
        steps: &'e [Step<Region>],
        _focus: bool,
        context: &Context<'e, 'i>,
        output: &mut dyn FnMut(View<'_, 'e, 'i>) -> Walk,
    ) -> Walk {
        debug_assert_eq!(steps.len(), 1);
        output(View::Operand(&self.run(&steps[0].node, context)?))
    }
}
impl<'e, 'i> Execute<'_, 'e, 'i, '_> {
    fn body(
        &self,
        program: &'e Program,
        context: &Context<'e, 'i>,
    ) -> Result<Operand<'e, 'i>, crate::Error> {
        let a = self.activation;
        if let Some(s) = context.scope.as_ref().filter(|s| s.controlled()) {
            s.item(program.offset)?
        }
        // A single requested row schema lives only for this body invocation.
        // Export Value fragments before the capture spans/workspace are reused.
        let mut result = Operand::Missing;
        a.input.each_values(
            std::iter::once(context.value.clone()),
            &program.row_schema,
            |_| true,
            |input, focus, _| {
                let value = program.run(
                    Activation {
                        input,
                        focus: &focus,
                        wrapped: context.wrapped,
                        scope: context.scope.as_ref(),
                        ..a
                    },
                    &mut self.frame.borrow_mut(),
                )?;
                result = value.operand(input);
                Ok::<_, crate::Error>(())
            },
        )?;
        Ok(result)
    }

    fn filters(
        &self,
        input: View<'_, 'e, 'i>,
        tests: &'e [Region],
        context: &Context<'e, 'i>,
        output: &mut dyn FnMut(View<'_, 'e, 'i>) -> Walk,
    ) -> Walk {
        let Some((p, rest)) = tests.split_first() else {
            return output(input);
        };
        if let Some(index) = p.position {
            return crate::filter::position(input, index, &mut |v| {
                self.filters(v, rest, context, output)
            });
        }
        let length = std::cell::Cell::new(None);
        let filtered = |emit: &mut Output<'_, 'e, 'i>| {
            let mut index = 0;
            input.transform(true, emit, |value, emit| {
                let result = self.run(
                    p,
                    &Context {
                        value: value.clone(),
                        wrapped: false,
                        scope: context.scope.clone(),
                    },
                )?;
                crate::filter::select(
                    &result,
                    p.offset,
                    index,
                    || {
                        if let Some(n) = length.get() {
                            return Ok(n);
                        }
                        let mut n = 0;
                        input.candidates(true, &mut |_| {
                            n += 1;
                            Ok(())
                        })?;
                        length.set(Some(n));
                        Ok(n)
                    },
                    &mut || emit(value.clone()),
                )?;
                index += 1;
                Ok(())
            })
        };
        if p.effects {
            let mut values = Vec::new();
            filtered(&mut |v| {
                values.push(v);
                Ok(())
            })?;
            self.filters(View::Items(&values), rest, context, output)
        } else {
            self.filters(View::Producer(&filtered), rest, context, output)
        }
    }
}
fn finish(result: Walk) -> Result<(), crate::Error> {
    result.map_err(|e| match e {
        Halt::Evaluation(e) => e,
        Halt::Stop => unreachable!("complete contextual region"),
    })
}
pub(super) fn walk<'e, 'i>(
    region: &'e Region,
    a: Activation<'_, 'e, 'i, '_>,
    out: &mut Output<'_, 'e, 'i>,
) -> Walk {
    let context = Context {
        value: a.focus.value(a.input).unwrap_or(Value::Undefined),
        wrapped: a.wrapped,
        scope: a.scope.cloned(),
    };
    crate::tuple::values(region, &context, out, &Execute::new(a))
}
pub(super) fn group<'e, 'i>(
    region: &'e Region,
    pairs: &'e [(Region, Region)],
    a: Activation<'_, 'e, 'i, '_>,
    offset: usize,
) -> Result<Cell<'e, 'i>, crate::Error> {
    let context = Context {
        value: a.focus.value(a.input).unwrap_or(Value::Undefined),
        wrapped: a.wrapped,
        scope: a.scope.cloned(),
    };
    crate::tuple::group_with(region, pairs, &context, offset, &Execute::new(a)).map(Cell::Value)
}

impl Region {
    pub(super) fn total(&self) -> bool {
        match &self.kind {
            Kind::Value(body) => super::consume::total(body),
            Kind::Route { steps, .. } => steps
                .iter()
                .all(|s| s.predicates.is_empty() && s.node.total()),
            Kind::Keep(body) => body.total(),
            Kind::Scope(nodes) => nodes.iter().all(Self::total),
            Kind::Filter(..) | Kind::Order(..) => false,
        }
    }
}
