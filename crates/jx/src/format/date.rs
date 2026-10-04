mod calendar;
mod parse;
use super::{
    error,
    integer::{Case, Integer},
};
use crate::Error;
use calendar::{DAY, Fields};
pub(super) use parse::Parser;
use std::fmt::Write;
pub(super) const ISO: &str = "[Y0001]-[M01]-[D01]T[H01]:[m01]:[s01].[f001][Z01:01t]";
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const DAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];
#[derive(Clone, Debug)]
struct Marker {
    component: char,
    integer: Option<Integer>,
    names: Option<Case>,
    maximum: Option<usize>,
    year_width: Option<usize>,
    parse_width: Option<usize>,
    timezone_z: bool,
}
#[derive(Clone, Debug)]
enum Part {
    Literal(Box<str>),
    Marker(Marker),
}
#[derive(Clone, Debug)]
pub(super) struct Date {
    parts: Box<[Part]>,
    capacity: usize,
}
fn default(c: char) -> Option<&'static str> {
    Some(match c {
        'Y' | 'M' | 'D' | 'd' | 'W' | 'w' | 'X' | 'x' | 'H' | 'h' | 'f' => "1",
        'F' | 'P' | 'C' | 'E' => "n",
        'm' | 's' => "01",
        'Z' | 'z' => "01:01",
        _ => return None,
    })
}
fn width(text: &str) -> Option<usize> {
    let length = text.bytes().take_while(u8::is_ascii_digit).count();
    text[..length].parse().ok()
}
impl Marker {
    fn new(text: &str, offset: usize) -> Result<Self, Error> {
        let text: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        let mut chars = text.chars();
        let component = chars.next().ok_or_else(|| error(offset, "D3132"))?;
        let rest = chars.as_str();
        let (mut presentation, min, max) = if let Some((p, w)) = rest.rsplit_once(',') {
            let (a, b) = w.split_once('-').map_or((w, None), |(a, b)| (a, Some(b)));
            (p, width(a), b.and_then(width))
        } else {
            (rest, None, None)
        };
        let mut secondary = None;
        if presentation.chars().count() > 1
            && presentation
                .chars()
                .next_back()
                .is_some_and(|c| "atco".contains(c))
        {
            secondary = presentation.chars().next_back();
            presentation = &presentation[..presentation.len() - 1];
        }
        if presentation.is_empty() {
            presentation = default(component).ok_or_else(|| error(offset, "D3132"))?;
        }
        let names = if presentation.starts_with('n') {
            Some(Case::Lower)
        } else if presentation.starts_with("Nn") {
            Some(Case::Title)
        } else if presentation.starts_with('N') {
            Some(Case::Upper)
        } else {
            None
        };
        let integer = if names.is_none() && "YMDdFWwXxHhmsf".contains(component)
            || "Zz".contains(component)
        {
            let mut integer = Integer::new(
                &if secondary == Some('o') {
                    format!("{presentation};o")
                } else {
                    presentation.into()
                },
                offset,
            )?;
            if !"Zz".contains(component) {
                integer.minimum = integer.minimum.max(min.unwrap_or(0));
            }
            Some(integer)
        } else {
            None
        };
        let mut result = Self {
            component,
            integer,
            names,
            maximum: max,
            year_width: None,
            parse_width: None,
            timezone_z: secondary == Some('t'),
        };
        if component == 'Y'
            && let Some(integer) = &mut result.integer
        {
            if let Some(max) = max {
                result.year_width = Some(max);
                integer.minimum = max;
            } else if integer.minimum + integer.optional >= 2 {
                result.year_width = Some(integer.minimum + integer.optional);
            }
        }
        Ok(result)
    }
}
impl Date {
    pub fn new(picture: &str, offset: usize) -> Result<Self, Error> {
        let mut parts = Vec::new();
        let mut at = 0;
        let mut start = 0;
        let literal = |parts: &mut Vec<Part>, s: &str| {
            if !s.is_empty() {
                parts.push(Part::Literal(s.replace("]]", "]").into()));
            }
        };
        while at < picture.len() {
            if picture.as_bytes()[at] == b'[' {
                literal(&mut parts, &picture[start..at]);
                if picture.as_bytes().get(at + 1) == Some(&b'[') {
                    parts.push(Part::Literal("[".into()));
                    at += 2;
                    start = at;
                    continue;
                }
                let end = picture[at + 1..]
                    .find(']')
                    .map(|i| i + at + 1)
                    .ok_or_else(|| error(offset, "D3135"))?;
                let marker = Marker::new(&picture[at + 1..end], offset)?;
                if marker.integer.is_some()
                    && !"Zz".contains(marker.component)
                    && let Some(Part::Marker(previous)) = parts.last_mut()
                    && let Some(integer) = &previous.integer
                {
                    previous.parse_width = Some(integer.minimum);
                }
                parts.push(Part::Marker(marker));
                at = end + 1;
                start = at;
            } else {
                at += picture[at..].chars().next().unwrap().len_utf8();
            }
        }
        literal(&mut parts, &picture[start..]);
        Ok(Self {
            parts: parts.into_boxed_slice(),
            capacity: picture.len(),
        })
    }
    pub fn format(&self, n: f64, timezone: Option<&str>, offset: usize) -> Result<String, Error> {
        if !n.is_finite() || n.abs() > 8.64e15 {
            return Err(crate::value::range_error(offset));
        }
        let tz = timezone.map(super::integer::parse_prefix).unwrap_or(0.0);
        if !tz.is_finite() || tz.abs() > 2400.0 {
            return Err(crate::value::range_error(offset));
        }
        // Match the pinned implementation's signed HHMM decomposition.
        let hours = (tz / 100.0).floor() as i64;
        let minutes = (tz % 100.0) as i64;
        let shifted = n + (hours * 60 + minutes) as f64 * 60_000.0;
        if shifted.abs() > 8.64e15 {
            return Err(crate::value::range_error(offset));
        }
        let fields = Fields::new(shifted as i64);
        let mut out = String::with_capacity(self.capacity);
        for part in &self.parts {
            match part {
                Part::Literal(s) => out.push_str(s),
                Part::Marker(m) => {
                    let c = m.component;
                    if (0..=99).contains(&fields.year) && "dWwXx".contains(c) {
                        return Err(super::unsupported(
                            offset,
                            "legacy week/day derivation in years 0..99 is deferred",
                        ));
                    }
                    if "YMDdFWwXxHhmsf".contains(c) {
                        let mut value = fields.get(c);
                        if c == 'Y'
                            && let Some(width) = m.year_width
                            && width < 19
                        {
                            value %= 10_i64.pow(width as u32);
                        }
                        if let Some(case) = m.names {
                            let name = match c {
                                'M' | 'x' => MONTHS[(value - 1) as usize],
                                'F' => DAYS[(value - 1) as usize],
                                _ => return Err(error(offset, "D3133")),
                            };
                            let name = &name[..m.maximum.unwrap_or(name.len()).min(name.len())];
                            let start = out.len();
                            out.push_str(name);
                            match case {
                                Case::Lower => out[start..].make_ascii_lowercase(),
                                Case::Upper => out[start..].make_ascii_uppercase(),
                                Case::Title => {}
                            }
                        } else {
                            write_integer(
                                m.integer.as_ref().ok_or_else(|| error(offset, "D3133"))?,
                                value,
                                offset,
                                &mut out,
                            )?;
                        }
                    } else if "Zz".contains(c) {
                        let integer = m.integer.as_ref().ok_or_else(|| error(offset, "D3134"))?;
                        let zone = hours * 100 + minutes;
                        if zone == 0 && m.timezone_z {
                            out.push('Z');
                            continue;
                        }
                        if c == 'z' {
                            out.push_str("GMT");
                        }
                        if zone >= 0 {
                            out.push('+');
                        }
                        if integer.repeat > 0 || matches!(integer.minimum, 3 | 4) {
                            write_integer(integer, zone, offset, &mut out)?;
                        } else if matches!(integer.minimum, 1 | 2) {
                            write_integer(integer, hours, offset, &mut out)?;
                            if minutes != 0 {
                                out.push(':');
                                write!(out, "{minutes:02}").unwrap();
                            }
                        } else {
                            return Err(error(offset, "D3134"));
                        }
                    } else if c == 'P' {
                        out.push_str(if matches!(m.names, Some(Case::Upper)) {
                            if fields.get('P') == 0 { "AM" } else { "PM" }
                        } else if fields.get('P') == 0 {
                            "am"
                        } else {
                            "pm"
                        });
                    } else if c == 'C' || c == 'E' {
                        out.push_str("ISO");
                    } else {
                        return Err(error(offset, "D3132"));
                    }
                }
            }
        }
        Ok(out)
    }
}
fn write_integer(
    integer: &Integer,
    value: i64,
    offset: usize,
    out: &mut String,
) -> Result<(), Error> {
    if value < 0 {
        out.push('-');
    }
    integer.write(value.unsigned_abs() as f64, offset, out)
}
fn utc(
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
    fraction: i64,
) -> Option<f64> {
    // Date.UTC normalizes fields and aliases years 0..99 to 1900..1999.
    let year = if (0..100).contains(&year) {
        year + 1900
    } else {
        year
    };
    if year.abs() > 300_000
        || month.abs() > 4_000_000
        || [day, hour, minute, second, fraction]
            .iter()
            .any(|n| n.unsigned_abs() > 10_000_000_000)
    {
        return None;
    }
    let n = calendar::days(year, month, day) * DAY
        + hour * 3_600_000
        + minute * 60_000
        + second * 1000
        + fraction;
    (n.unsigned_abs() <= 8_640_000_000_000_000).then_some(n as f64)
}
