use crate::{
    Error, Evaluation, EvaluationOptions, Expression, RawJson, acquire,
    expression::Path,
    json::{CAPTURE_SLOTS, Captures, Demand},
};

/// Immutable, bounded input acquisition for independently compiled expressions.
/// Expressions keep their order and evaluation state; only static input paths share
/// captures. Unsupported expressions and demands beyond the bound evaluate normally.
///
/// ```
/// let expressions = [jx::compile("price > 2")?, jx::compile("price * quantity")?];
/// let plan = jx::InputPlan::new(&expressions);
/// let input = plan.prepare(br#"{"price":2.5,"quantity":3}"#)?;
/// assert_eq!(input.evaluate(0)?.single()?.unwrap().as_bool(), Some(true));
/// assert_eq!(input.evaluate(1)?.single()?.unwrap().as_number(), Some(7.5));
/// # Ok::<(), jx::Error>(())
/// ```
#[derive(Debug)]
pub struct InputPlan<'e> {
    expressions: Box<[(&'e Expression, u32)]>,
    paths: Box<[Path]>,
    demand: Demand,
}
impl<'e> InputPlan<'e> {
    pub fn new(expressions: impl IntoIterator<Item = &'e Expression>) -> Self {
        let mut paths: Vec<Path> = Vec::new();
        let expressions = expressions
            .into_iter()
            .map(|expression| {
                let eligible = (!expression.runtime)
                    .then(|| acquire::paths(&expression.root, 1))
                    .flatten();
                let mut mask = 0;
                if let Some(eligible) = eligible {
                    let extra = eligible
                        .iter()
                        .filter(|p| !paths.iter().any(|q| p.fields == q.fields))
                        .count();
                    if paths.len() + extra <= CAPTURE_SLOTS {
                        for path in eligible {
                            let slot = paths
                                .iter()
                                .position(|p| p.fields == path.fields)
                                .unwrap_or_else(|| {
                                    paths.push(path);
                                    paths.len() - 1
                                });
                            mask |= 1 << slot;
                        }
                    }
                }
                (expression, mask)
            })
            .collect();
        let mut demand = Demand::default();
        for (slot, path) in paths.iter().enumerate() {
            demand.insert(&path.fields, slot);
        }
        Self {
            expressions,
            paths: paths.into_boxed_slice(),
            demand,
        }
    }

    /// Validate the complete input and capture eligible paths in that traversal.
    /// Does not evaluate any expression or run its effects.
    pub fn prepare<'p, 'i>(&'p self, input: &'i [u8]) -> Result<PreparedInput<'p, 'e, 'i>, Error> {
        let mut captures = Captures::default();
        let raw = if self.demand.is_empty() {
            crate::validate(input)?
        } else {
            crate::json::capture(input, &self.demand, &mut captures)?
        };
        Ok(PreparedInput {
            plan: self,
            raw,
            captures,
        })
    }

    /// Capture paths from an already validated value without validating it again.
    pub fn prepare_validated<'p, 'i>(&'p self, raw: RawJson<'i>) -> PreparedInput<'p, 'e, 'i> {
        let mut captures = Captures::default();
        if !self.demand.is_empty() {
            raw.capture(&self.demand, &mut captures);
        }
        PreparedInput {
            plan: self,
            raw,
            captures,
        }
    }
}

/// Borrowed input and bounded captures; no lexical state or evaluated results.
/// Results borrow the original input and expressions, never this capture frame.
pub struct PreparedInput<'p, 'e, 'i> {
    plan: &'p InputPlan<'e>,
    raw: RawJson<'i>,
    captures: Captures<'i>,
}
impl<'e, 'i> PreparedInput<'_, 'e, 'i> {
    /// Return the validated root, borrowing the original input.
    pub fn as_raw(&self) -> RawJson<'i> {
        self.raw
    }

    /// Evaluate one expression independently. May be called repeatedly or in any order.
    /// Deferred array paths use that expression's ordinary validated-input evaluation.
    ///
    /// # Panics
    /// Panics if `index` is outside the expressions supplied to the plan.
    #[inline]
    pub fn evaluate(&self, index: usize) -> Result<Evaluation<'e, 'i>, Error> {
        let (expression, mask) = self.plan.expressions[index];
        if mask != 0 && self.raw.as_bytes()[0] != b'[' && !self.captures.has_deferred(mask) {
            acquire::evaluate_captured(&expression.root, self.raw, &self.plan.paths, &self.captures)
        } else {
            expression.evaluate_validated(self.raw)
        }
    }

    /// Evaluate with per-expression settings. Focus, bindings and controls use
    /// ordinary evaluation; default options reuse the shared captures.
    ///
    /// # Panics
    /// Panics if `index` is outside the expressions supplied to the plan.
    pub fn evaluate_with(
        &self,
        index: usize,
        options: EvaluationOptions<'e, 'i>,
    ) -> Result<Evaluation<'e, 'i>, Error> {
        if options.bindings.is_empty()
            && options.focus.is_none()
            && options.limits.is_none()
            && options.cancellation.is_none()
            && options.deadline.is_none()
            && options.random.is_none()
        {
            self.evaluate(index)
        } else {
            self.plan.expressions[index]
                .0
                .evaluate_validated_with(Some(self.raw), options)
        }
    }
}
