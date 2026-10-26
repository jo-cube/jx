use crate::{Error, Value, evaluate::Operand, json::string, value::type_error};

pub(crate) fn values<'e, 'i>(
    object: Option<Value<'e, 'i>>,
    key: Option<Value<'e, 'i>>,
    offset: usize,
) -> Result<Operand<'e, 'i>, Error> {
    let key = key_body(&key, offset)?;
    let Some(object) = object else {
        return Ok(Operand::Missing);
    };
    if !object.is_array() {
        return Ok(field(&object, key).map_or(Operand::Missing, Operand::One));
    }
    fn walk<'e, 'i>(object: &Value<'e, 'i>, key: &str, emit: &mut dyn FnMut(Value<'e, 'i>)) {
        if object.is_array() {
            if let Value::Raw(raw) = object {
                raw.try_for_each_flattened(|item| {
                    walk(&Value::Raw(item), key, emit);
                    Ok::<_, std::convert::Infallible>(())
                })
                .unwrap();
                return;
            }
            for item in object.elements() {
                walk(&item, key, emit);
            }
        } else if let Some(value) = field(object, key) {
            if value.is_array() {
                for item in value.elements() {
                    emit(item);
                }
            } else {
                emit(value);
            }
        }
    }
    crate::retain::collect(|emit| {
        walk(&object, key, emit);
        Ok(())
    })
    .map(|v| v.map_or(Operand::Missing, Operand::One))
}

pub(crate) fn field<'e, 'i>(object: &Value<'e, 'i>, key: &str) -> Option<Value<'e, 'i>> {
    if let Value::Constant(value) = object {
        return value.field(|| string::fingerprint(key), |name| string::equal(name, key));
    }
    if !object.is_object() {
        return None;
    }
    let mut found = None;
    for (name, value) in object.members() {
        if string::equal(name, key) {
            found = Some(value);
        }
    }
    found
}

fn key_body<'a>(key: &'a Option<Value<'_, '_>>, offset: usize) -> Result<&'a str, Error> {
    match key {
        None | Some(Value::Undefined) => Ok("undefined"),
        Some(value) => value.string_body().ok_or_else(|| type_error(offset)),
    }
}

pub(crate) fn constant<'e, 'i>(
    data: &'e crate::constant::Data,
    key: Option<Value<'e, 'i>>,
    offset: usize,
) -> Result<Option<Value<'e, 'i>>, Error> {
    Ok(constant_data(data, key, offset)?.map(crate::constant::Data::value))
}

pub(crate) fn constant_data<'e, 'i>(
    data: &'e crate::constant::Data,
    key: Option<Value<'e, 'i>>,
    offset: usize,
) -> Result<Option<&'e crate::constant::Data>, Error> {
    let key = key_body(&key, offset)?;
    Ok(data.find(|| string::fingerprint(key), |name| string::equal(name, key)))
}
