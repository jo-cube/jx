mod arguments;
pub(crate) mod composition;
mod signature;
pub(crate) mod tail;
use crate::builtin::Builtin;
use crate::{Error, Value, evaluate::Operand, expression::Node, sequence::Context};
pub(crate) use arguments::Arguments;
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
}
#[derive(Debug)]
pub(crate) enum FunctionKind<'e, 'i> {
    Builtin(Builtin),
    Host(crate::HostFunction),
    Dynamic(Box<crate::dynamic::Callable<'e, 'i>>),
    Matcher(Rc<crate::matcher::State<'e>>),
    MatchNext(Rc<crate::matcher::Continuation<'e, 'i>>),

    Partial {
        target: Rc<Function<'e, 'i>>,
        arguments: Box<[composition::Argument<'e, 'i>]>,
    },
    Chain(Rc<Function<'e, 'i>>, Rc<Function<'e, 'i>>),

    ProgramTransform {
        definition: &'e crate::transform::Definition<crate::execution::Program>,
        frame: usize,
    },

    Program {
        definition: &'e crate::execution::call::Definition,
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
}

pub(crate) fn arity(function: &Function<'_, '_>) -> usize {
    match &function.kind {
        FunctionKind::Builtin(builtin) => builtin.arity(),

        FunctionKind::Program { definition, .. } => definition.params.len(),

        FunctionKind::ProgramTransform { .. } => 1,
        FunctionKind::Host(function) => function.arity,
        FunctionKind::Dynamic(callable) => callable.arity(),
        FunctionKind::Matcher(_) => 2,
        FunctionKind::MatchNext(_) => 0,

        FunctionKind::Partial { arguments, .. } => arguments
            .iter()
            .filter(|arg| matches!(arg, composition::Argument::Hole))
            .count(),
        FunctionKind::Chain(..) => 1,
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
    invoke_in(function, arguments, context, offset, validate, None)
}
pub(crate) fn invoke_in<'e, 'i>(
    function: &Function<'e, 'i>,
    arguments: &[Option<Value<'e, 'i>>],
    context: &Context<'e, 'i>,
    offset: usize,
    validate: bool,
    input: Option<&crate::execution::input::Input<'_, 'i, '_>>,
) -> Result<Operand<'e, 'i>, Error> {
    if matches!(
        function.kind,
        FunctionKind::Builtin(_) | FunctionKind::Matcher(_) | FunctionKind::MatchNext(_)
    ) && let Some(scope) = &context.scope
    {
        scope.checkpoint(offset)?;
    }
    match &function.kind {
        FunctionKind::Program { definition, .. } => {
            // Ordinary lambdas validate signatures before the call-depth guard;
            // tail transitions validate inside the trampoline. Preserve both
            // error phases while sharing its compiled-body execution loop.
            let validated;
            let (arguments, validate) = if !definition.body.call.tail
                && validate
                && let Some(signature) = &definition.signature
            {
                validated = signature.validate(arguments, &context.value, offset)?;
                (validated.as_slice(), false)
            } else {
                (arguments, validate)
            };
            context
                .scope
                .as_ref()
                .expect("lexical runtime")
                .evaluated(|| tail::invoke(function, arguments, context, offset, validate, input))
        }
        FunctionKind::Host(function) => function.invoke(arguments, context, offset, validate),
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

        FunctionKind::ProgramTransform { definition, frame } => {
            crate::transform::invoke(*definition, *frame, arguments, context, offset)
        }

        FunctionKind::Partial {
            target,
            arguments: bound,
        } => composition::apply_partial(target, bound, arguments, context, offset),
        FunctionKind::Chain(first, second) => {
            composition::apply_chain(first, second, arguments, context, offset)
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
