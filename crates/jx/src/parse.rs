mod lex;
mod lexical;
mod navigation;
mod reduce;
use crate::{
    Error, ErrorKind, Expression,
    expression::{Kind, Node, Op, Path, Step},
};
use lex::{Lexer, Token, error};
const MAX_DEPTH: usize = 128;

pub(crate) fn expression(source: &str) -> Result<Expression, Error> {
    let mut lexer = Lexer { source, at: 0 };
    let (token, offset) = lexer.next()?;
    let mut parser = Parser {
        lexer,
        token,
        offset,
    };
    let mut root = parser.expression(0, 0)?;
    if !matches!(parser.token, Token::End) {
        return Err(error(parser.offset));
    }
    crate::analysis::prepare(&mut root)?;
    crate::compile::prepare(&mut root);
    crate::plan::prepare(&mut root);
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
        let (first, lookup) = self.primary(nesting)?;
        let mut lhs = self.navigation(first, lookup, nesting)?;
        loop {
            if matches!(self.token, Token::ObjectOpen) && minimum < 70 {
                lhs = self.reduction(lhs, nesting)?;
                continue;
            }
            if matches!(self.token, Token::Sort) && minimum < 40 {
                lhs = self.ordering(lhs, nesting)?;
                continue;
            }
            if matches!(self.token, Token::Bind) && minimum < 10 {
                let name = match lhs.kind {
                    Kind::Variable(name) => name,
                    Kind::Path(path) if path.rooted && path.fields.is_empty() => "".into(),
                    _ => return Err(error(lhs.offset)),
                };
                let offset = self.offset;
                self.advance()?;
                let rhs = self.expression(9, nesting + 1)?;
                let depth = rhs.depth + 1;
                lhs = node(Kind::Bind(name, Box::new(rhs)), offset, depth)?;
                continue;
            }
            if matches!(self.token, Token::Question) && minimum < 20 {
                let offset = self.offset;
                self.advance()?;
                let yes = self.expression(0, nesting + 1)?;
                let no = if matches!(self.token, Token::Colon) {
                    self.advance()?;
                    Some(Box::new(self.expression(0, nesting + 1)?))
                } else {
                    None
                };
                let depth = 1 + lhs
                    .depth
                    .max(yes.depth)
                    .max(no.as_ref().map_or(0, |n| n.depth));
                lhs = node(
                    Kind::Conditional(Box::new(lhs), Box::new(yes), no),
                    offset,
                    depth,
                )?;
                continue;
            }
            let op = match self.token {
                Token::Operator(op) => op,
                Token::Name("and") => Op::And,
                Token::Name("or") => Op::Or,
                Token::Name("in") => Op::In,
                _ => break,
            };
            if op.precedence() <= minimum {
                break;
            }
            let offset = self.offset;
            self.advance()?;
            let rhs = self.expression(
                if matches!(op, Op::Default | Op::Coalesce) {
                    0
                } else {
                    op.precedence()
                },
                nesting + 1,
            )?;
            lhs = binary(op, lhs, rhs, offset)?;
        }
        Ok(lhs)
    }
    fn primary(&mut self, nesting: usize) -> Result<(Node, bool), Error> {
        if nesting >= MAX_DEPTH {
            return Err(depth_error(self.offset));
        }
        let offset = self.offset;
        let mut lookup = false;
        let kind = match self.advance()? {
            Token::Number(n) => Kind::Number(n),
            Token::String(s) => Kind::String(s),
            Token::Name("true") => Kind::Boolean(true),
            Token::Name("false") => Kind::Boolean(false),
            Token::Name("null") => Kind::Null,
            Token::Operator(Op::Multiply) => Kind::Wildcard,
            Token::Descendants => Kind::Descendants,
            Token::Name("function") => self.lambda(nesting)?,
            Token::Name(name) | Token::Quoted(name) => {
                lookup = true;
                Kind::Path(Path {
                    fields: vec![name.into()].into_boxed_slice(),
                    rooted: false,
                })
            }
            Token::FilterOpen => Kind::Array(self.list(nesting, true)?, false),
            Token::ObjectOpen => Kind::Object(self.object(nesting)?),
            Token::Variable(name) => Kind::Variable(name.into()),
            Token::Root => Kind::Path(Path {
                fields: Box::default(),
                rooted: true,
            }),
            Token::Operator(Op::Subtract) => {
                let child = self.expression(70, nesting + 1)?;
                // A literal negative position preserves selected arrays; computed
                // positions retain sequence shape, so only fold a bare number.
                if let Kind::Number(n) = child.kind {
                    Kind::Number(-n)
                } else {
                    Kind::Negate(Box::new(child))
                }
            }
            Token::Open => self.block(nesting, offset)?,
            _ => return Err(error(offset)),
        };
        let depth = match &kind {
            Kind::Group(n) | Kind::Negate(n) | Kind::Lambda(_, n) => 1 + n.depth,
            Kind::Object(pairs) => {
                1 + pairs
                    .iter()
                    .map(|(k, v)| k.depth.max(v.depth))
                    .max()
                    .unwrap_or(0)
            }
            Kind::Array(args, _) | Kind::Block(args) => {
                1 + args.iter().map(|n| n.depth).max().unwrap_or(0)
            }
            _ => 1,
        };
        Ok((node(kind, offset, depth)?, lookup))
    }
    fn arguments(&mut self, nesting: usize) -> Result<Box<[Node]>, Error> {
        if !matches!(self.token, Token::Open) {
            return Err(error(self.offset));
        }
        self.advance()?;
        self.list(nesting, false)
    }
    fn list(&mut self, nesting: usize, array: bool) -> Result<Box<[Node]>, Error> {
        let closed = |token: &Token| {
            matches!(
                (array, token),
                (true, Token::FilterClose) | (false, Token::Close)
            )
        };
        let mut arguments = Vec::new();
        if !closed(&self.token) {
            loop {
                let mut item = self.expression(0, nesting + 1)?;
                if array && matches!(self.token, Token::Range) {
                    let offset = self.offset;
                    self.advance()?;
                    let end = self.expression(0, nesting + 1)?;
                    let depth = 1 + item.depth.max(end.depth);
                    item = node(Kind::Range(Box::new(item), Box::new(end)), offset, depth)?;
                }
                arguments.push(item);
                if !matches!(self.token, Token::Comma) {
                    break;
                }
                self.advance()?;
            }
        }
        if !closed(&self.token) {
            return Err(error(self.offset));
        }
        self.advance()?;
        Ok(arguments.into_boxed_slice())
    }
}
fn node(kind: Kind, offset: usize, depth: usize) -> Result<Node, Error> {
    if depth > MAX_DEPTH {
        return Err(depth_error(offset));
    }
    Ok(Node {
        kind,
        offset,
        depth,
        effects: false,
    })
}
fn depth_error(offset: usize) -> Error {
    Error::new(
        ErrorKind::DepthLimit,
        offset,
        "expression depth exceeds 128",
    )
}

fn binary(op: Op, mut left: Node, right: Node, offset: usize) -> Result<Node, Error> {
    if matches!(op, Op::Coalesce) {
        let depth = left.depth + 1;
        left = node(
            Kind::Call(
                Box::new(node(Kind::Variable("exists".into()), offset, 1)?),
                vec![left].into_boxed_slice(),
            ),
            offset,
            depth,
        )?;
    }
    let depth = 1 + left.depth.max(right.depth);
    node(
        Kind::Binary(op, Box::new(left), Box::new(right)),
        offset,
        depth,
    )
}
