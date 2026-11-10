//! A conservative input-use proof over the existing lowered semantic regions.
use super::{Operation, Program};
use crate::{InputDependency, builtin::Builtin};

pub(super) fn classify(program: &Program) -> InputDependency {
    if independent(program, false) {
        InputDependency::Independent
    } else {
        InputDependency::MayRequireInput
    }
}

fn independent(program: &Program, independent_focus: bool) -> bool {
    program
        .code
        .iter()
        .all(|i| operation(&i.kind, independent_focus))
}

fn operation(op: &Operation, independent_focus: bool) -> bool {
    // If every operation passes, register values and lexical writes in this
    // region depend only on bindings/constants or an independent child focus.
    match op {
        Operation::Focus | Operation::Traverse(_) => independent_focus,
        Operation::Read(name) | Operation::Bind(name, _) => {
            name.as_ref() != "$" && !name.starts_with('\0')
        }
        Operation::EnterScope
        | Operation::LeaveScope
        | Operation::Argument(_)
        | Operation::Outer(..)
        | Operation::Literal(_)
        | Operation::Field(..)
        | Operation::Copy(_)
        | Operation::Acquire(..)
        | Operation::Materialize(_)
        | Operation::Negate(_)
        | Operation::Truth(_)
        | Operation::Accept(_)
        | Operation::Binary(..)
        | Operation::Range(..)
        | Operation::Lookup(..)
        | Operation::Object(_)
        | Operation::Array(..)
        | Operation::Keep(..)
        | Operation::JumpFalse(..)
        | Operation::Jump(_) => true,
        Operation::Builtin(Builtin::Runtime(_), ..) => false,
        Operation::Builtin(Builtin::Library(library), ..) if library.calls_functions() => false,
        Operation::Builtin(builtin, args, _) => {
            independent_focus || !builtin.contextual(args.len())
        }
        Operation::Formatted(function, args, _) => {
            independent_focus || !function.contextual(args.len())
        }
        Operation::Construct(body) => independent(body, independent_focus),
        Operation::Path(path) => {
            independent(&path.body, true) && path.predicates.iter().all(|p| independent(p, true))
        }
        Operation::Navigate(navigation) => {
            // An unwrapped first stage can iterate the enclosing focus even
            // when its body does not read it. Absolute heads avoid that edge.
            (independent_focus || navigation.variable || navigation.array_focus)
                && navigation.steps.iter().enumerate().all(|(index, step)| {
                    independent(&step.body, independent_focus || index > 0)
                        && step.predicates.iter().all(|p| independent(&p.body, true))
                })
        }
        Operation::Select(_, predicates) => predicates.iter().all(|p| independent(&p.body, true)),
        Operation::Group(source, pairs) => {
            (source.is_some() || independent_focus)
                && pairs
                    .iter()
                    .all(|(key, value)| independent(key, true) && independent(value, true))
        }
        Operation::Order(_, terms) => terms.iter().all(|(body, _, _)| independent(body, true)),
        // Optimized callbacks retain the enclosing focus; only arguments come from the collection.
        Operation::Each { body, .. } => independent(body, independent_focus),
        Operation::Reduce(_, producer, _) | Operation::Collect(producer) => {
            operation(producer, independent_focus)
        }
        // Closures, unknown/dynamic calls, correlated contexts and unreviewed
        // operations can observe hidden focus/root state. Never guess here.
        _ => false,
    }
}
