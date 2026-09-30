use crate::{Error, Value, value::range_error};
use std::rc::Rc;

/// Immutable clone view directly over input or compiled storage. Only cloning creates it.
#[derive(Clone, Debug)]
pub struct CopiedValue<'e, 'i> {
    pub(crate) source: Value<'e, 'i>,
    pub(crate) identity: Rc<()>,
}
impl<'e, 'i> CopiedValue<'e, 'i> {
    pub(crate) fn same(&self, other: &Self) -> bool {
        super::rewrite::identity(self) == super::rewrite::identity(other)
    }
    pub(crate) fn child(&self, value: Value<'e, 'i>) -> Value<'e, 'i> {
        leaf(value, &self.identity)
    }
    pub(crate) fn field(&self, field: &str) -> Option<Value<'e, 'i>> {
        self.source
            .field(field)
            .filter(|v| !matches!(v, Value::Undefined))
            .map(|v| self.child(v))
    }
}
fn leaf<'e, 'i>(value: Value<'e, 'i>, identity: &Rc<()>) -> Value<'e, 'i> {
    if value.is_array() || value.is_object() {
        Value::Copied(Rc::new(CopiedValue {
            source: value,
            identity: identity.clone(),
        }))
    } else {
        scalar(value)
    }
}
fn scalar<'e, 'i>(value: Value<'e, 'i>) -> Value<'e, 'i> {
    match value.atomic() {
        Value::Number(n) if n.is_nan() => Value::Null,
        Value::Number(n) => Value::Number(crate::convert::rounded_number(n)),
        Value::Undefined => Value::Null,
        Value::Function(_) => Value::StringLiteral(crate::RawJson(r#""""#)),
        _ => value,
    }
}
fn validate(value: &Value<'_, '_>, offset: usize) -> Result<(), Error> {
    if value.is_array() {
        for value in value.elements() {
            validate(&value, offset)?;
        }
    } else if value.is_object() {
        for (_, value) in crate::members::entries(value) {
            validate(&value, offset)?;
        }
    } else if matches!(value.atomic(), Value::Number(n) if n.is_infinite()) {
        return Err(range_error(offset));
    }
    Ok(())
}
pub(crate) fn clone<'e, 'i>(
    value: Option<Value<'e, 'i>>,
    offset: usize,
) -> Result<Option<Value<'e, 'i>>, Error> {
    let Some(value) = value.filter(|v| !matches!(v, Value::Undefined)) else {
        return Ok(None);
    };
    if !value.is_array() && !value.is_object() {
        return Err(crate::value::type_error(offset));
    }
    validate(&value, offset)?;
    Ok(Some(copy(value)))
}
fn copy<'e, 'i>(value: Value<'e, 'i>) -> Value<'e, 'i> {
    // Runtime constructors can share the same object at different locations.
    // A JSON clone breaks those aliases; raw/compiled trees already have unique locations.
    match value {
        Value::Array(array) => Value::array(array.items.iter().cloned().map(copy).collect(), false),
        Value::Object(object) => Value::object(
            object
                .members
                .iter()
                .filter(|(_, v)| !matches!(v, Value::Undefined))
                .map(|(k, v)| (k.clone(), copy(v.clone())))
                .collect(),
        ),
        Value::Copied(c) => copy(c.source.clone()),
        value if value.is_array() || value.is_object() => leaf(value, &Rc::new(())),
        value => scalar(value),
    }
}
