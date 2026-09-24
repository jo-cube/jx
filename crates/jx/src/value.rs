use crate::{Error, ErrorKind, RawJson, json::string};
use std::io::{self, Write};

/// An emitted value. Raw input and compiled string literals retain their own
/// borrowing lifetimes; computed scalars are unboxed. Constructed containers share
/// immutable member storage and retain borrowed leaves. Missing emits no value.
#[derive(Clone, Debug)]
pub enum Value<'expression, 'input> {
    Raw(RawJson<'input>),
    Number(f64),
    Boolean(bool),
    Null,
    /// Undefined retained inside a multi-item sequence; serializes as null.
    Undefined,
    StringLiteral(RawJson<'expression>),
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
            Self::Array(array) => array.write_compact(&mut output),
            Self::Object(object) => object.write_compact(&mut output),
            Self::Raw(value) => value.write_compact(output),
            Self::StringLiteral(value) => output.write_all(value.as_bytes()),
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
            _ => None,
        }
    }

    pub(crate) fn truth(&self, offset: usize) -> Result<bool, Error> {
        match self.atomic() {
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
