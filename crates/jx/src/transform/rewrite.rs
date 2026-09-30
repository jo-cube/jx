use super::update::{key_value, merge, remove};
use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Identity {
    owner: usize,
    storage: usize,
    kind: u8,
}
fn id(value: &Value<'_, '_>) -> Option<Identity> {
    Some(match value {
        Value::Raw(raw) if value.is_object() || value.is_array() => Identity {
            owner: 0,
            storage: raw.as_bytes().as_ptr() as usize,
            kind: 0,
        },
        Value::Array(a) => Identity {
            owner: 0,
            storage: Rc::as_ptr(a) as usize,
            kind: 1,
        },
        Value::Object(o) => Identity {
            owner: 0,
            storage: Rc::as_ptr(o) as usize,
            kind: 2,
        },
        Value::Constant(c) => Identity {
            owner: Rc::as_ptr(&c.identity) as usize,
            storage: std::ptr::from_ref(c.data) as usize,
            kind: 3,
        },
        Value::Copied(c) => identity(c),
        _ => return None,
    })
}
pub(super) fn identity(value: &CopiedValue<'_, '_>) -> Identity {
    let source = id(&value.source).expect("copy source container");
    Identity {
        owner: Rc::as_ptr(&value.identity) as usize,
        storage: source.storage,
        kind: source.kind + 4,
    }
}
struct Changes<'e, 'i> {
    values: BTreeMap<Identity, Value<'e, 'i>>,
    raw: BTreeMap<(usize, usize), usize>,
}
impl<'e, 'i> Changes<'e, 'i> {
    fn insert(&mut self, target: &Value<'e, 'i>, value: Value<'e, 'i>) {
        let Some(identity) = id(target) else { return };
        if let Value::Copied(c) = target
            && let Value::Raw(raw) = c.source
        {
            self.raw.insert(
                (identity.owner, identity.storage),
                identity.storage + raw.as_bytes().len(),
            );
        }
        self.values.insert(identity, value);
    }
    fn rewrite(&self, value: &Value<'e, 'i>) -> Option<Value<'e, 'i>> {
        if self.values.is_empty() {
            return None;
        }
        let identity = id(value)?;
        if let Some(replaced) = self.values.get(&identity) {
            return Some(self.rewrite(replaced).unwrap_or_else(|| replaced.clone()));
        }
        if let Value::Copied(c) = value
            && let Value::Raw(raw) = c.source
        {
            let end = identity.storage + raw.as_bytes().len();
            self.raw
                .range((identity.owner, identity.storage)..(identity.owner, end))
                .next()?;
        }
        if value.is_array() {
            let mut items = Vec::new();
            let mut changed = false;
            for item in value.elements() {
                if let Some(updated) = self.rewrite(&item) {
                    changed = true;
                    items.push(updated);
                } else {
                    items.push(item);
                }
            }
            changed.then(|| Value::array(items, false))
        } else {
            let mut members = Vec::new();
            let mut changed = false;
            for (key, item) in crate::members::entries(value) {
                let updated = self.rewrite(&item);
                changed |= updated.is_some();
                members.push((key_value(value, key), updated.unwrap_or(item)));
            }
            changed.then(|| Value::object(members))
        }
    }
}
pub(super) fn apply<'e, 'i>(
    definition: &'e Definition,
    value: Value<'e, 'i>,
    context: &Context<'e, 'i>,
) -> Result<Value<'e, 'i>, Error> {
    let selected = crate::retain::materialize(&definition.pattern, context)?;
    let Some(selected) = selected else {
        return Ok(value);
    };
    let matches: Vec<_> = if selected.is_array() {
        selected.elements().collect()
    } else {
        vec![selected]
    };
    let mut changes = Changes {
        values: BTreeMap::new(),
        raw: BTreeMap::new(),
    };
    for target in matches {
        if id(&target).is_some() && !belongs(&value, &target) {
            return Err(Error::new(
                crate::ErrorKind::UnsupportedExpression,
                definition.pattern.offset,
                "transform locations outside the cloned argument are deferred",
            ));
        }
        let mut current = changes.rewrite(&target).unwrap_or_else(|| target.clone());
        let local = Context {
            value: current.clone(),
            wrapped: false,
            ..context.clone()
        };
        let update = crate::retain::materialize(&definition.update, &local)?;
        if let Some(update) = update {
            if !update.is_object() {
                return Err(crate::value::type_error(definition.update.offset));
            }
            if function_value(&update) {
                return Err(Error::new(
                    crate::ErrorKind::UnsupportedExpression,
                    definition.update.offset,
                    "function-valued transform updates are deferred",
                ));
            }
            if update
                .members()
                .any(|(_, v)| contains(&v, &target) || contains(&v, &current))
            {
                return Err(Error::new(
                    crate::ErrorKind::UnsupportedExpression,
                    definition.update.offset,
                    "cyclic transform output is not JSON",
                ));
            }
            current = merge(&current, &update, definition.update.offset)?;
        }
        if let Some(delete) = &definition.delete {
            let local = Context {
                value: current.clone(),
                wrapped: false,
                ..context.clone()
            };
            if let Some(deletions) = crate::retain::materialize(delete, &local)? {
                let keys: Vec<_> = if deletions.is_array() {
                    deletions.elements().collect()
                } else {
                    vec![deletions]
                };
                if keys.iter().any(|key| key.string_body().is_none()) {
                    return Err(crate::value::type_error(delete.offset));
                }
                current = remove(&current, &keys, delete.offset)?;
            }
        }
        if id(&current) != id(&target) {
            changes.insert(&target, current);
        }
    }
    Ok(changes.rewrite(&value).unwrap_or(value))
}
fn belongs(value: &Value<'_, '_>, target: &Value<'_, '_>) -> bool {
    if let (Value::Copied(root), Value::Copied(target)) = (value, target) {
        return Rc::ptr_eq(&root.identity, &target.identity);
    }
    contains(value, target)
}
fn contains(value: &Value<'_, '_>, target: &Value<'_, '_>) -> bool {
    let Some(target_id) = id(target) else {
        return false;
    };
    if id(value) == Some(target_id) {
        return true;
    }
    if let Value::Raw(_) | Value::Constant(_) = value {
        return false;
    }
    if let Value::Copied(c) = value
        && identity(c).owner != target_id.owner
    {
        return false;
    }
    if let Value::Copied(c) = value
        && let Value::Raw(raw) = c.source
    {
        let value_id = identity(c);
        return value_id.owner == target_id.owner
            && (value_id.storage..value_id.storage + raw.as_bytes().len())
                .contains(&target_id.storage);
    }
    if value.is_array() {
        value.elements().any(|v| contains(&v, target))
    } else if value.is_object() {
        value.members().any(|(_, v)| contains(&v, target))
    } else {
        false
    }
}

fn function_value(value: &Value<'_, '_>) -> bool {
    if matches!(value, Value::Raw(_) | Value::Constant(_) | Value::Copied(_)) {
        return false;
    }
    if matches!(value, Value::Function(_)) {
        true
    } else if value.is_array() {
        value.elements().any(|v| function_value(&v))
    } else if value.is_object() {
        value.members().any(|(_, v)| function_value(&v))
    } else {
        false
    }
}
