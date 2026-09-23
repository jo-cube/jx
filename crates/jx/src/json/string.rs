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

pub(crate) struct Units<'a> {
    chars: std::str::Chars<'a>,
    pending: Option<u16>,
}

impl Iterator for Units<'_> {
    type Item = u16;

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
