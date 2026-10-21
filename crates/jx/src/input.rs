use crate::{
    Error, Evaluation, EvaluationOptions, Expression, RawJson, acquire,
    expression::{Kind, Path},
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
    expressions: Box<[InputExpression<'e>]>,
    paths: Box<[Path]>,
    demand: Demand,
}
#[derive(Debug)]
struct InputExpression<'e> {
    expression: &'e Expression,
    mask: u32,
    // Global capture slot -> the plan's local demand slots, resolved once.
    plan_slots: Box<[(u8, u32)]>,
}
impl<'e> InputPlan<'e> {
    pub fn new(expressions: impl IntoIterator<Item = &'e Expression>) -> Self {
        let mut paths: Vec<Path> = Vec::new();
        let expressions = expressions
            .into_iter()
            .map(|expression| {
                let mut planned = false;
                let eligible = if expression.runtime {
                    None
                } else if let Kind::Plan(plan) = &expression.root.kind {
                    planned = true;
                    let mut paths = Vec::new();
                    plan.demand().visit_paths(&mut |fields, slots| {
                        paths.push((
                            Path {
                                fields: fields.into(),
                                rooted: false,
                            },
                            slots,
                        ));
                    });
                    Some(paths)
                } else {
                    acquire::paths(&expression.root, 1)
                        .map(|paths| paths.into_vec().into_iter().map(|p| (p, 0)).collect())
                };
                let mut mask = 0;
                let mut plan_slots = Vec::new();
                if let Some(eligible) = eligible {
                    let extra = eligible
                        .iter()
                        .filter(|(p, _)| !paths.iter().any(|q| p.fields == q.fields))
                        .count();
                    if paths.len() + extra <= CAPTURE_SLOTS {
                        for (path, local_slots) in eligible {
                            let slot = paths
                                .iter()
                                .position(|p| p.fields == path.fields)
                                .unwrap_or_else(|| {
                                    paths.push(path);
                                    paths.len() - 1
                                });
                            mask |= 1 << slot;
                            if planned {
                                plan_slots.push((slot as u8, local_slots));
                            }
                        }
                    }
                }
                InputExpression {
                    expression,
                    mask,
                    plan_slots: plan_slots.into_boxed_slice(),
                }
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
        let entry = &self.plan.expressions[index];
        let expression = entry.expression;
        let mask = entry.mask;
        if mask != 0 && self.raw.as_bytes()[0] != b'[' && !self.captures.has_deferred(mask) {
            if !entry.plan_slots.is_empty() {
                let Kind::Plan(plan) = &expression.root.kind else {
                    unreachable!("compiled input plan")
                };
                let mut local = Captures::default();
                for &(global, slots) in &entry.plan_slots {
                    local.fill(slots, self.captures.get(usize::from(global)));
                }
                Ok(Evaluation {
                    result: crate::evaluate::results(plan.evaluate_captured(self.raw, &local)?),
                })
            } else {
                acquire::evaluate_captured(
                    &expression.root,
                    self.raw,
                    &self.plan.paths,
                    &self.captures,
                )
            }
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
                .expression
                .evaluate_validated_with(Some(self.raw), options)
        }
    }
}
