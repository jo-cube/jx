//! Delivery respects explicit materialization boundaries. Path consumers may
//! cancel between steps; declarations, conditionals and nested steps finish first.
use super::{
    frame::Frame,
    run::{Activation, Input},
    *,
};
use crate::ConsumeError;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Delivery {
    Value,
    Stream { producer: usize, retained: bool },
}
pub(super) fn plan(p: &Program) -> Delivery {
    // Scope release is administrative, but the scope must stay live while a
    // terminal producer runs. Materialization remains an observable boundary
    // for cancellable consumers; complete consumers can finish it incrementally.
    let mut instructions = p
        .code
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, i)| !matches!(i.kind, Operation::LeaveScope));
    let Some((mut producer, mut instruction)) = instructions.next() else {
        return Delivery::Value;
    };
    if instruction.out != p.result {
        return Delivery::Value;
    }
    let retained = if let Operation::Materialize(source) = instruction.kind {
        let Some((pc, value)) = instructions.next().filter(|(_, i)| i.out == source) else {
            return Delivery::Value;
        };
        producer = pc;
        instruction = value;
        true
    } else {
        false
    };
    match instruction.kind {
        Operation::Correlate(_)
        | Operation::Navigate(_)
        | Operation::Select(..)
        | Operation::Range(..)
        | Operation::Traverse(_)
        | Operation::Path(_)
        | Operation::Field(_, _, _, FieldMode::Value) => Delivery::Stream { producer, retained },
        _ => Delivery::Value,
    }
}
// A deferred root may expose its terminal producer while its caller can still
// cancel. This does not authorize streaming across inner eager boundaries.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Consumption {
    Cancellable,
    Deferred,
    Complete,
}
impl Program {
    pub fn try_for_each<'e, 'i, E>(
        &'e self,
        document: &Prepared<'i, '_>,
        mut output: impl FnMut(Value<'e, 'i>) -> Result<(), E>,
    ) -> Result<(), ConsumeError<E>> {
        let input = Input::Captured(document);
        let focus = input.root();
        let scope = self.scope(&input, None);
        self.consume(
            Activation {
                input: &input,
                focus: &focus,
                wrapped: true,
                args: &[],
                outer: &[],
                calls: Default::default(),
                scope: scope.as_ref(),
            },
            &mut |v| output(v).map_err(ConsumeError::Consumer),
            Consumption::Cancellable,
        )
    }
    /// Append one compact JSON value, wrapping multiple results in an array.
    /// No result returns false. Evaluation/serialization errors restore the
    /// original buffer length. This complete consumer can stream a terminal
    /// retained path because no prefix or cancellation is externally observable.
    pub fn write_json(
        &self,
        document: &Prepared<'_, '_>,
        output: &mut Vec<u8>,
    ) -> Result<bool, ConsumeError<std::io::Error>> {
        let input = Input::Captured(document);
        let focus = input.root();
        let scope = self.scope(&input, None);
        serialize(output, |emit| {
            self.consume(
                Activation {
                    input: &input,
                    focus: &focus,
                    wrapped: true,
                    args: &[],
                    outer: &[],
                    calls: Default::default(),
                    scope: scope.as_ref(),
                },
                emit,
                Consumption::Complete,
            )
        })
    }
    /// Deliver a prepared input with the same bindings, focus and cooperative
    /// controls as `evaluate_prepared_with`. Consumer cancellation is immediate.
    pub fn try_for_each_with<'e, 'i, E>(
        &'e self,
        document: &Prepared<'i, '_>,
        options: crate::EvaluationOptions<'e, 'i>,
        output: impl FnMut(Value<'e, 'i>) -> Result<(), E>,
    ) -> Result<(), ConsumeError<E>> {
        self.evaluate_prepared_with(document, options)
            .map_err(ConsumeError::Evaluation)?
            .try_for_each(output)
    }
    /// Complete settings-aware delivery. Any language, control or serialization
    /// failure restores the caller's output prefix.
    pub fn write_json_with<'e, 'i>(
        &'e self,
        document: &Prepared<'i, '_>,
        options: crate::EvaluationOptions<'e, 'i>,
        output: &mut Vec<u8>,
    ) -> Result<bool, ConsumeError<std::io::Error>> {
        let evaluation = self
            .evaluate_prepared_with(document, options)
            .map_err(ConsumeError::Evaluation)?;
        serialize(output, |emit| {
            evaluation.try_for_each(emit).map_err(|error| match error {
                ConsumeError::Evaluation(e) => ConsumeError::Evaluation(e),
                ConsumeError::Consumer(e) => e,
            })
        })
    }
    pub(super) fn with_view<'e, 'i>(
        &'e self,
        a: Activation<'_, 'e, 'i, '_>,
        frame: &mut Frame<'e, 'i>,
        complete: bool,
        emit: &mut dyn FnMut(crate::sequence::View<'_, 'e, 'i>) -> crate::sequence::Walk,
    ) -> crate::sequence::Walk {
        use crate::sequence::{Halt, View};
        let stop = match self.delivery {
            Delivery::Stream { producer, .. } if complete => Some(producer),
            _ => None,
        };
        if let Some(stop) = stop {
            match self.run_prefix(a, frame, Some(stop))? {
                call::Exit::Stop(pc, scopes) => {
                    let i = &self.code[pc];
                    let a = Activation {
                        outer: frame,
                        scope: scopes.current.as_ref(),
                        ..a
                    };
                    if let Operation::Field(base, path, contextual, _) = &i.kind {
                        return match a.input.select_field(
                            &frame[*base],
                            path,
                            *contextual && a.wrapped,
                            a.input.reads(self).and_then(|r| r.get(i.out)),
                        ) {
                            super::input::FieldResult::Cell(cell) => {
                                emit(View::Operand(&cell.operand(a.input)))
                            }
                            super::input::FieldResult::Path(path) => {
                                let walk = |out: &mut crate::sequence::Output<'_, 'e, 'i>| {
                                    path.try_for_each(&mut |v| out(v))
                                };
                                emit(View::Boundary(&walk))
                            }
                        };
                    }
                    let walk = |out: &mut crate::sequence::Output<'_, 'e, 'i>| match &i.kind {
                        Operation::Path(path) => path
                            .walk(a, &frame[path.source], i.offset, &mut |v| {
                                out(v).map_err(ConsumeError::Consumer)
                            })
                            .map_err(|e| match e {
                                ConsumeError::Evaluation(e) => Halt::Evaluation(e),
                                ConsumeError::Consumer(e) => e,
                            }),
                        _ => navigation::walk(&i.kind, a, frame, i.offset, out, true),
                    };
                    return emit(View::Boundary(&walk));
                }
                _ => unreachable!("view stops before its producer"),
            }
        }
        let value = self.run(a, frame)?;
        emit(View::Operand(&value.operand(a.input)))
    }
    fn consume<'e, 'i, E>(
        &'e self,
        a: Activation<'_, 'e, 'i, '_>,
        output: &mut dyn FnMut(Value<'e, 'i>) -> Result<(), ConsumeError<E>>,
        mode: Consumption,
    ) -> Result<(), ConsumeError<E>> {
        let input = a.input;
        let wrapped = a.wrapped;
        let mut frame = Frame::new();
        if self.scalar.is_some() && self.delivery == Delivery::Value {
            let value = self.run(a, &mut frame).map_err(ConsumeError::Evaluation)?;
            return emit(value.into_value(input), output);
        }
        let stop = {
            let stop = match self.delivery {
                Delivery::Stream { producer, retained }
                    if mode != Consumption::Cancellable || !retained =>
                {
                    Some(producer)
                }
                _ => None,
            };
            match self
                .run_prefix(a, &mut frame, stop)
                .map_err(ConsumeError::Evaluation)?
            {
                call::Exit::Done => None,
                call::Exit::Stop(pc, scopes) => Some((pc, scopes)),
                call::Exit::Call(_) => unreachable!("root program cannot tail call"),
            }
        };
        if let Some((pc, scopes)) = stop {
            let instruction = &self.code[pc];
            return match &instruction.kind {
                Operation::Path(path) => {
                    // A sole undefined result is missing; in a multi-item
                    // sequence its position remains observable. Defer only the
                    // first ambiguous value, preserving cancellation and errors.
                    let mut first = true;
                    let mut pending = None;
                    path.walk(
                        Activation {
                            outer: &frame,
                            scope: scopes.current.as_ref(),
                            ..a
                        },
                        &frame[path.source],
                        instruction.offset,
                        &mut |value| {
                            if first
                                && (matches!(value, Value::Undefined) || value.unpacks_sequence())
                            {
                                first = false;
                                pending = Some(value);
                                return Ok(());
                            }
                            first = false;
                            if let Some(value) = pending.take() {
                                output(value)?;
                            }
                            output(value)
                        },
                    )?;
                    if let Some(value) = pending
                        && value.unpacks_sequence()
                    {
                        for item in value.elements() {
                            output(item)?;
                        }
                    }
                    Ok(())
                }
                Operation::Correlate(_)
                | Operation::Navigate(_)
                | Operation::Select(..)
                | Operation::Range(..)
                | Operation::Traverse(_) => navigation::consume(
                    &instruction.kind,
                    Activation {
                        outer: &frame,
                        scope: scopes.current.as_ref(),
                        ..a
                    },
                    &frame,
                    instruction.offset,
                    output,
                    mode == Consumption::Complete,
                ),
                Operation::Field(base, path, contextual, FieldMode::Value) => input
                    .select_field(
                        &frame[*base],
                        path,
                        *contextual && wrapped,
                        input.reads(self).and_then(|r| r.get(instruction.out)),
                    )
                    .walk(input, output),
                _ => unreachable!("delivery is planned on path operations"),
            };
        }
        emit(frame.take_result(self.result).into_value(input), output)
    }
}

fn emit<'e, 'i, E>(
    value: Option<Value<'e, 'i>>,
    output: &mut dyn FnMut(Value<'e, 'i>) -> Result<(), ConsumeError<E>>,
) -> Result<(), ConsumeError<E>> {
    if let Some(value) = value {
        if value.unpacks_sequence() {
            for item in value.elements() {
                output(item)?;
            }
        } else {
            output(value)?;
        }
    }
    Ok(())
}

/// A one-shot continuation over the same program and acquired document. No prefix
/// interpretation, value graph or input scan occurs before the consumer asks.
#[derive(Debug)]
pub(crate) struct Deferred<'e, 'i> {
    program: &'e Program,
    input: acquisition::Snapshot<'i>,
    focus: super::run::Cell<'e, 'i>,
    scope: Option<crate::runtime::Scope<'e, 'i>>,
}
impl<'e, 'i> Deferred<'e, 'i> {
    pub(super) fn new(
        program: &'e Program,
        input: &Input<'_, 'i, '_>,
        focus: super::run::Cell<'e, 'i>,
        scope: Option<crate::runtime::Scope<'e, 'i>>,
    ) -> Self {
        Self {
            program,
            input: input.snapshot(),
            focus,
            scope,
        }
    }
    pub(crate) fn walk<E>(
        self,
        mut output: impl FnMut(Value<'e, 'i>) -> Result<(), E>,
    ) -> Result<(), ConsumeError<E>> {
        let prepared = self.input.prepared();
        let input = if let Some(d) = &prepared {
            Input::Captured(d)
        } else {
            let acquisition::Snapshot::Raw(raw) = self.input else {
                unreachable!()
            };
            Input::Raw(raw)
        };
        let focus = self.focus;
        self.program.consume(
            Activation {
                input: &input,
                focus: &focus,
                wrapped: true,
                args: &[],
                outer: &[],
                calls: Default::default(),
                scope: self.scope.as_ref(),
            },
            &mut |v| output(v).map_err(ConsumeError::Consumer),
            Consumption::Deferred,
        )
    }
}

fn serialize<'e, 'i>(
    output: &mut Vec<u8>,
    consume: impl FnOnce(
        &mut dyn FnMut(Value<'e, 'i>) -> Result<(), ConsumeError<std::io::Error>>,
    ) -> Result<(), ConsumeError<std::io::Error>>,
) -> Result<bool, ConsumeError<std::io::Error>> {
    let start = output.len();
    let mut first = None;
    let mut many = false;
    let mut encoding_error = None;
    let result = consume(&mut |value| {
        if encoding_error.is_some() {
            return Ok(());
        }
        // Serialization cannot cancel a complete evaluation: a later
        // language error takes precedence over an earlier unencodable
        // value, just as when evaluation precedes serialization.
        let encoded = (|| {
            if many {
                output.push(b',');
            } else if let Some(first) = first.take() {
                many = true;
                output.push(b'[');
                Value::write_compact(&first, &mut *output)?;
                output.push(b',');
            } else {
                first = Some(value);
                return Ok(());
            }
            value.write_compact(&mut *output)
        })();
        if let Err(error) = encoded {
            encoding_error = Some(error);
        }
        Ok(())
    });
    let result = result.and_then(|()| {
        if let Some(error) = encoding_error {
            return Err(ConsumeError::Consumer(error));
        }
        if many {
            output.push(b']');
            Ok(true)
        } else if let Some(first) = first {
            first
                .write_compact(&mut *output)
                .map_err(ConsumeError::Consumer)?;
            Ok(true)
        } else {
            Ok(false)
        }
    });
    if result.is_err() {
        output.truncate(start);
    }
    result
}
