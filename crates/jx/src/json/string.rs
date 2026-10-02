/// Compare a validated JSON string body with a compiled field name. UTF-16
/// units preserve JSON's escaped surrogate units without allocating a String.
pub(crate) fn matches(body: &str, field: &str) -> bool {
    if !body.as_bytes().contains(&b'\\') {
        return body == field;
    }
    Units {
        chars: body.chars(),
        pending: None,
    }
    .eq(field.encode_utf16())
}

pub(crate) fn equal(left: &str, right: &str) -> bool {
    if !left.as_bytes().contains(&b'\\') && !right.as_bytes().contains(&b'\\') {
        left == right
    } else {
        units(left).eq(units(right))
    }
}

pub(crate) fn fingerprint(body: &str) -> u64 {
    fingerprint_units(units(body))
}
pub(crate) fn fingerprint_units(units: impl IntoIterator<Item = u16>) -> u64 {
    use std::hash::{DefaultHasher, Hasher};
    let mut hash = DefaultHasher::new();
    for unit in units {
        hash.write_u16(unit);
    }
    hash.finish()
}

pub(crate) struct Units<'a> {
    chars: std::str::Chars<'a>,
    pending: Option<u16>,
}

impl Iterator for Units<'_> {
    type Item = u16;

    fn size_hint(&self) -> (usize, Option<usize>) {
        let bytes = self.chars.as_str().len();
        let pending = usize::from(self.pending.is_some());
        // A validated escape uses at most six bytes for one UTF-16 unit.
        (bytes.div_ceil(6) + pending, Some(bytes + pending))
    }

    fn next(&mut self) -> Option<Self::Item> {
        if let Some(unit) = self.pending.take() {
            return Some(unit);
        }
        let mut ch = self.chars.next()?;
        if ch == '\\' {
            ch = match self.chars.next().expect("validated escape") {
                'u' => {
                    let mut unit = 0;
                    for _ in 0..4 {
                        unit = unit * 16 + self.chars.next().unwrap().to_digit(16).unwrap() as u16;
                    }
                    return Some(unit);
                }
                'b' => '\u{8}',
                'f' => '\u{c}',
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                other => other,
            };
        }
        let mut units = [0; 2];
        let encoded = ch.encode_utf16(&mut units);
        if encoded.len() == 2 {
            self.pending = Some(encoded[1]);
        }
        Some(encoded[0])
    }
}

pub(crate) fn units(body: &str) -> Units<'_> {
    Units {
        chars: body.chars(),
        pending: None,
    }
}

#[cfg(test)]
mod tests {
    use super::units;

    #[test]
    fn equal_strings_have_equal_fingerprints() {
        for (a, b) in [
            ("a", r"\u0061"),
            ("é😀", r"\u00e9\ud83d\ude00"),
            (r"\ud800", r"\uD800"),
            ("a/b", r"a\/b"),
            ("", ""),
        ] {
            assert!(super::equal(a, b));
            assert_eq!(super::fingerprint(a), super::fingerprint(b));
        }
        assert!(!super::equal(r"\ud800", r"\udc00"));
    }
    #[test]
    fn size_hints_bound_remaining_units() {
        for (body, mut remaining) in [
            ("", 0),
            ("abc", 3),
            ("é😀", 3),
            (r"\ud800", 1),
            (r"\ud83d\ude00", 2),
            (r"a\n\u0000", 3),
        ] {
            let mut units = units(body);
            loop {
                let (lower, upper) = units.size_hint();
                assert!(lower <= remaining && upper.unwrap() >= remaining, "{body}");
                if units.next().is_none() {
                    assert_eq!(remaining, 0);
                    break;
                }
                remaining -= 1;
            }
        }
    }
}
