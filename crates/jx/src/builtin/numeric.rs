use crate::Value;
use std::fmt::{self, Write};

// Decimal exponent shifting uses the shortest decimal representation, rather
// than multiplication by 10^p (which changes half-even ties such as 4.525).
struct Decimal {
    bytes: [u8; 64],
    len: usize,
}
impl Write for Decimal {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let end = self.len + text.len();
        let target = self.bytes.get_mut(self.len..end).ok_or(fmt::Error)?;
        target.copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}
fn shift(n: f64, precision: i32) -> f64 {
    let mut text = Decimal {
        bytes: [0; 64],
        len: 0,
    };
    write!(text, "{n:e}").unwrap();
    let source = std::str::from_utf8(&text.bytes[..text.len]).unwrap();
    let Some((mantissa, exponent)) = source.split_once('e') else {
        return f64::NAN;
    };
    let exponent = exponent.parse::<i32>().unwrap() + precision;
    let mut shifted = Decimal {
        bytes: [0; 64],
        len: 0,
    };
    write!(shifted, "{mantissa}e{exponent}").unwrap();
    std::str::from_utf8(&shifted.bytes[..shifted.len])
        .unwrap()
        .parse()
        .unwrap_or(f64::NAN)
}
pub(super) fn round_value<'e, 'i, const N: usize>(
    args: &[Option<Value<'e, 'i>>; N],
) -> Option<Value<'e, 'i>> {
    let n = super::library::number(&args[0])?;
    let p = super::library::number(&args[1]).unwrap_or(0.0);
    let result = round(n, p);
    Some(Value::Number(if result == 0.0 { 0.0 } else { result }))
}

pub(crate) fn round(n: f64, p: f64) -> f64 {
    if p == 0.0 || p.is_nan() {
        n.round_ties_even()
    } else if p.fract() != 0.0 || !p.is_finite() || p.abs() >= 1e21 {
        f64::NAN
    } else if p.abs() > 10000.0 {
        if p < 0.0 || n == 0.0 { 0.0 } else { f64::NAN }
    } else {
        shift(shift(n, p as i32).round_ties_even(), -(p as i32))
    }
}

// Signature-bypassing native partials use ECMAScript numeric coercion. This is
// deliberately separate from strict $number and ordinary numeric signatures.
pub(super) fn native_number(value: &Value<'_, '_>, offset: usize) -> Result<f64, crate::Error> {
    Ok(match value.atomic() {
        Value::Number(n) => n,
        Value::Boolean(b) => f64::from(b),
        Value::Null => 0.0,
        Value::Undefined => f64::NAN,
        value if value.string_body().is_some() => {
            let Ok(text) = value.as_str() else {
                return Ok(f64::NAN);
            };
            let text = text
                .as_deref()
                .unwrap()
                .trim_matches(|c: char| matches!(c, '\u{0009}'..='\u{000d}' | '\u{0020}' | '\u{00a0}' | '\u{1680}' | '\u{2000}'..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'));
            if text.is_empty() {
                0.0
            } else if matches!(text, "Infinity" | "+Infinity") {
                f64::INFINITY
            } else if text == "-Infinity" {
                f64::NEG_INFINITY
            } else if let Some((digits, radix)) = text
                .strip_prefix("0x")
                .or_else(|| text.strip_prefix("0X"))
                .map(|s| (s, 16))
                .or_else(|| {
                    text.strip_prefix("0o")
                        .or_else(|| text.strip_prefix("0O"))
                        .map(|s| (s, 8))
                })
                .or_else(|| {
                    text.strip_prefix("0b")
                        .or_else(|| text.strip_prefix("0B"))
                        .map(|s| (s, 2))
                })
            {
                if digits.is_empty() {
                    f64::NAN
                } else {
                    digits
                        .chars()
                        .try_fold(0.0, |n, ch| {
                            ch.to_digit(radix).map(|d| n * radix as f64 + d as f64)
                        })
                        .unwrap_or(f64::NAN)
                }
            } else if text
                .bytes()
                .any(|b| b.is_ascii_alphabetic() && !matches!(b, b'e' | b'E'))
            {
                f64::NAN
            } else {
                text.parse().unwrap_or(f64::NAN)
            }
        }
        value if value.is_array() => {
            let mut items = value.elements();
            match (items.next(), items.next()) {
                (None, _) => 0.0,
                (Some(item), None) => match item.atomic() {
                    Value::Boolean(_) => f64::NAN,
                    Value::Null | Value::Undefined => 0.0,
                    Value::Number(0.0) => 0.0,
                    _ => return native_number(&item, offset),
                },
                _ => f64::NAN,
            }
        }
        _ => {
            return Err(crate::Error::new(
                crate::ErrorKind::UnsupportedExpression,
                offset,
                "native object/function numeric coercion is deferred",
            ));
        }
    })
}
