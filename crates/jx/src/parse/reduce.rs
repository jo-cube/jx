use super::*;

impl Parser<'_> {
    pub(super) fn object(&mut self, nesting: usize) -> Result<Box<[(Node, Node)]>, Error> {
        let mut pairs = Vec::new();
        if !matches!(self.token, Token::ObjectClose) {
            loop {
                let key = self.expression(0, nesting + 1)?;
                if !matches!(self.token, Token::Colon) {
                    return Err(error(self.offset));
                }
                self.advance()?;
                let value = self.expression(0, nesting + 1)?;
                pairs.push((key, value));
                if !matches!(self.token, Token::Comma) {
                    break;
                }
                self.advance()?;
            }
        }
        if !matches!(self.token, Token::ObjectClose) {
            return Err(error(self.offset));
        }
        self.advance()?;
        Ok(pairs.into_boxed_slice())
    }

    pub(super) fn reduction(&mut self, lhs: Node, nesting: usize) -> Result<Node, Error> {
        let offset = self.offset;
        if matches!(lhs.kind, Kind::Reduce(..)) {
            return Err(error(offset));
        }
        self.advance()?;
        let pairs = self.object(nesting)?;
        if !grouped_path(&lhs) {
            // Empty brackets retain cardinality; predicates cannot follow a
            // non-path grouping without a parenthesized boundary.
            let mut lookahead = Lexer {
                source: self.lexer.source,
                at: self.lexer.at,
            };
            let mut filter = matches!(self.token, Token::FilterOpen);
            let mut offset = self.offset;
            while filter {
                if !matches!(lookahead.next()?.0, Token::FilterClose) {
                    return Err(error(offset));
                }
                let (token, at) = lookahead.next()?;
                filter = matches!(token, Token::FilterOpen);
                offset = at;
            }
        }
        // Grouping belongs to the complete unparenthesized path, including any
        // following navigation and stage predicates. Parentheses establish a boundary.
        let lookup = matches!(&lhs.kind, Kind::Path(path) if !path.rooted);
        let call = matches!(self.token, Token::Open);
        let base = if call {
            lhs
        } else {
            self.navigation(lhs, lookup, nesting)?
        };
        let depth = 1 + base.depth.max(
            pairs
                .iter()
                .map(|(k, v)| k.depth.max(v.depth))
                .max()
                .unwrap_or(0),
        );
        let reduced = node(Kind::Reduce(Box::new(base), pairs), offset, depth)?;
        if call {
            self.navigation(reduced, false, nesting)
        } else {
            Ok(reduced)
        }
    }

    pub(super) fn ordering(&mut self, lhs: Node, nesting: usize) -> Result<Node, Error> {
        let offset = self.offset;
        if let Kind::Reduce(base, pairs) = lhs.kind {
            let ordered = self.ordering(*base, nesting)?;
            let depth = 1 + ordered.depth.max(
                pairs
                    .iter()
                    .map(|(k, v)| k.depth.max(v.depth))
                    .max()
                    .unwrap_or(0),
            );
            return node(Kind::Reduce(Box::new(ordered), pairs), lhs.offset, depth);
        }
        self.advance()?;
        if !matches!(self.token, Token::Open) {
            return Err(error(self.offset));
        }
        self.advance()?;
        let mut terms = Vec::new();
        loop {
            let descending = matches!(self.token, Token::Operator(Op::Greater));
            if matches!(self.token, Token::Operator(Op::Greater | Op::Less)) {
                self.advance()?;
            }
            let term = self.expression(0, nesting + 1)?;
            terms.push((term, descending));
            if !matches!(self.token, Token::Comma) {
                break;
            }
            self.advance()?;
        }
        if !matches!(self.token, Token::Close) {
            return Err(error(self.offset));
        }
        self.advance()?;
        let (base, keep) = match lhs.kind {
            Kind::Keep(base, _) => (*base, true),
            _ => (lhs, false),
        };
        let depth = 1 + base
            .depth
            .max(terms.iter().map(|(n, _)| n.depth).max().unwrap());
        let mut sorted = node(
            Kind::Sort(Box::new(base), terms.into_boxed_slice()),
            offset,
            depth,
        )?;
        if keep {
            sorted = node(Kind::Keep(Box::new(sorted), true), offset, depth + 1)?;
        }
        self.navigation(sorted, false, nesting)
    }
}

fn grouped_path(node: &Node) -> bool {
    match &node.kind {
        Kind::Path(path) => !path.fields.is_empty(),
        Kind::Route(..) | Kind::Tuples(..) | Kind::Sort(..) => true,
        Kind::Keep(child, _) => grouped_path(child),
        _ => false,
    }
}
