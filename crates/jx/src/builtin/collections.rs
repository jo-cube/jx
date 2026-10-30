use super::library::Library;
use crate::{Error, OwnedString, Value, compare, json::string, value::range_error};

pub(super) fn key<'e, 'i>(body: &str) -> Value<'e, 'i> {
    Value::String(OwnedString::body(body))
}
pub(super) fn items<'a, 'e: 'a, 'i: 'a>(
    value: &'a Value<'e, 'i>,
) -> impl Iterator<Item = Value<'e, 'i>> + 'a {
    let (array, scalar) = if value.is_array() {
        (Some(value.elements()), None)
    } else {
        (None, Some(value.clone()))
    };
    array.into_iter().flatten().chain(scalar)
}
pub(super) fn call<'e, 'i>(
    function: Library,
    args: &[Option<Value<'e, 'i>>; 3],
    offset: usize,
) -> Result<Option<Value<'e, 'i>>, Error> {
    if function == Library::Append {
        return Ok(match (&args[0], &args[1]) {
            (Some(left), Some(right)) => Some(Value::array(
                items(left).chain(items(right)).collect(),
                false,
            )?),
            (left, right) => left.clone().or_else(|| right.clone()),
        });
    }
    let Some(value) = &args[0] else {
        return (function == Library::Keys)
            .then(|| Value::sequence(Vec::new()))
            .transpose();
    };
    Ok(match function {
        Library::Type => {
            let name = match value.atomic() {
                Value::Number(n) if n.is_infinite() => return Err(range_error(offset)),
                Value::Number(n) if !n.is_nan() => r#""number""#,
                Value::Boolean(_) => r#""boolean""#,
                Value::Null => r#""null""#,
                Value::Function(_) => r#""function""#,
                value if value.is_array() => r#""array""#,
                value if value.string_body().is_some() => r#""string""#,
                _ => r#""object""#,
            };
            Some(Value::StringLiteral(crate::RawJson(name)))
        }
        Library::Reverse => {
            let mut values: Vec<_> = items(value).collect();
            if value.is_array() && values.len() <= 1 {
                Some(value.clone())
            } else {
                values.reverse();
                Some(Value::array(values, false)?)
            }
        }
        Library::Distinct if !value.is_array() => Some(value.clone()),
        Library::Distinct if value.elements().nth(1).is_none() => Some(value.clone()),
        Library::Distinct => {
            let mut values = Vec::new();
            for item in value.elements() {
                if !values
                    .iter()
                    .any(|previous: &Value<'e, 'i>| compare::values(previous.clone(), item.clone()))
                {
                    values.push(item);
                }
            }
            if value.is_sequence() {
                Some(Value::sequence(values)?)
            } else {
                Some(Value::array(values, false)?)
            }
        }
        Library::Keys => {
            let mut keys = Vec::new();
            collect_keys(value, &mut keys, &mut KeyIndex::default());
            keys.sort_by_key(|key| {
                crate::members::index(key.string_body().unwrap()).unwrap_or(u32::MAX)
            });
            Some(Value::sequence(keys)?)
        }
        Library::Spread => spread(value)?,
        Library::Merge => {
            let mut entries: Vec<(Value<'e, 'i>, Value<'e, 'i>)> = Vec::new();
            let mut index = KeyIndex::default();
            for object in items(value) {
                for (name, value) in crate::members::entries(&object) {
                    if entries.len() == 32 && !index.active() {
                        index = KeyIndex::with_capacity(entries.len());
                        for (i, (key, _)) in entries.iter().enumerate() {
                            index.insert(string::fingerprint(key.string_body().unwrap()), i);
                        }
                    }
                    let hash = index.active().then(|| string::fingerprint(name));
                    let found = if let Some(hash) = hash {
                        index.find(hash, name, |i| entries[i].0.string_body().unwrap())
                    } else {
                        entries.iter().position(|(key, _)| {
                            string::units(key.string_body().unwrap()).eq(string::units(name))
                        })
                    };
                    if let Some(i) = found {
                        entries[i].1 = value;
                    } else {
                        if let Some(hash) = hash {
                            index.insert(hash, entries.len());
                        }
                        entries.push((key(name), value));
                    }
                }
            }
            drop(index);
            entries.sort_by_key(|(key, _)| {
                crate::members::index(key.string_body().unwrap()).unwrap_or(u32::MAX)
            });
            Some(Value::object(entries)?)
        }
        _ => unreachable!(),
    })
}
fn collect_keys<'e, 'i>(
    value: &Value<'e, 'i>,
    keys: &mut Vec<Value<'e, 'i>>,
    index: &mut KeyIndex,
) {
    if value.is_array() {
        for item in value.elements() {
            collect_keys(&item, keys, index);
        }
    } else if value.is_object() {
        for (name, _) in crate::members::entries(value) {
            if keys.len() == 32 && !index.active() {
                *index = KeyIndex::with_capacity(keys.len());
                for (i, key) in keys.iter().enumerate() {
                    index.insert(string::fingerprint(key.string_body().unwrap()), i);
                }
            }
            let hash = index.active().then(|| string::fingerprint(name));
            let found = if let Some(hash) = hash {
                index
                    .find(hash, name, |i| keys[i].string_body().unwrap())
                    .is_some()
            } else {
                keys.iter()
                    .any(|k| string::units(k.string_body().unwrap()).eq(string::units(name)))
            };
            if !found {
                if let Some(hash) = hash {
                    index.insert(hash, keys.len());
                }
                keys.push(key(name));
            }
        }
    }
}
fn spread<'e, 'i>(value: &Value<'e, 'i>) -> Result<Option<Value<'e, 'i>>, Error> {
    Ok(if value.is_array() {
        let mut result = Vec::new();
        let mut appended = false;
        for item in value.elements() {
            if let Some(value) = spread(&item)? {
                appended = true;
                result.extend(items(&value));
            }
        }
        // Appending a defined value loses the initial sequence marker, even if
        // it is an empty array. Missing values leave that marker intact.
        if !appended {
            Some(Value::sequence(result)?)
        } else {
            Some(Value::array(result, false)?)
        }
    } else if value.is_object() {
        Some(Value::sequence(
            crate::members::entries(value)
                .into_iter()
                .map(|(name, value)| Value::object(vec![(key(name), value)]))
                .collect::<Result<_, _>>()?,
        )?)
    } else {
        (!matches!(value, Value::Undefined)).then(|| value.clone())
    })
}

// Positions refer to the output vector until its final sort. Fingerprints only
// narrow the search: every collision still compares the decoded UTF-16 keys.
#[derive(Default)]
struct KeyIndex {
    heads: std::collections::HashMap<u64, usize>,
    next: Vec<usize>,
}
impl KeyIndex {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            heads: std::collections::HashMap::with_capacity(capacity),
            next: Vec::with_capacity(capacity),
        }
    }
    fn active(&self) -> bool {
        !self.next.is_empty()
    }
    fn find<'a>(&self, hash: u64, name: &str, key: impl Fn(usize) -> &'a str) -> Option<usize> {
        let mut i = *self.heads.get(&hash)?;
        while i != usize::MAX {
            if string::units(key(i)).eq(string::units(name)) {
                return Some(i);
            }
            i = self.next[i];
        }
        None
    }
    fn insert(&mut self, hash: u64, position: usize) {
        debug_assert_eq!(position, self.next.len());
        self.next
            .push(self.heads.insert(hash, position).unwrap_or(usize::MAX));
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn fingerprint_collisions_compare_actual_utf16_keys() {
        let mut index = super::KeyIndex::default();
        index.insert(9, 0);
        index.insert(9, 1);
        index.insert(9, 2);
        index.insert(9, 3);
        index.insert(10, 4);
        let keys = ["a", r"\ud800", "😀", r"\udc00", "c"];
        assert_eq!(index.find(9, r"\u0061", |i| keys[i]), Some(0));
        assert_eq!(index.find(9, r"\uD800", |i| keys[i]), Some(1));
        assert_eq!(index.find(9, r"\ud83d\ude00", |i| keys[i]), Some(2));
        assert_eq!(index.find(9, r"\uDC00", |i| keys[i]), Some(3));
        assert_eq!(index.find(10, "c", |i| keys[i]), Some(4));
        assert_eq!(index.find(9, "c", |i| keys[i]), None);
        assert_eq!(index.find(11, "a", |i| keys[i]), None);
    }
}
