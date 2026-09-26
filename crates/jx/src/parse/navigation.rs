use super::*;

impl Parser<'_> {
    pub(super) fn navigation(
        &mut self,
        first: Node,
        lookup: bool,
        nesting: usize,
    ) -> Result<Node, Error> {
        let offset = first.offset;
        let (mut steps, mut keep) = start(first, lookup, offset)?;
        loop {
            let step = steps.pop().unwrap();
            let (mut target, predicates, retained) =
                self.suffix(step.node, step.predicates, nesting)?;
            keep |= retained;
            let lookup = step.lookup;
            let path_start = lookup || matches!(target.kind, Kind::Path(_));
            if retained && path_start {
                let depth = target.depth + 1;
                target = node(Kind::Keep(Box::new(target), false), offset, depth)?;
            }
            let predicates = if !path_start && !predicates.is_empty() {
                let depth = target.depth
                    + predicates.len()
                    + predicates.iter().map(|n| n.depth).max().unwrap();
                target = node(Kind::Filter(Box::new(target), predicates), offset, depth)?;
                Box::default()
            } else {
                predicates
            };
            if retained && !path_start {
                let depth = target.depth + 1;
                target = node(Kind::Keep(Box::new(target), false), offset, depth)?;
            }
            steps.push(Step {
                node: target,
                predicates,
                lookup,
                effects: false,
            });
            if !matches!(self.token, Token::Dot) {
                break;
            }
            if steps.len() >= MAX_DEPTH {
                return Err(depth_error(self.offset));
            }
            self.advance()?;
            if !matches!(
                self.token,
                Token::Name(_)
                    | Token::Quoted(_)
                    | Token::String(_)
                    | Token::Root
                    | Token::Open
                    | Token::Variable(_)
                    | Token::FilterOpen
                    | Token::ObjectOpen
                    | Token::Operator(Op::Multiply)
                    | Token::Descendants
            ) {
                return Err(error(self.offset));
            }
            let (target, lookup) = self.primary(nesting + 1)?;
            if matches!(target.kind, Kind::Boolean(_) | Kind::Null) {
                return Err(error(target.offset));
            }
            steps.push(Step {
                node: target,
                predicates: Box::default(),
                lookup,
                effects: false,
            });
        }
        finish(steps, keep, offset)
    }

    fn suffix(
        &mut self,
        mut target: Node,
        previous: Box<[Node]>,
        nesting: usize,
    ) -> Result<(Node, Box<[Node]>, bool), Error> {
        let mut predicates = previous.into_vec();
        let mut keep = false;
        loop {
            if matches!(self.token, Token::Open) {
                if !predicates.is_empty() {
                    let depth = target.depth
                        + predicates.len()
                        + predicates.iter().map(|n| n.depth).max().unwrap();
                    let offset = target.offset;
                    target = node(
                        Kind::Filter(
                            Box::new(target),
                            std::mem::take(&mut predicates).into_boxed_slice(),
                        ),
                        offset,
                        depth,
                    )?;
                }
                target = self.calls(target, nesting)?;
            }
            if !matches!(self.token, Token::FilterOpen) {
                break;
            }
            if predicates.len() >= MAX_DEPTH {
                return Err(depth_error(self.offset));
            }
            self.advance()?;
            if matches!(self.token, Token::FilterClose) {
                keep = true;
            } else {
                predicates.push(self.expression(0, nesting + 1)?);
            }
            if !matches!(self.token, Token::FilterClose) {
                return Err(error(self.offset));
            }
            self.advance()?;
        }
        Ok((target, predicates.into_boxed_slice(), keep))
    }
}

fn finish(mut steps: Vec<Step>, keep: bool, offset: usize) -> Result<Node, Error> {
    if steps.len() > 1 {
        for step in &mut steps {
            quoted(step)?;
            step.node.preserve_array();
        }
    }
    let plain_path = steps.iter().enumerate().all(|(index, step)| {
        step.predicates.is_empty()
            && matches!(&step.node.kind, Kind::Path(path)
                if step.lookup || (index == 0 && path.fields.is_empty()))
    });
    let result = if steps.len() == 1 && steps[0].predicates.is_empty() {
        steps.pop().unwrap().node
    } else if plain_path {
        let rooted = !steps[0].lookup;
        let fields = steps
            .into_iter()
            .flat_map(|step| match step.node.kind {
                Kind::Path(path) => path.fields.into_vec(),
                _ => unreachable!(),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        node(Kind::Path(Path { fields, rooted }), offset, 1)?
    } else {
        let depth = steps.len()
            + steps
                .iter()
                .map(|step| {
                    step.node.depth
                        + step.predicates.len()
                        + step.predicates.iter().map(|p| p.depth).max().unwrap_or(0)
                })
                .max()
                .unwrap();
        let array_focus = steps[0].node.array_focus();
        node(
            Kind::Route(steps.into_boxed_slice(), array_focus),
            offset,
            depth,
        )?
    };
    if keep {
        let depth = result.depth + 1;
        node(Kind::Keep(Box::new(result), true), offset, depth)
    } else {
        Ok(result)
    }
}

fn start(first: Node, lookup: bool, offset: usize) -> Result<(Vec<Step>, bool), Error> {
    let (first, inherited) = match first.kind {
        Kind::Keep(child, true) => (*child, true),
        _ => (first, false),
    };
    let steps = match first.kind {
        Kind::Route(steps, _) => steps.into_vec(),
        Kind::Path(path) if path.fields.len() > 1 => {
            let mut steps = Vec::new();
            if path.rooted {
                steps.push(Step {
                    node: node(
                        Kind::Path(Path {
                            fields: Box::default(),
                            rooted: true,
                        }),
                        offset,
                        1,
                    )?,
                    predicates: Box::default(),
                    lookup: false,
                    effects: false,
                });
            }
            for field in path.fields {
                steps.push(Step {
                    node: node(
                        Kind::Path(Path {
                            fields: vec![field].into_boxed_slice(),
                            rooted: false,
                        }),
                        offset,
                        1,
                    )?,
                    predicates: Box::default(),
                    lookup: true,
                    effects: false,
                });
            }
            steps
        }
        _ => vec![Step {
            node: first,
            predicates: Box::default(),
            lookup,
            effects: false,
        }],
    };
    Ok((steps, inherited))
}

fn quoted(step: &mut Step) -> Result<(), Error> {
    let mut base = &step.node;
    let keep = matches!(base.kind, Kind::Keep(_, false));
    if let Kind::Keep(child, false) = &base.kind {
        base = child;
    }
    if let Kind::Filter(child, _) = &base.kind {
        base = child;
    }
    if !matches!(base.kind, Kind::String(_)) {
        return Ok(());
    }
    let offset = base.offset;
    let mut kind = std::mem::replace(&mut step.node.kind, Kind::Missing);
    if let Kind::Keep(child, false) = kind {
        kind = child.kind;
    }
    if let Kind::Filter(child, predicates) = kind {
        kind = child.kind;
        step.predicates = predicates;
    }
    let Kind::String(encoded) = kind else {
        unreachable!()
    };
    let name = char::decode_utf16(crate::json::string::units(&encoded[1..encoded.len() - 1]))
        .collect::<Result<String, _>>()
        .map_err(|_| error(offset))?;
    let field = node(
        Kind::Path(Path {
            fields: vec![name.into()].into_boxed_slice(),
            rooted: false,
        }),
        offset,
        1,
    )?;
    step.node = if keep {
        node(Kind::Keep(Box::new(field), false), offset, 2)?
    } else {
        field
    };
    step.lookup = true;
    Ok(())
}
