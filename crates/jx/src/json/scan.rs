use super::demand::{Captured, Captures, Demand};
use super::{RawJson, string};
use crate::{Error, ErrorKind};

/// Maximum simultaneously nested JSON objects/arrays, bounding native stack use.
pub const MAX_DEPTH: usize = 128;

// Scanner diagnostics are static; user-message ownership belongs at the API boundary.
#[derive(Clone, Copy, Debug)]
struct ScanError {
    kind: ErrorKind,
    offset: usize,
    message: &'static str,
}
impl From<ScanError> for Error {
    fn from(error: ScanError) -> Self {
        Self::new(error.kind, error.offset, error.message).input()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    let mut scanner = Scanner::<false> { text, at: 0 };
    scanner.space();
    let selected = scanner.value(0, Some(fields))?;
    scanner.space();
    if scanner.at != text.len() {
        return Err(scanner.error("trailing content after JSON value").into());
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
    let mut scanner = Scanner::<false> { text, at: 0 };
    scanner.space();
    let raw = scanner.capture_value(0, demand, output)?;
    scanner.space();
    if scanner.at != text.len() {
        return Err(scanner.error("trailing content after JSON value").into());
    }
    Ok(raw)
}

struct Scanner<'a, const VALIDATED: bool> {
    text: &'a str,
    at: usize,
}

impl<'a, const VALIDATED: bool> Scanner<'a, VALIDATED> {
    fn value<'path>(
        &mut self,
        depth: usize,
        path: Option<&'path [Box<str>]>,
    ) -> Result<Selection<'a, 'path>, ScanError> {
        let start = self.at;
        let remaining = path.filter(|fields| !fields.is_empty());
        if VALIDATED && remaining.is_none() {
            let raw = self.raw_value();
            return Ok(if path.is_some() {
                Selection::Value(raw)
            } else {
                Selection::Missing
            });
        }
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
    ) -> Result<Selection<'a, 'path>, ScanError> {
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
        mut member: impl FnMut(&mut Self, &'a str) -> Result<(), ScanError>,
    ) -> Result<(), ScanError> {
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
    ) -> Result<RawJson<'a>, ScanError> {
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

    fn array(&mut self, depth: usize) -> Result<(), ScanError> {
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

    fn open(&mut self, depth: usize) -> Result<(), ScanError> {
        if depth >= MAX_DEPTH {
            return Err(ScanError {
                kind: ErrorKind::DepthLimit,
                offset: self.at,
                message: "JSON container depth exceeds 128",
            });
        }
        self.at += 1;
        Ok(())
    }

    #[inline(always)]
    fn string(&mut self) -> Result<(), ScanError> {
        if VALIDATED {
            self.skip_string();
            Ok(())
        } else {
            self.validate_string()
        }
    }

    fn validate_string(&mut self) -> Result<(), ScanError> {
        self.at += 1;
        if self.text.len() >= 64
            && let Some(end) = short_string_end::<false>(&self.text.as_bytes()[self.at..])
        {
            self.at += end + 1;
            return Ok(());
        }
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
                Some(_) => {
                    self.at += if self.text.len() >= 64 {
                        string_run::<false>(&self.text.as_bytes()[self.at..])
                    } else {
                        1
                    };
                }
                None => return Err(self.error("unterminated JSON string")),
            }
        }
    }

    fn number(&mut self) -> Result<(), ScanError> {
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

    fn digits(&mut self) -> Result<(), ScanError> {
        let start = self.at;
        while self.byte().is_some_and(|b| b.is_ascii_digit()) {
            self.at += 1;
        }
        if self.at == start {
            return Err(self.error("expected a decimal digit"));
        }
        Ok(())
    }

    fn literal(&mut self, literal: &[u8]) -> Result<(), ScanError> {
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

    fn require(&mut self, byte: u8, message: &'static str) -> Result<(), ScanError> {
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

    fn error(&self, message: &'static str) -> ScanError {
        ScanError {
            kind: ErrorKind::InvalidJson,
            offset: self.at,
            message,
        }
    }
}

// Unescaped strings ending in the first word need neither scanning loop.
#[inline]
fn short_string_end<const VALIDATED: bool>(bytes: &[u8]) -> Option<usize> {
    let head = bytes.first_chunk::<8>()?;
    let at = head
        .iter()
        .position(|&byte| byte == b'"' || byte == b'\\' || (!VALIDATED && byte < 32))?;
    (head[at] == b'"').then_some(at)
}

// Skip only words containing ordinary string bytes. Scalar search in the first
// special word preserves exact escape/error positions. Inlining penalizes escapes.
#[inline(never)]
fn string_run<const VALIDATED: bool>(bytes: &[u8]) -> usize {
    const ONES: u64 = 0x0101_0101_0101_0101;
    const HIGHS: u64 = 0x8080_8080_8080_8080;
    let has_zero = |word: u64| word.wrapping_sub(ONES) & !word & HIGHS != 0;
    let mut at = 0;
    while let Some(chunk) = bytes[at..].first_chunk::<8>() {
        let word = u64::from_ne_bytes(*chunk);
        if has_zero(word ^ (ONES * u64::from(b'"')))
            || has_zero(word ^ (ONES * u64::from(b'\\')))
            || (!VALIDATED && has_zero(word & 0xe0e0_e0e0_e0e0_e0e0))
        {
            break;
        }
        at += 8;
    }
    at + bytes[at..]
        .iter()
        .position(|&byte| byte == b'"' || byte == b'\\' || (!VALIDATED && byte < 32))
        .unwrap_or(bytes.len() - at)
}

// The validation and traversal modes share path/demand handling. Only RawJson
// cursors use VALIDATED: they locate boundaries without rechecking JSON grammar.
impl<'a, const VALIDATED: bool> Scanner<'a, VALIDATED> {
    #[inline(always)]
    fn skip_string(&mut self) {
        debug_assert!(VALIDATED);
        self.at += 1;
        let bytes = self.text.as_bytes();
        if self.text.len() < 64 {
            while bytes[self.at] != b'"' {
                if bytes[self.at] == b'\\' {
                    self.at += 1;
                }
                self.at += 1;
            }
            self.at += 1;
            return;
        }
        if let Some(end) = short_string_end::<true>(&bytes[self.at..]) {
            self.at += end + 1;
            return;
        }
        loop {
            match bytes[self.at] {
                b'"' => {
                    self.at += 1;
                    return;
                }
                b'\\' => self.at += 2,
                _ => self.at += string_run::<true>(&bytes[self.at..]),
            }
        }
    }

    fn raw_value(&mut self) -> RawJson<'a> {
        debug_assert!(VALIDATED);
        let start = self.at;
        match self.text.as_bytes()[self.at] {
            b'"' => self.skip_string(),
            b'{' | b'[' => {
                let mut depth = 1;
                self.at += 1;
                while depth != 0 {
                    match self.text.as_bytes()[self.at] {
                        b'"' => {
                            self.skip_string();
                            continue;
                        }
                        b'{' | b'[' => depth += 1,
                        b'}' | b']' => depth -= 1,
                        _ => {}
                    }
                    self.at += 1;
                }
            }
            b't' | b'n' => self.at += 4,
            b'f' => self.at += 5,
            _ => {
                while matches!(
                    self.byte(),
                    Some(b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                ) {
                    self.at += 1;
                }
            }
        }
        RawJson(&self.text[start..self.at])
    }
}

pub(crate) struct Elements<'a> {
    scanner: Scanner<'a, true>,
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
            scanner: Scanner::<true> {
                text: self.0,
                at: 1,
            },
        }
    }

    // Array delimiters need no subtree boundary scan. Non-array leaves use the
    // validated cursor so nested arrays do not repeatedly parse their contents.
    pub(crate) fn try_for_each_flattened<E>(
        self,
        mut output: impl FnMut(Self) -> Result<(), E>,
    ) -> Result<(), E> {
        let mut scanner = Scanner::<true> {
            text: self.0,
            at: 0,
        };
        loop {
            scanner.space();
            match scanner.byte() {
                Some(b'[' | b']' | b',') => scanner.at += 1,
                None => return Ok(()),
                _ => output(scanner.raw_value())?,
            }
        }
    }

    pub(crate) fn select<'path>(self, fields: &'path [Box<str>]) -> Selection<'a, 'path> {
        Scanner::<true> {
            text: self.0,
            at: 0,
        }
        .value(0, Some(fields))
        .expect("validated subtree")
    }

    pub(crate) fn field(self, name: &str) -> Option<Self> {
        if self.as_bytes()[0] != b'{' {
            return None;
        }
        let mut scanner = Scanner::<true> {
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
    scanner: Scanner<'a, true>,
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
            scanner: Scanner::<true> {
                text: self.0,
                at: 1,
            },
        }
    }
}

impl<'a> RawJson<'a> {
    pub(crate) fn capture(self, demand: &Demand, output: &mut Captures<'a>) {
        Scanner::<true> {
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

#[cfg(test)]
mod tests;
