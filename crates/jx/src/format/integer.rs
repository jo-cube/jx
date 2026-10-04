use super::{error, words};
use crate::Error;

const ZEROS: [u32; 37] = [
    0x30, 0x0660, 0x06f0, 0x07c0, 0x0966, 0x09e6, 0x0a66, 0x0ae6, 0x0b66, 0x0be6, 0x0c66, 0x0ce6,
    0x0d66, 0x0de6, 0x0e50, 0x0ed0, 0x0f20, 0x1040, 0x1090, 0x17e0, 0x1810, 0x1946, 0x19d0, 0x1a80,
    0x1a90, 0x1b50, 0x1bb0, 0x1c40, 0x1c50, 0xa620, 0xa8d0, 0xa900, 0xa9d0, 0xa9f0, 0xaa50, 0xabf0,
    0xff10,
];
#[derive(Clone, Copy, Debug)]
pub(super) enum Case {
    Lower,
    Upper,
    Title,
}
#[derive(Clone, Debug)]
pub(super) enum Style {
    Decimal(u32),
    Letters,
    Roman,
    Words,
    Sequence,
}
#[derive(Clone, Debug)]
pub(super) struct Integer {
    pub style: Style,
    pub case: Case,
    pub ordinal: bool,
    pub minimum: usize,
    pub optional: usize,
    pub separators: Box<[(usize, char)]>,
    pub repeat: usize,
}
pub(super) fn regular(positions: impl IntoIterator<Item = usize>) -> usize {
    let positions: Vec<_> = positions.into_iter().collect();
    let gcd = |mut a: usize, mut b: usize| {
        while b != 0 {
            (a, b) = (b, a % b);
        }
        a
    };
    let factor = positions.iter().copied().reduce(gcd).unwrap_or(0);
    if factor != 0 && (1..=positions.len()).all(|i| positions.contains(&(i * factor))) {
        factor
    } else {
        0
    }
}
impl Integer {
    pub fn new(picture: &str, offset: usize) -> Result<Self, Error> {
        let (primary, modifier) = picture.rsplit_once(';').unwrap_or((picture, ""));
        let mut result = Self {
            style: Style::Decimal(48),
            case: Case::Lower,
            ordinal: modifier.starts_with('o'),
            minimum: 0,
            optional: 0,
            separators: Box::new([]),
            repeat: 0,
        };
        result.style = match primary {
            "A" => {
                result.case = Case::Upper;
                Style::Letters
            }
            "a" => Style::Letters,
            "I" => {
                result.case = Case::Upper;
                Style::Roman
            }
            "i" => Style::Roman,
            "W" => {
                result.case = Case::Upper;
                Style::Words
            }
            "Ww" => {
                result.case = Case::Title;
                Style::Words
            }
            "w" => Style::Words,
            _ => {
                let mut zero = None;
                let mut positions = Vec::new();
                let mut position = 0;
                for ch in primary.chars().rev() {
                    if let Some(family) = ZEROS
                        .iter()
                        .copied()
                        .find(|z| (ch as u32) >= *z && (ch as u32) <= *z + 9)
                    {
                        if zero.is_some_and(|z| z != family) {
                            return Err(error(offset, "D3131"));
                        }
                        zero = Some(family);
                        result.minimum += 1;
                        position += 1;
                    } else if ch == '#' {
                        result.optional += 1;
                        position += 1;
                    } else {
                        positions.push((position, ch));
                    }
                }
                if let Some(zero) = zero {
                    if positions.iter().all(|(_, c)| *c == positions[0].1) {
                        result.repeat = regular(positions.iter().map(|(p, _)| *p));
                    }
                    result.separators = positions.into_boxed_slice();
                    Style::Decimal(zero)
                } else {
                    Style::Sequence
                }
            }
        };
        Ok(result)
    }
    pub fn format(&self, n: f64, offset: usize) -> Result<String, Error> {
        if !n.is_finite()
            && !(matches!(self.style, Style::Decimal(_))
                || n.is_nan() && matches!(self.style, Style::Letters | Style::Roman))
        {
            return Err(super::unsupported(
                offset,
                "non-finite word/alphabetic/Roman formatting is deferred",
            ));
        }
        let n = n.floor();
        let mut out = String::new();
        if n < 0.0 {
            out.push('-');
        }
        self.write(n.abs(), offset, &mut out)?;
        Ok(out)
    }
    pub fn write(&self, n: f64, offset: usize, out: &mut String) -> Result<(), Error> {
        match self.style {
            Style::Sequence => return Err(error(offset, "D3130")),
            Style::Words => {
                let start = out.len();
                words::write(n, self.ordinal, false, out);
                match self.case {
                    Case::Lower => out[start..].make_ascii_lowercase(),
                    Case::Upper => out[start..].make_ascii_uppercase(),
                    Case::Title => {}
                }
            }
            Style::Letters => {
                let mut value = n;
                let mut chars = [0u8; 256];
                let mut at = chars.len();
                let base = if matches!(self.case, Case::Upper) {
                    b'A'
                } else {
                    b'a'
                };
                while value > 0.0 {
                    at -= 1;
                    chars[at] = base + ((value - 1.0) % 26.0) as u8;
                    value = ((value - 1.0) / 26.0).floor();
                }
                out.push_str(std::str::from_utf8(&chars[at..]).unwrap());
            }
            Style::Roman => {
                if n > 1_000_000.0 {
                    return Err(crate::value::range_error(offset));
                }
                let mut left = n as u64;
                for (value, text) in [
                    (1000, "m"),
                    (900, "cm"),
                    (500, "d"),
                    (400, "cd"),
                    (100, "c"),
                    (90, "xc"),
                    (50, "l"),
                    (40, "xl"),
                    (10, "x"),
                    (9, "ix"),
                    (5, "v"),
                    (4, "iv"),
                    (1, "i"),
                ] {
                    while left >= value {
                        for b in text.bytes() {
                            out.push(if matches!(self.case, Case::Upper) {
                                b.to_ascii_uppercase() as char
                            } else {
                                b as char
                            });
                        }
                        left -= value;
                    }
                }
            }
            Style::Decimal(zero) => {
                if n.is_finite() && n >= 1e21 {
                    return Err(super::unsupported(
                        offset,
                        "large decimal integer output is deferred",
                    ));
                }
                if self.minimum > 1_000_000 {
                    return Err(crate::value::range_error(offset));
                }
                let buffer = super::decimal::Decimal::integer(n);
                let digits = if n.is_nan() {
                    "NaN"
                } else if n.is_infinite() {
                    "Infinity"
                } else {
                    buffer.as_str()
                };
                let length = digits.len().max(self.minimum);
                let mut ordinal_last = [0; 2];
                for index in 0..length {
                    let right = length - index;
                    if index > 0 {
                        if self.repeat > 0 && right.is_multiple_of(self.repeat) {
                            out.push(self.separators[0].1);
                        } else if self.repeat == 0 {
                            for (_, ch) in self.separators.iter().rev().filter(|(p, _)| *p == right)
                            {
                                out.push(*ch);
                            }
                        }
                    }
                    let digit = if index < length - digits.len() {
                        b'0'
                    } else {
                        digits.as_bytes()[index - (length - digits.len())]
                    };
                    let ch = char::from_u32(zero + u32::from(digit) - 48).unwrap();
                    out.push(ch);
                    ordinal_last = [ordinal_last[1], ch as u32];
                }
                if self.ordinal {
                    out.push_str(if ordinal_last[0] == 49 {
                        "th"
                    } else {
                        match ordinal_last[1] {
                            49 => "st",
                            50 => "nd",
                            51 => "rd",
                            _ => "th",
                        }
                    });
                }
            }
        }
        Ok(())
    }
    pub fn parse(&self, text: &str, offset: usize) -> Result<f64, Error> {
        Ok(match self.style {
            Style::Sequence => return Err(error(offset, "D3130")),
            Style::Words => words::parse(text),
            Style::Letters => {
                let base = if matches!(self.case, Case::Upper) {
                    'A' as u32
                } else {
                    'a' as u32
                };
                let mut result = 0.0;
                let mut position = 0;
                for ch in text.chars().rev() {
                    let mut units = [0; 2];
                    for unit in ch.encode_utf16(&mut units).iter().rev() {
                        result +=
                            (i64::from(*unit) - i64::from(base) + 1) as f64 * 26_f64.powi(position);
                        position += 1;
                    }
                }
                result
            }
            Style::Roman => {
                let mut max = 1.0;
                let mut value = 0.0;
                for ch in text.chars().rev() {
                    let digit = match ch.to_ascii_uppercase() {
                        'M' => 1000.0,
                        'D' => 500.0,
                        'C' => 100.0,
                        'L' => 50.0,
                        'X' => 10.0,
                        'V' => 5.0,
                        'I' => 1.0,
                        _ => f64::NAN,
                    };
                    if digit < max {
                        value -= digit;
                    } else {
                        max = digit;
                        value += digit;
                    }
                }
                value
            }
            Style::Decimal(zero) => {
                let text = if self.ordinal {
                    text.get(..text.len().saturating_sub(2)).unwrap_or("")
                } else {
                    text
                };
                if zero == 48 && self.separators.is_empty() {
                    return Ok(parse_prefix(text));
                }
                let mut digits = String::new();
                for ch in text.chars() {
                    if self.repeat > 0 && ch == ','
                        || self.repeat == 0 && self.separators.iter().any(|(_, s)| ch == *s)
                    {
                        continue;
                    }
                    let shifted = i64::from(ch as u32) - i64::from(zero) + 48;
                    if let Some(ch) = u32::try_from(shifted).ok().and_then(char::from_u32) {
                        digits.push(ch);
                    } else {
                        return Ok(f64::NAN);
                    }
                }
                let text = digits.trim_start();
                let mut end = usize::from(text.starts_with(['+', '-']));
                while text.as_bytes().get(end).is_some_and(u8::is_ascii_digit) {
                    end += 1;
                }
                text[..end].parse().unwrap_or(f64::NAN)
            }
        })
    }
    pub fn pattern(&self, width: Option<usize>, offset: usize) -> Result<String, Error> {
        Ok(match self.style {
            Style::Sequence => return Err(error(offset, "D3130")),
            Style::Letters => if matches!(self.case, Case::Upper) {
                "[A-Z]+"
            } else {
                "[a-z]+"
            }
            .into(),
            Style::Roman => if matches!(self.case, Case::Upper) {
                "[MDCLXVI]+"
            } else {
                "[mdclxvi]+"
            }
            .into(),
            Style::Words => words::pattern(),
            Style::Decimal(_) => {
                let mut pattern =
                    width.map_or_else(|| "[0-9]+".into(), |w| format!("[0-9]{{{w}}}"));
                if self.ordinal {
                    pattern.push_str("(?:th|st|nd|rd)");
                }
                pattern
            }
        })
    }
}

pub(super) fn parse_prefix(text: &str) -> f64 {
    let text = text.trim_start();
    let mut end = usize::from(text.starts_with(['+', '-']));
    while text.as_bytes().get(end).is_some_and(u8::is_ascii_digit) {
        end += 1;
    }
    text[..end].parse().unwrap_or(f64::NAN)
}
