pub(crate) mod acquire;
mod arguments;
pub(crate) mod composition;
mod signature;
mod tail;
use crate::builtin::Builtin;
use crate::{
    Error, Value,
    evaluate::Operand,
    expression::{Kind, Node},
    sequence::Context,
    value::type_error,
};
pub(crate) use arguments::Arguments;
pub(crate) use composition::{chain, partial};
pub(crate) use signature::Signature;
use std::rc::Rc;
pub(crate) use tail::possible as has_tail_calls;

/// Opaque JSONata function value. It can be called inside its evaluation, but has
/// no JSON encoding or public host invocation API.
#[derive(Debug)]
pub struct Function<'e, 'i> {
    pub(crate) kind: FunctionKind<'e, 'i>,
}
#[derive(Clone, Debug)]
pub(crate) struct Definition {
    pub params: Box<[Box<str>]>,
    pub body: Node,
    pub signature: Option<Signature>,
    pub tail: bool,
    pub plan: Option<std::sync::Arc<crate::plan::Callback>>,
}
#[derive(Debug)]
pub(crate) enum FunctionKind<'e, 'i> {
    Builtin(Builtin),
    Dynamic(Box<crate::dynamic::Callable<'e, 'i>>),
    Matcher(Rc<crate::matcher::State<'e>>),
    MatchNext(Rc<crate::matcher::Continuation<'e, 'i>>),
    Transform {
        definition: &'e crate::transform::Definition,
        frame: usize,
    },
    Partial {
        target: Rc<Function<'e, 'i>>,
        arguments: Box<[composition::Argument<'e, 'i>]>,
    },
    Chain(Rc<Function<'e, 'i>>, Rc<Function<'e, 'i>>),
    Lambda {
        definition: &'e Definition,
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
    pub(crate) fn lambda(definition: &'e Definition, context: &Context<'e, 'i>) -> Value<'e, 'i> {
        Value::Function(Rc::new(Self {
            kind: FunctionKind::Lambda {
                definition,
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
    if let Kind::Lambda(definition) = &target.kind {
        let arguments = arguments::Arguments::evaluate(args, context)?;
        return invoke_literal(definition, arguments.as_slice(), context, offset);
    }
    let target = crate::retain::materialize(target, context)?;
    let arguments = arguments::Arguments::evaluate(args, context)?;
    let Some(Value::Function(function)) = target else {
        return Err(type_error(offset));
    };
    invoke(&function, arguments.as_slice(), context, offset)
}

pub(super) fn invoke_literal<'e, 'i>(
    definition: &'e Definition,
    arguments: &[Option<Value<'e, 'i>>],
    context: &Context<'e, 'i>,
    offset: usize,
) -> Result<Operand<'e, 'i>, Error> {
    // Escaping body closures mark their own ancestry. An immediate literal
    // callee only borrows its definition and does not pin the caller frame.
    let function = Function {
        kind: FunctionKind::Lambda {
            definition,
            focus: context.value.clone(),
            wrapped: context.wrapped,
            frame: context.scope.as_ref().expect("lexical runtime").frame,
        },
    };
    invoke(&function, arguments, context, offset)
}

pub(crate) fn arity(function: &Function<'_, '_>) -> usize {
    match &function.kind {
        FunctionKind::Builtin(builtin) => builtin.arity(),
        FunctionKind::Dynamic(callable) => callable.arity(),
        FunctionKind::Matcher(_) => 2,
        FunctionKind::MatchNext(_) => 0,
        FunctionKind::Lambda { definition, .. } => definition.params.len(),
        FunctionKind::Partial { arguments, .. } => arguments
            .iter()
            .filter(|arg| matches!(arg, composition::Argument::Hole))
            .count(),
        FunctionKind::Chain(..) | FunctionKind::Transform { .. } => 1,
    }
}

pub(crate) fn invoke<'e, 'i>(
    function: &Function<'e, 'i>,
    arguments: &[Option<Value<'e, 'i>>],
    context: &Context<'e, 'i>,
    offset: usize,
) -> Result<Operand<'e, 'i>, Error> {
    invoke_checked(function, arguments, context, offset, true)
}
pub(crate) fn invoke_checked<'e, 'i>(
    function: &Function<'e, 'i>,
    arguments: &[Option<Value<'e, 'i>>],
    context: &Context<'e, 'i>,
    offset: usize,
    validate: bool,
) -> Result<Operand<'e, 'i>, Error> {
    match &function.kind {
        FunctionKind::Dynamic(callable) => callable.invoke(arguments, context, offset, validate),
        FunctionKind::Builtin(builtin) => {
            if validate {
                builtin.values(arguments, context, offset)
            } else {
                builtin.partial_values(arguments, context, offset)
            }
        }
        FunctionKind::Matcher(state) => crate::matcher::invoke(state, arguments, offset),
        FunctionKind::MatchNext(next) => next.invoke(offset),
        FunctionKind::Transform { definition, frame } => {
            crate::transform::invoke(definition, *frame, arguments, context, offset)
        }
        FunctionKind::Partial {
            target,
            arguments: bound,
        } => composition::apply_partial(target, bound, arguments, context, offset),
        FunctionKind::Chain(first, second) => {
            composition::apply_chain(first, second, arguments, context, offset)
        }
        FunctionKind::Lambda {
            definition,
            focus,
            wrapped,
            frame,
        } => {
            if definition.tail {
                return tail::invoke(function, arguments, context, offset, validate);
            }
            let scope = context.scope.as_ref().expect("lexical runtime");
            let validated;
            let arguments = if validate && let Some(signature) = &definition.signature {
                validated = signature.validate(arguments, &context.value, offset)?;
                validated.as_slice()
            } else {
                arguments
            };
            let params = &definition.params;
            let body = &definition.body;
            scope.call(offset, body.depth, || {
                if let Some(plan) = &definition.plan
                    && let Some(result) = plan.run(arguments, focus, *wrapped)
                {
                    return Ok(result);
                }
                let child = scope.child(*frame);
                child.bind_arguments(params, arguments);
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

pub(crate) fn retained<'e, 'i>(value: Operand<'e, 'i>) -> Result<Option<Value<'e, 'i>>, Error> {
    match value {
        Operand::Missing => Ok(None),
        Operand::One(value) => Ok(Some(value)),
        Operand::Many(stream) => crate::retain::collect(|emit| {
            stream.visit(|value| {
                emit(value);
                Ok(())
            })
        }),
    }
}
