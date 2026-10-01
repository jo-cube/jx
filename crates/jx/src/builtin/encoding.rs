use super::library::Library;
use crate::{Error, ErrorKind, OwnedString, Value, json::string};
use base64::{
    Engine, alphabet,
    engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig, general_purpose::STANDARD},
};
use std::fmt::Write;

fn invalid(offset: usize) -> Error {
    Error::new(ErrorKind::EncodingError, offset, "malformed URI encoding")
}
fn reserved(byte: u8) -> bool {
    b";/?:@&=+$,#".contains(&byte)
}
fn safe(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte)
}
fn hex(unit: u16) -> Option<u8> {
    match unit {
        48..=57 => Some((unit - 48) as u8),
        65..=70 => Some((unit - 55) as u8),
        97..=102 => Some((unit - 87) as u8),
        _ => None,
    }
}
fn percent(units: &[u16], at: usize, offset: usize) -> Result<u8, Error> {
    if units.get(at) != Some(&37) {
        return Err(invalid(offset));
    }
    let high = units
        .get(at + 1)
        .and_then(|u| hex(*u))
        .ok_or_else(|| invalid(offset))?;
    let low = units
        .get(at + 2)
        .and_then(|u| hex(*u))
        .ok_or_else(|| invalid(offset))?;
    Ok(high * 16 + low)
}
pub(super) fn call<'e, 'i>(
    kind: Library,
    value: Option<Value<'e, 'i>>,
    offset: usize,
) -> Result<Option<Value<'e, 'i>>, Error> {
    let Some(value) = value else { return Ok(None) };
    let body = value.string_body().unwrap();
    let result = match kind {
        Library::Base64Encode => {
            // The pinned Node implementation treats strings as binary: the low
            // byte of each UTF-16 unit, not UTF-8. Keep this conversion explicit.
            let bytes: Vec<_> = string::units(body).map(|u| u as u8).collect();
            let encoded = STANDARD.encode(bytes);
            OwnedString::body(&encoded)
        }
        Library::Base64Decode => {
            if string::units(body).any(|u| u > 255) {
                return Err(Error::new(
                    ErrorKind::UnsupportedExpression,
                    offset,
                    "non-Latin1 base64 decoding is host-dependent",
                ));
            }
            // Node accepts URL alphabet, ignores non-alphabet characters, stops
            // at padding, and permits omitted padding/nonzero trailing bits.
            let mut bytes: Vec<_> = string::units(body)
                .take_while(|u| *u != 61)
                .filter_map(|u| match u {
                    45 => Some(b'+'),
                    95 => Some(b'/'),
                    43 | 47 | 48..=57 | 65..=90 | 97..=122 => Some(u as u8),
                    _ => None,
                })
                .collect();
            if bytes.len() % 4 == 1 {
                bytes.pop();
            }
            let config = GeneralPurposeConfig::new()
                .with_decode_padding_mode(DecodePaddingMode::Indifferent)
                .with_decode_allow_trailing_bits(true);
            let decoded = GeneralPurpose::new(&alphabet::STANDARD, config)
                .decode(bytes)
                .expect("normalized base64");
            OwnedString::units(decoded.into_iter().map(u16::from))
        }
        Library::EncodeUrl | Library::EncodeComponent => {
            let uri = kind == Library::EncodeUrl;
            if string::units(body).all(|u| u < 128 && (safe(u as u8) || uri && reserved(u as u8))) {
                return Ok(Some(value));
            }
            let mut encoded = String::new();
            for ch in char::decode_utf16(string::units(body)) {
                let ch = ch.map_err(|_| invalid(offset))?;
                let mut bytes = [0; 4];
                for byte in ch.encode_utf8(&mut bytes).bytes() {
                    if safe(byte) || uri && reserved(byte) {
                        encoded.push(byte as char)
                    } else {
                        write!(encoded, "%{byte:02X}").unwrap()
                    }
                }
            }
            return Ok(Some(if encoded == body {
                value
            } else {
                Value::String(OwnedString::body(&encoded))
            }));
        }
        Library::DecodeUrl | Library::DecodeComponent => {
            if !string::units(body).any(|u| u == 37) {
                return Ok(Some(value));
            }
            let source: Vec<_> = string::units(body).collect();
            let mut output = Vec::new();
            let mut at = 0;
            while at < source.len() {
                if source[at] != 37 {
                    output.push(source[at]);
                    at += 1;
                    continue;
                }
                let byte = percent(&source, at, offset)?;
                if kind == Library::DecodeUrl && reserved(byte) {
                    output.extend_from_slice(&source[at..at + 3]);
                    at += 3;
                    continue;
                }
                let width = match byte {
                    0..=127 => 1,
                    194..=223 => 2,
                    224..=239 => 3,
                    240..=244 => 4,
                    _ => return Err(invalid(offset)),
                };
                let mut bytes = [0; 4];
                bytes[0] = byte;
                for (i, target) in bytes.iter_mut().enumerate().take(width).skip(1) {
                    *target = percent(&source, at + i * 3, offset)?
                }
                let decoded = std::str::from_utf8(&bytes[..width]).map_err(|_| invalid(offset))?;
                output.extend(decoded.encode_utf16());
                at += width * 3;
            }
            OwnedString::units(output)
        }
        _ => unreachable!(),
    };
    Ok(Some(Value::String(result)))
}
