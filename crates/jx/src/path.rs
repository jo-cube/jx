use crate::expression::Path;
use crate::{
    Error, Value,
    evaluate::Operand,
    json,
    sequence::{Context, Stream},
};

#[derive(Clone, Debug)]
pub(crate) enum Selection<'e, 'i> {
    Missing,
    Value(Value<'e, 'i>),
    Array(Value<'e, 'i>, &'e [Box<str>]),
}

/// A borrowed, consumable result stream. Missing emits nothing; null and each
/// array value emit once. A sequence emits its items in order, without collecting.
#[derive(Clone, Debug)]
pub(crate) struct PathEvaluation<'expression, 'input> {
    pub(crate) selection: Selection<'expression, 'input>,
    pub(crate) root_lookup: bool,
}

impl<'e, 'i> PathEvaluation<'e, 'i> {
    pub(crate) fn operand(self) -> Result<Operand<'e, 'i>, Error> {
        match self.selection {
            Selection::Missing => Ok(Operand::Missing),
            Selection::Value(value) if !value.unpacks_sequence() => Ok(match value {
                Value::Undefined => Operand::Missing,
                value => Operand::One(value),
            }),
            _ => Stream::Path(self).operand(),
        }
    }

    /// A consumer error stops traversal immediately and is returned unchanged.
    pub fn try_for_each<E>(
        &self,
        mut output: impl FnMut(Value<'e, 'i>) -> Result<(), E>,
    ) -> Result<(), E> {
        match &self.selection {
            Selection::Missing => Ok(()),
            Selection::Value(value) if value.unpacks_sequence() => {
                for item in value.elements() {
                    output(item)?;
                }
                Ok(())
            }
            Selection::Value(value) => output(value.clone()),
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
                        context(&item, fields, &mut result)?;
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
struct Sequence<'e, 'i, 'o, E> {
    first: Option<Value<'e, 'i>>,
    multiple: bool,
    output: &'o mut dyn FnMut(Value<'e, 'i>) -> Result<(), E>,
}

impl<'e, 'i, E> Sequence<'e, 'i, '_, E> {
    fn value(&mut self, value: Value<'e, 'i>) -> Result<(), E> {
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

    fn flatten(&mut self, value: Value<'e, 'i>) -> Result<(), E> {
        if value.is_array() && !value.preserves_array() {
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
            if first.is_sequence() {
                for item in first.elements() {
                    (self.output)(item)?;
                }
            } else {
                (self.output)(first)?;
            }
        }
        Ok(())
    }
}

fn context<'e, 'i, E>(
    input: &Value<'e, 'i>,
    fields: &[Box<str>],
    result: &mut Sequence<'e, 'i, '_, E>,
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

fn single<'e, 'i, E>(
    value: Value<'e, 'i>,
    rest: &[Box<str>],
    result: &mut Sequence<'e, 'i, '_, E>,
) -> Result<(), E> {
    if rest.is_empty() {
        return result.value(value);
    }
    if value.is_array() {
        for item in value.elements() {
            context(&item, rest, result)?;
        }
        Ok(())
    } else {
        context(&value, rest, result)
    }
}

fn sequence_item<'e, 'i, E>(
    value: Value<'e, 'i>,
    rest: &[Box<str>],
    result: &mut Sequence<'e, 'i, '_, E>,
) -> Result<(), E> {
    if rest.is_empty() {
        (result.output)(value)
    } else {
        context(&value, rest, result)
    }
}

// Recursive array lookup flattens returned arrays once at the object boundary;
// concatenating the recursive sequences must not flatten their array items again.
pub(crate) fn lookup<'e, 'i, E>(
    input: &Value<'e, 'i>,
    field: &str,
    output: &mut dyn FnMut(Value<'e, 'i>) -> Result<(), E>,
) -> Result<(), E> {
    if input.is_array() {
        if let Value::Raw(raw) = input {
            return raw.try_for_each_flattened(|item| lookup(&Value::Raw(item), field, output));
        }
        for item in input.elements() {
            lookup(&item, field, output)?;
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
        let selection = match crate::json::select(input, &self.fields)? {
            json::Selection::Missing => Selection::Missing,
            json::Selection::Value(value) => Selection::Value(Value::Raw(value)),
            json::Selection::Array(value, fields) => Selection::Array(Value::Raw(value), fields),
        };
        Ok(self.selection(selection))
    }

    pub(crate) fn select_validated<'e, 'i>(
        &'e self,
        input: json::RawJson<'i>,
    ) -> PathEvaluation<'e, 'i> {
        if self.fields.is_empty() {
            return self.selection(Selection::Value(Value::Raw(input)));
        }
        self.selection(match input.select(&self.fields) {
            json::Selection::Missing => Selection::Missing,
            json::Selection::Value(value) => Selection::Value(Value::Raw(value)),
            json::Selection::Array(value, fields) => Selection::Array(Value::Raw(value), fields),
        })
    }

    pub(crate) fn select_context<'e, 'i>(
        &'e self,
        context: &Context<'e, 'i>,
    ) -> PathEvaluation<'e, 'i> {
        let selection = match self.fields.as_ref() {
            [] => Selection::Value(context.value.clone()),
            [field] => {
                if context.value.is_array() {
                    Selection::Array(context.value.clone(), &self.fields)
                } else {
                    context
                        .value
                        .field(field)
                        .map_or(Selection::Missing, Selection::Value)
                }
            }
            fields => {
                if let Value::Raw(raw) = context.value
                    && raw.as_bytes()[0] == b'{'
                {
                    return raw_context(raw, fields);
                }
                let mut input = context.value.clone();
                let mut fields = fields;
                loop {
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
                }
            }
        };
        let mut selected = self.selection(selection);
        selected.root_lookup &= context.wrapped;
        selected
    }

    fn selection<'e, 'i>(&'e self, selection: Selection<'e, 'i>) -> PathEvaluation<'e, 'i> {
        let root_lookup = !self.rooted
            && matches!(&selection,
            Selection::Array(_, fields) if fields.len() == self.fields.len());
        PathEvaluation {
            selection,
            root_lookup,
        }
    }
}

// Keep selective scanner scratch out of ordinary context-path walks.
#[inline(never)]
fn raw_context<'e, 'i>(raw: json::RawJson<'i>, fields: &'e [Box<str>]) -> PathEvaluation<'e, 'i> {
    PathEvaluation {
        selection: match raw.select(fields) {
            json::Selection::Missing => Selection::Missing,
            json::Selection::Value(raw) => Selection::Value(Value::Raw(raw)),
            json::Selection::Array(raw, rest) => Selection::Array(Value::Raw(raw), rest),
        },
        // Object focus consumes a field before any deferred array is reached.
        root_lookup: false,
    }
}
