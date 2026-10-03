use crate::{
    Error, Value,
    evaluate::Operand,
    expression::{Kind, Node},
    path::PathEvaluation,
};

#[derive(Clone, Debug)]
pub(crate) struct Context<'e, 'i> {
    pub value: Value<'e, 'i>,
    // The top-level JSON record is one context, even when it is an array.
    pub wrapped: bool,
    pub scope: Option<crate::runtime::Scope<'e, 'i>>,
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
        if let Self::Expression(node, context) = self
            && let Some(scope) = &context.scope
            && scope.controlled()
        {
            scope.checkpoint(node.offset)?;
            return self.walk_unchecked(&mut |value| {
                scope.item(node.offset)?;
                output(value)
            });
        }
        self.walk_unchecked(output)
    }
    fn walk_unchecked(&self, output: &mut Output<'_, 'e, 'i>) -> Walk {
        match self {
            Self::Path(path) => path.try_for_each(output),
            Self::Expression(node, context) => match &node.kind {
                Kind::Path(path) => path.select_context(context).try_for_each(output),
                Kind::Wildcard => crate::navigate::wildcard(&context.value, output),
                Kind::Descendants => crate::navigate::descendants(context.value.clone(), output),
                Kind::Range(left, right) => {
                    crate::navigate::range(left, right, context, node.offset, output)
                }
                Kind::Route(steps, array_focus) => {
                    crate::route::walk(steps, *array_focus, false, context, &mut |view| {
                        view.walk(output)
                    })
                }
                Kind::Tuples(steps, focus) => {
                    crate::tuple::route_values(steps, *focus, context, output)
                }
                Kind::Filter(base, predicates) if crate::tuple::active(base) => {
                    crate::tuple::filtered_values(base, predicates, context, output)
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
        // These traversals cannot fail after validated input. Two items settle
        // cardinality; do not enumerate the entire subtree before replaying it.
        let fast = matches!(self, Self::Path(_))
            || matches!(&self, Self::Expression(node, _) if matches!(node.kind, Kind::Wildcard | Kind::Descendants));
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
    Map(&'s crate::route::Map<'s, 'e, 'i>),
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

impl Node {
    pub(crate) fn consume<'e, 'i>(
        &'e self,
        context: &Context<'e, 'i>,
        output: &mut Output<'_, 'e, 'i>,
    ) -> Walk {
        if matches!(self.kind, Kind::Tuples(..)) {
            return Stream::Expression(self, context.clone()).walk(output);
        }
        match self.stream(context) {
            Some(stream) => stream.walk(output),
            None => self.run(context)?.walk(output),
        }
    }

    // Expose deferred results without a cardinality preflight. Consumers that
    // require a scalar still normalize through Stream::operand.
    #[inline]
    pub(crate) fn stream<'e, 'i>(&'e self, input: &Context<'e, 'i>) -> Option<Stream<'e, 'i>> {
        if self.effects {
            return None;
        }
        match &self.kind {
            Kind::Path(path) if !path.fields.is_empty() => {
                if input.scope.as_ref().is_some_and(|s| s.controlled()) {
                    Some(Stream::Expression(self, input.clone()))
                } else {
                    Some(Stream::Path(path.select_context(input)))
                }
            }
            Kind::Route(..)
            | Kind::Filter(..)
            | Kind::Wildcard
            | Kind::Descendants
            | Kind::Range(..) => Some(Stream::Expression(self, input.clone())),
            Kind::Group(child) => child.stream(input),
            _ => None,
        }
    }
}
