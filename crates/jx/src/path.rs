use crate::expression::Path;
use crate::{RawJson, json::Selection};
use std::convert::Infallible;

/// A borrowed, consumable result stream. Missing emits nothing; null and each
/// array value emit once. A sequence emits its items in order, without collecting.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PathEvaluation<'expression, 'input> {
    pub(crate) selection: Selection<'input, 'expression>,
    pub(crate) root_lookup: bool,
}

impl<'i> PathEvaluation<'_, 'i> {
    pub fn for_each(self, mut output: impl FnMut(RawJson<'i>)) {
        self.try_for_each(|value| {
            output(value);
            Ok::<_, Infallible>(())
        })
        .unwrap();
    }

    /// A consumer error stops traversal immediately and is returned unchanged.
    pub fn try_for_each<E>(
        self,
        mut output: impl FnMut(RawJson<'i>) -> Result<(), E>,
    ) -> Result<(), E> {
        match self.selection {
            Selection::Missing => Ok(()),
            Selection::Value(value) => output(value),
            Selection::Array(value, fields) => {
                let mut result = Sequence {
                    first: None,
                    multiple: false,
                    output: &mut output,
                };
                if self.root_lookup {
                    context(value, fields, &mut result)?;
                } else {
                    for item in value.elements() {
                        context(item, fields, &mut result)?;
                    }
                }
                result.finish()
            }
        }
    }
}

// The last path stage preserves a sole raw array. A second defined result makes
// both contribute their immediate contents instead. Keep only the first value
// until that distinction is known, including an empty array as a defined result.
struct Sequence<'i, 'o, E> {
    first: Option<RawJson<'i>>,
    multiple: bool,
    output: &'o mut dyn FnMut(RawJson<'i>) -> Result<(), E>,
}

impl<'i, E> Sequence<'i, '_, E> {
    fn value(&mut self, value: RawJson<'i>) -> Result<(), E> {
        if self.multiple {
            return self.flatten(value);
        }
        if self.first.is_some() {
            self.start_sequence()?;
            self.flatten(value)
        } else {
            self.first = Some(value);
            Ok(())
        }
    }

    fn start_sequence(&mut self) -> Result<(), E> {
        self.multiple = true;
        if let Some(first) = self.first.take() {
            self.flatten(first)?;
        }
        Ok(())
    }

    fn flatten(&mut self, value: RawJson<'i>) -> Result<(), E> {
        if value.is_array() {
            for item in value.elements() {
                (self.output)(item)?;
            }
            Ok(())
        } else {
            (self.output)(value)
        }
    }

    fn finish(self) -> Result<(), E> {
        if let Some(first) = self.first {
            (self.output)(first)?;
        }
        Ok(())
    }
}

fn context<'i, E>(
    input: RawJson<'i>,
    fields: &[Box<str>],
    result: &mut Sequence<'i, '_, E>,
) -> Result<(), E> {
    let (field, rest) = fields.split_first().expect("remaining path step");
    if !input.is_array() {
        if let Some(value) = input.field(field) {
            single(value, rest, result)?;
        }
        return Ok(());
    }

    // Name lookup on an array yields a sequence. Normalize its cardinality
    // before the map stage: a singleton array item becomes a raw array again.
    let mut first = None;
    let mut multiple = false;
    lookup(input, field, &mut |value| {
        if multiple {
            return sequence_item(value, rest, result);
        }
        if let Some(pending) = first.take() {
            multiple = true;
            if rest.is_empty() {
                result.start_sequence()?;
            }
            sequence_item(pending, rest, result)?;
            sequence_item(value, rest, result)
        } else {
            first = Some(value);
            Ok(())
        }
    })?;
    if let Some(value) = first {
        single(value, rest, result)?;
    }
    Ok(())
}

fn single<'i, E>(
    value: RawJson<'i>,
    rest: &[Box<str>],
    result: &mut Sequence<'i, '_, E>,
) -> Result<(), E> {
    if rest.is_empty() {
        return result.value(value);
    }
    if value.is_array() {
        for item in value.elements() {
            context(item, rest, result)?;
        }
        Ok(())
    } else {
        context(value, rest, result)
    }
}

fn sequence_item<'i, E>(
    value: RawJson<'i>,
    rest: &[Box<str>],
    result: &mut Sequence<'i, '_, E>,
) -> Result<(), E> {
    if rest.is_empty() {
        (result.output)(value)
    } else {
        context(value, rest, result)
    }
}

// Recursive array lookup flattens returned arrays once at the object boundary;
// concatenating the recursive sequences must not flatten their array items again.
fn lookup<'i, E>(
    input: RawJson<'i>,
    field: &str,
    output: &mut dyn FnMut(RawJson<'i>) -> Result<(), E>,
) -> Result<(), E> {
    if input.is_array() {
        for item in input.elements() {
            lookup(item, field, output)?;
        }
    } else if let Some(value) = input.field(field) {
        if value.is_array() {
            for item in value.elements() {
                output(item)?;
            }
        } else {
            output(value)?;
        }
    }
    Ok(())
}

impl Path {
    pub(crate) fn select<'e, 'i>(
        &'e self,
        input: &'i [u8],
    ) -> Result<PathEvaluation<'e, 'i>, crate::Error> {
        let selection = crate::json::select(input, &self.fields)?;
        Ok(self.selection(selection))
    }

    pub(crate) fn select_raw<'e, 'i>(&'e self, mut input: RawJson<'i>) -> PathEvaluation<'e, 'i> {
        let mut fields = self.fields.as_ref();
        let selection = loop {
            if fields.is_empty() {
                break Selection::Value(input);
            }
            if input.is_array() {
                break Selection::Array(input, fields);
            }
            match input.field(&fields[0]) {
                Some(value) => input = value,
                None => break Selection::Missing,
            }
            fields = &fields[1..];
        };
        self.selection(selection)
    }

    fn selection<'e, 'i>(&'e self, selection: Selection<'i, 'e>) -> PathEvaluation<'e, 'i> {
        let root_lookup = !self.rooted
            && matches!(&selection,
            Selection::Array(_, fields) if fields.len() == self.fields.len());
        PathEvaluation {
            selection,
            root_lookup,
        }
    }
}
