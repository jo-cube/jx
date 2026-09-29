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
            let (step, retained) = self.suffix(step, nesting)?;
            let Step {
                node: mut target,
                predicates,
                bindings,
                lookup,
                ..
            } = step;
            keep |= retained;
            let path_start =
                lookup || matches!(target.kind, Kind::Path(_)) || crate::tuple::active(&target);
            if retained && path_start {
                let depth = target.depth + 1;
                target = node(Kind::Keep(Box::new(target), false), offset, depth)?;
            }
            let predicates = if !path_start && bindings.is_none() && !predicates.is_empty() {
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
                bindings,
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
                bindings: None,
                lookup,
                effects: false,
            });
        }
        finish(steps, keep, offset)
    }

    fn suffix(&mut self, step: Step, nesting: usize) -> Result<(Step, bool), Error> {
        let Step {
            node: mut target,
            predicates,
            mut bindings,
            lookup,
            ..
        } = step;
        let mut predicates = predicates.into_vec();
        let mut keep = false;
        loop {
            if matches!(self.token, Token::Focus | Token::Index) {
                let offset = self.offset;
                let focus = matches!(self.advance()?, Token::Focus);
                let Token::Variable(name) = self.advance()? else {
                    return Err(error(offset));
                };
                if name == "$" {
                    return Err(error(offset));
                }
                if focus && (!predicates.is_empty() || matches!(target.kind, Kind::Sort(..))) {
                    return Err(error(offset));
                }
                let binding = bindings.get_or_insert_with(Default::default);
                if focus {
                    binding.focus = Some(name.into());
                } else if predicates.is_empty() {
                    binding.index = Some(name.into());
                } else {
                    binding.indices.push((predicates.len(), name.into()));
                }
                continue;
            }
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
        Ok((
            Step {
                node: target,
                predicates: predicates.into_boxed_slice(),
                bindings,
                lookup,
                effects: false,
            },
            keep,
        ))
    }
}

fn finish(mut steps: Vec<Step>, keep: bool, offset: usize) -> Result<Node, Error> {
    // A lone @ annotation does not turn a non-path operand into a path.
    // Adding # or a following map step does (as in the pinned reference).
    if steps.len() == 1
        && !steps[0].lookup
        && steps[0]
            .bindings
            .as_ref()
            .is_some_and(|b| b.index.is_none() && b.indices.is_empty())
    {
        steps[0].bindings = None;
        if !steps[0].predicates.is_empty() {
            let step = steps.pop().unwrap();
            let depth =
                step.node.depth + 1 + step.predicates.iter().map(|p| p.depth).max().unwrap();
            let result = node(
                Kind::Filter(Box::new(step.node), step.predicates),
                offset,
                depth,
            )?;
            return if keep {
                node(Kind::Keep(Box::new(result), true), offset, depth + 1)
            } else {
                Ok(result)
            };
        }
    }
    if steps.len() > 1 {
        for step in &mut steps {
            quoted(step)?;
            step.node.preserve_array();
        }
    }
    let plain_path = steps.iter().enumerate().all(|(index, step)| {
        step.bindings.is_none()
            && step.predicates.is_empty()
            && matches!(&step.node.kind, Kind::Path(path)
                if step.lookup || (index == 0 && path.fields.is_empty()))
    });
    let result =
        if steps.len() == 1 && steps[0].predicates.is_empty() && steps[0].bindings.is_none() {
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
            let kind = if steps.iter().any(|s| s.bindings.is_some())
                || crate::tuple::active(&steps[0].node)
            {
                Kind::Tuples(steps.into_boxed_slice(), array_focus)
            } else {
                Kind::Route(steps.into_boxed_slice(), array_focus)
            };
            node(kind, offset, depth)?
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
        Kind::Route(steps, _) | Kind::Tuples(steps, _) => steps.into_vec(),
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
                    bindings: None,
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
                    bindings: None,
                    lookup: true,
                    effects: false,
                });
            }
            steps
        }
        _ => vec![Step {
            node: first,
            predicates: Box::default(),
            bindings: None,
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
