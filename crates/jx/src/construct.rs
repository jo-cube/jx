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

struct Group<'e, 'i, T> {
    key: Value<'e, 'i>,
    pair: usize,
    first: Option<T>,
    rest: Vec<T>,
}
pub(crate) struct Groups<'e, 'i, T>(Vec<Group<'e, 'i, T>>);
impl<'e, 'i, T: Clone> Groups<'e, 'i, T> {
    pub fn new(capacity: usize) -> Self {
        Self(Vec::with_capacity(capacity))
    }
    pub fn add(
        &mut self,
        pairs: &'e [(Node, Node)],
        context: &Context<'e, 'i>,
        item: Option<T>,
        offset: usize,
    ) -> Result<(), Error> {
        for (pair, (key, _)) in pairs.iter().enumerate() {
            let key = match key.run(context)? {
                Operand::Missing => continue,
                Operand::One(key) if key.string_body().is_some() => key,
                _ => return Err(type_error(offset)),
            };
            if let Some(group) = self.0.iter_mut().find(|g| {
                string::units(g.key.string_body().unwrap())
                    .eq(string::units(key.string_body().unwrap()))
            }) {
                if group.pair != pair {
                    return Err(Error::new(
                        ErrorKind::DuplicateKey,
                        offset,
                        "duplicate object key from different members",
                    ));
                }
                if group.first.is_none() {
                    group.first = item.clone();
                } else if let Some(item) = &item {
                    group.rest.push(item.clone());
                }
            } else {
                self.0.push(Group {
                    key,
                    pair,
                    first: item.clone(),
                    rest: Vec::new(),
                });
            }
        }
        Ok(())
    }
    pub fn finish(
        mut self,
        pairs: &'e [(Node, Node)],
        mut evaluate: impl FnMut((Option<T>, Vec<T>), &'e Node) -> Result<Option<Value<'e, 'i>>, Error>,
    ) -> Result<Value<'e, 'i>, Error> {
        self.0.sort_by_key(|g| {
            crate::members::index(g.key.string_body().unwrap()).unwrap_or(u32::MAX)
        });
        let mut members = Vec::with_capacity(self.0.len());
        for group in self.0 {
            if let Some(value) = evaluate((group.first, group.rest), &pairs[group.pair].1)? {
                members.push((group.key, value));
            }
        }
        Ok(Value::object(members))
    }
}
