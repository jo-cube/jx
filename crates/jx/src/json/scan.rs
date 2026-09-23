use super::{RawJson, string};
use crate::{Error, ErrorKind};

/// Maximum simultaneously nested JSON objects/arrays, bounding native stack use.
pub const MAX_DEPTH: usize = 128;

enum Selection<'a> {
    Missing,
    Value(RawJson<'a>),
    Array(usize),
}

pub(crate) fn select<'a>(
    input: &'a [u8],
    fields: &[Box<str>],
) -> Result<Option<RawJson<'a>>, Error> {
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
    match selected {
        Selection::Missing => Ok(None),
        Selection::Value(value) => Ok(Some(value)),
        Selection::Array(offset) => Err(Error::new(
            ErrorKind::ArrayTraversal,
            offset,
            "array path traversal is not implemented; see CONFORMANCE.md",
        )),
    }
}

struct Scanner<'a> {
    text: &'a str,
    at: usize,
}

impl<'a> Scanner<'a> {
    fn value(&mut self, depth: usize, path: Option<&[Box<str>]>) -> Result<Selection<'a>, Error> {
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
                if remaining.is_some() {
                    Selection::Array(start)
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

    fn object(&mut self, depth: usize, path: Option<&[Box<str>]>) -> Result<Selection<'a>, Error> {
        self.space();
        let mut selected = Selection::Missing;
        if self.take(b'}') {
            return Ok(selected);
        }
        loop {
            if self.byte() != Some(b'"') {
                return Err(self.error("expected an object key"));
            }
            let key_start = self.at + 1;
            self.string()?;
            let key = &self.text[key_start..self.at - 1];
            let tail =
                path.and_then(|fields| string::matches(key, &fields[0]).then_some(&fields[1..]));
            self.space();
            self.require(b':', "expected ':' after object key")?;
            self.space();
            let found = self.value(depth, tail)?;
            // Resolve duplicates before reporting unsupported traversal. A later
            // duplicate can replace an array with an object or a missing path.
            if tail.is_some() {
                selected = found;
            }
            self.space();
            if self.take(b'}') {
                return Ok(selected);
            }
            self.require(b',', "expected ',' or '}'")?;
            self.space();
        }
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
