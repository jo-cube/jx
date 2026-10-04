use crate::{Error, ErrorKind, Value, constant::Data};
use std::borrow::Cow;

/// Semantic type, independent of borrowed, compiled, or constructed storage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ValueType {
    Missing,
    Null,
    Boolean,
    Number,
    String,
    Array,
    Object,
    Function,
}

/// An explicit snapshot independent of both the input and compiled expression.
/// Functions cannot be detached from their evaluation and are rejected.
#[derive(Clone, Debug)]
pub struct OwnedValue {
    data: Data,
}
impl OwnedValue {
    pub fn as_value(&self) -> Value<'_, 'static> {
        self.data.value()
    }
    pub fn from_json(input: &[u8]) -> Result<Self, Error> {
        Value::Raw(crate::validate(input)?).to_owned()
    }
}
impl<'e, 'i> Value<'e, 'i> {
    pub fn value_type(&self) -> ValueType {
        if self.is_array() {
            return ValueType::Array;
        }
        if self.is_object() {
            return ValueType::Object;
        }
        match self.atomic() {
            Self::Undefined => ValueType::Missing,
            Self::Null => ValueType::Null,
            Self::Boolean(_) => ValueType::Boolean,
            Self::Number(_) => ValueType::Number,
            Self::Function(_) => ValueType::Function,
            _ => ValueType::String,
        }
    }
    pub fn as_number(&self) -> Option<f64> {
        if let Self::Number(n) = self.atomic() {
            Some(n)
        } else {
            None
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        if let Self::Boolean(b) = self.atomic() {
            Some(b)
        } else {
            None
        }
    }
    /// Borrow unescaped UTF-8; allocate only for decoding escapes. Isolated
    /// surrogate units cannot be represented by Rust str; use string_units instead.
    pub fn as_str(&self) -> Result<Option<Cow<'_, str>>, Error> {
        self.string_body().map(decode).transpose()
    }
    pub fn string_units(&self) -> Option<impl Iterator<Item = u16> + '_> {
        self.string_body().map(crate::json::string::units)
    }
    /// Container access does not flatten arrays or normalize sequences.
    pub fn array_items(&self) -> Option<impl Iterator<Item = Value<'e, 'i>> + '_> {
        self.is_array().then(|| self.elements())
    }
    /// Decoded keys in storage order; raw duplicate keys remain visible.
    pub fn object_entries(
        &self,
    ) -> Option<impl Iterator<Item = Result<(Cow<'_, str>, Value<'e, 'i>), Error>> + '_> {
        self.is_object()
            .then(|| self.members().map(|(k, v)| Ok((decode(k)?, v))))
    }
    pub fn get(&self, key: &str) -> Option<Self> {
        self.field(key)
    }
    pub fn is_missing(&self) -> bool {
        matches!(self, Self::Undefined)
    }
    pub fn is_null(&self) -> bool {
        matches!(self.atomic(), Self::Null)
    }
    pub fn from_json(input: &'i [u8]) -> Result<Self, Error> {
        crate::validate(input).map(Self::Raw)
    }
    pub fn from_string(text: impl Into<String>) -> Self {
        Self::String(crate::OwnedString::text(text.into()))
    }
    pub fn from_array(items: Vec<Self>) -> Self {
        Self::array(items, false)
    }
    pub fn from_object(items: impl IntoIterator<Item = (String, Self)>) -> Self {
        // Field lookup and serialization share last-write-wins semantics.
        let mut members: Vec<(Self, Self)> = Vec::new();
        for (key, value) in items {
            if let Some((_, old)) = members
                .iter_mut()
                .find(|(k, _)| crate::json::string::matches(k.string_body().unwrap(), &key))
            {
                *old = value;
            } else {
                members.push((Self::from_string(key), value));
            }
        }
        Self::object(members)
    }
    pub fn to_owned(&self) -> Result<OwnedValue, Error> {
        capture(self).map(|data| OwnedValue { data })
    }
}
fn decode(body: &str) -> Result<Cow<'_, str>, Error> {
    if !body.as_bytes().contains(&b'\\') {
        return Ok(Cow::Borrowed(body));
    }
    char::decode_utf16(crate::json::string::units(body))
        .collect::<Result<String, _>>()
        .map(Cow::Owned)
        .map_err(|_| {
            Error::new(
                ErrorKind::EncodingError,
                0,
                "isolated UTF-16 surrogate has no UTF-8 representation",
            )
            .result()
        })
}
fn capture(value: &Value<'_, '_>) -> Result<Data, Error> {
    if let Value::Constant(value) = value {
        return Ok(value.data.clone());
    }
    if let Some(items) = value.array_items() {
        return Ok(Data::Array(
            items.map(|v| capture(&v)).collect::<Result<_, _>>()?,
            match value {
                Value::Array(a) => a.shape,
                _ => crate::container::Shape::Array,
            },
        ));
    }
    if value.is_object() {
        let members = value
            .members()
            .map(|(k, v)| Ok((format!("\"{k}\"").into_boxed_str(), capture(&v)?)))
            .collect::<Result<Box<[_]>, Error>>()?;
        let mut index = members
            .iter()
            .enumerate()
            .map(|(i, (k, _))| (crate::json::string::fingerprint(&k[1..k.len() - 1]), i))
            .collect::<Vec<_>>();
        index.sort_unstable_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
        return Ok(Data::Object {
            members,
            index: index.into_boxed_slice(),
        });
    }
    match value.atomic() {
        Value::Null => Ok(Data::Null),
        Value::Boolean(b) => Ok(Data::Boolean(b)),
        Value::Number(n) => Ok(Data::Number(n)),
        Value::Undefined => Ok(Data::Missing),
        v if v.string_body().is_some() => Ok(Data::String(v.json().unwrap().as_str().into())),
        _ => Err(Error::new(
            ErrorKind::TypeError,
            0,
            "functions cannot be converted to owned results",
        )
        .result()),
    }
}
