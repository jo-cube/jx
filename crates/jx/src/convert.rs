use crate::{Error, OwnedString, RawJson, Value, json::string as json_string, value::range_error};
use std::fmt::Write;

pub(crate) use number::number;
mod number;

pub(crate) fn string<'e, 'i>(
    value: Option<Value<'e, 'i>>,
    pretty: bool,
    offset: usize,
) -> Result<Option<Value<'e, 'i>>, Error> {
    let Some(value) = value else { return Ok(None) };
    if value.string_body().is_some() {
        return Ok(Some(value));
    }
    let literal = match value.atomic() {
        Value::Undefined => return Ok(None),
        Value::Function(_) => Some(r#""""#),
        Value::Null => Some(r#""null""#),
        Value::Boolean(true) => Some(r#""true""#),
        Value::Boolean(false) => Some(r#""false""#),
        Value::Number(n) if !n.is_finite() => return Err(range_error(offset)),
        _ => None,
    };
    if let Some(literal) = literal {
        return Ok(Some(Value::StringLiteral(RawJson(literal))));
    }
    let mut text = String::new();
    write_json(&value, &mut text, pretty, 0, offset)?;
    Ok(Some(Value::String(OwnedString::units(text.encode_utf16()))))
}

pub(crate) fn concat<'e, 'i>(
    left: Option<Value<'e, 'i>>,
    right: Option<Value<'e, 'i>>,
    offset: usize,
) -> Result<Value<'e, 'i>, Error> {
    let left = string(left, false, offset)?;
    let right = string(right, false, offset)?;
    match (left, right) {
        (None, None) => Ok(Value::StringLiteral(RawJson(r#""""#))),
        (Some(value), None) | (None, Some(value)) => Ok(value),
        (Some(left), Some(right)) => {
            let left_body = left.string_body().unwrap();
            let right_body = right.string_body().unwrap();
            if left_body.is_empty() {
                return Ok(right);
            }
            if right_body.is_empty() {
                return Ok(left);
            }
            Ok(Value::String(OwnedString::concat(left_body, right_body)))
        }
    }
}

fn quote(body: &str, output: &mut String) {
    output.push('"');
    for ch in char::decode_utf16(json_string::units(body)) {
        match ch {
            Ok('"') => output.push_str("\\\""),
            Ok('\\') => output.push_str("\\\\"),
            Ok('\u{8}') => output.push_str("\\b"),
            Ok('\u{c}') => output.push_str("\\f"),
            Ok('\n') => output.push_str("\\n"),
            Ok('\r') => output.push_str("\\r"),
            Ok('\t') => output.push_str("\\t"),
            Ok(ch) if ch < ' ' => write!(output, "\\u{:04x}", ch as u32).unwrap(),
            Ok(ch) => output.push(ch),
            Err(ch) => write!(output, "\\u{:04x}", ch.unpaired_surrogate()).unwrap(),
        }
    }
    output.push('"');
}
fn separator(output: &mut String, pretty: bool, depth: usize) {
    if pretty {
        output.push('\n');
        for _ in 0..depth {
            output.push_str("  ");
        }
    }
}
fn write_json(
    value: &Value<'_, '_>,
    output: &mut String,
    pretty: bool,
    depth: usize,
    offset: usize,
) -> Result<(), Error> {
    if let Some(body) = value.string_body() {
        quote(body, output);
        return Ok(());
    }
    match value.atomic() {
        Value::Null | Value::Undefined => output.push_str("null"),
        Value::Boolean(b) => output.push_str(if b { "true" } else { "false" }),
        Value::Function(_) => output.push_str(r#""""#),
        Value::Number(n) => {
            if n.is_infinite() {
                return Err(range_error(offset));
            }
            if n.is_nan() {
                output.push_str("null");
            } else {
                write_number(n, output);
            }
        }
        value if value.is_array() => {
            output.push('[');
            let mut count = 0;
            for item in value.elements() {
                if count != 0 {
                    output.push(',');
                }
                separator(output, pretty, depth + 1);
                write_json(&item, output, pretty, depth + 1, offset)?;
                count += 1;
            }
            if count != 0 {
                separator(output, pretty, depth);
            }
            output.push(']');
        }
        value => {
            output.push('{');
            let mut count = 0;
            for (key, item) in crate::members::entries(&value) {
                if matches!(item, Value::Undefined) {
                    continue;
                }
                if count != 0 {
                    output.push(',');
                }
                separator(output, pretty, depth + 1);
                quote(key, output);
                output.push(':');
                if pretty {
                    output.push(' ');
                }
                write_json(&item, output, pretty, depth + 1, offset)?;
                count += 1;
            }
            if count != 0 {
                separator(output, pretty, depth);
            }
            output.push('}');
        }
    }
    Ok(())
}

fn write_number(n: f64, output: &mut String) {
    let n = if n.fract() != 0.0 {
        format!("{n:.14e}").parse::<f64>().unwrap()
    } else {
        n
    };
    if n == 0.0 {
        output.push('0');
        return;
    }
    let text = n.abs().to_string();
    if n < 0.0 {
        output.push('-');
    }
    if (1e-6..1e21).contains(&n.abs()) {
        output.push_str(&text);
        return;
    }
    let point = text.find('.').unwrap_or(text.len());
    let first = text.bytes().position(|b| b != b'0' && b != b'.').unwrap();
    let exponent = point as i32 - first as i32 - i32::from(first < point);
    let mut digits = text[first..]
        .bytes()
        .filter(|&b| b != b'.')
        .collect::<Vec<_>>();
    while digits.len() > 1 && digits.last() == Some(&b'0') {
        digits.pop();
    }
    output.push(char::from(digits[0]));
    if digits.len() > 1 {
        output.push('.');
        for &b in &digits[1..] {
            output.push(char::from(b));
        }
    }
    output.push('e');
    if exponent >= 0 {
        output.push('+');
    }
    write!(output, "{exponent}").unwrap();
}
