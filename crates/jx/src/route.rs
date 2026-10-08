use crate::{
    Error, Value,
    evaluate::Operand,
    expression::{Kind, Step},
    sequence::{Context, Halt, Output, View, Walk},
};

pub(crate) struct Map<'s, 'e, 'i> {
    input: View<'s, 'e, 'i>,
    step: &'e Step,
    last: bool,
    context: &'s Context<'e, 'i>,
}
impl<'e, 'i> Map<'_, 'e, 'i> {
    pub(crate) fn walk(&self, output: &mut Output<'_, 'e, 'i>) -> Walk {
        if let Some(value) = self.walk_items(output)? {
            output(value)?;
        }
        Ok(())
    }
    fn kept(&self) -> Result<Operand<'e, 'i>, Halt> {
        let mut items = Vec::new();
        if let Some(value) = self.walk_items(&mut |value| {
            items.push(value);
            Ok(())
        })? {
            if value.preserves_array() {
                items.push(value);
            } else {
                return Ok(Operand::One(value));
            }
        }
        Ok(if items.is_empty() {
            Operand::Missing
        } else {
            Operand::One(Value::kept(items))
        })
    }
    fn walk_items(&self, output: &mut Output<'_, 'e, 'i>) -> Result<Option<Value<'e, 'i>>, Halt> {
        let mut defined = false;
        let mut pending = None;
        self.input.transform(false, output, |value, output| {
            let context = Context {
                value,
                wrapped: self.step.lookup,
                scope: self.context.scope.clone(),
            };
            crate::filter::with_filters(
                &self.step.node,
                &self.step.predicates,
                &context,
                &mut |view| {
                    if matches!(view, View::Operand(Operand::Missing)) {
                        return Ok(());
                    }
                    if let Some(value) = pending.take() {
                        emit_mapped(value, output)?;
                    }
                    if self.last && !defined {
                        defined = true;
                        if let View::Operand(Operand::One(value)) = view
                            && value.is_array()
                            && !value.is_sequence()
                        {
                            pending = Some(value.clone());
                            return Ok(());
                        }
                    }
                    defined = true;
                    match &view {
                        View::Operand(Operand::One(value)) if value.preserves_array() => {
                            output(value.clone())
                        }
                        _ => view.candidates(false, output),
                    }
                },
            )
        })?;
        Ok(pending)
    }
}
fn emit_mapped<'e, 'i>(value: Value<'e, 'i>, output: &mut Output<'_, 'e, 'i>) -> Walk {
    if value.preserves_array() {
        output(value)
    } else {
        View::Operand(&Operand::One(value)).candidates(false, output)
    }
}
pub(crate) fn walk<'e, 'i>(
    steps: &'e [Step],
    array_focus: bool,
    keep: bool,
    context: &Context<'e, 'i>,
    output: &mut dyn FnMut(View<'_, 'e, 'i>) -> Walk,
) -> Walk {
    fn stages<'e, 'i>(
        input: View<'_, 'e, 'i>,
        steps: &'e [Step],
        keep: bool,
        context: &Context<'e, 'i>,
        output: &mut dyn FnMut(View<'_, 'e, 'i>) -> Walk,
    ) -> Walk {
        let Some((step, rest)) = steps.split_first() else {
            return output(input);
        };
        let map = Map {
            input,
            step,
            last: rest.is_empty(),
            context,
        };
        if keep && rest.is_empty() {
            return output(View::Operand(&map.kept()?));
        }
        // Stream::walk consumes the final view once. Only an intermediate
        // stateful stage needs storage to protect it from downstream replays.
        if step.effects && !rest.is_empty() {
            let mut items = Vec::new();
            map.walk(&mut |value| {
                items.push(value);
                Ok(())
            })?;
            stages(View::Items(&items), rest, keep, context, output)
        } else {
            stages(View::Map(&map), rest, keep, context, output)
        }
    }
    if array_focus {
        return crate::filter::with_filters(
            &steps[0].node,
            &steps[0].predicates,
            context,
            &mut |view| {
                if let View::Operand(Operand::One(value)) = &view
                    && !value.is_array()
                {
                    if matches!(value.atomic(), Value::Null) {
                        return Err(crate::value::type_error(steps[0].node.offset).into());
                    }
                    if value.string_body().is_some() {
                        return Err(crate::Error::new(
                            crate::ErrorKind::UnsupportedExpression,
                            steps[0].node.offset,
                            "string iteration after a filtered array constructor is deferred",
                        )
                        .into());
                    }
                    return output(View::Items(&[]));
                }
                if matches!(&view, View::Operand(Operand::One(value)) if value.is_array() && value.elements().next().is_none())
                {
                    if keep {
                        let mut items = Vec::new();
                        view.walk(&mut |value| {
                            items.push(value);
                            Ok(())
                        })?;
                        output(View::Operand(&Operand::One(Value::kept(items))))
                    } else {
                        output(view)
                    }
                } else {
                    stages(view, &steps[1..], keep, context, output)
                }
            },
        );
    }
    let variable = matches!(&steps[0].node.kind, Kind::Variable(_) | Kind::Sort(..))
        || matches!(&steps[0].node.kind, Kind::Prepared(p) if matches!(p.data, crate::constant::Storage::Shared(_)))
        || matches!(&steps[0].node.kind, Kind::Path(path) if path.rooted && path.fields.is_empty());
    if context.wrapped || variable {
        stages(
            View::Items(std::slice::from_ref(&context.value)),
            steps,
            keep,
            context,
            output,
        )
    } else {
        stages(
            View::Operand(&Operand::One(context.value.clone())),
            steps,
            keep,
            context,
            output,
        )
    }
}

pub(crate) fn keep<'e, 'i>(
    steps: &'e [Step],
    focus: bool,
    input: &Context<'e, 'i>,
) -> Result<Operand<'e, 'i>, Error> {
    let mut result = Operand::Missing;
    let walked = walk(steps, focus, true, input, &mut |view| {
        view.walk(&mut |value| {
            result = Operand::One(value);
            Ok(())
        })
    });
    match walked {
        Ok(()) => Ok(result),
        Err(Halt::Evaluation(error)) => Err(error),
        Err(Halt::Stop) => unreachable!(),
    }
}
