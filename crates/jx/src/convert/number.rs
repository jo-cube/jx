use crate::{Error, Value, json::string as json_string, value::type_error};

pub(crate) fn number<'e, 'i>(
    value: Option<Value<'e, 'i>>,
    offset: usize,
) -> Result<Option<Value<'e, 'i>>, Error> {
    let Some(value) = value else { return Ok(None) };
    let n = match value.atomic() {
        Value::Undefined => return Ok(None),
        Value::Number(n) => n,
        Value::Boolean(b) => f64::from(b),
        value => {
            let body = value.string_body().ok_or_else(|| type_error(offset))?;
            let decoded;
            let text = if body.as_bytes().contains(&b'\\') {
                decoded = json_string::units(body)
                    .map(|u| char::from_u32(u32::from(u)).unwrap_or('\u{fffd}'))
                    .collect::<String>();
                decoded.as_str()
            } else {
                body
            };
            numeric_text(text).ok_or_else(|| type_error(offset))?
        }
    };
    Ok(Some(Value::Number(n)))
}

fn numeric_text(text: &str) -> Option<f64> {
    let bytes = text.as_bytes();
    let mut at = usize::from(bytes.first() == Some(&b'-'));
    let digits = |at: &mut usize| {
        let start = *at;
        while bytes.get(*at).is_some_and(u8::is_ascii_digit) {
            *at += 1;
        }
        *at > start
    };
    let mut decimal = digits(&mut at);
    if bytes.get(at) == Some(&b'.') {
        at += 1;
        decimal &= digits(&mut at);
    }
    if matches!(bytes.get(at), Some(b'e' | b'E')) {
        at += 1;
        if matches!(bytes.get(at), Some(b'+' | b'-')) {
            at += 1;
        }
        decimal &= digits(&mut at);
    }
    if decimal && at == bytes.len() {
        return text.parse::<f64>().ok().filter(|n| n.is_finite());
    }
    // The pinned reference's radix regex has asymmetric anchors. Preserve its
    // accepted malformed-prefix cases (Number then returns NaN), not JS coercion.
    let radix = |b: u8| match b {
        b'x' | b'X' => Some(16),
        b'o' | b'O' => Some(8),
        b'b' | b'B' => Some(2),
        _ => None,
    };
    let digit = |b: u8| (b as char).to_digit(16);
    let accepted = bytes.windows(3).enumerate().any(|(i, w)| {
        if w[0] != b'0' {
            return false;
        }
        let Some(base) = radix(w[1]) else {
            return false;
        };
        digit(w[2]).is_some_and(|d| d < base)
            && match base {
                16 => i == 0,
                8 => true,
                _ => bytes[i + 2..]
                    .iter()
                    .all(|&b| digit(b).is_some_and(|d| d < base)),
            }
    });
    if !accepted {
        return None;
    }
    let Some(base) = bytes
        .get(1)
        .copied()
        .and_then(radix)
        .filter(|_| bytes[0] == b'0')
    else {
        return Some(f64::NAN);
    };
    if !bytes[2..]
        .iter()
        .all(|&b| digit(b).is_some_and(|d| d < base))
    {
        return Some(f64::NAN);
    }
    // Round once from the first 54 significant bits, even for very wide literals.
    let width = base.ilog2();
    let mut top = 0u64;
    let mut bits = 0i32;
    let mut sticky = false;
    for &byte in &bytes[2..] {
        let d = digit(byte).unwrap();
        for bit in (0..width).rev() {
            let one = (d >> bit) & 1;
            if bits == 0 && one == 0 {
                continue;
            }
            bits += 1;
            if bits <= 54 {
                top = top * 2 + u64::from(one);
            } else {
                sticky |= one != 0;
            }
        }
    }
    Some(if bits <= 53 {
        top as f64
    } else {
        let significant = top >> 1;
        let round = top & 1 != 0 && (sticky || significant & 1 != 0);
        (significant + u64::from(round)) as f64 * 2.0f64.powi(bits - 53)
    })
}
