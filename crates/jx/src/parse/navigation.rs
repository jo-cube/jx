use super::*;

impl Parser<'_> {
    pub(super) fn navigation(
        &mut self,
        first: Node,
        lookup: bool,
        nesting: usize,
    ) -> Result<Node, Error> {
        let offset = first.offset;
        let (first, predicates) = self.suffix(first, nesting)?;
        let path_start = lookup || matches!(first.kind, Kind::Path(_));
        let first = if !path_start && !predicates.is_empty() {
            let depth =
                first.depth + predicates.iter().map(|p| p.depth).max().unwrap() + predicates.len();
            Step {
                node: node(Kind::Filter(Box::new(first), predicates), offset, depth)?,
                predicates: Box::default(),
                lookup: false,
                effects: false,
            }
        } else {
            Step {
                node: first,
                predicates,
                lookup,
                effects: false,
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
                Token::Name(_)
                    | Token::Quoted(_)
                    | Token::Root
                    | Token::Open
                    | Token::Variable(_)
                    | Token::FilterOpen
                    | Token::ObjectOpen
            ) {
                return Err(error(self.offset));
            }
            let (node, lookup) = self.primary(nesting + 1)?;
            if matches!(node.kind, Kind::Boolean(_) | Kind::Null) {
                return Err(error(node.offset));
            }
            let (node, predicates) = self.suffix(node, nesting)?;
            steps.push(Step {
                node,
                predicates,
                lookup,
                effects: false,
            });
        }
        if steps.len() == 1 && steps[0].predicates.is_empty() {
            return Ok(steps.pop().unwrap().node);
        }
        for step in &mut steps {
            step.node.preserve_array();
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
        let array_focus = steps[0].node.array_focus();
        node(
            Kind::Route(steps.into_boxed_slice(), array_focus),
            offset,
            depth,
        )
    }
}
