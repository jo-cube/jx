mod lex;
mod lexical;
mod navigation;
mod reduce;
use crate::{
    Error, ErrorKind, Expression,
    expression::{Kind, Node, Op, Path, Step},
};
use lex::{Lexer, Token, error};
pub(crate) const MAX_DEPTH: usize = 128;

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
    crate::provenance::prepare(&mut root)?;
    crate::analysis::prepare(&mut root)?;
    crate::compile::prepare(&mut root);
    crate::analysis::check_composition(&mut root)?;
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
                lhs = self.binding(lhs, nesting)?;
                continue;
            }
            if matches!(self.token, Token::Question) && minimum < 20 {
                lhs = self.conditional(lhs, nesting)?;
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
        let token = if matches!(self.token, Token::Operator(Op::Divide)) {
            let pattern = self.lexer.regex(offset)?;
            self.advance()?;
            return Ok((node(Kind::Regex(Box::new(pattern)), offset, 1)?, false));
        } else {
            self.advance()?
        };
        let kind = match token {
            Token::Number(n) => Kind::Number(n),
            Token::String(s) => Kind::String(s),
            Token::Name("true") => Kind::Boolean(true),
            Token::Name("false") => Kind::Boolean(false),
            Token::Name("null") => Kind::Null,
            Token::Operator(Op::Multiply) => Kind::Wildcard,
            Token::Descendants => Kind::Descendants,
            Token::Operator(Op::Remainder) => Kind::Parent(Box::default()),
            Token::Pipe => self.transform(nesting)?,
            Token::Name("function" | "λ") if matches!(self.token, Token::Open) => {
                self.lambda(nesting)?
            }
            Token::Name(name) | Token::Quoted(name) => {
                lookup = true;
                Kind::Path(Path {
                    fields: vec![name.into()].into_boxed_slice(),
                    rooted: false,
                })
            }
            Token::FilterOpen => Kind::Array(self.array_items(nesting)?, false),
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
            Kind::Group(n) | Kind::Negate(n) => 1 + n.depth,
            Kind::Lambda(d) => 1 + d.body.depth,
            Kind::Object(pairs) => {
                1 + pairs
                    .iter()
                    .map(|(k, v)| k.depth.max(v.depth))
                    .max()
                    .unwrap_or(0)
            }
            Kind::Transform(definition) => 1 + definition.depth(),
            Kind::Array(args, _) | Kind::Block(args) => {
                1 + args.iter().map(|n| n.depth).max().unwrap_or(0)
            }
            _ => 1,
        };
        Ok((node(kind, offset, depth)?, lookup))
    }
    fn array_items(&mut self, nesting: usize) -> Result<Box<[Node]>, Error> {
        let closed = |token: &Token| matches!(token, Token::FilterClose);
        let mut arguments = Vec::new();
        if !closed(&self.token) {
            loop {
                let mut item = self.expression(0, nesting + 1)?;
                if matches!(self.token, Token::Range) {
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
        clock: false,
        tail_call: false,
    })
}
fn depth_error(offset: usize) -> Error {
    Error::new(
        ErrorKind::DepthLimit,
        offset,
        "expression depth exceeds 128",
    )
}

fn binary(op: Op, mut left: Node, mut right: Node, offset: usize) -> Result<Node, Error> {
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
    let mut keep = false;
    if matches!(op, Op::Chain) {
        keep = chain_retention(&left) || chain_retention(&right);
        let mut call = &right;
        while let Kind::Keep(child, _) | Kind::Filter(child, _) = &call.kind {
            call = child;
        }
        if matches!(call.kind, Kind::Call(..)) {
            // A bare RHS call bypasses its predicates; its retention belongs to
            // the apply expression. Other RHS values keep their own boundaries.
            while let Kind::Keep(child, _) | Kind::Filter(child, _) = right.kind {
                right = *child;
            }
        }
    }
    let depth = 1 + left.depth.max(right.depth);
    let expression = node(
        Kind::Binary(op, Box::new(left), Box::new(right)),
        offset,
        depth,
    )?;
    if keep {
        node(Kind::Keep(Box::new(expression), false), offset, depth + 1)
    } else {
        Ok(expression)
    }
}

// A path's first step can carry keepArray; a later [] keeps only that path's
// sequence. Parentheses do not forward a child's keepArray flag to chaining.
fn chain_retention(node: &Node) -> bool {
    match &node.kind {
        Kind::Keep(_, false) => true,
        Kind::Keep(child, true)
        | Kind::Filter(child, _)
        | Kind::Sort(child, _)
        | Kind::Reduce(child, _) => chain_retention(child),
        Kind::Route(steps, _) | Kind::Tuples(steps, _) => chain_retention(&steps[0].node),
        _ => false,
    }
}
