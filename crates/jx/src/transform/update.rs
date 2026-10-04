use super::*;
pub(super) fn key_value<'e, 'i>(object: &Value<'e, 'i>, body: &str) -> Value<'e, 'i> {
    match object {
        Value::Raw(raw) => {
            let at = body.as_ptr() as usize - raw.as_bytes().as_ptr() as usize;
            Value::Raw(crate::RawJson(&raw.as_str()[at - 1..at + body.len() + 1]))
        }
        Value::Copied(copy) => key_value(&copy.source, body),
        Value::Object(object) => object
            .members
            .iter()
            .find(|(k, _)| k.string_body().unwrap() == body)
            .unwrap()
            .0
            .clone(),
        Value::Constant(c) => {
            let crate::constant::Data::Object { members, .. } = c.data else {
                unreachable!()
            };
            let (key, _) = members
                .iter()
                .find(|(k, _)| &k[1..k.len() - 1] == body)
                .unwrap();
            Value::StringLiteral(crate::RawJson(key))
        }
        _ => unreachable!("object key"),
    }
}
fn same_key(left: &str, right: &str) -> bool {
    crate::json::string::units(left).eq(crate::json::string::units(right))
}
pub(super) fn merge<'e, 'i>(
    target: &Value<'e, 'i>,
    update: &Value<'e, 'i>,
    offset: usize,
) -> Result<Value<'e, 'i>, Error> {
    if update.members().next().is_none() {
        return Ok(target.clone());
    }
    if target.is_array() {
        let mut items: Vec<_> = target.elements().collect();
        for (key, value) in crate::members::entries(update) {
            if let Some(index) = crate::members::index(key) {
                let index = index as usize;
                if index >= 1_000_000 {
                    return Err(crate::value::range_error(offset));
                }
                items.resize(items.len().max(index + 1), Value::Undefined);
                items[index] = value;
            } else if same_key(key, "length") {
                let Value::Number(n) = value.atomic() else {
                    return Err(crate::value::type_error(offset));
                };
                if !(0.0..=1_000_000.0).contains(&n) || n.fract() != 0.0 {
                    return Err(crate::value::range_error(offset));
                }
                items.resize(n as usize, Value::Undefined);
            }
        }
        return Ok(Value::array(items, false));
    }
    if !target.is_object() {
        if update.members().next().is_none() {
            return Ok(target.clone());
        }
        return Err(crate::value::type_error(offset));
    }
    let mut members: Vec<_> = crate::members::entries(target)
        .into_iter()
        .map(|(key, value)| (key_value(target, key), value))
        .collect();
    for (key, value) in crate::members::entries(update) {
        if let Some((_, previous)) = members
            .iter_mut()
            .find(|(k, _)| same_key(k.string_body().unwrap(), key))
        {
            *previous = value;
        } else {
            members.push((key_value(update, key), value));
        }
    }
    members
        .sort_by_key(|(k, _)| crate::members::index(k.string_body().unwrap()).unwrap_or(u32::MAX));
    Ok(Value::object(members))
}
pub(super) fn remove<'e, 'i>(
    target: &Value<'e, 'i>,
    keys: &[Value<'e, 'i>],
    offset: usize,
) -> Result<Value<'e, 'i>, Error> {
    if target.is_array() {
        let mut items: Vec<_> = target.elements().collect();
        for key in keys {
            let body = key.string_body().unwrap();
            if same_key(body, "length") {
                return Err(crate::value::type_error(offset));
            }
            if let Some(index) = crate::members::index(body)
                && let Some(slot) = items.get_mut(index as usize)
            {
                *slot = Value::Undefined;
            }
        }
        Ok(Value::array(items, false))
    } else if target.is_object() {
        Ok(Value::object(
            crate::members::entries(target)
                .into_iter()
                .filter(|(k, _)| {
                    !keys
                        .iter()
                        .any(|key| same_key(k, key.string_body().unwrap()))
                })
                .map(|(k, v)| (key_value(target, k), v))
                .collect(),
        ))
    } else {
        Ok(target.clone())
    }
}
