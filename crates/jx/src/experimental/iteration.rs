//! Execution of semantic sequence producers and complete consumers. Keeping
//! their per-record iteration state outside value dispatch bounds that dispatch's
//! stack/code footprint. This module owns no planner or alternate instruction set.
use super::{
    run::{Activation, Arguments, Cell},
    *,
};
use crate::{Error, evaluate::Operand};

#[inline(never)]
pub(super) fn execute<'e, 'i>(
    instruction: &'e Instruction,
    activation: Activation<'_, 'e, 'i, '_>,
    reads: Option<bindings::Reads<'_>>,
) -> Result<Cell<'e, 'i>, Error> {
    let Activation {
        input,
        focus,
        wrapped,
        scope,
        calls,
        outer: frame,
        ..
    } = activation;
    let offset = instruction.offset;
    let value = match &instruction.kind {
        Operation::Navigate(_)
        | Operation::Select(..)
        | Operation::Range(..)
        | Operation::Traverse(_) => {
            let mut values = Vec::new();
            navigation::walk(&instruction.kind, activation, frame, offset, &mut |v| {
                values.push(v);
                Ok(())
            })
            .map_err(|e| match e {
                crate::sequence::Halt::Evaluation(e) => e,
                crate::sequence::Halt::Stop => unreachable!(),
            })?;
            sequence(values)
        }
        Operation::Path(path) => {
            let mut values = Vec::new();
            path.walk(activation, &frame[path.source], offset, &mut |value| {
                values.push(value);
                Ok::<_, crate::ConsumeError<std::convert::Infallible>>(())
            })
            .map_err(|e| match e {
                crate::ConsumeError::Evaluation(e) => e,
                crate::ConsumeError::Consumer(e) => match e {},
            })?;
            sequence(values)
        }
        Operation::Each {
            source,
            body,
            kind,
            native,
        } => {
            let mut child_frame = frame::Frame::repeated();
            let mut output = Vec::new();
            let mut invocation = call::Callback::new(scope, calls);
            let mut arguments = Arguments::new(input, &frame[*source], body);
            input.each(
                &frame[*source],
                &body.row_schema,
                |child_input, item, index| {
                    if let Some(scope) = scope.filter(|s| s.controlled()) {
                        scope.item(offset)?;
                    }
                    let args = arguments.at(&item, index);
                    let result = invocation.run(body, |calls| {
                        body.run(
                            Activation {
                                input: child_input,
                                focus,
                                wrapped,
                                args,
                                outer: frame,
                                calls,
                                scope,
                            },
                            &mut child_frame,
                        )
                    })?;
                    if *kind == Each::Filter {
                        if result.truth(child_input, offset)?
                            && let Some(v) = item.value(child_input)
                        {
                            output.push(v);
                        }
                    } else if let Some(value) = result.into_value(child_input) {
                        output.push(value);
                    }
                    Ok::<_, Error>(())
                },
            )?;
            if *native && frame[*source].value(input).is_some() {
                Cell::Value(Value::sequence(output))
            } else {
                sequence(output)
            }
        }
        Operation::Reduce(aggregate, producer, producer_offset) => {
            let mut reduction = Reduction::new(*aggregate);
            let mut push = |value| {
                reduction.push(value);
                Ok::<_, crate::ConsumeError<std::convert::Infallible>>(())
            };
            let result = match &**producer {
                Operation::Field(base, path, contextual, FieldMode::Value) => input
                    .select_field(
                        &frame[*base],
                        path,
                        *contextual && wrapped,
                        reads.and_then(|r| r.get(instruction.out)),
                    )
                    .walk(input, &mut push),
                Operation::Navigate(_)
                | Operation::Select(..)
                | Operation::Range(..)
                | Operation::Traverse(_) => {
                    navigation::consume(producer, activation, frame, *producer_offset, &mut push)
                }
                Operation::Path(path) => {
                    path.walk(activation, &frame[path.source], *producer_offset, &mut push)
                }
                _ => unreachable!("complete sequence producer"),
            };
            result.map_err(|e| match e {
                crate::ConsumeError::Evaluation(e) => e,
                crate::ConsumeError::Consumer(e) => match e {},
            })?;
            Cell::from_operand(reduction.finish(offset)?)?
        }
        Operation::Fold {
            source,
            body,
            predicate,
            aggregate,
            callback,
            eager,
        } => {
            let staged = body.scalar.as_ref().is_some_and(|s| s.gate.is_some());
            let mut child_frame = frame::Frame::repeated();
            let mut predicate_frame = frame::Frame::new();
            let mut reduction = Reduction::new(*aggregate);
            let mut mapper_error = None;
            let mut invocation = call::Callback::new(scope, calls);
            let mut arguments = Arguments::new(input, &frame[*source], body);
            let undefined = Cell::Value(Value::Undefined);
            let missing = frame[*source].value(input).is_none();
            if missing
                && *eager
                && let Some(p) = predicate
            {
                p.run(
                    Activation {
                        input,
                        focus: &undefined,
                        wrapped: false,
                        args: &[],
                        outer: frame,
                        calls,
                        scope,
                    },
                    &mut predicate_frame,
                )?
                .truth(input, offset)?;
            }
            if missing && *eager && staged {
                body.run_row(
                    Activation {
                        input,
                        focus: &undefined,
                        wrapped: false,
                        args: &[],
                        outer: frame,
                        calls,
                        scope,
                    },
                    &mut child_frame,
                    &mut mapper_error,
                    true,
                )?;
            }
            let source = if missing && !*eager && (predicate.is_some() || staged) {
                &undefined
            } else {
                &frame[*source]
            };
            input.each(source, &body.row_schema, |child_input, item, index| {
                if let Some(scope) = scope.filter(|s| s.controlled()) {
                    scope.item(offset)?;
                }
                if !callback && item.value(child_input).is_some_and(|v| v.is_array()) {
                    return Err(crate::Error::new(
                        crate::ErrorKind::UnsupportedExpression,
                        offset,
                        "research path sources must have non-array items",
                    ));
                }
                if let Some(p) = predicate
                    && !p
                        .run(
                            Activation {
                                input: child_input,
                                focus: &item,
                                wrapped: false,
                                args: &[],
                                outer: frame,
                                calls,
                                scope,
                            },
                            &mut predicate_frame,
                        )?
                        .truth(child_input, offset)?
                {
                    return Ok(());
                }
                let args = arguments.at(&item, index);
                if mapper_error.is_some() && !staged {
                    return Ok(());
                }
                let activation = Activation {
                    input: child_input,
                    focus: if *callback { focus } else { &item },
                    wrapped: if *callback { wrapped } else { false },
                    args,
                    outer: frame,
                    calls,
                    scope,
                };
                let mut evaluate = |calls| {
                    let activation = Activation {
                        calls,
                        ..activation
                    };
                    if let Some(layout) = body.scalar.as_ref().filter(|s| s.number_result) {
                        let accepted = if staged {
                            body.run_row(activation, &mut child_frame, &mut mapper_error, false)
                        } else {
                            body.run_scalar_frame(&activation, &mut child_frame)
                                .map(|()| true)
                        };
                        match accepted {
                            Ok(true) => {
                                match &child_frame[layout.slots[body.result]] {
                                    Cell::Value(Value::Number(n)) => reduction.push_number(*n),
                                    Cell::Missing => {}
                                    _ => {
                                        unreachable!("numeric semantic result")
                                    }
                                }
                                Ok(None)
                            }
                            Ok(false) => Ok(None),
                            Err(error) => Err(error),
                        }
                    } else if staged {
                        body.run_row(activation, &mut child_frame, &mut mapper_error, false)
                            .map(|accepted| {
                                if accepted {
                                    child_frame.take_result(
                                        body.scalar.as_ref().unwrap().slots[body.result],
                                    )
                                } else {
                                    Cell::Missing
                                }
                            })
                            .map(Some)
                    } else {
                        body.run(activation, &mut child_frame).map(Some)
                    }
                };
                let result = if *callback {
                    invocation.run(body, &mut evaluate)
                } else {
                    evaluate(calls)
                };
                let result = match result {
                    Ok(Some(value)) => value,
                    Ok(None) => return Ok(()),
                    Err(error) if predicate.is_some() => {
                        mapper_error = Some(error);
                        return Ok(());
                    }
                    Err(error) => return Err(error),
                };
                if let Some(value) = result
                    .into_value(child_input)
                    .filter(|v| !matches!(v, Value::Undefined))
                {
                    if !callback && value.is_array() {
                        return Err(crate::Error::new(
                            crate::ErrorKind::UnsupportedExpression,
                            offset,
                            "research path array results are not supported",
                        ));
                    }
                    reduction.push(value);
                }
                Ok(())
            })?;
            if let Some(error) = mapper_error {
                return Err(error);
            }
            Cell::from_operand(reduction.finish(offset)?)?
        }
        _ => unreachable!("semantic sequence operation"),
    };
    Ok(value)
}
fn sequence<'e, 'i>(mut values: Vec<Value<'e, 'i>>) -> Cell<'e, 'i> {
    match values.len() {
        0 => Cell::Missing,
        1 => match values.pop().unwrap() {
            Value::Undefined => Cell::Missing,
            value => Cell::Value(value),
        },
        _ => Cell::Value(Value::sequence(values)),
    }
}

// Sequence normalization belongs to the complete consumer. A sole array is the
// aggregate argument; arrays among multiple native sequence items remain values.
struct Reduction<'e, 'i> {
    fold: crate::aggregate::Fold,
    first: Option<Value<'e, 'i>>,
    many: bool,
    numeric: bool,
}
impl<'e, 'i> Reduction<'e, 'i> {
    fn new(aggregate: Aggregate) -> Self {
        Self {
            fold: crate::aggregate::Fold::new(aggregate),
            first: None,
            many: false,
            numeric: false,
        }
    }
    fn push_number(&mut self, value: f64) {
        self.numeric = true;
        self.fold.push_number(value);
    }
    fn push(&mut self, value: Value<'e, 'i>) {
        if matches!(value, Value::Undefined) {
            return;
        }
        if self.many {
            self.fold.push(value);
        } else if let Some(first) = self.first.take() {
            self.many = true;
            self.fold.push(first);
            self.fold.push(value);
        } else {
            self.first = Some(value);
        }
    }
    fn finish(mut self, offset: usize) -> Result<Operand<'e, 'i>, Error> {
        let defined = self.numeric || self.many || self.first.is_some();
        if let Some(value) = self.first {
            if value.is_array() {
                value.elements().for_each(|v| self.fold.push(v));
            } else {
                self.fold.push(value);
            }
        }
        self.fold.finish(defined, offset)
    }
}
