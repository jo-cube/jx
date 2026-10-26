use crate::{
    Error, Evaluation, EvaluationOptions, Expression, RawJson,
    execution::{InputTemplate, Workspace},
};
/// Immutable, demand-driven input acquisition for independently compiled expressions.
/// Expressions keep their order and evaluation state; only static input paths share
/// captures. Array navigation retains its sequence semantics.
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
    expressions: Box<[&'e Expression]>,
    template: InputTemplate,
}
impl<'e> InputPlan<'e> {
    pub fn new(expressions: impl IntoIterator<Item = &'e Expression>) -> Self {
        let expressions: Box<[_]> = expressions.into_iter().collect();
        let template = InputTemplate::new(expressions.iter().map(|e| e.execution.as_ref()));
        Self {
            expressions,
            template,
        }
    }
    /// Validate once and acquire query-sized spans without running expressions.
    pub fn prepare<'p, 'i>(&'p self, input: &'i [u8]) -> Result<PreparedInput<'p, 'e, 'i>, Error> {
        let mut workspace = Workspace::default();
        let document = self.template.prepare(input, &mut workspace)?;
        let raw = document.raw();
        let text = document.text;
        Ok(PreparedInput {
            text,
            workspace,
            plan: self,
            raw,
        })
    }
    pub fn prepare_validated<'p, 'i>(&'p self, raw: RawJson<'i>) -> PreparedInput<'p, 'e, 'i> {
        let mut workspace = Workspace::default();
        self.template.prepare_validated(raw, &mut workspace);
        PreparedInput {
            text: raw.as_str(),
            workspace,
            plan: self,
            raw,
        }
    }
}
/// Borrowed input and query-sized offsets. Results borrow code/input, never scratch.
pub struct PreparedInput<'p, 'e, 'i> {
    text: &'i str,
    workspace: Workspace,
    plan: &'p InputPlan<'e>,
    raw: RawJson<'i>,
}
impl<'e, 'i> PreparedInput<'_, 'e, 'i> {
    pub fn as_raw(&self) -> RawJson<'i> {
        self.raw
    }
    /// Evaluate independently. Panics if the index is outside this plan.
    pub fn evaluate(&self, index: usize) -> Result<Evaluation<'e, 'i>, Error> {
        self.plan.expressions[index]
            .execution
            .evaluate(&self.plan.template.document(self.text, &self.workspace))
    }
    /// Evaluate with lexical/focus/control settings. Panics for an invalid index.
    pub fn evaluate_with(
        &self,
        index: usize,
        options: EvaluationOptions<'e, 'i>,
    ) -> Result<Evaluation<'e, 'i>, Error> {
        self.plan.expressions[index]
            .execution
            .evaluate_prepared_with(
                &self.plan.template.document(self.text, &self.workspace),
                options,
            )
    }
}
