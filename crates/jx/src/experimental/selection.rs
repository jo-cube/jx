//! Ordered path stages. Evaluation failures are delayed while earlier stages
//! finish; caller cancellation propagates immediately. No prefix is replayed.
use super::{
    frame::Frame,
    run::{Activation, Arguments, Cell, Input},
    *,
};
use crate::{ConsumeError, Error};
#[derive(Clone, Debug)]
pub(super) struct Selection {
    pub source: usize,
    pub body: Box<Program>,
    pub predicates: Box<[Program]>,
    // An eager selection completes before its downstream body starts. Retain
    // borrowed input fragments, not a graph of Value cells.
    pub eager: bool,
    pub mapping: bool,
    // A candidate predicate may select positions (including duplicate numeric
    // entries), unlike effective-Boolean selection. A literal position is a
    // cardinality boundary in JSONata, including sole-array promotion.
    pub candidate: bool,
    pub positional: bool,
    // A lexical predicate stage must finish before exposing downstream work.
    // This does not normalize a singleton array as a value boundary does.
    pub preflight: bool,
}
impl Selection {
    pub(super) fn identity(&self) -> bool {
        !self.mapping
            && self.body.code.len() == 1
            && matches!(self.body.code[0].kind, Operation::Focus)
    }
    pub(super) fn walk<'e, 'i, E>(
        &'e self,
        activation: Activation<'_, 'e, 'i, '_>,
        source: &Cell<'e, 'i>,
        offset: usize,
        output: &mut dyn FnMut(Value<'e, 'i>) -> Result<(), ConsumeError<E>>,
    ) -> Result<(), ConsumeError<E>> {
        if self.candidate {
            return self.walk_candidates(activation, source, offset, output);
        }
        let Activation {
            input,
            outer,
            scope,
            calls,
            ..
        } = activation;
        // Filters visit an undefined candidate for a missing source. A direct
        // path can map that candidate; an eager boundary normalizes it away.
        let missing = source.value(input).is_none();
        let undefined = Cell::Value(Value::Undefined);
        let source = if missing && !self.predicates.is_empty() {
            &undefined
        } else {
            source
        };
        let mut predicate_frame = Frame::repeated();
        let mut body_frame = Frame::repeated();
        let mut pending: Option<(usize, Error)> = None;
        let mut rows = rows::Rows::All;
        let mut selected_count = 0;
        let mut selected_array = None;
        let preflight = self.eager || self.preflight;
        if preflight {
            for (stage, predicate) in self.predicates.iter().enumerate() {
                let mut selected_rows = rows::Rows::selection(source, input);
                rows.each(
                    input,
                    source,
                    &predicate.row_schema,
                    |child_input, item, index| {
                        if let Some(scope) = scope.filter(|s| s.controlled()) {
                            scope.item(offset).map_err(ConsumeError::Evaluation)?;
                        }
                        let selected = predicate
                            .run(
                                Activation {
                                    input: child_input,
                                    focus: &item,
                                    wrapped: false,
                                    args: &[],
                                    outer,
                                    calls,
                                    scope,
                                },
                                &mut predicate_frame,
                            )
                            .and_then(|v| v.truth(child_input, offset))
                            .map_err(ConsumeError::Evaluation)?;
                        if selected {
                            if stage + 1 == self.predicates.len() {
                                selected_count += 1;
                                selected_array = if selected_count == 1 {
                                    item.value(child_input).filter(|v| v.is_array())
                                } else {
                                    None
                                };
                            }
                            selected_rows.push(child_input, &item, index);
                        }
                        Ok(())
                    },
                )?;
                rows = selected_rows;
            }
            predicate_frame.reset(0);
        }
        if (self.eager && missing) || (preflight && selected_count == 0) {
            return Ok(());
        }
        // A singleton selected array becomes the next stage's array source;
        // multiple selected arrays remain individual sequence items.
        let singleton = if self.eager {
            selected_array.map(Cell::Value)
        } else {
            None
        };
        let source = singleton.as_ref().unwrap_or(source);
        if singleton.is_some() {
            rows = rows::Rows::All;
        }
        let mut arguments = Arguments::new(input, source, &self.body);
        let mut defined = false;
        let mut first_array = None;
        rows.each(
            input,
            source,
            &self.body.row_schema,
            |child_input, item, index| {
                if let Some(scope) = scope.filter(|s| s.controlled()) {
                    scope.item(offset).map_err(ConsumeError::Evaluation)?;
                }
                for (stage, predicate) in self.predicates.iter().enumerate().filter(|_| !preflight)
                {
                    if pending.as_ref().is_some_and(|(at, _)| *at <= stage) {
                        return Ok(());
                    }
                    let selected = predicate
                        .run(
                            Activation {
                                input: child_input,
                                focus: &item,
                                wrapped: false,
                                args: &[],
                                outer,
                                calls,
                                scope,
                            },
                            &mut predicate_frame,
                        )
                        .and_then(|v| v.truth(child_input, offset));
                    match selected {
                        Ok(true) => {}
                        Ok(false) => return Ok(()),
                        Err(error) if stage == 0 => return Err(ConsumeError::Evaluation(error)),
                        Err(error) => {
                            pending = Some((stage, error));
                            return Ok(());
                        }
                    }
                }
                if pending.is_some() {
                    return Ok(());
                }
                let args = arguments.at(&item, index);
                let value = self
                    .body
                    .run(
                        Activation {
                            input: child_input,
                            focus: &item,
                            wrapped: false,
                            args,
                            outer,
                            calls,
                            scope,
                        },
                        &mut body_frame,
                    )
                    .map(|v| v.into_value(child_input));
                match value {
                    Ok(Some(Value::Undefined)) => Ok(()),
                    Ok(Some(value)) => {
                        if self.identity() && !self.predicates.is_empty() {
                            return output(value);
                        }
                        if let Some(pending) = first_array.take() {
                            emit_mapped(pending, output)?;
                        }
                        if !defined {
                            defined = true;
                            if value.is_array() && !value.is_sequence() {
                                first_array = Some(value);
                                return Ok(());
                            }
                        }
                        emit_mapped(value, output)
                    }
                    Ok(None) => Ok(()),
                    Err(error) if preflight || self.predicates.is_empty() => {
                        Err(ConsumeError::Evaluation(error))
                    }
                    Err(error) => {
                        pending = Some((self.predicates.len(), error));
                        Ok(())
                    }
                }
            },
        )?;
        if let Some((_, error)) = pending {
            return Err(ConsumeError::Evaluation(error));
        }
        if let Some(value) = first_array {
            output(value)?;
        }
        Ok(())
    }
}
impl Selection {
    fn walk_candidates<'e, 'i, E>(
        &'e self,
        activation: Activation<'_, 'e, 'i, '_>,
        source: &Cell<'e, 'i>,
        offset: usize,
        output: &mut dyn FnMut(Value<'e, 'i>) -> Result<(), ConsumeError<E>>,
    ) -> Result<(), ConsumeError<E>> {
        let Activation {
            input,
            outer,
            scope,
            calls,
            ..
        } = activation;
        let missing = source.value(input).is_none();
        let undefined = Cell::Value(Value::Undefined);
        let source = if missing { &undefined } else { source };
        // Length belongs to this candidate input, not to the predicate result.
        // Count borrowed rows without executing expressions or retaining objects.
        let mut length = None;
        let mut predicate_frame = Frame::repeated();
        let mut body_frame = Frame::repeated();
        let mut pending: Option<(usize, Error)> = None;
        let mut selected = Vec::new();
        let preflight = self.preflight || self.eager || self.positional;
        let mut defined = false;
        let mut first_array = None;
        let mut emit_body = |child_input: &Input<'_, 'i, '_>, item: &Cell<'e, 'i>| {
            if self.identity() {
                if let Some(value) = item
                    .value(child_input)
                    .filter(|v| !(self.positional || self.eager) || !matches!(v, Value::Undefined))
                {
                    output(value)?;
                }
                return Ok(());
            }
            let result = self
                .body
                .run(
                    Activation {
                        input: child_input,
                        focus: item,
                        wrapped: false,
                        args: &[],
                        outer,
                        calls,
                        scope,
                    },
                    &mut body_frame,
                )
                .map(|v| v.into_value(child_input));
            match result {
                Ok(Some(value)) if !matches!(value, Value::Undefined) => {
                    if let Some(value) = first_array.take() {
                        emit_mapped(value, output)?;
                    }
                    if !defined {
                        defined = true;
                        if value.is_array() && !value.is_sequence() {
                            first_array = Some(value);
                            return Ok(());
                        }
                    }
                    emit_mapped(value, output)
                }
                Ok(_) => Ok(()),
                Err(error) => Err(ConsumeError::Evaluation(error)),
            }
        };
        let first = &self.predicates[0];
        let schema = if preflight {
            &first.row_schema
        } else {
            &self.body.row_schema
        };
        input.each(source, schema, |child_input, item, index| {
            if let Some(scope) = scope.filter(|s| s.controlled()) {
                scope.item(offset).map_err(ConsumeError::Evaluation)?;
            }
            let value = first
                .run(
                    Activation {
                        input: child_input,
                        focus: &item,
                        wrapped: false,
                        args: &[],
                        outer,
                        calls,
                        scope,
                    },
                    &mut predicate_frame,
                )
                .map_err(ConsumeError::Evaluation)?;
            let operand = value.operand(child_input);
            let mut consumer_error = None;
            let result = crate::filter::select(
                &operand,
                first.offset,
                index,
                || {
                    if let Some(count) = length {
                        return Ok(count);
                    }
                    // Counting replays borrowed input boundaries, never expressions.
                    // Positive positions and effective-Boolean results never count.
                    let count = source.value(input).map_or(0, |v| {
                        if v.is_array() {
                            v.elements().count()
                        } else {
                            1
                        }
                    });
                    length = Some(count);
                    Ok(count)
                },
                &mut || {
                    if preflight {
                        selected.push(item.value(child_input).unwrap_or(Value::Undefined));
                        return Ok(());
                    }
                    for (stage, predicate) in self.predicates.iter().enumerate().skip(1) {
                        if pending.as_ref().is_some_and(|(at, _)| *at <= stage) {
                            return Ok(());
                        }
                        let result = predicate
                            .run(
                                Activation {
                                    input: child_input,
                                    focus: &item,
                                    wrapped: false,
                                    args: &[],
                                    outer,
                                    calls,
                                    scope,
                                },
                                &mut predicate_frame,
                            )
                            .and_then(|v| v.truth(child_input, predicate.offset));
                        match result {
                            Ok(true) => {}
                            Ok(false) => return Ok(()),
                            Err(error) => {
                                pending = Some((stage, error));
                                return Ok(());
                            }
                        }
                    }
                    if pending.is_some() {
                        return Ok(());
                    }
                    match emit_body(child_input, &item) {
                        Ok(()) => Ok(()),
                        Err(ConsumeError::Evaluation(error)) => {
                            pending = Some((self.predicates.len(), error));
                            Ok(())
                        }
                        Err(ConsumeError::Consumer(error)) => {
                            consumer_error = Some(error);
                            Err(crate::sequence::Halt::Stop)
                        }
                    }
                },
            );
            match result {
                Ok(()) => Ok(()),
                Err(crate::sequence::Halt::Evaluation(error)) => {
                    Err(ConsumeError::Evaluation(error))
                }
                Err(crate::sequence::Halt::Stop) => {
                    Err(ConsumeError::Consumer(consumer_error.unwrap()))
                }
            }
        })?;
        if let Some((_, error)) = pending {
            return Err(ConsumeError::Evaluation(error));
        }
        if preflight {
            // Effects are stage-ordered: finish the preceding predicate over all
            // candidates before starting the next one, without replay.
            for predicate in self.predicates.iter().skip(1) {
                let mut next = Vec::new();
                input.each_values(
                    selected.into_iter(),
                    &predicate.row_schema,
                    |_| true,
                    |child_input, item, _| {
                        if let Some(scope) = scope.filter(|s| s.controlled()) {
                            scope.item(offset).map_err(ConsumeError::Evaluation)?;
                        }
                        let result = predicate
                            .run(
                                Activation {
                                    input: child_input,
                                    focus: &item,
                                    wrapped: false,
                                    args: &[],
                                    outer,
                                    calls,
                                    scope,
                                },
                                &mut predicate_frame,
                            )
                            .and_then(|v| v.truth(child_input, predicate.offset))
                            .map_err(ConsumeError::Evaluation)?;
                        if result {
                            next.push(item.into_value(child_input).unwrap_or(Value::Undefined));
                        }
                        Ok(())
                    },
                )?;
                selected = next;
            }
            if (self.positional || self.eager)
                && self.mapping
                && selected.len() == 1
                && selected[0].is_array()
            {
                input.each(
                    &Cell::Value(selected.pop().unwrap()),
                    &self.body.row_schema,
                    |child_input, item, _| emit_body(child_input, &item),
                )?;
            } else {
                input.each_values(
                    selected.into_iter(),
                    &self.body.row_schema,
                    |_| true,
                    |child_input, item, _| emit_body(child_input, &item),
                )?;
            }
        }
        if let Some(value) = first_array {
            output(value)?;
        }
        Ok(())
    }
}

fn emit_mapped<'e, 'i, E>(
    value: Value<'e, 'i>,
    output: &mut dyn FnMut(Value<'e, 'i>) -> Result<(), ConsumeError<E>>,
) -> Result<(), ConsumeError<E>> {
    if value.is_array() && !value.preserves_array() {
        for item in value.elements() {
            output(item)?;
        }
        Ok(())
    } else {
        output(value)
    }
}
