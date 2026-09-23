mod lex;
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
        let (first, lookup) = self.primary(nesting)?;
        let mut lhs = self.navigation(first, lookup, nesting)?;
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
            lhs = node(
                Kind::Binary(op, Box::new(lhs), Box::new(rhs)),
                offset,
                depth,
            )?;
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
            Token::Name("in" | "function") => return Err(error(offset)),
            Token::Name(name) | Token::Quoted(name) => {
                lookup = true;
                Kind::Path(Path {
                    fields: vec![name.into()].into_boxed_slice(),
                    rooted: false,
                })
            }
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
            Token::Open => {
                let child = if matches!(self.token, Token::Close) {
                    node(Kind::Missing, offset, 1)?
                } else {
                    self.expression(0, nesting + 1)?
                };
                if !matches!(self.token, Token::Close) {
                    return Err(error(self.offset));
                }
                self.advance()?;
                Kind::Group(Box::new(child))
            }
            _ => return Err(error(offset)),
        };
        let depth = match &kind {
            Kind::Group(n) | Kind::Negate(n) => 1 + n.depth,
            _ => 1,
        };
        Ok((node(kind, offset, depth)?, lookup))
    }
    fn predicates(&mut self, nesting: usize) -> Result<Box<[Node]>, Error> {
        let mut predicates = Vec::new();
        while matches!(self.token, Token::FilterOpen) {
            if predicates.len() >= MAX_DEPTH {
                return Err(depth_error(self.offset));
            }
            self.advance()?;
            predicates.push(self.expression(0, nesting + 1)?);
            if !matches!(self.token, Token::FilterClose) {
                return Err(error(self.offset));
            }
            self.advance()?;
        }
        Ok(predicates.into_boxed_slice())
    }
    fn navigation(&mut self, first: Node, lookup: bool, nesting: usize) -> Result<Node, Error> {
        let offset = first.offset;
        let predicates = self.predicates(nesting)?;
        let path_start = lookup || matches!(first.kind, Kind::Path(_));
        let first = if !path_start && !predicates.is_empty() {
            let depth =
                first.depth + predicates.iter().map(|p| p.depth).max().unwrap() + predicates.len();
            Step {
                node: node(Kind::Filter(Box::new(first), predicates), offset, depth)?,
                predicates: Box::default(),
                lookup: false,
            }
        } else {
            Step {
                node: first,
                predicates,
                lookup,
            }
        };
        let mut steps = vec![first];
        while matches!(self.token, Token::Dot) {
            if steps.len() >= MAX_DEPTH {
                return Err(depth_error(self.offset));
            }
            self.advance()?;
            if !matches!(
                self.token,
                Token::Name(_) | Token::Quoted(_) | Token::Root | Token::Open
            ) {
                return Err(error(self.offset));
            }
            let (node, lookup) = self.primary(nesting + 1)?;
            if matches!(node.kind, Kind::Boolean(_) | Kind::Null) {
                return Err(error(node.offset));
            }
            let predicates = self.predicates(nesting)?;
            steps.push(Step {
                node,
                predicates,
                lookup,
            });
        }
        if steps.len() == 1 && steps[0].predicates.is_empty() {
            return Ok(steps.pop().unwrap().node);
        }
        // Keep static paths in their fused validating representation.
        if steps.iter().enumerate().all(|(index, step)| {
            step.predicates.is_empty()
                && matches!(&step.node.kind, Kind::Path(path)
                    if step.lookup || (index == 0 && path.fields.is_empty()))
        }) {
            let rooted = !steps[0].lookup;
            let fields = steps
                .into_iter()
                .flat_map(|step| match step.node.kind {
                    Kind::Path(path) => path.fields.into_vec(),
                    _ => unreachable!(),
                })
                .collect::<Vec<_>>()
                .into_boxed_slice();
            return node(Kind::Path(Path { fields, rooted }), offset, 1);
        }
        let depth = steps.len()
            + steps
                .iter()
                .map(|s| {
                    s.node.depth
                        + s.predicates.len()
                        + s.predicates.iter().map(|p| p.depth).max().unwrap_or(0)
                })
                .max()
                .unwrap();
        node(Kind::Route(steps.into_boxed_slice()), offset, depth)
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
    })
}
fn depth_error(offset: usize) -> Error {
    Error::new(
        ErrorKind::DepthLimit,
        offset,
        "expression depth exceeds 128",
    )
}
