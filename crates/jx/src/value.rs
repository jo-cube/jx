use crate::{Error, ErrorKind, RawJson, json::string};
use std::io::{self, Write};

/// An emitted value. Raw input and compiled string literals retain their own
/// borrowing lifetimes; computed scalars are unboxed. Constructed containers share
/// immutable member storage and retain borrowed leaves. Missing emits no value.
#[derive(Clone, Debug)]
pub enum Value<'expression, 'input> {
    Raw(RawJson<'input>),
    Constant(crate::ConstantValue<'expression>),
    Function(std::rc::Rc<crate::Function<'expression, 'input>>),
    Number(f64),
    Boolean(bool),
    Null,
    /// Undefined retained inside a multi-item sequence; serializes as null.
    Undefined,
    StringLiteral(RawJson<'expression>),
    /// A computed string, with shared immutable JSON encoding.
    String(OwnedString),
    Array(std::rc::Rc<crate::container::Array<'expression, 'input>>),
    Object(std::rc::Rc<crate::container::Object<'expression, 'input>>),
}

impl<'i> Value<'_, 'i> {
    /// Recover an input slice independently of the compiled expression's lifetime.
    pub fn as_raw(&self) -> Option<RawJson<'i>> {
        match self {
            Self::Raw(value) => Some(*value),
            _ => None,
        }
    }

    /// Preserve input tokens; encode computed binary64 values as compact JSON.
    /// Like JSONata's JSON serialization, non-finite results serialize as null.
    pub fn write_compact(&self, mut output: impl Write) -> io::Result<()> {
        match self {
            Self::Function(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "functions have no JSON encoding",
            )),
            Self::Constant(value) => value.data.write(&mut output),
            Self::Array(array) => array.write_compact(&mut output),
            Self::Object(object) => object.write_compact(&mut output),
            Self::Raw(value) => value.write_compact(output),
            Self::StringLiteral(value) => output.write_all(value.as_bytes()),
            Self::String(value) => output.write_all(value.json.as_bytes()),
            Self::Number(value) if !value.is_finite() => output.write_all(b"null"),
            Self::Number(0.0) => output.write_all(b"0"),
            Self::Number(value) => write!(output, "{value}"),
            Self::Boolean(true) => output.write_all(b"true"),
            Self::Boolean(false) => output.write_all(b"false"),
            Self::Null | Self::Undefined => output.write_all(b"null"),
        }
    }

    pub(crate) fn atomic(&self) -> Self {
        match *self {
            Self::Raw(raw) => match raw.as_bytes()[0] {
                b'n' => Self::Null,
                b't' => Self::Boolean(true),
                b'f' => Self::Boolean(false),
                b'-' | b'0'..=b'9' => Self::Number(raw.as_str().parse().expect("validated number")),
                _ => self.clone(),
            },
            _ => self.clone(),
        }
    }

    pub(crate) fn json(&self) -> Option<RawJson<'_>> {
        match self {
            Self::Raw(raw) => Some(*raw),
            Self::StringLiteral(raw) => Some(*raw),
            Self::String(value) => Some(RawJson(&value.json)),
            _ => None,
        }
    }

    pub(crate) fn truth(&self, offset: usize) -> Result<bool, Error> {
        match self.atomic() {
            Self::Function(_) => Ok(false),
            Self::Boolean(value) => Ok(value),
            Self::Null | Self::Undefined => Ok(false),
            Self::Number(value) => {
                if value.is_infinite() {
                    return Err(range_error(offset));
                }
                Ok(value != 0.0 && !value.is_nan())
            }
            Self::Array(array) => {
                let mut truth = false;
                for item in &array.items {
                    truth |= item.truth(offset)?;
                }
                Ok(truth)
            }
            Self::Object(object) => Ok(!object.members.is_empty()),
            Self::Constant(value) => {
                if value.shape().is_some() {
                    let mut truth = false;
                    let mut index = 0;
                    while let Some(item) = value.element(index) {
                        truth |= item.truth(offset)?;
                        index += 1;
                    }
                    Ok(truth)
                } else {
                    Ok(value.member(0).is_some())
                }
            }
            value => {
                let raw = value.json().unwrap();
                match raw.as_bytes()[0] {
                    b'"' => Ok(string::units(&raw.as_str()[1..raw.as_str().len() - 1])
                        .next()
                        .is_some()),
                    b'{' => Ok(raw.members().next().is_some()),
                    b'[' => {
                        let mut truth = false;
                        // All items are semantically evaluated, including numeric errors
                        // after a truthy member. This is distinct from and/or short-circuiting.
                        for item in raw.elements() {
                            truth |= Value::Raw(item).truth(offset)?;
                        }
                        Ok(truth)
                    }
                    _ => unreachable!("atomic conversion handled other JSON types"),
                }
            }
        }
    }
}

pub(crate) fn type_error(offset: usize) -> Error {
    Error::new(ErrorKind::TypeError, offset, "invalid operand type")
}

pub(crate) fn range_error(offset: usize) -> Error {
    Error::new(
        ErrorKind::NumericRange,
        offset,
        "numeric operand exceeds binary64 range",
    )
}

/// Immutable storage for a computed JSON string, including escaped surrogate units.
#[derive(Clone, Debug)]
pub struct OwnedString {
    json: std::rc::Rc<str>,
}
impl OwnedString {
    pub(crate) fn units(units: impl IntoIterator<Item = u16>) -> Self {
        use std::fmt::Write;
        let mut json = String::from("\"");
        for ch in char::decode_utf16(units) {
            match ch {
                Ok('"') => json.push_str("\\\""),
                Ok('\\') => json.push_str("\\\\"),
                Ok(ch) if ch < ' ' => write!(json, "\\u{:04x}", ch as u32).unwrap(),
                Ok(ch) => json.push(ch),
                Err(ch) => write!(json, "\\u{:04x}", ch.unpaired_surrogate()).unwrap(),
            }
        }
        json.push('"');
        Self { json: json.into() }
    }
    // Both bodies are valid JSON string encodings. Joining them preserves UTF-16
    // units, including a surrogate pair spanning the operand boundary.
    pub(crate) fn concat(left: &str, right: &str) -> Self {
        let mut json = String::with_capacity(left.len() + right.len() + 2);
        json.push('"');
        json.push_str(left);
        json.push_str(right);
        json.push('"');
        Self { json: json.into() }
    }
    pub(crate) fn body(body: &str) -> Self {
        Self {
            json: format!("\"{body}\"").into(),
        }
    }
}
