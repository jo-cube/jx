use super::{collections::items, library::Library};
use crate::{
    Error, ErrorKind, Value, evaluate::Operand, function, json::string, sequence::Context,
    value::type_error,
};
use std::cmp::Ordering;

fn invoke<'e, 'i>(
    f: &crate::Function<'e, 'i>,
    args: &[Option<Value<'e, 'i>>],
    context: &Context<'e, 'i>,
    offset: usize,
) -> Result<bool, Error> {
    function::invoke(f, args, context, offset)?.truth(offset)
}
pub(super) fn call<'e, 'i>(
    kind: Library,
    args: &[Option<Value<'e, 'i>>; 3],
    context: &Context<'e, 'i>,
    offset: usize,
) -> Result<Option<Value<'e, 'i>>, Error> {
    let Some(input) = &args[0] else {
        return Ok(None);
    };
    let callback = match &args[1] {
        Some(Value::Function(f)) => Some(f),
        _ => None,
    };
    if kind == Library::Single {
        let arity = callback.map_or(0, |f| function::arity(f));
        let whole = (arity >= 3).then(|| {
            if input.is_array() {
                input.clone()
            } else {
                Value::array(vec![input.clone()], false)
            }
        });
        let mut found = None;
        for (index, value) in items(input).enumerate() {
            let arguments = [
                Some(value.clone()),
                Some(Value::Number(index as f64)),
                whole.clone(),
            ];
            if let Some(callback) = callback
                && !invoke(callback, &arguments[..arity.clamp(1, 3)], context, offset)?
            {
                continue;
            }
            if found.is_some() {
                return Err(Error::new(
                    ErrorKind::CardinalityError,
                    offset,
                    "$single matched more than one item",
                ));
            }
            found = Some(value);
        }
        return found.map(Some).ok_or_else(|| {
            Error::new(
                ErrorKind::CardinalityError,
                offset,
                "$single matched no items",
            )
        });
    }
    let values: Vec<_> = items(input).collect();
    if values.len() <= 1 {
        return Ok(Some(if input.is_array() {
            input.clone()
        } else {
            Value::array(values, false)
        }));
    }
    if callback.is_none() {
        let mut numeric = true;
        for value in &values {
            match value.atomic() {
                Value::Number(n) if n.is_infinite() => {
                    return Err(crate::value::range_error(offset));
                }
                Value::Number(n) if !n.is_nan() => {}
                _ => numeric = false,
            }
        }
        if !numeric && !values.iter().all(|v| v.string_body().is_some()) {
            return Err(type_error(offset));
        }
    }
    let indices = crate::ordering::indices(values.len(), |a, b| {
        if let Some(scope) = &context.scope {
            scope.checkpoint(offset)?;
        }
        let swap = if let Some(callback) = callback {
            // $sort's comparator uses JavaScript truth, not effective boolean
            // conversion. Containers and functions are always truthy here.
            match function::invoke(
                callback,
                &[Some(values[a].clone()), Some(values[b].clone())],
                context,
                offset,
            )? {
                Operand::Missing => false,
                Operand::One(value) => native_truth(&value),
                Operand::Many(_) => true,
            }
        } else {
            match (values[a].atomic(), values[b].atomic()) {
                (Value::Number(a), Value::Number(b)) => a > b,
                _ => {
                    string::units(values[a].string_body().unwrap())
                        .cmp(string::units(values[b].string_body().unwrap()))
                        == Ordering::Greater
                }
            }
        };
        Ok(if swap {
            Ordering::Greater
        } else {
            Ordering::Less
        })
    })?;
    Ok(Some(Value::array(
        indices.into_iter().map(|i| values[i].clone()).collect(),
        false,
    )))
}
fn native_truth(value: &Value<'_, '_>) -> bool {
    match value.atomic() {
        Value::Undefined | Value::Null => false,
        Value::Boolean(b) => b,
        Value::Number(n) => n != 0.0 && !n.is_nan(),
        value => value
            .string_body()
            .is_none_or(|s| string::units(s).next().is_some()),
    }
}
pub(super) fn zip<'e, 'i>(args: &[Option<Value<'e, 'i>>]) -> Value<'e, 'i> {
    let mut cursors: Vec<_> = args.iter().map(|v| v.as_ref().map(items)).collect();
    let mut rows = Vec::new();
    loop {
        let mut row = Vec::with_capacity(args.len());
        for cursor in &mut cursors {
            let Some(value) = cursor.as_mut().and_then(Iterator::next) else {
                return Value::array(rows, false);
            };
            row.push(value);
        }
        rows.push(Value::array(row, false));
    }
}
