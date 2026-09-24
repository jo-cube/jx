use crate::{
    Error, Value,
    evaluate::Operand,
    expression::{Kind, Node, Step},
    path::PathEvaluation,
};

#[derive(Clone, Debug)]
pub(crate) struct Context<'e, 'i> {
    pub value: Value<'e, 'i>,
    // The top-level JSON record is one context, even when it is an array.
    pub wrapped: bool,
}

pub(crate) enum Halt {
    Evaluation(Error),
    Stop,
}
impl From<Error> for Halt {
    fn from(error: Error) -> Self {
        Self::Evaluation(error)
    }
}
pub(crate) type Walk = Result<(), Halt>;
pub(crate) type Output<'a, 'e, 'i> = dyn FnMut(Value<'e, 'i>) -> Walk + 'a;

#[derive(Clone, Debug)]
pub(crate) enum Stream<'e, 'i> {
    Path(PathEvaluation<'e, 'i>),
    Expression(&'e Node, Context<'e, 'i>),
}
impl<'e, 'i> Stream<'e, 'i> {
    pub fn walk(&self, output: &mut Output<'_, 'e, 'i>) -> Walk {
        match self {
            Self::Path(path) => path.try_for_each(output),
            Self::Expression(node, context) => match &node.kind {
                Kind::Route(steps, array_focus) => {
                    route(steps, *array_focus, context, &mut |view| view.walk(output))
                }
                Kind::Filter(base, predicates) => {
                    crate::filter::with_filters(base, predicates, context, &mut |view| {
                        view.walk(output)
                    })
                }
                _ => unreachable!("expression streams contain mapped steps or filters"),
            },
        }
    }
    pub fn operand(self) -> Result<Operand<'e, 'i>, Error> {
        let mut first = None;
        let mut many = false;
        let fast = matches!(self, Self::Path(_));
        let result = self.walk(&mut |value| {
            if first.is_some() {
                many = true;
                if fast {
                    return Err(Halt::Stop);
                }
            } else {
                first = Some(value);
            }
            Ok(())
        });
        if let Err(Halt::Evaluation(error)) = result {
            return Err(error);
        }
        Ok(if many {
            Operand::Many(self)
        } else {
            match first {
                None | Some(Value::Undefined) => Operand::Missing,
                Some(value) => Operand::One(value),
            }
        })
    }
    pub fn visit(
        &self,
        mut output: impl FnMut(Value<'e, 'i>) -> Result<(), Error>,
    ) -> Result<(), Error> {
        match self.walk(&mut |value| output(value).map_err(Halt::Evaluation)) {
            Ok(()) => Ok(()),
            Err(Halt::Evaluation(error)) => Err(error),
            Err(Halt::Stop) => unreachable!(),
        }
    }
}

// Scoped views borrow preceding stages on the stack. In particular, a filter's
// computed length survives replays by later positional filters, avoiding recursive
// recounting without retaining all candidates.
#[derive(Clone, Copy)]
pub(crate) enum View<'s, 'e, 'i> {
    Operand(&'s Operand<'e, 'i>),
    Items(&'s [Value<'e, 'i>]),
    Map(&'s Map<'s, 'e, 'i>),
    Filter(&'s crate::filter::Filter<'s, 'e, 'i>),
}
impl<'e, 'i> View<'_, 'e, 'i> {
    pub fn walk(&self, output: &mut Output<'_, 'e, 'i>) -> Walk {
        match self {
            Self::Operand(value) => value.walk(output),
            Self::Items(items) => {
                for value in *items {
                    output(value.clone())?;
                }
                Ok(())
            }
            Self::Map(map) => map.walk(output),
            Self::Filter(filter) => filter.walk(output),
        }
    }
    // Defer downstream failures while checking this stage. If this stage fails,
    // replay its input for earlier errors. This preserves eager JSONata error
    // precedence without an eager validation pass on successful streamed results.
    pub fn transform(
        &self,
        missing: bool,
        output: &mut Output<'_, 'e, 'i>,
        mut transform: impl FnMut(Value<'e, 'i>, &mut Output<'_, 'e, 'i>) -> Walk,
    ) -> Walk {
        let mut downstream = None;
        let result = self.candidates(missing, &mut |value| {
            transform(value, &mut |item| {
                if downstream.is_some() {
                    return Ok(());
                }
                match output(item) {
                    Err(Halt::Evaluation(error)) => {
                        downstream = Some(error);
                        Ok(())
                    }
                    result => result,
                }
            })
        });
        if matches!(result, Err(Halt::Evaluation(_))) {
            self.candidates(missing, &mut |_| Ok(()))?;
        }
        result?;
        downstream.map_or(Ok(()), |error| Err(Halt::Evaluation(error)))
    }
    pub fn candidates(&self, missing: bool, output: &mut Output<'_, 'e, 'i>) -> Walk {
        match self {
            Self::Operand(Operand::Missing) if missing => output(Value::Undefined),
            Self::Operand(Operand::One(value)) if value.is_array() => {
                for item in value.elements() {
                    output(item)?;
                }
                Ok(())
            }
            _ => self.walk(output),
        }
    }
}

pub(crate) struct Map<'s, 'e, 'i> {
    input: View<'s, 'e, 'i>,
    step: &'e Step,
    last: bool,
}
impl<'e, 'i> Map<'_, 'e, 'i> {
    fn walk(&self, output: &mut Output<'_, 'e, 'i>) -> Walk {
        let mut defined = false;
        let mut pending = None;
        self.input.transform(false, output, |value, output| {
            let context = Context {
                value,
                wrapped: self.step.lookup,
            };
            crate::filter::with_filters(
                &self.step.node,
                &self.step.predicates,
                &context,
                &mut |view| {
                    if matches!(view, View::Operand(Operand::Missing)) {
                        return Ok(());
                    }
                    if let Some(value) = pending.take() {
                        emit_mapped(value, output)?;
                    }
                    if self.last && !defined {
                        defined = true;
                        if let View::Operand(Operand::One(value)) = view
                            && value.is_array()
                            && !value.is_sequence()
                        {
                            pending = Some(value.clone());
                            return Ok(());
                        }
                    }
                    defined = true;
                    match &view {
                        View::Operand(Operand::One(value)) if value.preserves_array() => {
                            output(value.clone())
                        }
                        _ => view.candidates(false, output),
                    }
                },
            )
        })?;
        if let Some(value) = pending {
            output(value)?;
        }
        Ok(())
    }
}
fn emit_mapped<'e, 'i>(value: Value<'e, 'i>, output: &mut Output<'_, 'e, 'i>) -> Walk {
    if value.preserves_array() {
        output(value)
    } else {
        View::Operand(&Operand::One(value)).candidates(false, output)
    }
}
fn route<'e, 'i>(
    steps: &'e [Step],
    array_focus: bool,
    context: &Context<'e, 'i>,
    output: &mut dyn FnMut(View<'_, 'e, 'i>) -> Walk,
) -> Walk {
    fn stages<'e, 'i>(
        input: View<'_, 'e, 'i>,
        steps: &'e [Step],
        output: &mut dyn FnMut(View<'_, 'e, 'i>) -> Walk,
    ) -> Walk {
        let Some((step, rest)) = steps.split_first() else {
            return output(input);
        };
        let map = Map {
            input,
            step,
            last: rest.is_empty(),
        };
        stages(View::Map(&map), rest, output)
    }
    if array_focus {
        return crate::filter::with_filters(
            &steps[0].node,
            &steps[0].predicates,
            context,
            &mut |view| {
                if let View::Operand(Operand::One(value)) = &view
                    && !value.is_array()
                {
                    if matches!(value.atomic(), Value::Null) {
                        return Err(crate::value::type_error(steps[0].node.offset).into());
                    }
                    if value.string_body().is_some() {
                        return Err(crate::Error::new(
                            crate::ErrorKind::UnsupportedExpression,
                            steps[0].node.offset,
                            "string iteration after a filtered array constructor is deferred",
                        )
                        .into());
                    }
                    return output(View::Items(&[]));
                }
                if matches!(&view, View::Operand(Operand::One(value)) if value.is_array() && value.elements().next().is_none())
                {
                    output(view)
                } else {
                    stages(view, &steps[1..], output)
                }
            },
        );
    }
    let variable =
        matches!(&steps[0].node.kind, Kind::Path(path) if path.rooted && path.fields.is_empty());
    if context.wrapped || variable {
        stages(
            View::Items(std::slice::from_ref(&context.value)),
            steps,
            output,
        )
    } else {
        stages(
            View::Operand(&Operand::One(context.value.clone())),
            steps,
            output,
        )
    }
}

impl Node {
    // Expose deferred results without a cardinality preflight. Consumers that
    // require a scalar still normalize through Stream::operand.
    #[inline]
    pub(crate) fn stream<'e, 'i>(&'e self, input: &Context<'e, 'i>) -> Option<Stream<'e, 'i>> {
        match &self.kind {
            Kind::Path(path) if !path.fields.is_empty() => {
                Some(Stream::Path(path.select_context(input)))
            }
            Kind::Route(..) | Kind::Filter(..) => Some(Stream::Expression(self, input.clone())),
            Kind::Group(child) => child.stream(input),
            _ => None,
        }
    }
}
