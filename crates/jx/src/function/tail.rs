use super::{Function, FunctionKind, arguments::Arguments};
use crate::{
    Error, Value,
    evaluate::Operand,
    expression::{Kind, Node},
    runtime::Scope,
    sequence::Context,
};
use std::rc::Rc;

pub(crate) struct Call<'e, 'i> {
    pub function: Rc<Function<'e, 'i>>,
    pub arguments: Arguments<'e, 'i>,
    pub offset: usize,
    pub validate: bool,
    pub null_focus: bool,
}
pub(crate) enum Outcome<'e, 'i> {
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

// Tail transitions reuse the common program backend and lexical retention rules.

pub(super) fn invoke<'e, 'i>(
    initial: &Function<'e, 'i>,
    arguments: &[Option<Value<'e, 'i>>],
    context: &Context<'e, 'i>,
    offset: usize,
    validate: bool,
    input: Option<&crate::execution::input::Input<'_, 'i, '_>>,
) -> Result<Operand<'e, 'i>, Error> {
    let scope = context.scope.as_ref().expect("lexical runtime");
    let mut pending: Option<Call<'e, 'i>> = None;
    let mut steps = 0;
    let max_tail_calls = scope.max_tail_calls();
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
        let (params, signature, body_depth, tail, focus, wrapped, frame) = match &function.kind {
            FunctionKind::Program {
                definition,
                focus,
                wrapped,
                frame,
            } => (
                &definition.params,
                &definition.signature,
                definition.body.call.depth,
                definition.body.call.tail,
                focus,
                wrapped,
                frame,
            ),
            _ => {
                if let Some(child) = child.take() {
                    child.release();
                }
                return super::invoke_checked(function, arguments, caller, offset, validate);
            }
        };
        // General compiled functions also enter this loop for the common call
        // ABI. A non-tail initial call does not consume the tail-call budget.
        if tail || pending.is_some() {
            if steps == max_tail_calls {
                return Err(crate::Error::new(
                    crate::ErrorKind::EvaluationLimit,
                    offset,
                    "tail-call execution exceeds one million iterations",
                ));
            }
            steps += 1;
        }
        let outcome = scope.call(offset, body_depth, || {
            let validated;
            let arguments = if validate && let Some(signature) = signature {
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
            child.bind_arguments(params, arguments);
            let local = Context {
                value: focus.clone(),
                wrapped: *wrapped,
                scope: Some(child.clone()),
            };
            match &function.kind {
                FunctionKind::Program { definition, .. } => {
                    definition.body.run_function(&local, arguments, input)
                }
                _ => unreachable!("validated lambda definition"),
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
