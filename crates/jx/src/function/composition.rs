use super::{Function, FunctionKind, arity, invoke};
use crate::{
    Error, Value,
    evaluate::Operand,
    expression::{Kind, Node},
    sequence::Context,
    value::type_error,
};
use std::rc::Rc;

#[derive(Clone, Debug)]
pub(crate) enum Argument<'e, 'i> {
    Hole,
    Value(Option<Value<'e, 'i>>),
}

pub(crate) fn partial<'e, 'i>(
    target: &'e Node,
    args: &'e [Option<Node>],
    context: &Context<'e, 'i>,
    offset: usize,
) -> Result<Value<'e, 'i>, Error> {
    // Partial application evaluates bound arguments before resolving its target.
    let mut arguments = Vec::with_capacity(args.len());
    for arg in args {
        arguments.push(match arg {
            Some(arg) => Argument::Value(crate::retain::materialize(arg, context)?),
            None => Argument::Hole,
        });
    }
    let Some(Value::Function(target)) = crate::retain::materialize(target, context)? else {
        return Err(type_error(offset));
    };
    let count = match target.kind {
        FunctionKind::Builtin(builtin) => builtin.partial_arity(offset)?,
        _ => arity(&target),
    };
    arguments.truncate(count);
    arguments.resize_with(count, || Argument::Value(None));
    let target = if let FunctionKind::Partial {
        target: original,
        arguments: bound,
    } = &target.kind
    {
        let mut supplied = arguments.into_iter();
        arguments = bound
            .iter()
            .map(|arg| match arg {
                Argument::Hole => supplied.next().unwrap(),
                Argument::Value(_) => arg.clone(),
            })
            .collect();
        original.clone()
    } else {
        target
    };
    Ok(Value::Function(Rc::new(Function {
        kind: FunctionKind::Partial {
            target,
            arguments: arguments.into_boxed_slice(),
        },
    })))
}

pub(crate) fn chain<'e, 'i>(
    left: &'e Node,
    right: &'e Node,
    context: &Context<'e, 'i>,
    offset: usize,
) -> Result<Operand<'e, 'i>, Error> {
    use super::arguments::Arguments;
    let left = crate::retain::materialize(left, context)?;
    if let Kind::Call(target, args) = &right.kind {
        let target = crate::retain::materialize(target, context)?;
        let mut arguments = Arguments::new(args.len() + 1);
        arguments.push(left);
        for arg in args {
            arguments.push(crate::retain::materialize(arg, context)?);
        }
        let Some(Value::Function(function)) = target else {
            return Err(type_error(offset));
        };
        return invoke(&function, arguments.as_slice(), context, offset);
    }
    // Grouped calls are evaluated normally. Bare function chaining invokes with
    // a null focus in the reference; a pair of functions creates composition.
    let Some(Value::Function(second)) = crate::retain::materialize(right, context)? else {
        return Err(type_error(offset));
    };
    if let Some(Value::Function(first)) = left {
        Ok(Operand::One(Value::Function(Rc::new(Function {
            kind: FunctionKind::Chain(first, second),
        }))))
    } else {
        invoke(
            &second,
            &[left],
            &Context {
                value: Value::Null,
                wrapped: false,
                scope: context.scope.clone(),
            },
            offset,
        )
    }
}
pub(super) fn partial_arguments<'e, 'i>(
    bound: &[Argument<'e, 'i>],
    arguments: &[Option<Value<'e, 'i>>],
) -> super::arguments::Arguments<'e, 'i> {
    let mut incoming = arguments.iter();
    let mut result = super::arguments::Arguments::new(bound.len());
    for arg in bound {
        result.push(match arg {
            Argument::Hole => incoming.next().cloned().flatten(),
            Argument::Value(value) => value.clone(),
        });
    }
    result
}

pub(super) fn apply_partial<'e, 'i>(
    target: &Function<'e, 'i>,
    bound: &[Argument<'e, 'i>],
    arguments: &[Option<Value<'e, 'i>>],
    context: &Context<'e, 'i>,
    offset: usize,
) -> Result<Operand<'e, 'i>, Error> {
    let supplied = partial_arguments(bound, arguments);
    let arguments = supplied.as_slice();
    if let FunctionKind::Builtin(builtin) = target.kind {
        context
            .scope
            .as_ref()
            .expect("lexical runtime")
            .call(offset, 1, || {
                builtin.partial_values(arguments, context, offset)
            })
    } else {
        super::invoke_checked(target, arguments, context, offset, false)
    }
}

pub(super) fn apply_chain<'e, 'i>(
    first: &Function<'e, 'i>,
    second: &Function<'e, 'i>,
    arguments: &[Option<Value<'e, 'i>>],
    context: &Context<'e, 'i>,
    offset: usize,
) -> Result<Operand<'e, 'i>, Error> {
    context
        .scope
        .as_ref()
        .expect("lexical runtime")
        .call(offset, 1, || {
            let context = Context {
                value: Value::Null,
                wrapped: false,
                scope: context.scope.clone(),
            };
            let value = invoke(
                first,
                &[arguments.first().cloned().flatten()],
                &context,
                offset,
            )?
            .normalize();
            let value = match value {
                Operand::Missing => None,
                Operand::One(value) => Some(value),
                Operand::Many(stream) => crate::retain::collect(|emit| {
                    stream.visit(|value| {
                        emit(value);
                        Ok(())
                    })
                })?,
            };
            invoke(second, &[value], &context, offset)
        })
}
