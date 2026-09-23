mod lex;

use crate::{
    Error, ErrorKind, Expression,
    expression::{Kind, Node, Op, Path},
};
use lex::{Lexer, Token, error};

// Bound both parser recursion and the final tree (including flat operator chains).
const MAX_DEPTH: usize = 128;

pub(crate) fn expression(source: &str) -> Result<Expression, Error> {
    let mut lexer = Lexer { source, at: 0 };
    let (token, offset) = lexer.next()?;
    let mut parser = Parser {
        lexer,
        token,
        offset,
    };
    let root = parser.expression(0, 0)?;
    if !matches!(parser.token, Token::End) {
        return Err(error(parser.offset));
    }
    Ok(Expression { root })
}

struct Parser<'a> {
    lexer: Lexer<'a>,
    token: Token<'a>,
    offset: usize,
}

impl<'a> Parser<'a> {
    fn advance(&mut self) -> Result<Token<'a>, Error> {
        let (token, offset) = self.lexer.next()?;
        self.offset = offset;
        Ok(std::mem::replace(&mut self.token, token))
    }

    fn expression(&mut self, minimum: u8, nesting: usize) -> Result<Node, Error> {
        if nesting >= MAX_DEPTH {
            return Err(depth_error(self.offset));
        }
        let offset = self.offset;
        let token = self.advance()?;
        let kind = match token {
            Token::Number(value) => Kind::Number(value),
            Token::String(value) => Kind::String(value),
            Token::Name("true") => Kind::Boolean(true),
            Token::Name("false") => Kind::Boolean(false),
            Token::Name("null") => Kind::Null,
            Token::Name("function" | "in") => return Err(error(offset)),
            Token::Name(field) | Token::Quoted(field) => {
                let field: Box<str> = field.into();
                Kind::Path(self.path(false, vec![field])?)
            }
            Token::Root => Kind::Path(self.path(true, Vec::new())?),
            Token::Operator(Op::Subtract) => {
                Kind::Negate(Box::new(self.expression(70, nesting + 1)?))
            }
            Token::Open => {
                if matches!(self.token, Token::Close) {
                    self.advance()?;
                    Kind::Missing
                } else {
                    let expression = self.expression(0, nesting + 1)?;
                    if !matches!(self.token, Token::Close) {
                        return Err(error(self.offset));
                    }
                    self.advance()?;
                    // Grouping changes precedence, not the path's sequence boundaries.
                    return self.binary(expression, minimum, nesting);
                }
            }
            _ => return Err(error(offset)),
        };
        let depth = if let Kind::Negate(child) = &kind {
            child.depth + 1
        } else {
            1
        };
        if depth > MAX_DEPTH {
            return Err(depth_error(offset));
        }
        self.binary(
            Node {
                kind,
                offset,
                depth,
            },
            minimum,
            nesting,
        )
    }

    fn binary(&mut self, mut lhs: Node, minimum: u8, nesting: usize) -> Result<Node, Error> {
        loop {
            let op = match self.token {
                Token::Operator(op) => op,
                Token::Name("and") => Op::And,
                Token::Name("or") => Op::Or,
                _ => break,
            };
            if op.precedence() <= minimum {
                break;
            }
            let offset = self.offset;
            self.advance()?;
            let rhs = self.expression(op.precedence(), nesting + 1)?;
            let depth = 1 + lhs.depth.max(rhs.depth);
            if depth > MAX_DEPTH {
                return Err(depth_error(offset));
            }
            lhs = Node {
                kind: Kind::Binary(op, Box::new(lhs), Box::new(rhs)),
                offset,
                depth,
            };
        }
        Ok(lhs)
    }

    fn path(&mut self, rooted: bool, mut fields: Vec<Box<str>>) -> Result<Path, Error> {
        while matches!(self.token, Token::Dot) {
            self.advance()?;
            let offset = self.offset;
            match self.advance()? {
                Token::Name("true" | "false" | "null" | "function" | "in") => {
                    return Err(error(offset));
                }
                Token::Name(field) | Token::Quoted(field) => fields.push(field.into()),
                _ => return Err(error(offset)),
            }
        }
        Ok(Path {
            fields: fields.into_boxed_slice(),
            rooted,
        })
    }
}

fn depth_error(offset: usize) -> Error {
    Error::new(
        ErrorKind::DepthLimit,
        offset,
        "expression depth exceeds 128",
    )
}
