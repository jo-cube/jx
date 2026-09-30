mod composition;
use crate::builtin::Builtin;
use crate::{
    Error, Value, evaluate::Operand, expression::Node, sequence::Context, value::type_error,
};
pub(crate) use composition::{chain, partial};
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
    Matcher(Rc<crate::matcher::State<'e>>),
    MatchNext(Rc<crate::matcher::Continuation<'e, 'i>>),
    Partial {
        target: Rc<Function<'e, 'i>>,
        arguments: Box<[composition::Argument<'e, 'i>]>,
    },
    Chain(Rc<Function<'e, 'i>>, Rc<Function<'e, 'i>>),
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
    invoke(&function, &arguments, context, offset)
}

pub(crate) fn arity(function: &Function<'_, '_>) -> usize {
    match &function.kind {
        FunctionKind::Builtin(builtin) => builtin.arity(),
        FunctionKind::Matcher(_) => 2,
        FunctionKind::MatchNext(_) => 0,
        FunctionKind::Lambda { params, .. } => params.len(),
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
    match &function.kind {
        FunctionKind::Builtin(builtin) => builtin.values(arguments, context, offset),
        FunctionKind::Matcher(state) => crate::matcher::invoke(state, arguments, offset),
        FunctionKind::MatchNext(next) => next.invoke(offset),
        FunctionKind::Partial {
            target,
            arguments: bound,
        } => composition::apply_partial(target, bound, arguments, context, offset),
        FunctionKind::Chain(first, second) => {
            composition::apply_chain(first, second, arguments, context, offset)
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
