use crate::{Error, Value, evaluate::Operand, path::PathEvaluation};

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
}
impl<'e, 'i> Stream<'e, 'i> {
    pub fn walk(&self, output: &mut Output<'_, 'e, 'i>) -> Walk {
        match self {
            Self::Path(path) => path.try_for_each(output),
        }
    }
    pub fn operand(self) -> Result<Operand<'e, 'i>, Error> {
        let mut first = None;
        let mut many = false;
        // These traversals cannot fail after validated input. Two items settle
        // cardinality; do not enumerate the entire subtree before replaying it.
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

    Producer(&'s dyn Fn(&mut Output<'_, 'e, 'i>) -> Walk),
    // A native candidate sequence at a value boundary. Resolve cardinality with
    // one pending item instead of constructing an intermediate sequence graph.
    Boundary(&'s dyn Fn(&mut Output<'_, 'e, 'i>) -> Walk),
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

            Self::Producer(walk) => walk(output),

            Self::Boundary(_) => self.resolve(&mut |view| view.walk(output)),
        }
    }

    pub fn resolve(&self, emit: &mut dyn FnMut(View<'_, 'e, 'i>) -> Walk) -> Walk {
        let Self::Boundary(walk) = self else {
            return emit(*self);
        };
        let mut first = None;
        let mut many = false;
        let mut downstream = None;
        let mut deliver = |value| {
            if downstream.is_some() {
                return Ok(());
            }
            match emit(View::Items(std::slice::from_ref(&value))) {
                Err(Halt::Evaluation(error)) => {
                    downstream = Some(error);
                    Ok(())
                }
                result => result,
            }
        };
        walk(&mut |value| {
            if many {
                return deliver(value);
            }
            if let Some(first) = first.take() {
                many = true;
                deliver(first)?;
                deliver(value)
            } else {
                first = Some(value);
                Ok(())
            }
        })?;
        if many {
            downstream.map_or(Ok(()), |e| Err(Halt::Evaluation(e)))
        } else {
            let operand = match first {
                None | Some(Value::Undefined) => Operand::Missing,
                Some(value) => Operand::One(value),
            };
            emit(View::Operand(&operand))
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
            Self::Boundary(_) => self.resolve(&mut |view| view.candidates(missing, output)),
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
