use crate::{Error, ErrorKind, Expression};

pub(crate) fn expression(source: &str) -> Result<Expression, Error> {
    let mut parser = Parser { source, at: 0 };
    parser.space();
    let rooted = parser.take(b'$');
    if rooted {
        parser.space();
        if parser.at == source.len() {
            return Ok(Expression {
                fields: Box::default(),
                rooted,
            });
        }
        if !parser.take(b'.') {
            return Err(parser.error());
        }
    }
    let mut fields = Vec::new();
    loop {
        parser.space();
        let quoted = parser.take(b'`');
        let start = parser.at;
        if quoted {
            while parser.at < source.len() && parser.byte() != Some(b'`') {
                parser.at += 1;
            }
            if parser.at == source.len() {
                return Err(parser.error());
            }
        } else {
            if !parser
                .byte()
                .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
            {
                return Err(parser.error());
            }
            parser.at += 1;
            while parser
                .byte()
                .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_')
            {
                parser.at += 1;
            }
        }
        let name = &source[start..parser.at];
        if !quoted
            && matches!(
                name,
                "true" | "false" | "null" | "and" | "or" | "in" | "function"
            )
        {
            return Err(parser.error());
        }
        fields.push(name.into());
        if quoted {
            parser.at += 1;
        }
        parser.space();
        if parser.at == source.len() {
            return Ok(Expression {
                fields: fields.into_boxed_slice(),
                rooted,
            });
        }
        if !parser.take(b'.') {
            return Err(parser.error());
        }
    }
}

struct Parser<'a> {
    source: &'a str,
    at: usize,
}

impl Parser<'_> {
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

    fn space(&mut self) {
        while self
            .byte()
            .is_some_and(|b| matches!(b, b' ' | b'\n' | b'\r' | b'\t'))
        {
            self.at += 1;
        }
    }

    fn error(&self) -> Error {
        Error::new(
            ErrorKind::UnsupportedExpression,
            self.at,
            "expected identity ($) or an object field path; see CONFORMANCE.md",
        )
    }
}
