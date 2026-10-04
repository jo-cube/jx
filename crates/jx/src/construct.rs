mod groups;
use crate::retain::{materialize, visit};
use crate::{Error, Value, expression::Node, sequence::Context};
pub(crate) use groups::Groups;

pub(crate) fn array<'e, 'i>(
    items: &'e [Node],
    preserve: bool,
    context: &Context<'e, 'i>,
) -> Result<Value<'e, 'i>, Error> {
    array_with(
        items,
        preserve,
        |n| materialize(n, context),
        |n, emit| visit(n, context, emit),
    )
}
pub(crate) fn array_with<'e, 'i>(
    items: &'e [Node],
    preserve: bool,
    mut materialize: impl FnMut(&'e Node) -> Result<Option<Value<'e, 'i>>, Error>,
    mut visit: impl FnMut(&'e Node, &mut dyn FnMut(Value<'e, 'i>)) -> Result<(), Error>,
) -> Result<Value<'e, 'i>, Error> {
    let mut values = Vec::with_capacity(items.len());
    for item in items {
        if item.is_array_constructor() {
            if let Some(value) = materialize(item)? {
                values.push(value);
            }
            continue;
        }
        let start = values.len();
        visit(item, &mut |value| values.push(value))?;
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
    object_with(pairs, context, offset, materialize)
}
pub(crate) fn object_with<'e, 'i>(
    pairs: &'e [(Node, Node)],
    context: &Context<'e, 'i>,
    offset: usize,
    materialize: impl FnMut(&'e Node, &Context<'e, 'i>) -> Result<Option<Value<'e, 'i>>, Error>,
) -> Result<Value<'e, 'i>, Error> {
    grouped_with(
        pairs,
        context,
        offset,
        |add| {
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
        },
        materialize,
    )
}

pub(crate) fn reduce<'e, 'i>(
    base: &'e Node,
    pairs: &'e [(Node, Node)],
    context: &Context<'e, 'i>,
    offset: usize,
) -> Result<Value<'e, 'i>, Error> {
    if crate::tuple::active(base) {
        return crate::tuple::group(base, pairs, context, offset);
    }
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
    grouped_with(pairs, context, offset, input, materialize)
}
fn grouped_with<'e, 'i>(
    pairs: &'e [(Node, Node)],
    context: &Context<'e, 'i>,
    offset: usize,
    input: impl FnOnce(&mut dyn FnMut(Value<'e, 'i>) -> Result<(), Error>) -> Result<(), Error>,
    mut materialize: impl FnMut(&'e Node, &Context<'e, 'i>) -> Result<Option<Value<'e, 'i>>, Error>,
) -> Result<Value<'e, 'i>, Error> {
    let mut groups = Groups::new(pairs.len());
    input(&mut |value| {
        groups.add(
            pairs,
            &Context {
                value: value.clone(),
                wrapped: false,
                scope: context.scope.clone(),
            },
            (!matches!(value, Value::Undefined)).then_some(value),
            offset,
        )
    })?;
    groups.finish(pairs, |values, node| {
        let (first, rest) = values;
        let first = first.unwrap_or(Value::Undefined);
        let value = if rest.is_empty() {
            first
        } else {
            let mut items = Vec::with_capacity(1 + rest.len());
            for value in std::iter::once(first).chain(rest) {
                if value.is_array() {
                    items.extend(value.elements());
                } else if !matches!(value, Value::Undefined) {
                    items.push(value);
                }
            }
            Value::array(items, false)
        };
        materialize(
            node,
            &Context {
                value,
                wrapped: false,
                scope: context.scope.clone(),
            },
        )
    })
}
