use crate::matcher::{
    cursor::{Cursor, Hit},
    text::Text,
};
use crate::{Error, OwnedString, Value, json::string, sequence::Context, value::type_error};

fn append(value: &Value<'_, '_>, output: &mut Vec<u16>) {
    output.extend(string::units(value.string_body().unwrap()));
}
fn finish<'e, 'i>(output: Vec<u16>) -> Value<'e, 'i> {
    Value::String(OwnedString::units(output))
}
pub(super) fn literal<'e, 'i>(
    input: &Text<'e, 'i>,
    pattern: &Value<'e, 'i>,
    replacement: Option<&Value<'e, 'i>>,
    limit: f64,
    offset: usize,
) -> Result<Value<'e, 'i>, Error> {
    let pattern = string::units(pattern.string_body().unwrap()).collect::<Vec<_>>();
    if pattern.is_empty() {
        return Err(type_error(offset));
    }
    let Some(replacement) = replacement.filter(|v| v.string_body().is_some()) else {
        return Err(Error::new(
            crate::ErrorKind::UnsupportedExpression,
            offset,
            "literal replacement requires a string",
        ));
    };
    if limit.is_nan() || limit <= 0.0 {
        return Ok(input.value.clone());
    }
    let Some(mut at) = input.find(&pattern, 0) else {
        return Ok(input.value.clone());
    };
    let mut output = Vec::new();
    let mut position = 0;
    let mut count = 0;
    loop {
        input.append(position..at, &mut output);
        append(replacement, &mut output);
        position = at + pattern.len();
        count += 1;
        if count as f64 >= limit {
            break;
        }
        let Some(next) = input.find(&pattern, position) else {
            break;
        };
        at = next;
    }
    input.append(position..input.len(), &mut output);
    Ok(finish(output))
}
pub(super) fn matcher<'e, 'i>(
    input: &Text<'e, 'i>,
    cursor: &mut Cursor<'e, 'i>,
    replacement: Option<&Value<'e, 'i>>,
    limit: f64,
    context: &Context<'e, 'i>,
    offset: usize,
) -> Result<Value<'e, 'i>, Error> {
    if limit.is_nan() || limit <= 0.0 {
        return Ok(input.value.clone());
    }
    let replacement = replacement.ok_or_else(|| type_error(offset))?;
    let Some(mut hit) = cursor.next(context, offset)? else {
        return Ok(input.value.clone());
    };
    let template = replacement
        .string_body()
        .map(|body| string::units(body).collect::<Vec<_>>());
    let mut output = Vec::new();
    let mut position = 0;
    let mut count = 0;
    loop {
        let start = hit.start().min(input.len());
        input.append(
            position.min(start)..position.max(start).min(input.len()),
            &mut output,
        );
        if let Value::Function(function) = replacement {
            let replaced =
                crate::function::invoke(function, &[Some(hit.object())], context, offset)?
                    .normalize();
            let crate::evaluate::Operand::One(value) = replaced else {
                return Err(type_error(offset));
            };
            if value.string_body().is_none() {
                return Err(type_error(offset));
            }
            append(&value, &mut output);
        } else {
            expand(template.as_ref().unwrap(), &hit, &mut output);
        }
        position = (hit.start() + hit.matched_len()).min(input.len());
        count += 1;
        let next = cursor.next(context, offset)?;
        if count as f64 >= limit {
            break;
        }
        let Some(next) = next else {
            break;
        };
        hit = next;
    }
    input.append(position..input.len(), &mut output);
    Ok(finish(output))
}
fn expand(source: &[u16], hit: &Hit<'_, '_>, output: &mut Vec<u16>) {
    let groups = hit.group_count();
    let digits = if groups == 0 {
        1
    } else {
        groups.ilog10() as usize + 1
    };
    let mut at = 0;
    while at < source.len() {
        if source[at] != 36 {
            output.push(source[at]);
            at += 1;
            continue;
        }
        at += 1;
        match source.get(at).copied() {
            Some(36) => {
                output.push(36);
                at += 1;
            }
            Some(48) => {
                hit.append_group(0, output);
                at += 1;
            }
            Some(49..=57) => {
                let parse = |count| {
                    source[at..]
                        .iter()
                        .take(count)
                        .take_while(|&&u| (48..=57).contains(&u))
                        .fold(0usize, |n, &u| n * 10 + usize::from(u - 48))
                };
                let mut index = parse(digits);
                if digits > 1 && index > groups {
                    index = parse(digits - 1);
                }
                hit.append_group(index, output);
                at += index.ilog10() as usize + 1;
            }
            _ => output.push(36),
        }
    }
}
