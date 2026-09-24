use crate::{
    Error, ErrorKind, Value, evaluate::Operand, expression::Node, json::string, sequence::Context,
    value::type_error,
};

pub(crate) fn array<'e, 'i>(
    items: &'e [Node],
    preserve: bool,
    context: &Context<'e, 'i>,
) -> Result<Value<'e, 'i>, Error> {
    let mut values = Vec::with_capacity(items.len());
    for item in items {
        if item.is_array_constructor() {
            if let Some(value) = materialize(item, context)? {
                values.push(value);
            }
            continue;
        }
        let start = values.len();
        visit(item, context, &mut |value| values.push(value))?;
        if values.len() == start + 1 {
            let value = values.pop().unwrap();
            if value.is_array() {
                values.extend(value.elements());
            } else if !matches!(value, Value::Undefined) {
                values.push(value);
            }
        }
    }
    Ok(Value::array(values, preserve))
}

pub(crate) fn object<'e, 'i>(
    pairs: &'e [(Node, Node)],
    context: &Context<'e, 'i>,
    offset: usize,
) -> Result<Value<'e, 'i>, Error> {
    struct Group<'e, 'i> {
        key: Value<'e, 'i>,
        pair: usize,
        first: Value<'e, 'i>,
        rest: Vec<Value<'e, 'i>>,
    }
    let mut groups: Vec<Group<'e, 'i>> = Vec::with_capacity(pairs.len());
    let mut add = |value: Value<'e, 'i>| -> Result<(), Error> {
        for (pair, (key, _)) in pairs.iter().enumerate() {
            let key = match key.run(&Context {
                value: value.clone(),
                wrapped: false,
            })? {
                Operand::Missing => continue,
                Operand::One(key) if key.string_body().is_some() => key,
                _ => return Err(type_error(offset)),
            };
            if let Some(group) = groups.iter_mut().find(|group| {
                string::units(group.key.string_body().unwrap())
                    .eq(string::units(key.string_body().unwrap()))
            }) {
                if group.pair != pair {
                    return Err(Error::new(
                        ErrorKind::DuplicateKey,
                        offset,
                        "duplicate object key from different members",
                    ));
                }
                group.rest.push(value.clone());
            } else {
                groups.push(Group {
                    key,
                    pair,
                    first: value.clone(),
                    rest: Vec::new(),
                });
            }
        }
        Ok(())
    };
    if !context.wrapped && context.value.is_array() {
        let mut empty = true;
        for item in context.value.elements() {
            empty = false;
            add(item)?;
        }
        if empty {
            add(Value::Undefined)?;
        }
    } else {
        add(context.value.clone())?;
    }
    let mut members = Vec::with_capacity(groups.len());
    for group in groups {
        let value = if group.rest.is_empty() {
            group.first
        } else {
            let mut items = Vec::with_capacity(1 + group.rest.len());
            // Group contexts use the same one-level append as the reference.
            for value in std::iter::once(group.first).chain(group.rest) {
                if value.is_array() {
                    items.extend(value.elements());
                } else if !matches!(value, Value::Undefined) {
                    items.push(value);
                }
            }
            Value::array(items, false)
        };
        let Some(value) = materialize(
            &pairs[group.pair].1,
            &Context {
                value,
                wrapped: false,
            },
        )?
        else {
            continue;
        };
        members.push((group.key, value));
    }
    Ok(Value::object(members))
}

// Construction is the retention boundary: stream directly into its storage.
fn visit<'e, 'i>(
    node: &'e Node,
    context: &Context<'e, 'i>,
    output: &mut dyn FnMut(Value<'e, 'i>),
) -> Result<(), Error> {
    let mut emit = |value| {
        output(value);
        Ok(())
    };
    let result = match node.stream(context) {
        Some(stream) => stream.walk(&mut emit),
        None => node.run(context)?.walk(&mut emit),
    };
    match result {
        Ok(()) => Ok(()),
        Err(crate::sequence::Halt::Evaluation(error)) => Err(error),
        Err(crate::sequence::Halt::Stop) => unreachable!("construction consumes its members"),
    }
}
fn materialize<'e, 'i>(
    node: &'e Node,
    context: &Context<'e, 'i>,
) -> Result<Option<Value<'e, 'i>>, Error> {
    let mut first = None;
    let mut values = Vec::new();
    visit(node, context, &mut |value| {
        if !values.is_empty() {
            values.push(value);
        } else if let Some(first) = first.take() {
            values.push(first);
            values.push(value);
        } else {
            first = Some(value);
        }
    })?;
    Ok(if !values.is_empty() {
        Some(Value::sequence(values))
    } else {
        first.filter(|value| !matches!(value, Value::Undefined))
    })
}
