use crate::{
    Error, RawJson, Value,
    expression::{Kind, Node, Op},
    path::PathEvaluation,
    sequence::{Context, Halt, Output, Stream, Walk},
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
    Path(PathEvaluation<'e, 'i>),
    Expression(&'e Node, Context<'e, 'i>),
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
impl<E: std::error::Error + 'static> std::error::Error for ConsumeError<E> {}

impl<'e, 'i> Evaluation<'e, 'i> {
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
        match self.result {
            Results::Path(path) => path
                .try_for_each(|raw| output(Value::Raw(raw)))
                .map_err(ConsumeError::Consumer),
            Results::Scalar(Some(value)) => output(value).map_err(ConsumeError::Consumer),
            Results::Scalar(None) => Ok(()),
            Results::Expression(node, context) => {
                let stream = Stream::Expression(node, context);
                let mut error = None;
                let mut consumer = |value| {
                    output(value).map_err(|e| {
                        error = Some(e);
                        Halt::Stop
                    })
                };
                let mut first = true;
                let mut pending_undefined = false;
                let result = stream.walk(&mut |value| {
                    if first && matches!(value, Value::Undefined) {
                        first = false;
                        pending_undefined = true;
                        return Ok(());
                    }
                    first = false;
                    if pending_undefined {
                        pending_undefined = false;
                        consumer(Value::Undefined)?;
                    }
                    consumer(value)
                });
                match result {
                    Ok(()) => Ok(()),
                    Err(Halt::Evaluation(e)) => Err(ConsumeError::Evaluation(e)),
                    Err(Halt::Stop) => Err(ConsumeError::Consumer(error.unwrap())),
                }
            }
        }
    }
}

pub(crate) fn scalar<'e, 'i>(node: &'e Node, input: &'i [u8]) -> Result<Evaluation<'e, 'i>, Error> {
    let context = Context {
        value: Value::Raw(crate::validate(input)?),
        wrapped: true,
    };
    let result = if matches!(node.kind, Kind::Route(_) | Kind::Filter(..)) {
        Results::Expression(node, context)
    } else {
        match node.run(context)? {
            Operand::Missing => Results::Scalar(None),
            Operand::One(value) => Results::Scalar(Some(value)),
            Operand::Many(Stream::Path(path)) => Results::Path(path),
            Operand::Many(Stream::Expression(node, context)) => Results::Expression(node, context),
        }
    };
    Ok(Evaluation { result })
}

#[derive(Clone, Copy)]
pub(crate) enum Operand<'e, 'i> {
    Missing,
    One(Value<'e, 'i>),
    Many(Stream<'e, 'i>),
}

impl<'e, 'i> Operand<'e, 'i> {
    pub(crate) fn walk(self, output: &mut Output<'_, 'e, 'i>) -> Walk {
        match self {
            Self::Missing => Ok(()),
            Self::One(value) => output(value),
            Self::Many(stream) => stream.walk(output),
        }
    }

    pub(crate) fn truth(self, offset: usize) -> Result<bool, Error> {
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

    fn number(self, offset: usize) -> Result<Option<f64>, Error> {
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

impl Node {
    pub(crate) fn run<'e, 'i>(&'e self, input: Context<'e, 'i>) -> Result<Operand<'e, 'i>, Error> {
        let value = match &self.kind {
            Kind::Path(path) => {
                if path.fields.is_empty() {
                    return Ok(if matches!(input.value, Value::Undefined) {
                        Operand::Missing
                    } else {
                        Operand::One(input.value)
                    });
                }
                let Value::Raw(_) = input.value else {
                    return Ok(Operand::Missing);
                };
                return self.stream(input).unwrap().operand();
            }
            Kind::Route(_) | Kind::Filter(..) => return Stream::Expression(self, input).operand(),
            Kind::Group(child) => return child.run(input),
            Kind::Aggregate(aggregate, args) => {
                return aggregate.evaluate(args, input, self.offset);
            }
            Kind::Missing => return Ok(Operand::Missing),
            Kind::Number(value) => Value::Number(*value),
            Kind::Boolean(value) => Value::Boolean(*value),
            Kind::Null => Value::Null,
            Kind::String(value) => Value::StringLiteral(RawJson(value)),
            Kind::Negate(child) => {
                return Ok(child
                    .run(input)?
                    .number(self.offset)?
                    .map_or(Operand::Missing, |value| {
                        Operand::One(Value::Number(-value))
                    }));
            }
            Kind::Binary(op, lhs, rhs) => {
                let left = lhs.run(input)?;
                if matches!(op, Op::And | Op::Or) {
                    let truth = left.truth(self.offset)?;
                    let value = match op {
                        Op::And => truth && rhs.run(input)?.truth(self.offset)?,
                        Op::Or => truth || rhs.run(input)?.truth(self.offset)?,
                        _ => unreachable!(),
                    };
                    return Ok(Operand::One(Value::Boolean(value)));
                }
                let right = rhs.run(input)?;
                match op {
                    Op::Equal | Op::NotEqual => Value::Boolean(crate::compare::equal(
                        left,
                        right,
                        matches!(op, Op::NotEqual),
                    )?),
                    Op::Less | Op::LessEqual | Op::Greater | Op::GreaterEqual => {
                        return crate::compare::order(left, right, *op, self.offset);
                    }
                    _ => {
                        // Type-check both operands before propagating missing.
                        let left = left.number(self.offset)?;
                        let right = right.number(self.offset)?;
                        let (Some(left), Some(right)) = (left, right) else {
                            return Ok(Operand::Missing);
                        };
                        Value::Number(match op {
                            Op::Add => left + right,
                            Op::Subtract => left - right,
                            Op::Multiply => left * right,
                            Op::Divide => left / right,
                            Op::Remainder => left % right,
                            _ => unreachable!(),
                        })
                    }
                }
            }
        };
        Ok(Operand::One(value))
    }
}
