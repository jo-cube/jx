use crate::retain::{materialize, visit};
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
    grouped(pairs, context, offset, |add| {
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
        Ok(())
    })
}

pub(crate) fn reduce<'e, 'i>(
    base: &'e Node,
    pairs: &'e [(Node, Node)],
    context: &Context<'e, 'i>,
    offset: usize,
) -> Result<Value<'e, 'i>, Error> {
    grouped(pairs, context, offset, |add| {
        let mut pending = None;
        let mut multiple = false;
        let mut error = None;
        visit(base, context, &mut |value| {
            if error.is_some() {
                return;
            }
            if let Some(first) = pending.take() {
                multiple = true;
                error = add(first).err();
            }
            if multiple {
                if error.is_none() {
                    error = add(value).err();
                }
            } else {
                pending = Some(value);
            }
        })?;
        if let Some(error) = error {
            return Err(error);
        }
        if !multiple {
            match pending {
                Some(value) if value.is_array() => {
                    let mut empty = true;
                    for item in value.elements() {
                        empty = false;
                        add(item)?;
                    }
                    if empty {
                        add(Value::Undefined)?;
                    }
                }
                value => add(value.unwrap_or(Value::Undefined))?,
            }
        }
        Ok(())
    })
}

fn grouped<'e, 'i>(
    pairs: &'e [(Node, Node)],
    context: &Context<'e, 'i>,
    offset: usize,
    input: impl FnOnce(&mut dyn FnMut(Value<'e, 'i>) -> Result<(), Error>) -> Result<(), Error>,
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
                scope: context.scope.clone(),
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
                if matches!(group.first, Value::Undefined) {
                    group.first = value.clone();
                } else if !matches!(value, Value::Undefined) {
                    group.rest.push(value.clone());
                }
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
    input(&mut add)?;
    groups.sort_by_key(|group| {
        crate::members::index(group.key.string_body().unwrap()).unwrap_or(u32::MAX)
    });
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
                scope: context.scope.clone(),
            },
        )?
        else {
            continue;
        };
        members.push((group.key, value));
    }
    Ok(Value::object(members))
}
