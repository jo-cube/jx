pub(crate) mod pure;
use crate::{
    Error, Value,
    path::PathEvaluation,
    sequence::{Output, Stream, Walk},
    value::{range_error, type_error},
};
use std::convert::Infallible;

/// Results over fully validated input. Predicate failures can occur during consumption.
#[derive(Debug)]
pub struct Evaluation<'expression, 'input> {
    pub(crate) result: Results<'expression, 'input>,
}

#[derive(Debug)]
pub(crate) enum Results<'e, 'i> {
    Controlled(Box<(Results<'e, 'i>, crate::controls::Control)>),

    Execution(Box<crate::execution::Deferred<'e, 'i>>),
    Path(PathEvaluation<'e, 'i>),
    Scalar(Option<Value<'e, 'i>>),
}

/// A streamed evaluation failure or the caller's original output error.
#[derive(Debug, PartialEq, Eq)]
pub enum ConsumeError<E> {
    Evaluation(Error),
    Consumer(E),
}
impl<E: std::fmt::Display> std::fmt::Display for ConsumeError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Evaluation(e) => e.fmt(f),
            Self::Consumer(e) => e.fmt(f),
        }
    }
}
impl<E: std::error::Error + 'static> std::error::Error for ConsumeError<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Evaluation(e) => Some(e),
            Self::Consumer(e) => Some(e),
        }
    }
}

impl<'e, 'i> Evaluation<'e, 'i> {
    /// Collect independent owned snapshots. Missing is an empty vector; an array
    /// is one value, and a sequence is multiple values.
    pub fn collect_owned(self) -> Result<Vec<crate::OwnedValue>, Error> {
        let mut values = Vec::new();
        self.try_for_each(|v| {
            values.push(v.to_owned()?);
            Ok(())
        })
        .map_err(|e| match e {
            ConsumeError::Evaluation(e) | ConsumeError::Consumer(e) => e,
        })?;
        Ok(values)
    }
    /// Require at most one result; stop at the second item without running later work.
    pub fn single(self) -> Result<Option<Value<'e, 'i>>, Error> {
        let mut first = None;
        self.try_for_each(|v| {
            if first.is_some() {
                return Err(Error::new(
                    crate::ErrorKind::CardinalityError,
                    0,
                    "expected at most one result",
                )
                .result());
            }
            first = Some(v);
            Ok(())
        })
        .map_err(|e| match e {
            ConsumeError::Evaluation(e) | ConsumeError::Consumer(e) => e,
        })?;
        Ok(first)
    }

    /// Stream values without collecting; evaluation errors may follow earlier output.
    pub fn for_each(self, mut output: impl FnMut(Value<'e, 'i>)) -> Result<(), Error> {
        match self.try_for_each(|value| {
            output(value);
            Ok::<_, Infallible>(())
        }) {
            Ok(()) => Ok(()),
            Err(ConsumeError::Evaluation(error)) => Err(error),
            Err(ConsumeError::Consumer(error)) => match error {},
        }
    }
    /// A consumer error stops traversal immediately, without evaluating later items.
    pub fn try_for_each<E>(
        self,
        mut output: impl FnMut(Value<'e, 'i>) -> Result<(), E>,
    ) -> Result<(), ConsumeError<E>> {
        let result = match self.result {
            Results::Controlled(controlled) => {
                let (result, control) = *controlled;
                enum Failure<E> {
                    Control(Error),
                    Consumer(E),
                }
                return Evaluation { result }
                    .consume(|value| {
                        control.result(&value).map_err(Failure::Control)?;
                        output(value).map_err(Failure::Consumer)
                    })
                    .map_err(|e| match e {
                        ConsumeError::Evaluation(e)
                        | ConsumeError::Consumer(Failure::Control(e)) => {
                            ConsumeError::Evaluation(e)
                        }
                        ConsumeError::Consumer(Failure::Consumer(e)) => ConsumeError::Consumer(e),
                    });
            }
            result => result,
        };
        Evaluation { result }.consume(output)
    }
    fn consume<E>(
        self,
        mut output: impl FnMut(Value<'e, 'i>) -> Result<(), E>,
    ) -> Result<(), ConsumeError<E>> {
        match self.result {
            Results::Execution(deferred) => deferred.walk(output),
            Results::Controlled(_) => unreachable!("controls are applied once by try_for_each"),
            Results::Path(path) => path.try_for_each(output).map_err(ConsumeError::Consumer),
            Results::Scalar(Some(value)) if value.unpacks_sequence() => {
                for item in value.elements() {
                    output(item).map_err(ConsumeError::Consumer)?;
                }
                Ok(())
            }
            Results::Scalar(Some(value)) => output(value).map_err(ConsumeError::Consumer),
            Results::Scalar(None) => Ok(()),
        }
    }
}

#[derive(Clone)]
pub(crate) enum Operand<'e, 'i> {
    Missing,
    One(Value<'e, 'i>),
    Many(Stream<'e, 'i>),
}

impl<'e, 'i> Operand<'e, 'i> {
    pub(crate) fn normalize(self) -> Self {
        if let Self::One(value) = &self
            && value.unpacks_sequence()
        {
            let mut items = value.elements();
            let Some(first) = items.next() else {
                return Self::Missing;
            };
            if items.next().is_none() {
                return if matches!(first, Value::Undefined) {
                    Self::Missing
                } else {
                    Self::One(first)
                };
            }
        }
        self
    }

    pub(crate) fn walk(&self, output: &mut Output<'_, 'e, 'i>) -> Walk {
        match self {
            Self::Missing => Ok(()),
            Self::One(value) if value.unpacks_sequence() => {
                for item in value.elements() {
                    output(item)?;
                }
                Ok(())
            }
            Self::One(value) => output(value.clone()),
            Self::Many(stream) => stream.walk(output),
        }
    }

    pub(crate) fn truth(&self, offset: usize) -> Result<bool, Error> {
        match self {
            Self::Missing => Ok(false),
            Self::One(value) => value.truth(offset),
            Self::Many(path) => {
                let mut truth = false;
                path.visit(|value| {
                    truth |= value.truth(offset)?;
                    Ok(())
                })?;
                Ok(truth)
            }
        }
    }

    pub(crate) fn number(&self, offset: usize) -> Result<Option<f64>, Error> {
        match self {
            Self::Missing => Ok(None),
            Self::One(value) => match value.atomic() {
                Value::Undefined => Ok(None),
                Value::Number(value) if value.is_infinite() => Err(range_error(offset)),
                Value::Number(value) if !value.is_nan() => Ok(Some(value)),
                _ => Err(type_error(offset)),
            },
            Self::Many(_) => Err(type_error(offset)),
        }
    }
}
