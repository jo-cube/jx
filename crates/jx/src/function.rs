use crate::builtin::Builtin;
use crate::{
    Error, Value, evaluate::Operand, expression::Node, sequence::Context, value::type_error,
};
use std::rc::Rc;

/// Opaque JSONata function value. It can be called inside its evaluation, but has
/// no JSON encoding or public host invocation API.
#[derive(Debug)]
pub struct Function<'e, 'i> {
    pub(crate) kind: FunctionKind<'e, 'i>,
}
#[derive(Debug)]
pub(crate) enum FunctionKind<'e, 'i> {
    Builtin(Builtin),
    Lambda {
        params: &'e [Box<str>],
        body: &'e Node,
        focus: Value<'e, 'i>,
        wrapped: bool,
        frame: usize,
    },
}
impl<'e, 'i> Function<'e, 'i> {
    pub(crate) fn builtin(builtin: Builtin) -> Value<'e, 'i> {
        Value::Function(Rc::new(Self {
            kind: FunctionKind::Builtin(builtin),
        }))
    }
    pub(crate) fn lambda(
        params: &'e [Box<str>],
        body: &'e Node,
        context: &Context<'e, 'i>,
    ) -> Value<'e, 'i> {
        Value::Function(Rc::new(Self {
            kind: FunctionKind::Lambda {
                params,
                body,
                focus: context.value.clone(),
                wrapped: context.wrapped,
                frame: context.scope.as_ref().expect("lexical runtime").capture(),
            },
        }))
    }
}

pub(crate) fn call<'e, 'i>(
    target: &'e Node,
    args: &'e [Node],
    context: &Context<'e, 'i>,
    offset: usize,
) -> Result<Operand<'e, 'i>, Error> {
    let target = crate::retain::materialize(target, context)?;
    let mut arguments = Vec::with_capacity(args.len());
    for arg in args {
        arguments.push(crate::retain::materialize(arg, context)?);
    }
    let Some(Value::Function(function)) = target else {
        return Err(type_error(offset));
    };
    match &function.kind {
        FunctionKind::Builtin(builtin @ Builtin::Deferred(_)) => builtin.value(None, offset),
        FunctionKind::Builtin(builtin) => {
            if arguments.is_empty() && matches!(builtin, Builtin::Boolean | Builtin::Not) {
                return builtin.value(Some(context.value.clone()), offset);
            }
            if arguments.len() != 1 {
                return Err(type_error(offset));
            }
            builtin.value(arguments.pop().unwrap(), offset)
        }
        FunctionKind::Lambda {
            params,
            body,
            focus,
            wrapped,
            frame,
        } => {
            let scope = context.scope.as_ref().expect("lexical runtime");
            scope.call(offset, body.depth, || {
                let child = scope.child(*frame);
                for (index, param) in params.iter().enumerate() {
                    child.bind(
                        param,
                        arguments
                            .get(index)
                            .cloned()
                            .flatten()
                            .unwrap_or(Value::Undefined),
                    );
                }
                let context = Context {
                    value: focus.clone(),
                    wrapped: *wrapped,
                    scope: Some(child.clone()),
                };
                let result = crate::retain::materialize(body, &context);
                child.release();
                result.map(|value| value.map_or(Operand::Missing, Operand::One))
            })
        }
    }
}
