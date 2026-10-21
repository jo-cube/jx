//! Public input/settings adaptation. All executable work stays in the program.
use super::{
    run::{Activation, Cell, Input},
    *,
};
use crate::{Error, Evaluation, EvaluationOptions, evaluate::Results};
impl Program {
    pub fn evaluate_with<'e, 'i>(
        &'e self,
        input: Option<&'i [u8]>,
        options: EvaluationOptions<'e, 'i>,
    ) -> Result<Evaluation<'e, 'i>, Error> {
        // Full grammar validation precedes bindings, cancellation and host calls.
        self.evaluate_validated_with(input.map(crate::validate).transpose()?, options)
    }
    pub fn evaluate_validated_with<'e, 'i>(
        &'e self,
        input: Option<crate::RawJson<'i>>,
        options: EvaluationOptions<'e, 'i>,
    ) -> Result<Evaluation<'e, 'i>, Error> {
        self.evaluate_settings(Input::Raw(input), options)
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
            return self.evaluate_input(input);
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
                false,
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
        let focus = options.focus.map_or(root, Cell::Value);
        let result = if self.deferred_options {
            Results::Execution(Box::new(Deferred::new(self, &input, focus, scope)))
        } else {
            let value = self.run(
                Activation {
                    input: &input,
                    focus: &focus,
                    wrapped: true,
                    args: &[],
                    outer: &[],
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
