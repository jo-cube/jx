use super::demand::{Captured, Captures, Demand};
use super::{RawJson, string};
use crate::{Error, ErrorKind};

/// Maximum simultaneously nested JSON objects/arrays, bounding native stack use.
pub const MAX_DEPTH: usize = 128;

#[derive(Clone, Copy, Debug)]
pub(crate) enum Selection<'a, 'path> {
    Missing,
    Value(RawJson<'a>),
    Array(RawJson<'a>, &'path [Box<str>]),
}

pub(crate) fn select<'a, 'path>(
    input: &'a [u8],
    fields: &'path [Box<str>],
) -> Result<Selection<'a, 'path>, Error> {
    let text = std::str::from_utf8(input).map_err(|error| {
        Error::new(ErrorKind::InvalidJson, error.valid_up_to(), "invalid UTF-8")
    })?;
    let mut scanner = Scanner { text, at: 0 };
    scanner.space();
    let selected = scanner.value(0, Some(fields))?;
    scanner.space();
    if scanner.at != text.len() {
        return Err(scanner.error("trailing content after JSON value"));
    }
    Ok(selected)
}

/// Validate every byte, retaining only the requested raw spans.
pub(crate) fn capture<'a>(
    input: &'a [u8],
    demand: &Demand,
    output: &mut Captures<'a>,
) -> Result<RawJson<'a>, Error> {
    let text = std::str::from_utf8(input).map_err(|error| {
        Error::new(ErrorKind::InvalidJson, error.valid_up_to(), "invalid UTF-8")
    })?;
    let mut scanner = Scanner { text, at: 0 };
    scanner.space();
    let raw = scanner.capture_value(0, demand, output)?;
    scanner.space();
    if scanner.at != text.len() {
        return Err(scanner.error("trailing content after JSON value"));
    }
    Ok(raw)
}

struct Scanner<'a> {
    text: &'a str,
    at: usize,
}

impl<'a> Scanner<'a> {
    fn value<'path>(
        &mut self,
        depth: usize,
        path: Option<&'path [Box<str>]>,
    ) -> Result<Selection<'a, 'path>, Error> {
        let start = self.at;
        let remaining = path.filter(|fields| !fields.is_empty());
        let selection = match self.byte() {
            Some(b'{') => {
                self.open(depth)?;
                self.object(depth + 1, remaining)?
            }
            Some(b'[') => {
                self.open(depth)?;
                self.array(depth + 1)?;
                if let Some(fields) = remaining {
                    Selection::Array(RawJson(&self.text[start..self.at]), fields)
                } else {
                    Selection::Missing
                }
            }
            Some(b'"') => {
                self.string()?;
                Selection::Missing
            }
            Some(b't') => {
                self.literal(b"true")?;
                Selection::Missing
            }
            Some(b'f') => {
                self.literal(b"false")?;
                Selection::Missing
            }
            Some(b'n') => {
                self.literal(b"null")?;
                Selection::Missing
            }
            Some(b'-' | b'0'..=b'9') => {
                self.number()?;
                Selection::Missing
            }
            _ => return Err(self.error("expected a JSON value")),
        };
        if path.is_some_and(<[Box<str>]>::is_empty) {
            Ok(Selection::Value(RawJson(&self.text[start..self.at])))
        } else {
            Ok(selection)
        }
    }

    fn object<'path>(
        &mut self,
        depth: usize,
        path: Option<&'path [Box<str>]>,
    ) -> Result<Selection<'a, 'path>, Error> {
        let mut selected = Selection::Missing;
        self.object_members(|scanner, key| {
            let tail =
                path.and_then(|fields| string::matches(key, &fields[0]).then_some(&fields[1..]));
            let found = scanner.value(depth, tail)?;
            // Last decoded key wins, including missing and deferred array paths.
            if tail.is_some() {
                selected = found;
            }
            Ok(())
        })?;
        Ok(selected)
    }

    fn object_members(
        &mut self,
        mut member: impl FnMut(&mut Self, &'a str) -> Result<(), Error>,
    ) -> Result<(), Error> {
        self.space();
        if self.take(b'}') {
            return Ok(());
        }
        loop {
            if self.byte() != Some(b'"') {
                return Err(self.error("expected an object key"));
            }
            let key_start = self.at + 1;
            self.string()?;
            let key = &self.text[key_start..self.at - 1];
            self.space();
            self.require(b':', "expected ':' after object key")?;
            self.space();
            member(self, key)?;
            self.space();
            if self.take(b'}') {
                return Ok(());
            }
            self.require(b',', "expected ',' or '}'")?;
            self.space();
        }
    }

    fn capture_value(
        &mut self,
        depth: usize,
        demand: &Demand,
        output: &mut Captures<'a>,
    ) -> Result<RawJson<'a>, Error> {
        let start = self.at;
        // A duplicate parent replaces every descendant, including absent fields.
        output.fill(demand.subtree & !demand.slots, Captured::Missing);
        if self.byte() == Some(b'{') && !demand.children.is_empty() {
            self.open(depth)?;
            self.object_members(|scanner, key| {
                if let Some((_, child)) = demand
                    .children
                    .iter()
                    .find(|(name, _)| string::matches(key, name))
                {
                    scanner.capture_value(depth + 1, child, output)?;
                } else {
                    scanner.value(depth + 1, None)?;
                }
                Ok(())
            })?;
        } else {
            let array = self.byte() == Some(b'[');
            self.value(depth, None)?;
            if array {
                output.fill(demand.subtree & !demand.slots, Captured::Deferred);
            }
        }
        let raw = RawJson(&self.text[start..self.at]);
        output.fill(demand.slots, Captured::Raw(raw));
        Ok(raw)
    }

    fn array(&mut self, depth: usize) -> Result<(), Error> {
        self.space();
        if self.take(b']') {
            return Ok(());
        }
        loop {
            self.value(depth, None)?;
            self.space();
            if self.take(b']') {
                return Ok(());
            }
            self.require(b',', "expected ',' or ']'")?;
            self.space();
        }
    }

    fn open(&mut self, depth: usize) -> Result<(), Error> {
        if depth >= MAX_DEPTH {
            return Err(Error::new(
                ErrorKind::DepthLimit,
                self.at,
                "JSON container depth exceeds 128",
            ));
        }
        self.at += 1;
        Ok(())
    }

    fn string(&mut self) -> Result<(), Error> {
        self.at += 1;
        loop {
            match self.byte() {
                Some(b'"') => {
                    self.at += 1;
                    return Ok(());
                }
                Some(b'\\') => {
                    self.at += 1;
                    match self.byte() {
                        Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => {
                            self.at += 1
                        }
                        Some(b'u') => {
                            self.at += 1;
                            for _ in 0..4 {
                                if !self.byte().is_some_and(|b| b.is_ascii_hexdigit()) {
                                    return Err(
                                        self.error("expected four hexadecimal escape digits")
                                    );
                                }
                                self.at += 1;
                            }
                        }
                        _ => return Err(self.error("invalid JSON string escape")),
                    }
                }
                Some(0..=31) => return Err(self.error("unescaped control byte in string")),
                Some(_) => self.at += 1,
                None => return Err(self.error("unterminated JSON string")),
            }
        }
    }

    fn number(&mut self) -> Result<(), Error> {
        self.take(b'-');
        if !self.take(b'0') {
            self.digits()?;
        }
        if self.take(b'.') {
            self.digits()?;
        }
        if self.take(b'e') || self.take(b'E') {
            if !self.take(b'+') {
                self.take(b'-');
            }
            self.digits()?;
        }
        Ok(())
    }

    fn digits(&mut self) -> Result<(), Error> {
        let start = self.at;
        while self.byte().is_some_and(|b| b.is_ascii_digit()) {
            self.at += 1;
        }
        if self.at == start {
            return Err(self.error("expected a decimal digit"));
        }
        Ok(())
    }

    fn literal(&mut self, literal: &[u8]) -> Result<(), Error> {
        if !self.text.as_bytes()[self.at..].starts_with(literal) {
            return Err(self.error("invalid JSON literal"));
        }
        self.at += literal.len();
        Ok(())
    }

    fn byte(&self) -> Option<u8> {
        self.text.as_bytes().get(self.at).copied()
    }

    fn take(&mut self, byte: u8) -> bool {
        if self.byte() == Some(byte) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn require(&mut self, byte: u8, message: &'static str) -> Result<(), Error> {
        if self.take(byte) {
            Ok(())
        } else {
            Err(self.error(message))
        }
    }

    fn space(&mut self) {
        while matches!(self.byte(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.at += 1;
        }
    }

    fn error(&self, message: &'static str) -> Error {
        Error::new(ErrorKind::InvalidJson, self.at, message)
    }
}

// These cursors only receive validated RawJson. Reuse the scanner for value
// boundaries; do not maintain a second JSON grammar for traversal.
impl<'a> Scanner<'a> {
    fn raw_value(&mut self) -> RawJson<'a> {
        let start = self.at;
        self.value(0, None).expect("validated subtree");
        RawJson(&self.text[start..self.at])
    }
}

pub(crate) struct Elements<'a> {
    scanner: Scanner<'a>,
}

impl<'a> Iterator for Elements<'a> {
    type Item = RawJson<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.scanner.space();
        if self.scanner.byte() == Some(b']') {
            return None;
        }
        let value = self.scanner.raw_value();
        self.scanner.space();
        self.scanner.take(b',');
        Some(value)
    }
}

impl<'a> RawJson<'a> {
    pub(crate) fn is_array(self) -> bool {
        self.as_bytes()[0] == b'['
    }

    pub(crate) fn elements(self) -> Elements<'a> {
        debug_assert!(self.is_array());
        Elements {
            scanner: Scanner {
                text: self.0,
                at: 1,
            },
        }
    }

    pub(crate) fn field(self, name: &str) -> Option<Self> {
        if self.as_bytes()[0] != b'{' {
            return None;
        }
        let mut scanner = Scanner {
            text: self.0,
            at: 1,
        };
        let mut selected = None;
        scanner.space();
        while scanner.byte() != Some(b'}') {
            let start = scanner.at + 1;
            scanner.string().expect("validated key");
            let matches = string::matches(&self.0[start..scanner.at - 1], name);
            scanner.space();
            scanner.at += 1; // validated ':'
            scanner.space();
            let value = scanner.raw_value();
            if matches {
                selected = Some(value);
            }
            scanner.space();
            scanner.take(b',');
            scanner.space();
        }
        selected
    }
}

/// Object members retain encoded keys so comparisons need no decoded allocation.
pub(crate) struct Members<'a> {
    scanner: Scanner<'a>,
}

impl<'a> Iterator for Members<'a> {
    type Item = (&'a str, RawJson<'a>);
    fn next(&mut self) -> Option<Self::Item> {
        self.scanner.space();
        if self.scanner.byte() == Some(b'}') {
            return None;
        }
        let start = self.scanner.at + 1;
        self.scanner.string().expect("validated key");
        let key = &self.scanner.text[start..self.scanner.at - 1];
        self.scanner.space();
        self.scanner.at += 1;
        self.scanner.space();
        let value = self.scanner.raw_value();
        self.scanner.space();
        self.scanner.take(b',');
        Some((key, value))
    }
}

impl<'a> RawJson<'a> {
    pub(crate) fn members(self) -> Members<'a> {
        debug_assert_eq!(self.as_bytes()[0], b'{');
        Members {
            scanner: Scanner {
                text: self.0,
                at: 1,
            },
        }
    }
}

impl<'a> RawJson<'a> {
    pub(crate) fn capture(self, demand: &Demand, output: &mut Captures<'a>) {
        Scanner {
            text: self.0,
            at: 0,
        }
        .capture_value(0, demand, output)
        .expect("validated subtree");
    }
}
impl<'a> Elements<'a> {
    pub(crate) fn next_captured(
        &mut self,
        demand: &Demand,
        output: &mut Captures<'a>,
    ) -> Option<RawJson<'a>> {
        self.scanner.space();
        if self.scanner.byte() == Some(b']') {
            return None;
        }
        let raw = self
            .scanner
            .capture_value(0, demand, output)
            .expect("validated subtree");
        self.scanner.space();
        self.scanner.take(b',');
        Some(raw)
    }
}
