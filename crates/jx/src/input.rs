use crate::{
    ConsumeError, Error, Evaluation, EvaluationOptions, Expression, RawJson,
    execution::{InputTemplate, Workspace},
};
/// Conservative dependence on the original input's focus or root.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InputDependency {
    /// Evaluation does not semantically observe the original focus or root.
    Independent,
    /// Independence has not been proved; the expression may need the input.
    MayRequireInput,
}

/// Conservative demand on a declared external lexical binding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BindingDemand<'a> {
    /// The binding is not observed by this expression.
    Unused,
    /// Only these top-level object fields of the binding may be observed.
    Fields(&'a [Box<str>]),
    /// The whole binding or arbitrary fields may be observed.
    All,
}

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
            workspace: WorkspaceRef::Owned(workspace),
            plan: self,
            raw,
        })
    }
    /// Validate once and acquire query-sized spans using caller-owned reusable scratch.
    pub fn prepare_with_workspace<'p, 'i>(
        &'p self,
        input: &'i [u8],
        workspace: &'p mut Workspace,
    ) -> Result<PreparedInput<'p, 'e, 'i>, Error> {
        let document = self.template.prepare(input, workspace)?;
        let raw = document.raw();
        let text = document.text;
        Ok(PreparedInput {
            text,
            workspace: WorkspaceRef::Borrowed(workspace),
            plan: self,
            raw,
        })
    }
    pub fn prepare_validated<'p, 'i>(&'p self, raw: RawJson<'i>) -> PreparedInput<'p, 'e, 'i> {
        let mut workspace = Workspace::default();
        self.template.prepare_validated(raw, &mut workspace);
        PreparedInput {
            text: raw.as_str(),
            workspace: WorkspaceRef::Owned(workspace),
            plan: self,
            raw,
        }
    }
    /// Acquire spans for validated input using caller-owned reusable scratch.
    pub fn prepare_validated_with_workspace<'p, 'i>(
        &'p self,
        raw: RawJson<'i>,
        workspace: &'p mut Workspace,
    ) -> PreparedInput<'p, 'e, 'i> {
        self.template.prepare_validated(raw, workspace);
        PreparedInput {
            text: raw.as_str(),
            workspace: WorkspaceRef::Borrowed(workspace),
            plan: self,
            raw,
        }
    }
}
enum WorkspaceRef<'p> {
    Owned(Workspace),
    Borrowed(&'p Workspace),
}
impl WorkspaceRef<'_> {
    fn as_ref(&self) -> &Workspace {
        match self {
            Self::Owned(workspace) => workspace,
            Self::Borrowed(workspace) => workspace,
        }
    }
}
/// Borrowed input and query-sized offsets. Results borrow code/input, never scratch.
pub struct PreparedInput<'p, 'e, 'i> {
    text: &'i str,
    workspace: WorkspaceRef<'p>,
    plan: &'p InputPlan<'e>,
    raw: RawJson<'i>,
}
impl<'e, 'i> PreparedInput<'_, 'e, 'i> {
    pub fn as_raw(&self) -> RawJson<'i> {
        self.raw
    }
    /// Evaluate independently. Panics if the index is outside this plan.
    pub fn evaluate(&self, index: usize) -> Result<Evaluation<'e, 'i>, Error> {
        self.plan.expressions[index].execution.evaluate(
            &self
                .plan
                .template
                .document(self.text, self.workspace.as_ref()),
        )
    }
    /// Append one compact JSON value, wrapping multiple results in an array.
    /// Missing returns false. On evaluation or encoding failure, restore the
    /// original buffer length; evaluation errors take precedence over encoding
    /// failures. Panics if the index is outside this plan.
    pub fn write_json(
        &self,
        index: usize,
        output: &mut Vec<u8>,
    ) -> Result<bool, ConsumeError<std::io::Error>> {
        self.plan.expressions[index].execution.write_json(
            &self
                .plan
                .template
                .document(self.text, self.workspace.as_ref()),
            output,
        )
    }
    /// Append complete compact JSON with lexical/focus/control settings.
    /// Uses the same cardinality, encoding and rollback rules as [`Self::write_json`].
    /// Panics if the index is outside this plan.
    pub fn write_json_with(
        &self,
        index: usize,
        options: EvaluationOptions<'e, 'i>,
        output: &mut Vec<u8>,
    ) -> Result<bool, ConsumeError<std::io::Error>> {
        self.plan.expressions[index].execution.write_json_with(
            &self
                .plan
                .template
                .document(self.text, self.workspace.as_ref()),
            options,
            output,
        )
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
                &self
                    .plan
                    .template
                    .document(self.text, self.workspace.as_ref()),
                options,
            )
    }
}
