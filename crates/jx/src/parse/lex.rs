use crate::{
    Error, ErrorKind,
    expression::{Aggregate, Op},
};
use std::fmt::Write;

#[derive(Debug)]
pub(super) enum Token<'a> {
    Name(&'a str),
    Quoted(&'a str),
    String(Box<str>),
    Number(f64),
    Operator(Op),
    Root,
    Aggregate(Aggregate),
    Comma,
    Dot,
    Open,
    FilterOpen,
    FilterClose,
    Close,
    End,
}

pub(super) struct Lexer<'a> {
    pub source: &'a str,
    pub at: usize,
}

impl<'a> Lexer<'a> {
    pub fn next(&mut self) -> Result<(Token<'a>, usize), Error> {
        while self
            .byte()
            .is_some_and(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0x0b))
        {
            self.at += 1;
        }
        let start = self.at;
        let Some(byte) = self.byte() else {
            return Ok((Token::End, start));
        };
        self.at += 1;
        let token = match byte {
            b'$' => {
                let name = self.at;
                while self
                    .byte()
                    .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
                {
                    self.at += 1;
                }
                match &self.source[name..self.at] {
                    "" => Token::Root,
                    "count" => Token::Aggregate(Aggregate::Count),
                    "sum" => Token::Aggregate(Aggregate::Sum),
                    "min" => Token::Aggregate(Aggregate::Min),
                    "max" => Token::Aggregate(Aggregate::Max),
                    _ => return Err(error(start)),
                }
            }
            b',' => Token::Comma,
            b'.' => Token::Dot,
            b'(' => Token::Open,
            b'[' => Token::FilterOpen,
            b']' => Token::FilterClose,
            b')' => Token::Close,
            b'+' => Token::Operator(Op::Add),
            b'-' => Token::Operator(Op::Subtract),
            b'*' => Token::Operator(Op::Multiply),
            b'/' => Token::Operator(Op::Divide),
            b'%' => Token::Operator(Op::Remainder),
            b'=' => Token::Operator(Op::Equal),
            b'!' if self.take(b'=') => Token::Operator(Op::NotEqual),
            b'<' => Token::Operator(if self.take(b'=') {
                Op::LessEqual
            } else {
                Op::Less
            }),
            b'>' => Token::Operator(if self.take(b'=') {
                Op::GreaterEqual
            } else {
                Op::Greater
            }),
            b'`' => {
                while self.byte().is_some_and(|b| b != b'`') {
                    self.at += 1;
                }
                if !self.take(b'`') {
                    return Err(error(start));
                }
                Token::Quoted(&self.source[start + 1..self.at - 1])
            }
            b'\'' | b'"' => Token::String(self.string(byte, start)?),
            b'0'..=b'9' => {
                if byte != b'0' {
                    self.digits();
                }
                if self.byte() == Some(b'.')
                    && self
                        .source
                        .as_bytes()
                        .get(self.at + 1)
                        .is_some_and(u8::is_ascii_digit)
                {
                    self.at += 1;
                    self.digits();
                }
                if self.byte().is_some_and(|b| b == b'e' || b == b'E') {
                    self.at += 1;
                    if self.byte().is_some_and(|b| b == b'+' || b == b'-') {
                        self.at += 1;
                    }
                    let digits = self.at;
                    self.digits();
                    if digits == self.at {
                        return Err(error(self.at));
                    }
                }
                let number: f64 = self.source[start..self.at]
                    .parse()
                    .map_err(|_| error(start))?;
                if !number.is_finite() {
                    return Err(Error::new(
                        ErrorKind::NumericRange,
                        start,
                        "number literal exceeds binary64 range",
                    ));
                }
                Token::Number(number)
            }
            b if b.is_ascii_alphabetic() || b == b'_' => {
                while self
                    .byte()
                    .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
                {
                    self.at += 1;
                }
                Token::Name(&self.source[start..self.at])
            }
            _ => return Err(error(start)),
        };
        Ok((token, start))
    }

    fn string(&mut self, quote: u8, start: usize) -> Result<Box<str>, Error> {
        let mut json = String::from("\"");
        while let Some(byte) = self.byte() {
            if byte == quote {
                self.at += 1;
                json.push('"');
                return Ok(json.into_boxed_str());
            }
            if byte == b'\\' {
                let escape = self.at;
                self.at += 1;
                match self.byte() {
                    Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => self.at += 1,
                    Some(b'u') => {
                        self.at += 1;
                        for _ in 0..4 {
                            if !self.byte().is_some_and(|b| b.is_ascii_hexdigit()) {
                                return Err(error(self.at));
                            }
                            self.at += 1;
                        }
                    }
                    _ => return Err(error(self.at)),
                }
                json.push_str(&self.source[escape..self.at]);
            } else {
                let ch = self.source[self.at..].chars().next().unwrap();
                self.at += ch.len_utf8();
                if ch == '"' {
                    json.push_str("\\\"");
                } else if ch < ' ' {
                    write!(json, "\\u{:04x}", ch as u32).unwrap();
                } else {
                    json.push(ch);
                }
            }
        }
        Err(error(start))
    }

    fn byte(&self) -> Option<u8> {
        self.source.as_bytes().get(self.at).copied()
    }
    fn take(&mut self, byte: u8) -> bool {
        if self.byte() == Some(byte) {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn digits(&mut self) {
        while self.byte().is_some_and(|b| b.is_ascii_digit()) {
            self.at += 1;
        }
    }
}

pub(super) fn error(offset: usize) -> Error {
    Error::new(
        ErrorKind::UnsupportedExpression,
        offset,
        "invalid or unsupported expression; see CONFORMANCE.md",
    )
}
