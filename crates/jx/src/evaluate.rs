use crate::{
    Error, RawJson, Value,
    expression::{Kind, Node, Op},
    path::PathEvaluation,
    value::{range_error, type_error},
};
use std::convert::Infallible;

/// A fully validated result, consumed without materializing path sequences.
#[derive(Debug)]
pub struct Evaluation<'expression, 'input> {
    pub(crate) result: Results<'expression, 'input>,
}

#[derive(Debug)]
pub(crate) enum Results<'e, 'i> {
    Path(PathEvaluation<'e, 'i>),
    Scalar(Option<Value<'e, 'i>>),
}

impl<'e, 'i> Evaluation<'e, 'i> {
    pub fn for_each(self, mut output: impl FnMut(Value<'e, 'i>)) {
        self.try_for_each(|value| {
            output(value);
            Ok::<_, Infallible>(())
        })
        .unwrap();
    }

    /// A consumer error stops traversal immediately and is returned unchanged.
    /// JSON validation and scalar runtime errors are reported by `evaluate` first.
    pub fn try_for_each<E>(
        self,
        mut output: impl FnMut(Value<'e, 'i>) -> Result<(), E>,
    ) -> Result<(), E> {
        match self.result {
            Results::Path(path) => path.try_for_each(|raw| output(Value::Raw(raw))),
            Results::Scalar(Some(value)) => output(value),
            Results::Scalar(None) => Ok(()),
        }
    }
}

pub(crate) fn scalar<'e, 'i>(node: &'e Node, input: &'i [u8]) -> Result<Evaluation<'e, 'i>, Error> {
    let input = crate::validate(input)?;
    let value = match node.run(input)? {
        Operand::Missing => None,
        Operand::One(value) => Some(value),
        Operand::Many(_) => unreachable!("scalar operators produce at most one item"),
    };
    Ok(Evaluation {
        result: Results::Scalar(value),
    })
}

#[derive(Clone, Copy)]
pub(crate) enum Operand<'e, 'i> {
    Missing,
    One(Value<'e, 'i>),
    Many(PathEvaluation<'e, 'i>),
}

impl<'e, 'i> Operand<'e, 'i> {
    fn path(path: PathEvaluation<'e, 'i>) -> Self {
        let mut first = None;
        let cardinality = path.try_for_each(|value| {
            if first.is_some() {
                return Err(());
            }
            first = Some(value);
            Ok(())
        });
        if cardinality.is_err() {
            Self::Many(path)
        } else {
            first.map_or(Self::Missing, |raw| Self::One(Value::Raw(raw)))
        }
    }

    pub(crate) fn truth(self, offset: usize) -> Result<bool, Error> {
        match self {
            Self::Missing => Ok(false),
            Self::One(value) => value.truth(offset),
            Self::Many(path) => {
                let mut truth = false;
                path.try_for_each(|value| {
                    truth |= Value::Raw(value).truth(offset)?;
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
                Value::Number(value) if value.is_infinite() => Err(range_error(offset)),
                Value::Number(value) if !value.is_nan() => Ok(Some(value)),
                _ => Err(type_error(offset)),
            },
            Self::Many(_) => Err(type_error(offset)),
        }
    }
}

impl Node {
    pub(crate) fn run<'e, 'i>(&'e self, input: RawJson<'i>) -> Result<Operand<'e, 'i>, Error> {
        let value = match &self.kind {
            Kind::Path(path) => return Ok(Operand::path(path.select_raw(input))),
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
                    )),
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
