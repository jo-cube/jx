use super::*;

impl Parser<'_> {
    pub(super) fn block(&mut self, nesting: usize, offset: usize) -> Result<Kind, Error> {
        let mut items = Vec::new();
        while !matches!(self.token, Token::Close) {
            items.push(self.expression(0, nesting + 1)?);
            if !matches!(self.token, Token::Semi) {
                break;
            }
            self.advance()?;
        }
        if !matches!(self.token, Token::Close) {
            return Err(error(self.offset));
        }
        self.advance()?;
        Ok(match items.len() {
            0 => Kind::Group(Box::new(node(Kind::Missing, offset, 1)?)),
            1 => Kind::Group(Box::new(items.pop().unwrap())),
            _ => Kind::Block(items.into_boxed_slice()),
        })
    }

    pub(super) fn transform(&mut self, nesting: usize) -> Result<Kind, Error> {
        let pattern = self.expression(0, nesting + 1)?;
        if !matches!(self.token, Token::Pipe) {
            return Err(error(self.offset));
        }
        self.advance()?;
        let update = self.expression(0, nesting + 1)?;
        let delete = if matches!(self.token, Token::Comma) {
            self.advance()?;
            Some(self.expression(0, nesting + 1)?)
        } else {
            None
        };
        if !matches!(self.token, Token::Pipe) {
            return Err(error(self.offset));
        }
        self.advance()?;
        Ok(Kind::Transform(Box::new(crate::transform::Definition {
            pattern,
            update,
            delete,
        })))
    }

    pub(super) fn lambda(&mut self, nesting: usize) -> Result<Kind, Error> {
        if !matches!(self.token, Token::Open) {
            return Err(error(self.offset));
        }
        self.advance()?;
        let mut params = Vec::new();
        if !matches!(self.token, Token::Close) {
            loop {
                let name = match self.advance()? {
                    Token::Variable(name) => name,
                    Token::Root => "",
                    _ => return Err(error(self.offset)),
                };
                params.push(name.into());
                if !matches!(self.token, Token::Comma) {
                    break;
                }
                self.advance()?;
            }
        }
        if !matches!(self.token, Token::Close) {
            return Err(error(self.offset));
        }
        self.advance()?;
        if !matches!(self.token, Token::ObjectOpen) {
            return Err(error(self.offset));
        }
        self.advance()?;
        let body = self.expression(0, nesting + 1)?;
        if !matches!(self.token, Token::ObjectClose) {
            return Err(error(self.offset));
        }
        self.advance()?;
        Ok(Kind::Lambda(params.into_boxed_slice(), Box::new(body)))
    }

    pub(super) fn calls(&mut self, mut target: Node, nesting: usize) -> Result<Node, Error> {
        while matches!(self.token, Token::Open) {
            let offset = target.offset;
            self.advance()?;
            let mut args = Vec::new();
            if !matches!(self.token, Token::Close) {
                loop {
                    args.push(if matches!(self.token, Token::Question) {
                        self.advance()?;
                        None
                    } else {
                        Some(self.expression(0, nesting + 1)?)
                    });
                    if !matches!(self.token, Token::Comma) {
                        break;
                    }
                    self.advance()?;
                }
            }
            if !matches!(self.token, Token::Close) {
                return Err(error(self.offset));
            }
            self.advance()?;
            let depth = 1 + target
                .depth
                .max(args.iter().flatten().map(|n| n.depth).max().unwrap_or(0));
            let kind = if args.iter().any(Option::is_none) {
                Kind::Partial(Box::new(target), args.into_boxed_slice())
            } else {
                Kind::Call(
                    Box::new(target),
                    args.into_iter().map(Option::unwrap).collect(),
                )
            };
            target = node(kind, offset, depth)?;
        }
        Ok(target)
    }
}
