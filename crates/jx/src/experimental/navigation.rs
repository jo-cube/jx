//! General route/filter execution over compiled IR bodies. Views are stack scoped;
//! only effectful replay boundaries retain candidates. Mapping and selection use
//! the same cardinality, cancellation and error-order kernels as the reference.
use super::{
    frame::Frame,
    run::{Activation, Cell},
    *,
};
use crate::{
    evaluate::Operand,
    sequence::{Halt, Output, View, Walk},
};
#[derive(Clone, Debug)]
pub(super) struct Predicate {
    pub body: Program,
    pub position: Option<f64>,
    pub effects: bool,
}
#[derive(Clone, Debug)]
pub(super) struct Step {
    pub body: Program,
    pub predicates: Box<[Predicate]>,
    pub lookup: bool,
    pub effects: bool,
}
#[derive(Clone, Debug)]
pub(super) struct Navigation {
    pub steps: Box<[Step]>,
    pub array_focus: bool,
    pub variable: bool,
}

pub(super) fn filters<'e, 'i>(
    input: View<'_, 'e, 'i>,
    predicates: &'e [Predicate],
    a: Activation<'_, 'e, 'i, '_>,
    emit: &mut dyn FnMut(View<'_, 'e, 'i>) -> Walk,
) -> Walk {
    let Some((p, rest)) = predicates.split_first() else {
        return emit(input);
    };
    if let Some(index) = p.position {
        return crate::filter::position(input, index, &mut |view| filters(view, rest, a, emit));
    }
    let length = std::cell::Cell::new(None);
    let filtered = |output: &mut Output<'_, 'e, 'i>| {
        let mut index = 0;
        let mut frame = Frame::repeated();
        input.transform(true, output, |value, output| {
            let focus = a.input.restore(Some(value.clone()));
            let predicate = p.body.run(
                Activation {
                    focus: &focus,
                    wrapped: false,
                    ..a
                },
                &mut frame,
            )?;
            crate::filter::select(
                &predicate.operand(a.input),
                p.body.offset,
                index,
                || {
                    if let Some(n) = length.get() {
                        return Ok(n);
                    }
                    let mut n = 0;
                    input.candidates(true, &mut |_| {
                        n += 1;
                        Ok(())
                    })?;
                    length.set(Some(n));
                    Ok(n)
                },
                &mut || output(value.clone()),
            )?;
            index += 1;
            Ok(())
        })
    };
    if p.effects {
        let mut values = Vec::new();
        filtered(&mut |v| {
            values.push(v);
            Ok(())
        })?;
        filters(View::Items(&values), rest, a, emit)
    } else {
        filters(View::Producer(&filtered), rest, a, emit)
    }
}
impl Navigation {
    pub(super) fn walk<'e, 'i>(
        &'e self,
        a: Activation<'_, 'e, 'i, '_>,
        output: &mut Output<'_, 'e, 'i>,
    ) -> Walk {
        fn stages<'e, 'i>(
            input: View<'_, 'e, 'i>,
            steps: &'e [Step],
            a: Activation<'_, 'e, 'i, '_>,
            output: &mut Output<'_, 'e, 'i>,
        ) -> Walk {
            let Some((step, rest)) = steps.split_first() else {
                return input.walk(output);
            };
            let mapped = |out: &mut Output<'_, 'e, 'i>| {
                let mut frame = Frame::repeated();
                let pending =
                    crate::route::map_items(input, rest.is_empty(), out, |value, emit| {
                        let focus = a.input.restore(Some(value));
                        let value = step.body.run(
                            Activation {
                                focus: &focus,
                                wrapped: step.lookup,
                                ..a
                            },
                            &mut frame,
                        )?;
                        filters(
                            View::Operand(&value.operand(a.input)),
                            &step.predicates,
                            a,
                            emit,
                        )
                    })?;
                if let Some(value) = pending {
                    out(value)?;
                }
                Ok(())
            };
            if step.effects && !rest.is_empty() {
                let mut values = Vec::new();
                mapped(&mut |v| {
                    values.push(v);
                    Ok(())
                })?;
                stages(View::Items(&values), rest, a, output)
            } else {
                stages(View::Producer(&mapped), rest, a, output)
            }
        }
        if self.array_focus {
            let first = &self.steps[0];
            let mut frame = Frame::new();
            let value = first.body.run(a, &mut frame)?;
            return filters(
                View::Operand(&value.operand(a.input)),
                &first.predicates,
                a,
                &mut |view| {
                    if let View::Operand(Operand::One(value)) = view {
                        if !value.is_array() {
                            if matches!(value.atomic(), Value::Null) {
                                return Err(crate::value::type_error(first.body.offset).into());
                            }
                            if value.string_body().is_some() {
                                return Err(crate::Error::new(crate::ErrorKind::UnsupportedExpression, first.body.offset, "string iteration after a filtered array constructor is deferred").into());
                            }
                            return Ok(());
                        }
                        if value.elements().next().is_none() {
                            return view.walk(output);
                        }
                    }
                    stages(view, &self.steps[1..], a, output)
                },
            );
        }
        let focus = a.focus.value(a.input).unwrap_or(Value::Undefined);
        if a.wrapped || self.variable {
            stages(
                View::Items(std::slice::from_ref(&focus)),
                &self.steps,
                a,
                output,
            )
        } else {
            stages(View::Operand(&Operand::One(focus)), &self.steps, a, output)
        }
    }
}
pub(super) fn walk<'e, 'i>(
    op: &'e Operation,
    a: Activation<'_, 'e, 'i, '_>,
    frame: &[Cell<'e, 'i>],
    offset: usize,
    output: &mut Output<'_, 'e, 'i>,
) -> Walk {
    match op {
        Operation::Navigate(n) => n.walk(a, output),
        Operation::Select(source, predicates) => filters(
            View::Operand(&frame[*source].operand(a.input)),
            predicates,
            a,
            &mut |view| view.walk(output),
        ),
        Operation::Range(a_reg, b) => crate::navigate::range_values(
            frame[*a_reg].operand(a.input),
            frame[*b].operand(a.input),
            offset,
            output,
        ),
        Operation::Traverse(descendants) => {
            let value = a.focus.value(a.input).unwrap_or(Value::Undefined);
            if *descendants {
                crate::navigate::descendants(value, output)
            } else {
                crate::navigate::wildcard(&value, output)
            }
        }
        _ => unreachable!("general sequence operation"),
    }
}
pub(super) fn consume<'e, 'i, E>(
    op: &'e Operation,
    a: Activation<'_, 'e, 'i, '_>,
    frame: &[Cell<'e, 'i>],
    offset: usize,
    output: &mut dyn FnMut(Value<'e, 'i>) -> Result<(), crate::ConsumeError<E>>,
) -> Result<(), crate::ConsumeError<E>> {
    let mut consumer = None;
    let result = walk(op, a, frame, offset, &mut |v| match output(v) {
        Ok(()) => Ok(()),
        Err(crate::ConsumeError::Evaluation(e)) => Err(Halt::Evaluation(e)),
        Err(crate::ConsumeError::Consumer(e)) => {
            consumer = Some(e);
            Err(Halt::Stop)
        }
    });
    match result {
        Ok(()) => Ok(()),
        Err(Halt::Evaluation(e)) => Err(crate::ConsumeError::Evaluation(e)),
        Err(Halt::Stop) => Err(crate::ConsumeError::Consumer(consumer.unwrap())),
    }
}
