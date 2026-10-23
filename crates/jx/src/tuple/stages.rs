use super::*;
use crate::expression::Op;

pub(super) fn pipeline<'e, 'i, B: Expression + 'e>(
    input: &mut Source<'_, 'e, 'i>,
    steps: &'e [Step<B>],
    context: &Context<'e, 'i>,
    output: &mut Emit<'_, 'e, 'i>,
    evaluator: &impl Evaluator<'e, 'i, B>,
) -> Walk {
    let Some((step, rest)) = steps.split_first() else {
        return input(output);
    };
    let mut mapped = |emit: &mut Emit<'_, 'e, 'i>| {
        transform(input, |row| {
            row.scoped(context, |local| {
                let prepared = step
                    .bindings
                    .as_ref()
                    .filter(|b| !b.ancestors.is_empty())
                    .and_then(|_| evaluator.parent(&step.node, &local.value));
                // The escaping parent needs portable metadata; the producing
                // body still uses its original active input view and focus.
                let mut position = 0;
                let mut selected = |mut next: Row<'e, 'i>| {
                    if let Some(scope) = local.scope.as_ref().filter(|s| s.controlled()) {
                        scope.item(step.node.offset())?;
                    }
                    if let Some(name) = step.bindings.as_ref().and_then(|b| b.focus.as_deref()) {
                        let value = std::mem::replace(&mut next.value, row.value.clone());
                        next.bind(name, value);
                    }
                    if let Some(bindings) = &step.bindings {
                        for name in &bindings.ancestors {
                            next.bind(name, prepared.as_ref().unwrap_or(&local.value).clone());
                        }
                    }
                    let index = step.bindings.as_ref().and_then(|b| b.index.as_deref());
                    // The reference ignores a direct index annotation on tuple sorts.
                    if let Some(name) = index.filter(|_| !step.node.sorted_tuple()) {
                        next.bind(name, Value::Number(position as f64));
                    }
                    position += 1;
                    emit(next)
                };
                if correlated(&step.node) {
                    return walk(
                        &step.node,
                        &Context {
                            wrapped: context.wrapped,
                            ..local.clone()
                        },
                        &mut |mut nested| {
                            for (name, value) in row.bindings.iter() {
                                if !nested.bindings.iter().any(|(key, _)| key == name) {
                                    nested.bind(name, value.clone());
                                }
                            }
                            selected(nested)
                        },
                        evaluator,
                    );
                }
                View::Operand(&evaluator.run(
                    &step.node,
                    &Context {
                        wrapped: step.lookup,
                        ..local.clone()
                    },
                )?)
                .candidates(false, &mut |value| {
                    let mut next = row.clone();
                    next.value = value;
                    next.object_context = false;
                    selected(next)
                })
            })
        })
    };
    let mut filtered =
        |emit: &mut Emit<'_, 'e, 'i>| filters(&mut mapped, step, 0, context, emit, evaluator);
    pipeline(&mut filtered, rest, context, output, evaluator)
}

fn filters<'e, 'i, B: Expression + 'e>(
    input: &mut Source<'_, 'e, 'i>,
    step: &'e Step<B>,
    at: usize,
    context: &Context<'e, 'i>,
    output: &mut Emit<'_, 'e, 'i>,
    evaluator: &impl Evaluator<'e, 'i, B>,
) -> Walk {
    let mut indexed = |emit: &mut Emit<'_, 'e, 'i>| {
        let mut position = 0;
        transform(input, |mut row| {
            if let Some(bindings) = &step.bindings {
                for (_, name) in bindings.indices.iter().filter(|(after, _)| *after == at) {
                    row.bind(name, Value::Number(position as f64));
                }
            }
            position += 1;
            emit(row)
        })
    };
    let Some(predicate) = step.predicates.get(at) else {
        return indexed(output);
    };
    let mut selected =
        |emit: &mut Emit<'_, 'e, 'i>| filter(&mut indexed, predicate, context, emit, evaluator);
    filters(&mut selected, step, at + 1, context, output, evaluator)
}

fn filter<'e, 'i, B: Expression + 'e>(
    input: &mut Source<'_, 'e, 'i>,
    predicate: &'e B,
    context: &Context<'e, 'i>,
    output: &mut Emit<'_, 'e, 'i>,
    evaluator: &impl Evaluator<'e, 'i, B>,
) -> Walk {
    // Boolean predicates and nonnegative literal positions need no sequence length.
    // Other predicates may index from the end. Retain rows once, never replay effects.
    if predicate.boolean() || predicate.position().is_some_and(|n| n >= 0.0) {
        let mut index = 0;
        transform(input, |row| {
            let result = select(&row, predicate, context, (index, 0), output, evaluator);
            index += 1;
            result
        })
    } else {
        let mut rows = Vec::new();
        input(&mut |row| {
            rows.push(row);
            Ok(())
        })?;
        for (index, row) in rows.iter().enumerate() {
            select(
                row,
                predicate,
                context,
                (index, rows.len()),
                output,
                evaluator,
            )?;
        }
        Ok(())
    }
}
fn select<'e, 'i, B: Expression + 'e>(
    row: &Row<'e, 'i>,
    predicate: &'e B,
    context: &Context<'e, 'i>,
    position: (usize, usize),
    output: &mut Emit<'_, 'e, 'i>,
    evaluator: &impl Evaluator<'e, 'i, B>,
) -> Walk {
    let (index, length) = position;
    row.stage(context, |context| {
        let value = evaluator.run(predicate, context)?;
        crate::filter::select(
            &value,
            predicate.offset(),
            index,
            || Ok(length),
            &mut || output(row.clone()),
        )
    })
}
pub(super) fn boolean(node: &Node) -> bool {
    match &node.kind {
        Kind::Boolean(_) => true,
        Kind::Binary(op, _, _) => matches!(
            op,
            Op::Equal
                | Op::NotEqual
                | Op::Less
                | Op::LessEqual
                | Op::Greater
                | Op::GreaterEqual
                | Op::And
                | Op::Or
                | Op::In
        ),
        Kind::Group(n) => boolean(n),
        Kind::Plan(plan) => boolean(&plan.source),
        Kind::Builtin(b, _) => matches!(
            b,
            crate::builtin::Builtin::Boolean
                | crate::builtin::Builtin::Not
                | crate::builtin::Builtin::Exists
        ),
        _ => false,
    }
}

pub(super) fn postfilters<'e, 'i, B: Expression + 'e>(
    input: &mut Source<'_, 'e, 'i>,
    predicates: &'e [B],
    context: &Context<'e, 'i>,
    output: &mut Emit<'_, 'e, 'i>,
    evaluator: &impl Evaluator<'e, 'i, B>,
) -> Walk {
    let Some((first, rest)) = predicates.split_first() else {
        return input(output);
    };
    let mut selected = |emit: &mut Emit<'_, 'e, 'i>| filter(input, first, context, emit, evaluator);
    postfilters(&mut selected, rest, context, output, evaluator)
}
