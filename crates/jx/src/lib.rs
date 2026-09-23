#![forbid(unsafe_code)]
//! Compile once, then evaluate independent UTF-8 JSON records without a DOM.
//!
//! ```
//! let expression = jx::compile("customer.id")?;
//! for input in [br#"{"customer":{"id":42}}"#.as_slice(), b"{}"] {
//!     for value in expression.evaluate(input)? {
//!         println!("{}", value.as_str());
//!     }
//! }
//! # Ok::<(), jx::Error>(())
//! ```

mod error;
mod json;
mod parse;

pub use error::{Error, ErrorKind};
pub use json::{MAX_DEPTH, RawJson, validate};

/// Immutable compiled expression; share across callers with independent inputs.
#[derive(Clone, Debug)]
pub struct Expression {
    fields: Box<[Box<str>]>,
}

pub fn compile(source: &str) -> Result<Expression, Error> {
    Ok(Expression {
        fields: parse::path(source)?,
    })
}

impl Expression {
    /// Validate the complete record before returning any result. Missing is an
    /// empty iterator; JSON null and a selected array each yield one raw value.
    pub fn evaluate<'a>(&self, input: &'a [u8]) -> Result<Evaluation<'a>, Error> {
        Ok(Evaluation {
            value: json::select(input, &self.fields)?,
        })
    }
}

/// Provisional result iterator. Currently contains zero or one borrowed value.
/// Its representation can change when result sequences are implemented.
#[derive(Debug)]
pub struct Evaluation<'a> {
    value: Option<RawJson<'a>>,
}

impl<'a> Iterator for Evaluation<'a> {
    type Item = RawJson<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.value.take()
    }
}
