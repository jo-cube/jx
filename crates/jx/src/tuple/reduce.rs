use super::*;

pub(super) fn evaluate<'e, 'i, B: Expression + 'e>(
    base: &'e B,
    pairs: &'e [(B, B)],
    context: &Context<'e, 'i>,
    offset: usize,
    evaluator: &impl Evaluator<'e, 'i, B>,
) -> Result<Value<'e, 'i>, Error> {
    let mut groups = crate::construct::Groups::new(pairs.len());
    match walk(
        base,
        context,
        &mut |row| {
            row.stage(context, |local| {
                groups.add_with(pairs.len(), Some(row.clone()), offset, |pair| {
                    evaluator.run(&pairs[pair].0, local)
                })
            })
            .map_err(Halt::Evaluation)
        },
        evaluator,
    ) {
        Err(Halt::Evaluation(error)) => return Err(error),
        Err(Halt::Stop) => unreachable!(),
        Ok(()) => {}
    }
    groups.finish_with(|(first, rest), pair| {
        let value_node = &pairs[pair].1;
        let first = first.unwrap();
        if first.object_context {
            let value = if rest.is_empty() {
                first.object()
            } else {
                append(std::iter::once(&first).chain(&rest).map(Row::object))
            };
            return materialize(
                value_node,
                &Context {
                    value,
                    wrapped: false,
                    scope: context.scope.clone(),
                },
                evaluator,
            );
        }
        let mut merged = first.clone();
        if !rest.is_empty() {
            let rows = || std::iter::once(&first).chain(&rest);
            merged.value = append(rows().map(|r| r.value.clone()));
            let mut bindings = Vec::new();
            for row in rows() {
                for (name, _) in row.bindings.iter() {
                    if bindings.iter().any(|(key, _)| key == name) {
                        continue;
                    }
                    let values = rows().filter_map(|row| {
                        row.bindings
                            .iter()
                            .find(|(key, _)| key == name)
                            .map(|(_, value)| value.clone())
                    });
                    bindings.push((*name, append(values)));
                }
            }
            merged.bindings = Rc::new(bindings);
        }
        merged.scoped(context, |local| materialize(value_node, local, evaluator))
    })
}
fn append<'e, 'i>(values: impl Iterator<Item = Value<'e, 'i>>) -> Value<'e, 'i> {
    let mut output = Vec::new();
    for value in values {
        if value.is_array() {
            output.extend(value.elements());
        } else if !matches!(value, Value::Undefined) {
            output.push(value);
        }
    }
    Value::array(output, false)
}
