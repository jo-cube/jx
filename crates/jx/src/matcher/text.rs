use crate::{Error, OwnedString, Value, json::string, value::type_error};
use std::{ops::Range, rc::Rc};

// Keep the source value alive for continuations. Plain ASCII borrows its encoded
// body; all other inputs decode once to ECMAScript's UTF-16 code units.
#[derive(Clone, Debug)]
pub(super) struct Text<'e, 'i> {
    pub value: Value<'e, 'i>,
    units: Option<Rc<[u16]>>,
}
impl<'e, 'i> Text<'e, 'i> {
    pub fn new(value: Value<'e, 'i>, offset: usize) -> Result<Self, Error> {
        let body = value.string_body().ok_or_else(|| type_error(offset))?;
        let units = (!(body.is_ascii() && !body.as_bytes().contains(&b'\\')))
            .then(|| string::units(body).collect::<Rc<[u16]>>());
        Ok(Self { value, units })
    }
    pub fn check_case(&self, icase: bool, offset: usize) -> Result<(), Error> {
        if icase
            && self
                .units
                .as_ref()
                .is_some_and(|u| u.iter().any(|&u| matches!(u, 0x131 | 0x17f)))
        {
            return Err(super::legacy_case_error(offset));
        }
        Ok(())
    }
    pub fn len(&self) -> usize {
        self.units
            .as_ref()
            .map_or_else(|| self.value.string_body().unwrap().len(), |u| u.len())
    }
    pub fn search(&self, pattern: &regress::Regex, start: usize) -> Option<regress::Match> {
        if start > self.len() {
            return None;
        }
        match &self.units {
            Some(units) => pattern.find_from_ucs2(units, start).next(),
            None => pattern
                .find_from_ascii(self.value.string_body().unwrap(), start)
                .next(),
        }
    }
    pub fn slice(&self, range: Range<usize>) -> Value<'e, 'i> {
        if range == (0..self.len()) {
            return self.value.clone();
        }
        match &self.units {
            Some(units) => Value::String(OwnedString::units(units[range].iter().copied())),
            None => Value::String(OwnedString::body(&self.value.string_body().unwrap()[range])),
        }
    }
    pub fn append(&self, range: Range<usize>, output: &mut Vec<u16>) {
        match &self.units {
            Some(units) => output.extend_from_slice(&units[range]),
            None => output.extend(
                self.value.string_body().unwrap().as_bytes()[range]
                    .iter()
                    .map(|&b| u16::from(b)),
            ),
        }
    }
    pub fn find(&self, pattern: &[u16], start: usize) -> Option<usize> {
        if pattern.is_empty() {
            return Some(start);
        }
        match &self.units {
            Some(units) => units[start..]
                .windows(pattern.len())
                .position(|s| s == pattern)
                .map(|i| i + start),
            None => self.value.string_body().unwrap().as_bytes()[start..]
                .windows(pattern.len())
                .position(|s| s.iter().map(|&b| u16::from(b)).eq(pattern.iter().copied()))
                .map(|i| i + start),
        }
    }
}
