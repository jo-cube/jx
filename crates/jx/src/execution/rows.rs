//! A retained selection stores input fragments, never a graph of raw Values.
//! Dense runs are compact; alternating selections retain one range per match.
//! Constructed sources use ordinal ranges and keep their existing owner alive.
use super::{
    acquisition::{Schema, Span},
    run::{Cell, Input},
};
use crate::{RawJson, Value};

pub(super) enum Rows<'i> {
    All,
    Raw(RawJson<'i>, Vec<Span>),
    Indices(Vec<Span>),
}
impl<'i> Rows<'i> {
    pub fn selection<'e>(source: &Cell<'e, 'i>, input: &Input<'_, 'i, '_>) -> Self {
        match source.value(input) {
            Some(Value::Raw(raw)) if raw.is_array() => Self::Raw(raw, Vec::new()),
            _ => Self::Indices(Vec::new()),
        }
    }
    pub fn push<'e>(&mut self, input: &Input<'_, 'i, '_>, item: &Cell<'e, 'i>, index: usize) {
        match self {
            Self::Raw(source, spans) => {
                let Some(Value::Raw(raw)) = item.value(input) else {
                    unreachable!("raw array row")
                };
                let start = (raw.as_bytes().as_ptr() as usize)
                    .checked_sub(source.as_bytes().as_ptr() as usize)
                    .expect("row belongs to source");
                let end = start + raw.as_bytes().len();
                if let Some(previous) = spans.last_mut()
                    && source.as_bytes()[previous.end..start]
                        .iter()
                        .all(|b| b.is_ascii_whitespace() || *b == b',')
                {
                    previous.end = end;
                } else {
                    spans.push(Span { start, end });
                }
            }
            Self::Indices(spans) => {
                if let Some(previous) = spans.last_mut()
                    && previous.end == index
                {
                    previous.end = index + 1;
                } else {
                    spans.push(Span {
                        start: index,
                        end: index + 1,
                    });
                }
            }
            Self::All => unreachable!("selection builder"),
        }
    }
    pub fn each<'e, E>(
        &self,
        input: &Input<'_, 'i, '_>,
        source: &Cell<'e, 'i>,
        schema: &'e Schema,
        emit: impl FnMut(&Input<'_, 'i, '_>, Cell<'e, 'i>, usize) -> Result<(), E>,
    ) -> Result<(), E> {
        match self {
            Self::All => input.each(source, schema, emit),
            Self::Raw(raw, ranges) => input.each_ranges(*raw, ranges, schema, emit),
            Self::Indices(ranges) => {
                let mut at = 0;
                input.each_selected(
                    source,
                    schema,
                    |index| {
                        while at < ranges.len() && index >= ranges[at].end {
                            at += 1;
                        }
                        at < ranges.len() && index >= ranges[at].start
                    },
                    emit,
                )
            }
        }
    }
}
