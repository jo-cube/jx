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
            )),
            (left, right) => left.clone().or_else(|| right.clone()),
        });
    }
    let Some(value) = &args[0] else {
        return Ok((function == Library::Keys).then(|| Value::sequence(Vec::new())));
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
                Some(Value::array(values, false))
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
                Some(Value::sequence(values))
            } else {
                Some(Value::array(values, false))
            }
        }
        Library::Keys => {
            let mut keys = Vec::new();
            collect_keys(value, &mut keys);
            keys.sort_by_key(|key| {
                crate::members::index(key.string_body().unwrap()).unwrap_or(u32::MAX)
            });
            Some(Value::sequence(keys))
        }
        Library::Spread => spread(value),
        Library::Merge => {
            let mut entries: Vec<(Value<'e, 'i>, Value<'e, 'i>)> = Vec::new();
            for object in items(value) {
                for (name, value) in crate::members::entries(&object) {
                    if let Some((_, previous)) = entries.iter_mut().find(|(key, _)| {
                        string::units(key.string_body().unwrap()).eq(string::units(name))
                    }) {
                        *previous = value;
                    } else {
                        entries.push((key(name), value));
                    }
                }
            }
            entries.sort_by_key(|(key, _)| {
                crate::members::index(key.string_body().unwrap()).unwrap_or(u32::MAX)
            });
            Some(Value::object(entries))
        }
        _ => unreachable!(),
    })
}
fn collect_keys<'e, 'i>(value: &Value<'e, 'i>, keys: &mut Vec<Value<'e, 'i>>) {
    if value.is_array() {
        for item in value.elements() {
            collect_keys(&item, keys);
        }
    } else if value.is_object() {
        for (name, _) in crate::members::entries(value) {
            if !keys
                .iter()
                .any(|k| string::units(k.string_body().unwrap()).eq(string::units(name)))
            {
                keys.push(key(name));
            }
        }
    }
}
fn spread<'e, 'i>(value: &Value<'e, 'i>) -> Option<Value<'e, 'i>> {
    if value.is_array() {
        let mut result = Vec::new();
        let mut appended = false;
        for item in value.elements() {
            if let Some(value) = spread(&item) {
                appended = true;
                result.extend(items(&value));
            }
        }
        // Appending a defined value loses the initial sequence marker, even if
        // it is an empty array. Missing values leave that marker intact.
        if !appended {
            Some(Value::sequence(result))
        } else {
            Some(Value::array(result, false))
        }
    } else if value.is_object() {
        Some(Value::sequence(
            crate::members::entries(value)
                .into_iter()
                .map(|(name, value)| Value::object(vec![(key(name), value)]))
                .collect(),
        ))
    } else {
        (!matches!(value, Value::Undefined)).then(|| value.clone())
    }
}
