use crate::{Error, OwnedString, Value, json::string, value::range_error};

pub(super) fn pad<'e, 'i>(
    args: &[Option<Value<'e, 'i>>; 3],
    offset: usize,
) -> Result<Option<Value<'e, 'i>>, Error> {
    let Some(value) = &args[0] else {
        return Ok(None);
    };
    let body = value.string_body().unwrap();
    let width = super::library::number(&args[1]).unwrap_or(f64::NAN).trunc();
    let count = char::decode_utf16(string::units(body)).count();
    if width.is_nan() || width.abs() <= count as f64 {
        return Ok(Some(value.clone()));
    }
    let length = width.abs() - count as f64;
    // Match the engine's existing construction guard instead of risking an
    // unbounded allocation for record-derived widths.
    if length > 1_000_000.0 {
        return Err(range_error(offset));
    }
    let pattern = args[2]
        .as_ref()
        .and_then(Value::string_body)
        .filter(|s| !s.is_empty())
        .unwrap_or(" ");
    let mut padding = Vec::new();
    let mut remaining = length as usize;
    while remaining > 0 {
        for ch in char::decode_utf16(string::units(pattern)) {
            match ch {
                Ok(ch) => {
                    let mut units = [0; 2];
                    padding.extend_from_slice(ch.encode_utf16(&mut units));
                }
                Err(ch) => padding.push(ch.unpaired_surrogate()),
            }
            remaining -= 1;
            if remaining == 0 {
                break;
            }
        }
    }
    let result = if width > 0.0 {
        OwnedString::units(string::units(body).chain(padding))
    } else {
        OwnedString::units(padding.into_iter().chain(string::units(body)))
    };
    Ok(Some(Value::String(result)))
}
