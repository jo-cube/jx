use crate::{
    Error, Value, evaluate::Operand, expression::Node, json::string, sequence::Context,
    value::type_error,
};

pub(crate) fn evaluate<'e, 'i>(
    args: &'e [Node],
    input: &Context<'e, 'i>,
    offset: usize,
) -> Result<Operand<'e, 'i>, Error> {
    let (object, key) = match args {
        [key] => (
            Some(input.value.clone()),
            crate::retain::materialize(key, input)?,
        ),
        [object, key] => {
            let object = crate::retain::materialize(object, input)?;
            let key = crate::retain::materialize(key, input)?;
            (object, key)
        }
        _ => {
            for arg in args {
                crate::retain::materialize(arg, input)?;
            }
            return Err(type_error(offset));
        }
    };
    values(object, key, offset)
}

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
        return value.field(|name| string::units(name).cmp(string::units(key)));
    }
    if !object.is_object() {
        return None;
    }
    let mut found = None;
    for (name, value) in object.members() {
        if string::units(name).eq(string::units(key)) {
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
    let key = key_body(&key, offset)?;
    Ok(data
        .find(|name| string::units(name).cmp(string::units(key)))
        .map(crate::constant::Data::value))
}

pub(crate) fn select<'e, 'i>(
    data: &'e crate::constant::Data,
    path: &'e crate::expression::Path,
    input: &'i [u8],
    offset: usize,
) -> Result<crate::Evaluation<'e, 'i>, Error> {
    // Reuse validating path capture: no second record scan or lexical context.
    let key = match path.select(input)?.operand()? {
        Operand::Missing => None,
        Operand::One(value) => Some(value),
        Operand::Many(_) => return Err(type_error(offset)),
    };
    Ok(crate::Evaluation {
        result: crate::evaluate::Results::Scalar(constant(data, key, offset)?),
    })
}
