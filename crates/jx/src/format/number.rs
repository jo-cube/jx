use super::{error, integer::regular};
use crate::{Error, Value};

#[derive(Clone, Debug)]
struct Symbols {
    decimal: char,
    group: char,
    exponent: char,
    zero: u32,
    digit: char,
    separator: char,
    percent: String,
    permille: String,
    minus: String,
}
impl Symbols {
    fn new(options: Option<&Value<'_, '_>>, offset: usize) -> Result<Self, Error> {
        let get = |key: &str, fallback: &str| -> Result<String, Error> {
            let value = options.and_then(|o| o.field(key));
            value.as_ref().map_or_else(
                || Ok(fallback.into()),
                |v| super::text(v, offset).map(|s| s.into_owned()),
            )
        };
        let single = |key: &str, fallback: &str| -> Result<char, Error> {
            let s = get(key, fallback)?;
            let mut chars = s.chars();
            let Some(ch) = chars.next() else {
                return Err(crate::value::type_error(offset));
            };
            if chars.next().is_some() || ch.len_utf16() != 1 {
                return Err(super::unsupported(
                    offset,
                    "multi-unit decimal-format symbols are deferred",
                ));
            }
            Ok(ch)
        };
        let zero = single("zero-digit", "0")? as u32;
        if (zero..=zero + 9).any(|u| char::from_u32(u).is_none()) {
            return Err(super::unsupported(
                offset,
                "surrogate decimal digit families are deferred",
            ));
        }
        Ok(Self {
            decimal: single("decimal-separator", ".")?,
            group: single("grouping-separator", ",")?,
            exponent: single("exponent-separator", "e")?,
            zero,
            digit: single("digit", "#")?,
            separator: single("pattern-separator", ";")?,
            percent: get("percent", "%")?,
            permille: get("per-mille", "‰")?,
            minus: get("minus-sign", "-")?,
        })
    }
    fn mandatory(&self, ch: char) -> bool {
        (ch as u32) >= self.zero && (ch as u32) < self.zero + 10
    }
    fn active(&self, ch: char) -> bool {
        self.mandatory(ch)
            || [
                self.decimal,
                self.group,
                self.exponent,
                self.digit,
                self.separator,
            ]
            .contains(&ch)
    }
    fn digits(&self, part: &str) -> usize {
        part.chars()
            .filter(|c| self.mandatory(*c) || *c == self.digit)
            .count()
    }
}
#[derive(Clone, Debug)]
struct Subpicture {
    prefix: String,
    suffix: String,
    min_integer: usize,
    min_fraction: usize,
    max_fraction: usize,
    scale: usize,
    exponent: usize,
    decimal: bool,
    multiplier: f64,
    integer_groups: Box<[usize]>,
    fraction_groups: Box<[usize]>,
    repeat: usize,
}
#[derive(Clone, Debug)]
pub(super) struct Number {
    symbols: Symbols,
    positive: Subpicture,
    negative: Subpicture,
}
impl Number {
    pub fn new(
        picture: &str,
        options: Option<&Value<'_, '_>>,
        offset: usize,
    ) -> Result<Self, Error> {
        let symbols = Symbols::new(options, offset)?;
        let mut parts = picture.split(symbols.separator);
        let first = parts.next().unwrap();
        let second = parts.next();
        if parts.next().is_some() {
            return Err(error(offset, "D3080"));
        }
        let positive = Self::subpicture(first, &symbols, offset)?;
        let negative = if let Some(second) = second {
            Self::subpicture(second, &symbols, offset)?
        } else {
            let mut neg = positive.clone();
            neg.prefix.insert_str(0, &symbols.minus);
            neg
        };
        Ok(Self {
            symbols,
            positive,
            negative,
        })
    }
    fn subpicture(picture: &str, s: &Symbols, offset: usize) -> Result<Subpicture, Error> {
        let first = picture
            .char_indices()
            .find(|(_, c)| s.active(*c) && *c != s.exponent)
            .map_or(0, |(i, _)| i);
        let last = picture
            .char_indices()
            .rfind(|(_, c)| s.active(*c) && *c != s.exponent)
            .map_or(picture.len(), |(i, c)| i + c.len_utf8());
        let prefix = &picture[..first];
        let suffix = &picture[last..];
        let active = &picture[first..last];
        // The pinned reference computes an absolute exponent position, including
        // the prefix. Keep that picture-language quirk separate from execution.
        let split = picture[first..]
            .find(s.exponent)
            .map(|i| i + first)
            .filter(|i| *i <= last);
        let (mantissa, exponent) = if let Some(i) = split {
            let units = picture[..i].encode_utf16().count();
            let byte = |n| {
                let mut units = 0;
                active
                    .char_indices()
                    .find_map(|(i, c)| {
                        let at = units;
                        units += c.len_utf16();
                        (at >= n).then_some(i)
                    })
                    .unwrap_or(active.len())
            };
            let i = byte(units);
            (
                active.get(..i).ok_or_else(|| error(offset, "D3093"))?,
                Some(&active[byte(units + 1)..]),
            )
        } else {
            (active, None)
        };
        let (integer, fraction) = mantissa.split_once(s.decimal).unwrap_or((mantissa, suffix));
        let mut invalid = None;
        if picture.matches(s.decimal).count() > 1 {
            invalid = Some("D3081");
        }
        if !s.percent.is_empty() && picture.matches(&s.percent).count() > 1 {
            invalid = Some("D3082");
        }
        if !s.permille.is_empty() && picture.matches(&s.permille).count() > 1 {
            invalid = Some("D3083");
        }
        let percent = !s.percent.is_empty() && picture.contains(&s.percent);
        let permille = !s.permille.is_empty() && picture.contains(&s.permille);
        if percent && permille {
            invalid = Some("D3084");
        }
        if s.digits(mantissa) == 0 {
            invalid = Some("D3085");
        }
        if active.chars().any(|c| !s.active(c)) {
            invalid = Some("D3086");
        }
        let chars: Vec<_> = picture.chars().collect();
        if let Some(i) = chars.iter().position(|c| *c == s.decimal) {
            if i > 0 && chars[i - 1] == s.group || chars.get(i + 1) == Some(&s.group) {
                invalid = Some("D3087");
            }
        } else if integer.ends_with(s.group) {
            invalid = Some("D3088");
        }
        if picture.contains(&format!("{}{}", s.group, s.group)) {
            invalid = Some("D3089");
        }
        if integer
            .split_once(s.digit)
            .is_some_and(|(before, _)| before.chars().any(|c| s.mandatory(c)))
        {
            invalid = Some("D3090");
        }
        if fraction
            .rsplit_once(s.digit)
            .is_some_and(|(_, after)| after.chars().any(|c| s.mandatory(c)))
        {
            invalid = Some("D3091");
        }
        if exponent.is_some_and(|e| !e.is_empty()) && (percent || permille) {
            invalid = Some("D3092");
        }
        if exponent.is_some_and(|e| e.is_empty() || e.chars().any(|c| !s.mandatory(c))) {
            invalid = Some("D3093");
        }
        if let Some(code) = invalid {
            return Err(error(offset, code));
        }
        let groups = |part: &str, left: bool| -> Box<[usize]> {
            part.char_indices()
                .filter(|(_, c)| *c == s.group)
                .map(|(i, _)| s.digits(if left { &part[..i] } else { &part[i..] }))
                .collect()
        };
        let integer_groups = groups(integer, false);
        let fraction_groups = groups(fraction, true);
        let repeat = regular(integer_groups.iter().copied());
        let mut min_integer = integer.chars().filter(|c| s.mandatory(*c)).count();
        let scale = min_integer;
        let mut min_fraction = fraction.chars().filter(|c| s.mandatory(*c)).count();
        let mut max_fraction = s.digits(fraction);
        if min_integer == 0 && max_fraction == 0 {
            if exponent.is_some() {
                min_fraction = 1;
                max_fraction = 1;
            } else {
                min_integer = 1;
            }
        }
        if exponent.is_some() && min_integer == 0 && integer.contains(s.digit) {
            min_integer = 1;
        }
        if min_integer == 0 && min_fraction == 0 {
            min_fraction = 1;
        }
        if max_fraction > 100 || min_integer > 1_000_000 {
            return Err(crate::value::range_error(offset));
        }
        Ok(Subpicture {
            prefix: prefix.into(),
            suffix: suffix.into(),
            min_integer,
            min_fraction,
            max_fraction,
            scale,
            exponent: exponent.map_or(0, |e| e.chars().count()),
            decimal: picture.contains(s.decimal),
            multiplier: if percent {
                100.0
            } else if permille {
                1000.0
            } else {
                1.0
            },
            integer_groups,
            fraction_groups,
            repeat,
        })
    }
    pub fn format(&self, value: f64, offset: usize) -> Result<String, Error> {
        let pic = if value >= 0.0 {
            &self.positive
        } else {
            &self.negative
        };
        let s = &self.symbols;
        let mut mantissa = value * pic.multiplier;
        let mut exponent = 0_i32;
        if pic.exponent > 0 && !mantissa.is_finite() {
            return Err(super::unsupported(
                offset,
                "non-finite exponent formatting is deferred",
            ));
        }
        if pic.exponent == 0 && mantissa.is_finite() && mantissa.abs() >= 1e21 {
            return Err(super::unsupported(
                offset,
                "large fixed decimal output is deferred; use an exponent picture",
            ));
        }
        if pic.exponent > 0 && mantissa != 0.0 {
            let max = 10_f64.powi(pic.scale as i32);
            let min = 10_f64.powi(pic.scale as i32 - 1);
            while mantissa.abs() < min {
                mantissa *= 10.0;
                exponent -= 1;
            }
            while mantissa.abs() > max {
                mantissa /= 10.0;
                exponent += 1;
            }
        }
        let rounded = crate::builtin::numeric::round(mantissa, pic.max_fraction as f64).abs();
        if pic.exponent > 0 && !rounded.is_finite() {
            return Err(crate::value::range_error(offset));
        }
        if rounded.is_finite() && rounded >= 1e21 {
            return Err(super::unsupported(
                offset,
                "large fixed decimal output is deferred; use an exponent picture",
            ));
        }
        let buffer = super::decimal::Decimal::fixed(rounded, pic.max_fraction);
        let fixed = if rounded.is_nan() {
            "NaN"
        } else if rounded.is_infinite() {
            "Infinity"
        } else {
            buffer.as_str()
        };
        let (whole, fraction) = fixed.split_once('.').unwrap_or((fixed, ""));
        let whole = whole.trim_start_matches('0');
        let fraction = fraction.trim_end_matches('0');
        let integer_length = whole.len().max(pic.min_integer);
        let fraction_length = fraction.len().max(pic.min_fraction);
        let digit = |d: u8| {
            if d.is_ascii_digit() {
                char::from_u32(s.zero + u32::from(d) - 48).unwrap()
            } else {
                d as char
            }
        };
        let mut out = String::with_capacity(
            pic.prefix.len() + pic.suffix.len() + integer_length + fraction_length + 16,
        );
        out.push_str(&pic.prefix);
        for i in 0..integer_length {
            let right = integer_length - i;
            if pic.repeat > 0 {
                if i > 0 && right.is_multiple_of(pic.repeat) {
                    out.push(s.group);
                }
            } else {
                for _ in pic.integer_groups.iter().filter(|p| **p == right) {
                    out.push(s.group);
                }
            }
            out.push(digit(if i < integer_length - whole.len() {
                b'0'
            } else {
                whole.as_bytes()[i - (integer_length - whole.len())]
            }));
        }
        if pic.decimal && fraction_length > 0 {
            out.push(s.decimal);
        }
        for i in 0..fraction_length {
            for _ in pic.fraction_groups.iter().filter(|p| **p == i) {
                out.push(s.group);
            }
            out.push(digit(*fraction.as_bytes().get(i).unwrap_or(&b'0')));
        }
        if pic.exponent > 0 {
            out.push(s.exponent);
            if exponent < 0 {
                out.push_str(&s.minus);
            }
            let e = exponent.unsigned_abs().to_string();
            for _ in e.len()..pic.exponent {
                out.push(digit(b'0'));
            }
            for d in e.bytes() {
                out.push(digit(d));
            }
        }
        out.push_str(&pic.suffix);
        Ok(out)
    }
}
