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
mod acquire;
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
mod json;
mod lookup;
mod matcher;
mod members;
mod navigate;
mod ordering;
mod parse;
mod path;
mod plan;
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
pub use json::{MAX_DEPTH, RawJson, validate};
pub use random::Random;
pub use transform::CopiedValue;
pub use value::{OwnedString, Value};

/// Immutable compiled expression; share across callers with independent inputs.
#[derive(Clone, Debug)]
pub struct Expression {
    root: expression::Node,
    runtime: bool,
    acquisition: Option<json::Demand>,
    region: Option<Box<acquire::Region>>,
    bindings: Box<[Box<str>]>,
}

pub fn compile(source: &str) -> Result<Expression, Error> {
    parse::expression(source).map_err(Error::compilation)
}

/// Newly installed native kernels and failures; unsupported plans are left unchanged.
#[cfg(feature = "jit")]
#[derive(Default, Debug)]
pub struct NativeStats {
    pub kernels: usize,
    pub code_bytes: usize,
    pub failures: usize,
}
#[cfg(feature = "jit")]
impl Expression {
    /// Compile eligible primitive plan regions with the host's native backend.
    /// Unsupported regions and compilation failures keep their existing interpreter.
    /// Call once before sharing/cloning; cloned expressions share immutable code.
    pub fn enable_native(&mut self) -> NativeStats {
        plan::native_prepare(&mut self.root)
    }
}

impl Expression {
    /// Evaluate with a shared random source. Seed it for reproducible evaluation;
    /// successive records consume the same stream. Pure expressions ignore it.
    pub fn evaluate_with_random<'e, 'i>(
        &'e self,
        input: &'i [u8],
        random: &Random,
    ) -> Result<Evaluation<'e, 'i>, Error> {
        if let Some(demand) = &self.acquisition {
            function::acquire::evaluate(&self.root, demand, input, Some(random))
        } else if self.runtime {
            evaluate::scalar(&self.root, input, Some(random), true)
        } else {
            self.evaluate(input)
        }
    }

    /// Validate the entire record before exposing results. Pure routes defer
    /// traversal; scalar operations and lexical retention finish before returning.
    #[inline]
    pub fn evaluate<'e, 'i>(&'e self, input: &'i [u8]) -> Result<Evaluation<'e, 'i>, Error> {
        if let Some(region) = &self.region {
            return region.evaluate(&self.root, input);
        }
        match &self.root.kind {
            expression::Kind::Path(path) => Ok(Evaluation {
                result: evaluate::Results::Path(path.select(input)?),
            }),
            expression::Kind::StaticLookup(data, key)
                if matches!(key.kind, expression::Kind::Path(_)) =>
            {
                let expression::Kind::Path(path) = &key.kind else {
                    unreachable!()
                };
                lookup::select(data, path, input, self.root.offset)
            }
            expression::Kind::Builtin(builtin, args)
                if builtin.is_conversion()
                    && args.len() == 1
                    && matches!(args[0].kind, expression::Kind::Path(_)) =>
            {
                let expression::Kind::Path(path) = &args[0].kind else {
                    unreachable!()
                };
                evaluate::path_conversion(*builtin, path, input, self.root.offset)
            }
            expression::Kind::Call(..) if self.acquisition.is_some() => {
                function::acquire::evaluate(
                    &self.root,
                    self.acquisition.as_ref().unwrap(),
                    input,
                    None,
                )
            }
            _ => evaluate::scalar(&self.root, input, None, self.runtime),
        }
    }
}
