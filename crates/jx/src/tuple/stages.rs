use super::*;
use crate::expression::Op;

pub(super) fn pipeline<'e, 'i>(
    input: &mut Source<'_, 'e, 'i>,
    steps: &'e [Step],
    context: &Context<'e, 'i>,
    output: &mut Emit<'_, 'e, 'i>,
) -> Walk {
    let Some((step, rest)) = steps.split_first() else {
        return input(output);
    };
    let mut mapped = |emit: &mut Emit<'_, 'e, 'i>| {
        transform(input, |row| {
            row.scoped(context, |local| {
                let mut position = 0;
                let mut selected = |mut next: Row<'e, 'i>| {
                    if let Some(name) = step.bindings.as_ref().and_then(|b| b.focus.as_deref()) {
                        let value = std::mem::replace(&mut next.value, row.value.clone());
                        next.bind(name, value);
                    }
                    if let Some(bindings) = &step.bindings {
                        for name in &bindings.ancestors {
                            next.bind(name, local.value.clone());
                        }
                    }
                    let index = step.bindings.as_ref().and_then(|b| b.index.as_deref());
                    // The reference ignores a direct index annotation on tuple sorts.
                    if let Some(name) = index
                        .filter(|_| !matches!(&step.node.kind, Kind::Sort(base, _) if active(base)))
                    {
                        next.bind(name, Value::Number(position as f64));
                    }
                    position += 1;
                    emit(next)
                };
                if active(&step.node) {
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
                    );
                }
                View::Operand(&step.node.run(&Context {
                    wrapped: step.lookup,
                    ..local.clone()
                })?)
                .candidates(false, &mut |value| {
                    let mut next = row.clone();
                    next.value = value;
                    selected(next)
                })
            })
        })
    };
    let mut filtered = |emit: &mut Emit<'_, 'e, 'i>| filters(&mut mapped, step, 0, context, emit);
    pipeline(&mut filtered, rest, context, output)
}

fn filters<'e, 'i>(
    input: &mut Source<'_, 'e, 'i>,
    step: &'e Step,
    at: usize,
    context: &Context<'e, 'i>,
    output: &mut Emit<'_, 'e, 'i>,
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
    let mut selected = |emit: &mut Emit<'_, 'e, 'i>| filter(&mut indexed, predicate, context, emit);
    filters(&mut selected, step, at + 1, context, output)
}

fn filter<'e, 'i>(
    input: &mut Source<'_, 'e, 'i>,
    predicate: &'e Node,
    context: &Context<'e, 'i>,
    output: &mut Emit<'_, 'e, 'i>,
) -> Walk {
    // Boolean predicates and nonnegative literal positions need no sequence length.
    // Other predicates may index from the end. Retain rows once, never replay effects.
    if boolean(predicate) || matches!(predicate.kind, Kind::Number(n) if n >= 0.0) {
        let mut index = 0;
        transform(input, |row| {
            let result = select(&row, predicate, context, index, 0, output);
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
            select(row, predicate, context, index, rows.len(), output)?;
        }
        Ok(())
    }
}
fn select<'e, 'i>(
    row: &Row<'e, 'i>,
    predicate: &'e Node,
    context: &Context<'e, 'i>,
    index: usize,
    length: usize,
    output: &mut Emit<'_, 'e, 'i>,
) -> Walk {
    row.scoped(context, |context| {
        let value = predicate.run(context)?;
        crate::filter::select(&value, predicate.offset, index, || Ok(length), &mut || {
            output(row.clone())
        })
    })
}
fn boolean(node: &Node) -> bool {
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

pub(super) fn postfilters<'e, 'i>(
    input: &mut Source<'_, 'e, 'i>,
    predicates: &'e [Node],
    context: &Context<'e, 'i>,
    output: &mut Emit<'_, 'e, 'i>,
) -> Walk {
    let Some((first, rest)) = predicates.split_first() else {
        return input(output);
    };
    let mut selected = |emit: &mut Emit<'_, 'e, 'i>| filter(input, first, context, emit);
    postfilters(&mut selected, rest, context, output)
}
