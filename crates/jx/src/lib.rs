#![forbid(unsafe_code)]
//! Compile once, then evaluate independent UTF-8 JSON records without a DOM.
//!
//! ```
//! let expression = jx::compile("price * quantity")?;
//! expression.evaluate(br#"{"price":2.5,"quantity":3}"#)?.for_each(|value| {
//!     assert!(matches!(value, jx::Value::Number(7.5)));
//! })?;
//! # Ok::<(), jx::Error>(())
//! ```

mod compare;
mod error;
mod evaluate;
mod expression;
mod filter;
mod json;
mod parse;
mod path;
mod sequence;
mod value;

pub use error::{Error, ErrorKind};
pub use evaluate::{ConsumeError, Evaluation};
pub use json::{MAX_DEPTH, RawJson, validate};
pub use value::Value;

/// Immutable compiled expression; share across callers with independent inputs.
#[derive(Clone, Debug)]
pub struct Expression {
    root: expression::Node,
}

pub fn compile(source: &str) -> Result<Expression, Error> {
    parse::expression(source)
}

impl Expression {
    /// Validate the entire record before exposing results. Traversal is deferred
    /// until consumption; scalar operations finish before returning results.
    #[inline]
    pub fn evaluate<'e, 'i>(&'e self, input: &'i [u8]) -> Result<Evaluation<'e, 'i>, Error> {
        match &self.root.kind {
            expression::Kind::Path(path) => Ok(Evaluation {
                result: evaluate::Results::Path(path.select(input)?),
            }),
            _ => evaluate::scalar(&self.root, input),
        }
    }
}
