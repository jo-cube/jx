//! Opt-in routing only. Compilation and execution remain in the common IR.
use super::{Program, Workspace};
use crate::{Error, Evaluation, EvaluationOptions, Expression, RawJson};
impl Expression {
    /// The selected IR program, or None for the default legacy engine.
    pub fn execution_program(&self) -> Option<&Program> {
        self.execution.as_deref()
    }
    /// Reuse acquisition offsets. Legacy expressions retain ordinary behavior.
    pub fn evaluate_with_workspace<'e, 'i>(
        &'e self,
        input: Option<&'i [u8]>,
        options: EvaluationOptions<'e, 'i>,
        workspace: &mut Workspace,
    ) -> Result<Evaluation<'e, 'i>, Error> {
        match &self.execution {
            Some(p) => p.evaluate_with_workspace(input, options, workspace),
            None => self.evaluate_with(input, options),
        }
    }
    pub fn evaluate_validated_with_workspace<'e, 'i>(
        &'e self,
        input: Option<RawJson<'i>>,
        options: EvaluationOptions<'e, 'i>,
        workspace: &mut Workspace,
    ) -> Result<Evaluation<'e, 'i>, Error> {
        match &self.execution {
            Some(p) => p.evaluate_validated_with_workspace(input, options, workspace),
            None => self.evaluate_validated_with(input, options),
        }
    }
}
