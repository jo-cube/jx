use crate::{Error, Value};

// Containers and lexical storage retain evaluated values; already retained
// sequences pass through without copying their members.

pub(crate) fn collect<'e, 'i>(
    walk: impl FnOnce(&mut dyn FnMut(Value<'e, 'i>)) -> Result<(), Error>,
) -> Result<Option<Value<'e, 'i>>, Error> {
    let mut first = None;
    let mut values = Vec::new();
    walk(&mut |value| {
        if !values.is_empty() {
            values.push(value);
        } else if let Some(first) = first.take() {
            values.push(first);
            values.push(value);
        } else {
            first = Some(value);
        }
    })?;
    Ok(if !values.is_empty() {
        Some(Value::sequence(values))
    } else {
        first.filter(|value| !matches!(value, Value::Undefined))
    })
}
