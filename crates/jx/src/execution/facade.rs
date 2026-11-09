//! Public execution-state access; scratch is always caller-owned.
use super::{Program, Workspace};
use crate::{Error, Evaluation, EvaluationOptions, Expression, RawJson};
impl Expression {
    /// Whether this expression can be evaluated without the original JSON input.
    /// The proof assumes the same external bindings and evaluation settings.
    /// It is conservative about implicit context, closures and dynamic calls;
    /// it does not imply purity or JSON-encodable results. Supplied input is
    /// still validated by the ordinary evaluation APIs.
    pub fn input_dependency(&self) -> crate::InputDependency {
        self.execution.input_dependency
    }

    /// Immutable executable code shared by every evaluation entry point.
    pub fn execution_program(&self) -> &Program {
        &self.execution
    }
    pub fn evaluate_with_workspace<'e, 'i>(
        &'e self,
        input: Option<&'i [u8]>,
        options: EvaluationOptions<'e, 'i>,
        workspace: &mut Workspace,
    ) -> Result<Evaluation<'e, 'i>, Error> {
        self.execution
            .evaluate_with_workspace(input, options, workspace)
    }
    pub fn evaluate_validated_with_workspace<'e, 'i>(
        &'e self,
        input: Option<RawJson<'i>>,
        options: EvaluationOptions<'e, 'i>,
        workspace: &mut Workspace,
    ) -> Result<Evaluation<'e, 'i>, Error> {
        self.execution
            .evaluate_validated_with_workspace(input, options, workspace)
    }
}
