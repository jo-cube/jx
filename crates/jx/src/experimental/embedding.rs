//! Public input/settings adaptation. All executable work stays in the program.
use super::{
    run::{Activation, Input},
    *,
};
use crate::{Error, Evaluation, EvaluationOptions, evaluate::Results};
impl Program {
    pub fn evaluate_with<'e, 'i>(
        &'e self,
        input: Option<&'i [u8]>,
        options: EvaluationOptions<'e, 'i>,
    ) -> Result<Evaluation<'e, 'i>, Error> {
        self.evaluate_with_workspace(input, options, &mut Workspace::default())
    }
    pub fn evaluate_validated_with<'e, 'i>(
        &'e self,
        input: Option<crate::RawJson<'i>>,
        options: EvaluationOptions<'e, 'i>,
    ) -> Result<Evaluation<'e, 'i>, Error> {
        self.evaluate_validated_with_workspace(input, options, &mut Workspace::default())
    }
    /// Reuse query-sized offsets; returned values never borrow the workspace.
    pub fn evaluate_with_workspace<'e, 'i>(
        &'e self,
        input: Option<&'i [u8]>,
        options: EvaluationOptions<'e, 'i>,
        workspace: &mut Workspace,
    ) -> Result<Evaluation<'e, 'i>, Error> {
        if let (Some(bytes), Some(template)) = (input, self.input_template.as_ref())
            && template.has_fields()
        {
            let document = template.prepare(bytes, workspace)?;
            self.evaluate_settings(Input::Captured(&document), options)
        } else {
            // Validation precedes bindings, cancellation and host calls.
            self.evaluate_settings(Input::Raw(input.map(crate::validate).transpose()?), options)
        }
    }
    pub fn evaluate_validated_with_workspace<'e, 'i>(
        &'e self,
        input: Option<crate::RawJson<'i>>,
        options: EvaluationOptions<'e, 'i>,
        workspace: &mut Workspace,
    ) -> Result<Evaluation<'e, 'i>, Error> {
        if let (Some(raw), Some(template)) = (input, self.input_template.as_ref())
            && template.has_fields()
        {
            let document = template.prepare_validated(raw, workspace);
            self.evaluate_settings(Input::Captured(&document), options)
        } else {
            self.evaluate_settings(Input::Raw(input), options)
        }
    }
    pub fn evaluate_prepared_with<'e, 'i>(
        &'e self,
        input: &Prepared<'i, '_>,
        options: EvaluationOptions<'e, 'i>,
    ) -> Result<Evaluation<'e, 'i>, Error> {
        self.evaluate_settings(Input::Captured(input), options)
    }
    fn evaluate_settings<'e, 'i>(
        &'e self,
        input: Input<'_, 'i, '_>,
        options: EvaluationOptions<'e, 'i>,
    ) -> Result<Evaluation<'e, 'i>, Error> {
        if options.bindings.is_empty()
            && options.focus.is_none()
            && options.limits.is_none()
            && options.cancellation.is_none()
            && options.deadline.is_none()
            && input.root().value(&input).is_some()
        {
            return self.evaluate_input_random(input, options.random.as_ref());
        }
        let focus_functions = crate::embedding::validate_options(&self.bindings, &options)?;
        let control =
            crate::controls::Control::new(options.limits, options.cancellation, options.deadline);
        let root = input.root();
        let scope = if self.scoped
            || focus_functions
            || !options.bindings.is_empty()
            || control.is_some()
        {
            let scope = crate::runtime::Scope::with_random(
                root.value(&input).unwrap_or(Value::Undefined),
                self.clock,
                options.random.as_ref(),
            )
            .for_program()
            .constants(&self.constants)
            .configure(options.bindings, control.clone());
            scope.checkpoint(self.offset)?;
            Some(scope)
        } else {
            None
        };
        // Explicit focus is value movement too: retain an existing acquired port
        // only when its raw byte identity matches this input preparation.
        let focus = options.focus.map_or(root, |v| input.restore(Some(v)));
        let total = control.is_none()
            && self.deferred
            && matches!(self.delivery, consume::Delivery::Stream { total: true, .. });
        let result = if self.deferred_options || total {
            Deferred::result(self, &input, focus, scope)?
        } else {
            let value = self.run(
                Activation {
                    input: &input,
                    focus: &focus,
                    wrapped: true,
                    args: &[],
                    outer: Default::default(),
                    calls: Default::default(),
                    scope: scope.as_ref(),
                },
                &mut frame::Frame::new(),
            )?;
            Results::Scalar(value.into_value(&input))
        };
        Ok(Evaluation {
            result: match control {
                Some(c) => Results::Controlled(Box::new((result, c))),
                None => result,
            },
        })
    }
}
