use super::{cursor::Cursor, key, text::Text};
use crate::{
    Error, Value,
    builtin::library::Library,
    sequence::Context,
    value::{range_error, type_error},
};
mod replacement;

pub(crate) fn call<'e, 'i>(
    function: Library,
    args: &[Option<Value<'e, 'i>>],
    context: &Context<'e, 'i>,
    offset: usize,
) -> Result<Option<Value<'e, 'i>>, Error> {
    let Some(value) = &args[0] else {
        return Ok(None);
    };
    let limit = super::super::builtin::library::number(
        &args[if function == Library::Replace { 3 } else { 2 }],
    );
    if function == Library::Replace
        && args[1]
            .as_ref()
            .is_some_and(|v| v.string_body() == Some(""))
    {
        return Err(type_error(offset));
    }
    if limit.is_some_and(|n| n < 0.0) {
        return Err(range_error(offset));
    }
    let limit = limit.unwrap_or(f64::INFINITY);
    if function != Library::Contains && (limit.is_nan() || limit <= 0.0) {
        return Ok(Some(match function {
            Library::Replace => value.clone(),
            Library::Match => Value::sequence(Vec::new()),
            Library::Split => Value::array(Vec::new(), false),
            _ => unreachable!(),
        }));
    }
    let input = Text::new(value.clone(), offset)?;
    if let Some(pattern) = &args[1]
        && pattern.string_body().is_some()
    {
        if function == Library::Replace {
            return replacement::literal(&input, pattern, args[2].as_ref(), limit, offset)
                .map(Some);
        }
        unreachable!("literal contains/split stay on existing string execution");
    }
    let Some(Value::Function(function_value)) = &args[1] else {
        return Err(type_error(offset));
    };
    let mut cursor = Cursor::new(function_value.clone(), input.clone());
    if function == Library::Contains {
        return Ok(Some(Value::Boolean(
            cursor.next(context, offset)?.is_some(),
        )));
    }
    if function == Library::Replace {
        return replacement::matcher(
            &input,
            &mut cursor,
            args[2].as_ref(),
            limit,
            context,
            offset,
        )
        .map(Some);
    }
    let mut items = Vec::new();
    let mut hit = cursor.next(context, offset)?;
    if function == Library::Split && hit.is_none() {
        items.push(value.clone());
    }
    let matched = hit.is_some();
    let mut position = 0;
    while let Some(found) = hit {
        if items.len() as f64 >= limit {
            break;
        }
        if function == Library::Match {
            items.push(Value::object(vec![
                (key("\"match\""), found.matched()),
                (key("\"index\""), Value::Number(found.start() as f64)),
                (key("\"groups\""), found.groups()),
            ]));
        } else {
            let start = found.start().min(input.len());
            items.push(input.slice(position.min(start)..start.max(position).min(input.len())));
            position = found.end().min(input.len());
        }
        // The reference invokes next even when this item meets the limit. Empty
        // continuation errors and matcher side effects remain observable.
        hit = cursor.next(context, offset)?;
    }
    if function == Library::Split && matched && (items.len() as f64) < limit {
        items.push(input.slice(position..input.len()));
    }
    Ok(Some(if function == Library::Match {
        Value::sequence(items)
    } else {
        Value::array(items, false)
    }))
}
