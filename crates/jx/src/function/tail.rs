use super::{Function, FunctionKind, arguments::Arguments};
use crate::{
    Error, Value,
    evaluate::Operand,
    expression::{Kind, Node},
    runtime::Scope,
    sequence::Context,
    value::type_error,
};
use std::rc::Rc;

pub(super) struct Call<'e, 'i> {
    pub function: Rc<Function<'e, 'i>>,
    pub arguments: Arguments<'e, 'i>,
    pub offset: usize,
    pub validate: bool,
    pub null_focus: bool,
}
pub(super) enum Outcome<'e, 'i> {
    Done(Option<Value<'e, 'i>>),
    Call(Call<'e, 'i>),
}

pub(crate) fn possible(node: &Node) -> bool {
    match &node.kind {
        Kind::Call(..) | Kind::Eval(..) => true,
        Kind::Builtin(builtin, args) => builtin.contextual(args.len()),
        Kind::Formatted(call) => call.function.contextual(call.args.len()),
        Kind::Group(body) => possible(body),
        Kind::Block(nodes) => nodes.last().is_some_and(possible),
        Kind::Conditional(_, yes, no) => possible(yes) || no.as_deref().is_some_and(possible),
        _ => false,
    }
}

// Evaluate only tail control flow here. Expressions, tests, arguments and
// terminal results continue through the ordinary evaluator and retention rules.
fn evaluate<'e, 'i>(
    node: &'e Node,
    context: &Context<'e, 'i>,
    caller: &Context<'e, 'i>,
) -> Result<Outcome<'e, 'i>, Error> {
    match &node.kind {
        Kind::Group(body) => block(std::slice::from_ref(body.as_ref()), context, caller),
        Kind::Block(nodes) => block(nodes, context, caller),
        Kind::Conditional(test, yes, no) => {
            if test.run(context)?.truth(node.offset)? {
                evaluate(yes, context, caller)
            } else if let Some(no) = no {
                evaluate(no, context, caller)
            } else {
                Ok(Outcome::Done(None))
            }
        }
        Kind::Call(target, nodes) => {
            let target = crate::retain::materialize(target, context)?;
            let arguments = Arguments::evaluate(nodes, context)?;
            let Some(Value::Function(function)) = target else {
                return Err(type_error(node.offset));
            };
            Ok(Outcome::Call(Call {
                function,
                arguments,
                offset: node.offset,
                validate: true,
                null_focus: false,
            }))
        }
        Kind::Eval(call) => {
            let target = crate::dynamic::Call::target(context);
            let arguments = Arguments::evaluate(&call.args, context)?;
            let target = crate::dynamic::Call::resolve(target, node.offset)?;
            if let Some(function) = target {
                Ok(Outcome::Call(Call {
                    function,
                    arguments,
                    offset: node.offset,
                    validate: true,
                    null_focus: false,
                }))
            } else {
                super::retained(call.values(arguments.as_slice(), caller, node.offset)?)
                    .map(Outcome::Done)
            }
        }
        Kind::Builtin(builtin, args) => {
            super::retained(builtin.evaluate_in(args, context, caller, node.offset)?)
                .map(Outcome::Done)
        }
        Kind::Formatted(call) => {
            super::retained(call.evaluate_in(context, caller, node.offset)?).map(Outcome::Done)
        }
        _ => crate::retain::materialize(node, context).map(Outcome::Done),
    }
}
fn block<'e, 'i>(
    nodes: &'e [Node],
    context: &Context<'e, 'i>,
    caller: &Context<'e, 'i>,
) -> Result<Outcome<'e, 'i>, Error> {
    let scope = context.scope.as_ref().expect("lexical runtime");
    let child = scope.child(scope.frame);
    let context = Context {
        scope: Some(child.clone()),
        ..context.clone()
    };
    let result = (|| {
        let Some((last, prefix)) = nodes.split_last() else {
            return Ok(Outcome::Done(None));
        };
        for node in prefix {
            crate::retain::materialize(node, &context)?;
        }
        evaluate(last, &context, caller)
    })();
    child.release();
    result
}

pub(super) fn invoke<'e, 'i>(
    initial: &Function<'e, 'i>,
    arguments: &[Option<Value<'e, 'i>>],
    context: &Context<'e, 'i>,
    offset: usize,
    validate: bool,
) -> Result<Operand<'e, 'i>, Error> {
    let scope = context.scope.as_ref().expect("lexical runtime");
    let mut pending: Option<Call<'e, 'i>> = None;
    let mut steps = 0;
    let mut child: Option<Scope<'e, 'i>> = None;
    let result = (|| loop {
        let null_focus = pending.as_ref().is_some_and(|p| p.null_focus);
        let null_context;
        let caller = if null_focus {
            null_context = Context {
                value: Value::Null,
                wrapped: false,
                scope: context.scope.clone(),
            };
            &null_context
        } else {
            context
        };
        let (function, arguments, offset, validate) = match &pending {
            Some(call) => (
                call.function.as_ref(),
                call.arguments.as_slice(),
                call.offset,
                call.validate,
            ),
            None => (initial, arguments, offset, validate),
        };
        if let FunctionKind::Partial {
            target,
            arguments: bound,
        } = &function.kind
        {
            pending = Some(Call {
                function: target.clone(),
                arguments: super::composition::partial_arguments(bound, arguments),
                offset,
                validate: false,
                null_focus,
            });
            continue;
        }
        if let FunctionKind::Chain(first, second) = &function.kind {
            let local = Context {
                value: Value::Null,
                wrapped: false,
                scope: context.scope.clone(),
            };
            let value = super::invoke(
                first,
                &[arguments.first().cloned().flatten()],
                &local,
                offset,
            )?
            .normalize();
            let mut arguments = Arguments::new(1);
            arguments.push(super::retained(value)?);
            pending = Some(Call {
                function: second.clone(),
                arguments,
                offset,
                validate: true,
                null_focus: true,
            });
            continue;
        }
        let FunctionKind::Lambda {
            definition,
            focus,
            wrapped,
            frame,
        } = &function.kind
        else {
            if let Some(child) = child.take() {
                child.release();
            }
            return super::invoke_checked(function, arguments, caller, offset, validate);
        };
        if steps == 1_000_000 {
            return Err(crate::Error::new(
                crate::ErrorKind::EvaluationLimit,
                offset,
                "tail-call execution exceeds one million iterations",
            ));
        }
        steps += 1;
        let outcome = scope.call(offset, definition.body.depth, || {
            let validated;
            let arguments = if validate && let Some(signature) = &definition.signature {
                validated = signature.validate(arguments, &caller.value, offset)?;
                validated.as_slice()
            } else {
                arguments
            };
            if !child.as_ref().is_some_and(|child| child.reset(*frame)) {
                if let Some(child) = child.take() {
                    child.release();
                }
                child = Some(scope.child(*frame));
            }
            let child = child.as_ref().unwrap();
            child.bind_arguments(&definition.params, arguments);
            let local = Context {
                value: focus.clone(),
                wrapped: *wrapped,
                scope: Some(child.clone()),
            };
            if definition.tail {
                evaluate(&definition.body, &local, caller)
            } else {
                crate::retain::materialize(&definition.body, &local).map(Outcome::Done)
            }
        })?;
        match outcome {
            Outcome::Done(value) => return Ok(value.map_or(Operand::Missing, Operand::One)),
            Outcome::Call(mut call) => {
                call.null_focus = null_focus;
                pending = Some(call);
            }
        }
    })();
    if let Some(child) = child {
        child.release();
    }
    result
}
