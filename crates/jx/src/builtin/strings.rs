use super::library::{Library, number};
use crate::{
    Error, OwnedString, Value,
    json::string,
    value::{range_error, type_error},
};

fn units(value: &Value<'_, '_>) -> Vec<u16> {
    string::units(value.string_body().expect("validated string")).collect()
}
fn text<'e, 'i>(units: impl IntoIterator<Item = u16>) -> Value<'e, 'i> {
    Value::String(OwnedString::units(units))
}
fn find(haystack: &[u16], needle: &[u16]) -> Option<usize> {
    if needle.is_empty() {
        Some(0)
    } else {
        haystack.windows(needle.len()).position(|s| s == needle)
    }
}
fn characters(units: &[u16]) -> Vec<usize> {
    let mut positions = Vec::new();
    let mut at = 0;
    while at < units.len() {
        positions.push(at);
        at += if (0xd800..=0xdbff).contains(&units[at])
            && units
                .get(at + 1)
                .is_some_and(|n| (0xdc00..=0xdfff).contains(n))
        {
            2
        } else {
            1
        };
    }
    positions.push(at);
    positions
}
fn index(n: f64, len: usize) -> usize {
    let n = if n.is_nan() { 0.0 } else { n.trunc() };
    (if n < 0.0 { len as f64 + n } else { n }).clamp(0.0, len as f64) as usize
}
// UTF-8 and UTF-16 searches agree when both strings contain complete codepoints.
// Escapes can contain isolated surrogates, so those keep the unit-based path.
fn unescaped<'a>(value: &'a Value<'_, '_>) -> Option<&'a str> {
    let body = value.string_body()?;
    (!body.as_bytes().contains(&b'\\')).then_some(body)
}
fn utf8<'e, 'i>(function: Library, args: &[Option<Value<'e, 'i>>; 3]) -> Option<Value<'e, 'i>> {
    let value = args[0].as_ref()?;
    let source = unescaped(value)?;
    Some(match function {
        Library::Uppercase => Value::String(OwnedString::text(source.to_uppercase())),
        Library::Lowercase => Value::String(OwnedString::text(source.to_lowercase())),
        Library::Contains | Library::Before | Library::After => {
            let pattern = match args[1].as_ref() {
                Some(pattern) => unescaped(pattern)?,
                None if function == Library::Before => "undefined",
                _ => return None,
            };
            let at = source.find(pattern);
            match function {
                Library::Contains => Value::Boolean(at.is_some()),
                _ => match at {
                    None => value.clone(),
                    Some(at) => Value::String(OwnedString::body(if function == Library::Before {
                        &source[..at]
                    } else {
                        &source[at + pattern.len()..]
                    })),
                },
            }
        }
        Library::Substring => {
            let len = source.chars().count();
            let mut start = number(&args[1]).unwrap_or(f64::NAN);
            if len as f64 + start < 0.0 {
                start = 0.0;
            }
            let end = number(&args[2]).map_or(len, |length| {
                if length <= 0.0 {
                    0
                } else {
                    index(
                        if start >= 0.0 {
                            start + length
                        } else {
                            len as f64 + start + length
                        },
                        len,
                    )
                }
            });
            let start = index(start, len);
            let boundary = |index| {
                source
                    .char_indices()
                    .nth(index)
                    .map_or(source.len(), |(at, _)| at)
            };
            Value::String(OwnedString::body(
                &source[boundary(start)..boundary(end.max(start))],
            ))
        }
        _ => return None,
    })
}
pub(super) fn call<'e, 'i>(
    function: Library,
    args: &[Option<Value<'e, 'i>>; 3],
    offset: usize,
) -> Result<Option<Value<'e, 'i>>, Error> {
    let Some(value) = &args[0] else {
        return Ok(None);
    };
    if function == Library::Join {
        let separator = args[1].as_ref().map(units).unwrap_or_default();
        let mut result = Vec::new();
        let mut first = true;
        let mut append = |item: Value<'e, 'i>| {
            if !first {
                result.extend_from_slice(&separator);
            }
            first = false;
            result.extend(string::units(item.string_body().unwrap()));
        };
        if value.is_array() {
            value.elements().for_each(append);
        } else {
            append(value.clone());
        }
        return Ok(Some(text(result)));
    }
    if function == Library::Length {
        // Count Unicode codepoints without decoding/copying the string.
        let mut count = 0;
        let mut high = false;
        for unit in string::units(value.string_body().unwrap()) {
            if !(high && (0xdc00..=0xdfff).contains(&unit)) {
                count += 1;
            }
            high = (0xd800..=0xdbff).contains(&unit);
        }
        return Ok(Some(Value::Number(count as f64)));
    }
    if let Some(result) = utf8(function, args) {
        return Ok(Some(result));
    }
    let source = units(value);
    let result = match function {
        Library::Uppercase | Library::Lowercase => {
            // Transform valid runs together (e.g. final Greek sigma), retaining
            // unpaired UTF-16 surrogates unchanged across the JSON boundary.
            let mut result = Vec::new();
            let mut run = String::new();
            let flush = |run: &mut String, result: &mut Vec<u16>| {
                let mapped = if function == Library::Uppercase {
                    run.to_uppercase()
                } else {
                    run.to_lowercase()
                };
                result.extend(mapped.encode_utf16());
                run.clear();
            };
            for ch in char::decode_utf16(source.iter().copied()) {
                match ch {
                    Ok(ch) => run.push(ch),
                    Err(ch) => {
                        flush(&mut run, &mut result);
                        result.push(ch.unpaired_surrogate());
                    }
                }
            }
            flush(&mut run, &mut result);
            text(result)
        }
        Library::Trim => {
            let mut result = Vec::new();
            let mut space = false;
            for unit in source {
                if matches!(unit, 9 | 10 | 13 | 32) {
                    space = !result.is_empty();
                } else {
                    if space {
                        result.push(32);
                        space = false;
                    }
                    result.push(unit);
                }
            }
            text(result)
        }
        Library::Substring => {
            let positions = characters(&source);
            let len = positions.len() - 1;
            let mut start = number(&args[1]).unwrap_or(f64::NAN);
            if len as f64 + start < 0.0 {
                start = 0.0;
            }
            let end = number(&args[2])
                .map(|length| {
                    if length <= 0.0 {
                        0
                    } else {
                        index(
                            if start >= 0.0 {
                                start + length
                            } else {
                                len as f64 + start + length
                            },
                            len,
                        )
                    }
                })
                .unwrap_or(len);
            let start = index(start, len);
            text(
                source[positions[start]..positions[end.max(start)]]
                    .iter()
                    .copied(),
            )
        }
        Library::Before | Library::After | Library::Contains | Library::Split => {
            let pattern = match &args[1] {
                Some(pattern) => units(pattern),
                None if matches!(function, Library::Before | Library::After) => {
                    "undefined".encode_utf16().collect()
                }
                None => return Err(type_error(offset)),
            };
            if function == Library::After && args[1].is_none() && find(&source, &pattern).is_some()
            {
                return Err(type_error(offset));
            }
            match function {
                Library::Contains => Value::Boolean(find(&source, &pattern).is_some()),
                Library::Before | Library::After => match find(&source, &pattern) {
                    None => value.clone(),
                    Some(at) => text(
                        if function == Library::Before {
                            &source[..at]
                        } else {
                            &source[at + pattern.len()..]
                        }
                        .iter()
                        .copied(),
                    ),
                },
                Library::Split => {
                    let limit = number(&args[2]);
                    if limit.is_some_and(|n| n < 0.0) {
                        return Err(range_error(offset));
                    }
                    // JavaScript string split uses ToUint32 for its optional limit.
                    let limit = limit.map_or(u32::MAX as usize, |n| {
                        (n.trunc().rem_euclid(4294967296.0)) as usize
                    });
                    let mut result = Vec::new();
                    if pattern.is_empty() {
                        result.extend(source.into_iter().take(limit).map(|u| text([u])));
                    } else if limit != 0 {
                        let mut rest = source.as_slice();
                        while result.len() < limit {
                            let Some(at) = find(rest, &pattern) else {
                                result.push(text(rest.iter().copied()));
                                break;
                            };
                            result.push(text(rest[..at].iter().copied()));
                            rest = &rest[at + pattern.len()..];
                        }
                    }
                    Value::array(result, false)
                }
                _ => unreachable!(),
            }
        }
        _ => unreachable!(),
    };
    Ok(Some(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn escaped(text: &str) -> String {
        use std::fmt::Write;
        let mut json = String::from("\"");
        for unit in text.encode_utf16() {
            write!(json, "\\u{unit:04x}").unwrap();
        }
        json.push('"');
        json
    }
    fn same(a: Option<Value<'_, '_>>, b: Option<Value<'_, '_>>) {
        match (a, b) {
            (Some(a), Some(b)) if a.string_body().is_some() => {
                assert!(a.string_units().unwrap().eq(b.string_units().unwrap()));
            }
            (Some(a), Some(b)) => assert_eq!(a.as_bool(), b.as_bool()),
            (None, None) => {}
            _ => panic!("cardinality mismatch"),
        }
    }
    #[test]
    fn utf8_operations_agree_with_utf16_for_unicode_and_numeric_boundaries() {
        for source in ["", "Ada Lovelace", "é中😀😀", "ΟΣ ΟΣΑ", "ßİı", "a/b"] {
            let input = escaped(source);
            let raw = Value::Raw(crate::validate(input.as_bytes()).unwrap());
            let plain = Value::from_string(source);
            for function in [Library::Uppercase, Library::Lowercase] {
                same(
                    call(function, &[Some(plain.clone()), None, None], 0).unwrap(),
                    call(function, &[Some(raw.clone()), None, None], 0).unwrap(),
                );
            }
            for pattern in ["", "a", "😀", "é中", " ", "missing"] {
                let pattern_json = escaped(pattern);
                for function in [Library::Contains, Library::Before, Library::After] {
                    same(
                        call(
                            function,
                            &[Some(plain.clone()), Some(Value::from_string(pattern)), None],
                            0,
                        )
                        .unwrap(),
                        call(
                            function,
                            &[
                                Some(raw.clone()),
                                Some(Value::Raw(
                                    crate::validate(pattern_json.as_bytes()).unwrap(),
                                )),
                                None,
                            ],
                            0,
                        )
                        .unwrap(),
                    );
                }
            }
            for start in [
                f64::NEG_INFINITY,
                -99.0,
                -2.5,
                -1.0,
                -0.0,
                0.0,
                0.5,
                1.0,
                2.5,
                99.0,
                f64::INFINITY,
                f64::NAN,
            ] {
                for len in [
                    None,
                    Some(-1.0),
                    Some(0.0),
                    Some(0.5),
                    Some(1.0),
                    Some(3.5),
                    Some(f64::INFINITY),
                    Some(f64::NAN),
                ] {
                    let args = [
                        Some(plain.clone()),
                        Some(Value::Number(start)),
                        len.map(Value::Number),
                    ];
                    let escaped_args = [
                        Some(raw.clone()),
                        Some(Value::Number(start)),
                        len.map(Value::Number),
                    ];
                    same(
                        call(Library::Substring, &args, 0).unwrap(),
                        call(Library::Substring, &escaped_args, 0).unwrap(),
                    );
                }
            }
        }
    }
    #[test]
    fn surrogate_searches_and_missing_patterns_keep_unit_semantics() {
        let source = Value::from_string("a😀b");
        let pattern = Value::Raw(crate::validate(br#""\ud83d""#).unwrap());
        assert_eq!(
            call(
                Library::Contains,
                &[Some(source.clone()), Some(pattern), None],
                0
            )
            .unwrap()
            .unwrap()
            .as_bool(),
            Some(true)
        );
        let source = Value::Raw(crate::validate(br#""a\ud800b""#).unwrap());
        assert!(utf8(Library::Uppercase, &[Some(source), None, None]).is_none());
        for text in ["nothing", "hasundefinedsuffix"] {
            let source = Value::from_string(text);
            let encoded = escaped(text);
            let fallback = Value::Raw(crate::validate(encoded.as_bytes()).unwrap());
            for function in [Library::Before, Library::After] {
                let a = call(function, &[Some(source.clone()), None, None], 0);
                let b = call(function, &[Some(fallback.clone()), None, None], 0);
                match (a, b) {
                    (Ok(a), Ok(b)) => same(a, b),
                    (Err(a), Err(b)) => assert_eq!(a, b),
                    _ => panic!("error mismatch"),
                }
            }
        }
    }
}
