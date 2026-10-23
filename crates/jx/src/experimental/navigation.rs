//! General route/filter execution over compiled IR bodies. Views are stack scoped;
//! only effectful replay boundaries retain candidates. Mapping and selection use
//! the same cardinality, cancellation and error-order kernels as the reference.
use super::{
    frame::Frame,
    run::{Activation, Cell, Input},
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
    input_view: View<'_, 'e, 'i>,
    predicates: &'e [Predicate],
    a: Activation<'_, 'e, 'i, '_>,
    emit: &mut dyn FnMut(View<'_, 'e, 'i>) -> Walk,
) -> Walk {
    let Some((p, rest)) = predicates.split_first() else {
        return emit(input_view);
    };
    if let Some(index) = p.position {
        return crate::filter::position(input_view, index, &mut |view| {
            filters(view, rest, a, emit)
        });
    }
    let length = std::cell::Cell::new(None);
    let filtered = |output: &mut Output<'_, 'e, 'i>| {
        let mut index = 0;
        let mut frame = Frame::repeated();
        let mut test =
            |input: &Input<'_, 'i, '_>, focus: Cell<'e, 'i>, output: &mut Output<'_, 'e, 'i>| {
                let predicate = p.body.run(
                    Activation {
                        input,
                        focus: &focus,
                        wrapped: false,
                        ..a
                    },
                    &mut frame,
                )?;
                crate::filter::select(
                    &predicate.operand(input),
                    p.body.offset,
                    index,
                    || {
                        if let Some(n) = length.get() {
                            return Ok(n);
                        }
                        let mut n = 0;
                        input_view.candidates(true, &mut |_| {
                            n += 1;
                            Ok(())
                        })?;
                        length.set(Some(n));
                        Ok(n)
                    },
                    &mut || output(focus.value(input).unwrap_or(Value::Undefined)),
                )?;
                index += 1;
                Ok(())
            };
        // A retained operand is replay-free. Acquire its candidate schema in
        // the same scan that finds elements, rather than scanning each element
        // again for every predicate field. Keep earlier predicate errors ahead
        // of downstream failures; consumer cancellation still stops immediately.
        let source = match input_view {
            View::Operand(Operand::One(Value::Undefined)) => Some(Cell::Value(Value::Undefined)),
            View::Operand(Operand::One(value)) => Some(a.input.restore(Some(value.clone()))),
            View::Operand(Operand::Missing) => Some(Cell::Value(Value::Undefined)),
            _ => None,
        };
        if let Some(source) = source {
            let mut downstream = None;
            a.input
                .each(&source, &p.body.row_schema, |input, focus, _| {
                    test(input, focus, &mut |value| {
                        if downstream.is_some() {
                            return Ok(());
                        }
                        match output(value) {
                            Err(Halt::Evaluation(error)) => {
                                downstream = Some(error);
                                Ok(())
                            }
                            result => result,
                        }
                    })
                })?;
            downstream.map_or(Ok(()), |error| Err(Halt::Evaluation(error)))
        } else {
            input_view.transform(true, output, |value, output| {
                a.input.each_values(
                    std::iter::once(value),
                    &p.body.row_schema,
                    |_| true,
                    |input, focus, _| test(input, focus, output),
                )
            })
        }
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
        complete: bool,
    ) -> Walk {
        fn stages<'e, 'i>(
            input: View<'_, 'e, 'i>,
            steps: &'e [Step],
            a: Activation<'_, 'e, 'i, '_>,
            output: &mut Output<'_, 'e, 'i>,
            complete: bool,
        ) -> Walk {
            let Some((step, rest)) = steps.split_first() else {
                return input.walk(output);
            };
            let mapped = |out: &mut Output<'_, 'e, 'i>| {
                let mut frame = Frame::repeated();
                let pending =
                    crate::route::map_items(input, rest.is_empty(), out, |value, emit| {
                        // General stages honor the same acquisition demands as
                        // compact row kernels. Export values while the borrowed
                        // row spans are live; later stages may restore them.
                        a.input.each_values(
                            std::iter::once(value),
                            &step.body.row_schema,
                            |_| true,
                            |input, focus, _| {
                                step.body.with_view(
                                    Activation {
                                        input,
                                        focus: &focus,
                                        wrapped: step.lookup,
                                        ..a
                                    },
                                    &mut frame,
                                    complete && !step.effects && rest.iter().all(|s| !s.effects),
                                    &mut |view| {
                                        filters(
                                            view,
                                            &step.predicates,
                                            Activation { input, ..a },
                                            emit,
                                        )
                                    },
                                )
                            },
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
                stages(View::Items(&values), rest, a, output, complete)
            } else {
                stages(View::Producer(&mapped), rest, a, output, complete)
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
                    stages(view, &self.steps[1..], a, output, complete)
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
                complete,
            )
        } else {
            stages(
                View::Operand(&Operand::One(focus)),
                &self.steps,
                a,
                output,
                complete,
            )
        }
    }
}
pub(super) fn walk<'e, 'i>(
    op: &'e Operation,
    a: Activation<'_, 'e, 'i, '_>,
    frame: &[Cell<'e, 'i>],
    offset: usize,
    output: &mut Output<'_, 'e, 'i>,
    complete: bool,
) -> Walk {
    match op {
        Operation::Correlate(region) => context::walk(region, a, output),
        Operation::Navigate(n) => n.walk(a, output, complete),
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
    complete: bool,
) -> Result<(), crate::ConsumeError<E>> {
    let mut consumer = None;
    let result = walk(
        op,
        a,
        frame,
        offset,
        &mut |v| match output(v) {
            Ok(()) => Ok(()),
            Err(crate::ConsumeError::Evaluation(e)) => Err(Halt::Evaluation(e)),
            Err(crate::ConsumeError::Consumer(e)) => {
                consumer = Some(e);
                Err(Halt::Stop)
            }
        },
        complete,
    );
    match result {
        Ok(()) => Ok(()),
        Err(Halt::Evaluation(e)) => Err(crate::ConsumeError::Evaluation(e)),
        Err(Halt::Stop) => Err(crate::ConsumeError::Consumer(consumer.unwrap())),
    }
}
