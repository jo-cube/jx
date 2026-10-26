#![forbid(unsafe_code)]
//! Compile once, then evaluate independent UTF-8 JSON records without a DOM.
//!
//! ```
//! let expression = jx::compile("price * quantity")?;
//! expression.evaluate(br#"{"price":2.5,"quantity":3}"#)?.for_each(|value| {
//!     assert_eq!(value.as_number(), Some(7.5));
//! })?;
//! # Ok::<(), jx::Error>(())
//! ```

mod access;
mod aggregate;
mod analysis;
mod builtin;
mod compare;
mod compile;
mod constant;
mod construct;
mod container;
mod controls;
mod convert;
mod dynamic;
mod embedding;
mod error;
mod evaluate;
mod expression;
mod filter;
mod format;
mod function;
mod input;
mod json;
mod lookup;
mod matcher;
mod members;
mod navigate;
mod ordering;
mod parse;
mod path;
mod provenance;
mod random;
mod retain;
mod route;
mod runtime;
mod sequence;
mod transform;
mod tuple;
mod value;

pub use access::{OwnedValue, ValueType};
pub use constant::ConstantValue;
pub use container::{Array, Object};
pub use controls::{Cancellation, Limits};
pub use embedding::{CompileOptions, EvaluationOptions, HostContext, HostFunction};
pub use error::{Error, ErrorKind, Phase, Source, Span};
pub use evaluate::{ConsumeError, Evaluation};
pub use function::Function;
pub use input::{InputPlan, PreparedInput};
pub use json::{MAX_DEPTH, RawJson, validate};
pub use random::Random;
pub use transform::CopiedValue;
pub use value::{OwnedString, Value};

/// Shared semantic IR, acquisition planning, and execution workspace.
pub mod execution;

/// Immutable compiled program; clones share code and permit concurrent evaluation.
#[derive(Clone, Debug)]
pub struct Expression {
    execution: std::sync::Arc<execution::Program>,
}

pub fn compile(source: &str) -> Result<Expression, Error> {
    CompileOptions::default().compile(source)
}

impl Expression {
    /// Validate and acquire the input before exposing borrowed results.
    pub fn evaluate<'e, 'i>(&'e self, input: &'i [u8]) -> Result<Evaluation<'e, 'i>, Error> {
        self.execution.evaluate_bytes(input)
    }
    pub fn evaluate_validated<'e, 'i>(
        &'e self,
        input: RawJson<'i>,
    ) -> Result<Evaluation<'e, 'i>, Error> {
        self.execution.evaluate_validated(input)
    }
    /// Evaluate with a shared, optionally seeded random source.
    pub fn evaluate_with_random<'e, 'i>(
        &'e self,
        input: &'i [u8],
        random: &Random,
    ) -> Result<Evaluation<'e, 'i>, Error> {
        self.execution.evaluate_with(
            Some(input),
            EvaluationOptions {
                random: Some(random.clone()),
                ..Default::default()
            },
        )
    }
}
