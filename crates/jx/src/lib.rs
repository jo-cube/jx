#![forbid(unsafe_code)]
//! Compile once, then evaluate independent UTF-8 JSON records without a DOM.
//!
//! ```
//! let expression = jx::compile("customer.id")?;
//! for input in [br#"{"customer":{"id":42}}"#.as_slice(), b"{}"] {
//!     expression.evaluate(input)?.for_each(|value| {
//!         println!("{}", value.as_str());
//!     });
//! }
//! # Ok::<(), jx::Error>(())
//! ```

mod error;
mod evaluate;
mod json;
mod parse;

pub use error::{Error, ErrorKind};
pub use evaluate::Evaluation;
pub use json::{MAX_DEPTH, RawJson, validate};

/// Immutable compiled expression; share across callers with independent inputs.
#[derive(Clone, Debug)]
pub struct Expression {
    fields: Box<[Box<str>]>,
    rooted: bool,
}

pub fn compile(source: &str) -> Result<Expression, Error> {
    parse::expression(source)
}

impl Expression {
    /// Validate the entire record before exposing results. Traversal is deferred
    /// until consumption; values borrow the input, not the expression.
    pub fn evaluate<'e, 'i>(&'e self, input: &'i [u8]) -> Result<Evaluation<'e, 'i>, Error> {
        let selection = json::select(input, &self.fields)?;
        let root_lookup = !self.rooted
            && matches!(&selection,
            json::Selection::Array(_, fields) if fields.len() == self.fields.len());
        Ok(Evaluation {
            selection,
            root_lookup,
        })
    }
}
