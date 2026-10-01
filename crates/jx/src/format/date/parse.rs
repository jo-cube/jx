use super::super::{error, integer::parse_prefix};
use super::{
    DAYS, Date, MONTHS, Marker, Part,
    calendar::{self, DAY, Fields},
    utc,
};
use crate::{Error, sequence::Context};
#[derive(Clone, Debug)]
pub(in crate::format) enum Parser {
    Iso,
    Picture { date: Date, matcher: regress::Regex },
}
fn escape(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if "\\.*+?^${}()|[]".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}
impl Parser {
    pub fn new(picture: Option<&str>, offset: usize) -> Result<Self, Error> {
        let Some(picture) = picture else {
            return Ok(Self::Iso);
        };
        let date = Date::new(picture, offset)?;
        if !picture.is_ascii() {
            return Err(super::super::unsupported(
                offset,
                "non-ASCII case-insensitive date pictures are deferred",
            ));
        }
        let mut pattern = String::from("^");
        for part in &date.parts {
            pattern.push('(');
            match part {
                Part::Literal(s) => pattern.push_str(&escape(s)),
                Part::Marker(m) => {
                    let c = m.component;
                    if "Zz".contains(c) {
                        if c == 'z' {
                            pattern.push_str("GMT");
                        }
                        pattern.push_str("[-+][0-9]+");
                        if let Some(i) = &m.integer
                            && i.repeat > 0
                        {
                            pattern.push_str(&escape(&i.separators[0].1.to_string()));
                            pattern.push_str("[0-9]+");
                        }
                    } else if c == 'f' {
                        pattern.push_str("[0-9]+");
                    } else if let Some(integer) = &m.integer {
                        pattern.push_str(&integer.pattern(m.parse_width, offset)?);
                    } else if "MxFP".contains(c) {
                        pattern.push_str("[a-zA-Z]+");
                    } else {
                        return Err(error(offset, "D3133"));
                    }
                }
            }
            pattern.push(')');
        }
        pattern.push('$');
        let matcher =
            regress::Regex::with_flags(&pattern, "i").map_err(|_| error(offset, "D3135"))?;
        Ok(Self::Picture { date, matcher })
    }
    pub fn needs_clock(&self) -> bool {
        match self {
            Self::Iso => false,
            Self::Picture { date, .. } => {
                let mut any = false;
                let mut year = false;
                let mut week = false;
                for part in &date.parts {
                    if let Part::Marker(m) = part {
                        any = true;
                        year |= m.component == 'Y';
                        week |= "XxWw".contains(m.component);
                    }
                }
                any && (!year || week)
            }
        }
    }
    pub fn parse(
        &self,
        text: &str,
        context: &Context<'_, '_>,
        offset: usize,
    ) -> Result<Option<f64>, Error> {
        let Self::Picture { date, matcher } = self else {
            return iso(text, offset).map(Some);
        };
        if text.contains(['\u{0131}', '\u{017f}']) {
            return Err(super::super::unsupported(
                offset,
                "legacy date matcher case folding is deferred for dotless-i and long-s",
            ));
        }
        let Some(found) = matcher.find(text) else {
            return Ok(None);
        };
        let mut fields = [None; 128];
        let mut any = false;
        for (i, part) in date.parts.iter().enumerate() {
            if let Part::Marker(marker) = part {
                any = true;
                let text = &text[found.group(i + 1).unwrap()];
                fields[marker.component as usize] = component(marker, text, offset)?;
            }
        }
        if !any {
            return Ok(None);
        }
        let mask = |names: &str| {
            names.bytes().fold(0u32, |n, c| {
                n * 2 + u32::from(fields[c as usize].is_some_and(|v| v != 0.0 && !v.is_nan()))
            })
        };
        let is_type = |mask: u32, kind: u32| mask & !kind == 0 && mask & kind != 0;
        let date_mask = mask("YXMxWwdD");
        let time_mask = mask("PHhmsf");
        let date_a = is_type(date_mask, 161);
        let date_b = !date_a && is_type(date_mask, 130);
        let date_c = is_type(date_mask, 84);
        let date_d = !date_c && is_type(date_mask, 72);
        let time_b = !is_type(time_mask, 23) && is_type(time_mask, 47);
        let date_names = if date_b {
            "YD"
        } else if date_c {
            "XxwF"
        } else if date_d {
            "XWF"
        } else {
            "YMD"
        };
        let time_names = if time_b { "Phmsf" } else { "Hmsf" };
        let mut started = false;
        let mut ended = false;
        for c in date_names.bytes().chain(time_names.bytes()) {
            if fields[c as usize].is_some() {
                if ended {
                    return Err(error(offset, "D3136"));
                }
                started = true;
            } else if started {
                ended = true;
                fields[c as usize] = Some(if b"MDd".contains(&c) { 1.0 } else { 0.0 });
            } else {
                let now = context
                    .scope
                    .as_ref()
                    .expect("date default scope")
                    .timestamp();
                fields[c as usize] = Some(Fields::new(now).get(c as char) as f64);
            }
        }
        if date_c || date_d {
            return Err(error(offset, "D3136"));
        }
        let get = |c: char| fields[c as usize].unwrap_or(0.0);
        let year = get('Y');
        let mut month = if get('M') > 0.0 { get('M') } else { 1.0 };
        let mut day = get('D');
        if date_b {
            let ordinal = get('d');
            if !year.is_finite() || !ordinal.is_finite() || ordinal.abs() > 100_000_000.0 {
                return Ok(Some(f64::NAN));
            }
            let first = utc(year as i64, 1, 1, 0, 0, 0, 0).unwrap_or(f64::NAN);
            if !first.is_finite() {
                return Ok(Some(f64::NAN));
            }
            let derived = Fields::new(first as i64 + (ordinal as i64 - 1) * DAY);
            month = derived.month as f64;
            day = derived.day as f64;
        }
        let hour = if time_b {
            let h = get('h');
            (if h == 12.0 { 0.0 } else { h }) + if get('P') == 1.0 { 12.0 } else { 0.0 }
        } else {
            get('H')
        };
        let used = [year, month, day, hour, get('m'), get('s'), get('f')];
        if used
            .iter()
            .any(|n| !n.is_finite() || n.abs() > 10_000_000_000.0)
        {
            return Ok(Some(f64::NAN));
        }
        let n = utc(
            year as i64,
            month as i64,
            day as i64,
            hour as i64,
            get('m') as i64,
            get('s') as i64,
            get('f') as i64,
        )
        .unwrap_or(f64::NAN);
        let zone = if get('Z') != 0.0 && !get('Z').is_nan() {
            get('Z')
        } else {
            get('z')
        };
        let n = n - if zone.is_nan() { 0.0 } else { zone * 60_000.0 };
        Ok(Some(n))
    }
}
fn component(m: &Marker, text: &str, offset: usize) -> Result<Option<f64>, Error> {
    let c = m.component;
    if "Zz".contains(c) {
        let text = if c == 'z' { &text[3..] } else { text };
        let integer = m.integer.as_ref().unwrap();
        let (h, min) = if integer.repeat > 0 {
            let (h, min) = text.split_once(integer.separators[0].1).unwrap();
            (parse_prefix(h), parse_prefix(min))
        } else if text.len() - 1 <= 2 {
            (parse_prefix(text), 0.0)
        } else {
            (parse_prefix(&text[..3]), parse_prefix(&text[3..]))
        };
        return Ok(Some(h * 60.0 + min));
    }
    if c == 'f' {
        let length = text.len().min(3);
        return Ok(Some(
            (text[..length].parse::<i64>().unwrap() * 10_i64.pow((3 - length) as u32)) as f64,
        ));
    }
    if let Some(integer) = &m.integer {
        let n = integer.parse(text, offset)?;
        return Ok(Some(n));
    }
    if c == 'P' {
        return Ok(match text {
            "am" | "AM" => Some(0.0),
            "pm" | "PM" => Some(1.0),
            _ => None,
        });
    }
    let names = if c == 'M' || c == 'x' {
        &MONTHS[..]
    } else if c == 'F' {
        &DAYS[..]
    } else {
        return Err(error(offset, "D3133"));
    };
    Ok(names
        .iter()
        .enumerate()
        .find(|(_, s)| &s[..m.maximum.unwrap_or(s.len()).min(s.len())] == text)
        .map(|(i, _)| i as f64 + 1.0))
}
fn iso(text: &str, offset: usize) -> Result<f64, Error> {
    let mut at = 0;
    let number = |at: &mut usize, n: usize| -> Option<i64> {
        let part = text.get(*at..*at + n)?;
        if !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        *at += n;
        part.parse().ok()
    };
    let Some(year) = number(&mut at, 4) else {
        return Err(error(offset, "D3110"));
    };
    let (mut month, mut day, mut h, mut min, mut s, mut f) = (1, 1, 0, 0, 0, 0);
    if text.as_bytes().get(at) == Some(&b'-') {
        at += 1;
        month = number(&mut at, 2).ok_or_else(|| error(offset, "D3110"))?;
        if month > 19 {
            return Err(error(offset, "D3110"));
        }
    }
    if text.as_bytes().get(at) == Some(&b'-') {
        at += 1;
        day = number(&mut at, 2).ok_or_else(|| error(offset, "D3110"))?;
        if day > 39 {
            return Err(error(offset, "D3110"));
        }
    }
    let mut time = false;
    if text.as_bytes().get(at) == Some(&b'T') {
        time = true;
        at += 1;
        h = number(&mut at, 2).ok_or_else(|| error(offset, "D3110"))?;
        if text.as_bytes().get(at) != Some(&b':') {
            return Err(error(offset, "D3110"));
        }
        at += 1;
        min = number(&mut at, 2).ok_or_else(|| error(offset, "D3110"))?;
        if text.as_bytes().get(at) != Some(&b':') {
            return Err(error(offset, "D3110"));
        }
        at += 1;
        s = number(&mut at, 2).ok_or_else(|| error(offset, "D3110"))?;
        if h > 29 || min > 59 || s > 59 {
            return Err(error(offset, "D3110"));
        }
    }
    if text.as_bytes().get(at) == Some(&b'.') {
        at += 1;
        let start = at;
        while text.as_bytes().get(at).is_some_and(u8::is_ascii_digit) {
            at += 1;
        }
        if at == start {
            return Err(error(offset, "D3110"));
        }
        if !time {
            return Err(super::super::unsupported(
                offset,
                "legacy date-only fractional syntax is deferred",
            ));
        }
        let n = (at - start).min(3);
        f = text[start..start + n].parse::<i64>().unwrap() * 10_i64.pow((3 - n) as u32);
    }
    let mut zone = 0;
    let mut explicit_zone = false;
    let mut invalid_zone = false;
    if text.as_bytes().get(at) == Some(&b'Z') {
        explicit_zone = true;
        at += 1;
    } else if matches!(text.as_bytes().get(at), Some(b'+' | b'-')) {
        explicit_zone = true;
        let sign = if text.as_bytes()[at] == b'-' { -1 } else { 1 };
        at += 1;
        let hh = number(&mut at, 2).ok_or_else(|| error(offset, "D3110"))?;
        if text.as_bytes().get(at) == Some(&b':') {
            at += 1;
        }
        let mm = number(&mut at, 2).ok_or_else(|| error(offset, "D3110"))?;
        if hh > 29 || mm > 59 {
            return Err(error(offset, "D3110"));
        }
        invalid_zone = hh > 23 || !time;
        zone = sign * (hh * 60 + mm);
    }
    if at != text.len() {
        return Err(error(offset, "D3110"));
    }
    if invalid_zone
        || month == 0
        || month > 12
        || day == 0
        || day > 31
        || h > 24
        || h == 24 && (min != 0 || s != 0 || f != 0)
    {
        return Ok(f64::NAN);
    }
    if time && !explicit_zone {
        return Err(super::super::unsupported(
            offset,
            "ISO timestamps without a timezone are host-dependent",
        ));
    }
    let n = calendar::days(year, month, day) * DAY + h * 3_600_000 + min * 60_000 + s * 1000 + f
        - zone * 60_000;
    Ok(n as f64)
}
