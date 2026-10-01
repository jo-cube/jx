mod operators;
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
            Results::Path(path) => path.try_for_each(output).map_err(ConsumeError::Consumer),
            Results::Scalar(Some(value)) if value.unpacks_sequence() => {
                for item in value.elements() {
                    output(item).map_err(ConsumeError::Consumer)?;
                }
                Ok(())
            }
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
                let mut pending = None;
                let result = stream
                    .walk(&mut |value| {
                        if first && (matches!(value, Value::Undefined) || value.unpacks_sequence())
                        {
                            first = false;
                            pending = Some(value);
                            return Ok(());
                        }
                        first = false;
                        if let Some(value) = pending.take() {
                            consumer(value)?;
                        }
                        consumer(value)
                    })
                    .and_then(|()| {
                        if let Some(value) = pending
                            && value.unpacks_sequence()
                        {
                            for item in value.elements() {
                                consumer(item)?;
                            }
                        }
                        Ok(())
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

pub(crate) fn path_conversion<'e, 'i>(
    builtin: crate::builtin::Builtin,
    path: &'e crate::expression::Path,
    input: &'i [u8],
    offset: usize,
) -> Result<Evaluation<'e, 'i>, Error> {
    let selected = path.select(input)?;
    let value = crate::retain::collect(|emit| {
        selected.try_for_each(|value| {
            emit(value);
            Ok(())
        })
    })?;
    let context = Context {
        value: Value::Undefined,
        wrapped: true,
        scope: None,
    };
    Ok(Evaluation {
        result: results(builtin.values(&[value], &context, offset)?),
    })
}

pub(crate) fn scalar<'e, 'i>(node: &'e Node, input: &'i [u8]) -> Result<Evaluation<'e, 'i>, Error> {
    if let Kind::Plan(plan) = &node.kind {
        return Ok(Evaluation {
            result: results(plan.evaluate(input)?),
        });
    }
    let value = Value::Raw(crate::validate(input)?);
    let scope =
        (node.effects || node.clock).then(|| crate::runtime::Scope::new(value.clone(), node.clock));
    let context = Context {
        scope,
        value,
        wrapped: true,
    };
    let result = if (!node.effects || matches!(node.kind, Kind::Tuples(..)))
        && matches!(
            node.kind,
            Kind::Route(..)
                | Kind::Tuples(..)
                | Kind::Filter(..)
                | Kind::Wildcard
                | Kind::Descendants
                | Kind::Range(..)
        ) {
        Results::Expression(node, context)
    } else {
        results(node.run(&context)?)
    };
    Ok(Evaluation { result })
}

fn results<'e, 'i>(operand: Operand<'e, 'i>) -> Results<'e, 'i> {
    match operand {
        Operand::Missing => Results::Scalar(None),
        Operand::One(value) => Results::Scalar(Some(value)),
        Operand::Many(Stream::Path(path)) => Results::Path(path),
        Operand::Many(Stream::Expression(node, context)) => Results::Expression(node, context),
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

impl Node {
    pub(crate) fn run<'e, 'i>(&'e self, input: &Context<'e, 'i>) -> Result<Operand<'e, 'i>, Error> {
        let value = match &self.kind {
            Kind::Plan(plan) => return plan.run(input).map_or_else(|| plan.source.run(input), Ok),
            Kind::StaticLookup(data, key) => {
                let key = crate::retain::materialize(key, input)?;
                return crate::lookup::constant(data, key, self.offset)
                    .map(|v| v.map_or(Operand::Missing, Operand::One));
            }
            Kind::Prepared(prepared) => {
                let value = prepared.data.value();
                return Ok(if matches!(value, Value::Undefined) {
                    Operand::Missing
                } else {
                    Operand::One(value)
                });
            }
            Kind::BuiltinReference(builtin) => crate::Function::builtin(*builtin),
            Kind::Regex(pattern) => crate::matcher::literal(pattern),
            Kind::Transform(definition) => crate::transform::literal(definition, input),
            Kind::Path(path) => {
                if path.fields.is_empty() {
                    return Ok(if matches!(input.value, Value::Undefined) {
                        Operand::Missing
                    } else {
                        Operand::One(input.value.clone())
                    });
                }
                return path.select_context(input).operand();
            }
            Kind::Route(..)
            | Kind::Tuples(..)
            | Kind::Filter(..)
            | Kind::Wildcard
            | Kind::Descendants
            | Kind::Range(..) => {
                let stream = Stream::Expression(self, input.clone());
                if self.effects {
                    return crate::retain::collect(|emit| {
                        stream.visit(|v| {
                            emit(v);
                            Ok(())
                        })
                    })
                    .map(|v| v.map_or(Operand::Missing, Operand::One));
                }
                return stream.operand();
            }
            Kind::Keep(child, path) => return crate::navigate::keep(child, *path, input),
            Kind::Reduce(base, pairs) => crate::construct::reduce(base, pairs, input, self.offset)?,
            Kind::Sort(base, terms) => {
                return crate::ordering::evaluate(base, terms, input, self.offset);
            }
            Kind::Group(child) if !self.effects => return child.run(input),
            Kind::Group(child) => {
                return crate::runtime::block(std::slice::from_ref(child), input)
                    .map(|v| v.map_or(Operand::Missing, Operand::One));
            }
            Kind::Block(items) => {
                return crate::runtime::block(items, input)
                    .map(|v| v.map_or(Operand::Missing, Operand::One));
            }
            Kind::Formatted(call) => return call.evaluate(input, self.offset),
            Kind::Builtin(builtin, args) => {
                return builtin.evaluate(args, input, self.offset).map(|result| {
                    if self.tail_call {
                        result
                    } else {
                        result.normalize()
                    }
                });
            }
            Kind::Variable(name) | Kind::Parent(name) => {
                let value = input
                    .scope
                    .as_ref()
                    .and_then(|s| s.lookup(name))
                    .or_else(|| crate::builtin::Builtin::named(name).map(crate::Function::builtin));
                return Ok(match value {
                    None | Some(Value::Undefined) => Operand::Missing,
                    Some(value) => Operand::One(value),
                });
            }
            Kind::Bind(name, child) => {
                let value = crate::retain::materialize(child, input)?;
                input
                    .scope
                    .as_ref()
                    .expect("lexical runtime")
                    .bind(name, value.clone().unwrap_or(Value::Undefined));
                return Ok(value.map_or(Operand::Missing, Operand::One));
            }
            Kind::Conditional(test, yes, no) => {
                return if test.run(input)?.truth(self.offset)? {
                    yes.run(input)
                } else if let Some(no) = no {
                    no.run(input)
                } else {
                    Ok(Operand::Missing)
                };
            }
            Kind::Lambda(d) => crate::Function::lambda(d, input),
            Kind::Call(target, args) => {
                return crate::function::call(target, args, input, self.offset).map(|result| {
                    if self.tail_call {
                        result
                    } else {
                        result.normalize()
                    }
                });
            }
            Kind::Partial(target, args) => {
                crate::function::partial(target, args, input, self.offset)?
            }
            Kind::Binary(Op::Chain, left, right) => {
                return crate::function::chain(left, right, input, self.offset).map(|result| {
                    if self.tail_call {
                        result
                    } else {
                        result.normalize()
                    }
                });
            }
            Kind::Binary(Op::Concat, left, right) => {
                // Complete both operands before conversion; neither may be replayed.
                let left = crate::retain::materialize(left, input)?;
                let right = crate::retain::materialize(right, input)?;
                crate::convert::concat(left, right, self.offset)?
            }
            Kind::Array(items, preserve) => crate::construct::array(items, *preserve, input)?,
            Kind::Object(pairs) => crate::construct::object(pairs, input, self.offset)?,
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
            Kind::Binary(op @ (Op::Default | Op::Coalesce), test, no) => {
                return if test.run(input)?.truth(self.offset)? {
                    // Coalescing stores its left expression once, as the argument
                    // of the shadowable $exists call. Both fallbacks re-evaluate
                    // the selected branch without duplicating the compiled tree.
                    let yes = if matches!(op, Op::Coalesce) {
                        match &test.kind {
                            Kind::Call(_, args) | Kind::Builtin(_, args) => &args[0],
                            _ => unreachable!("coalescing test is an exists call"),
                        }
                    } else {
                        test
                    };
                    yes.run(input)
                } else {
                    no.run(input)
                };
            }
            Kind::Binary(op, lhs, rhs) => {
                return operators::binary(op, lhs, rhs, input, self.offset);
            }
        };
        Ok(Operand::One(value))
    }
}
