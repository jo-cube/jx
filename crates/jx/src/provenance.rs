use crate::{
    Error, ErrorKind,
    expression::{Kind, Node, Path, Step},
};

// Parent is an expression-path relationship, not a property of JSON values.
// Resolve it to a binding of one demanded stage's incoming context.
struct Seek {
    name: Box<str>,
    level: usize,
    offset: usize,
}
pub(crate) fn prepare(root: &mut Node) -> Result<(), Error> {
    let mut parents = 0;
    let pending = analyze(root, &mut parents)?;
    if let Some(parent) = pending.first() {
        return Err(unsupported(parent.offset));
    }
    if parents > 0 {
        refresh_depth(root)?;
    }
    Ok(())
}
fn unsupported(offset: usize) -> Error {
    Error::new(
        ErrorKind::UnsupportedExpression,
        offset,
        "parent cannot be derived from this expression",
    )
}
fn analyze(node: &mut Node, next: &mut usize) -> Result<Vec<Seek>, Error> {
    let mut pending = Vec::new();
    match &mut node.kind {
        Kind::Parent(name) => {
            *name = format!("!{}", *next).into();
            *next += 1;
            pending.push(Seek {
                name: name.clone(),
                level: 1,
                offset: node.offset,
            });
        }
        Kind::Route(steps, _) | Kind::Tuples(steps, _) => {
            for at in 0..steps.len() {
                let mut requests = analyze(&mut steps[at].node, next)?;
                for predicate in &mut steps[at].predicates {
                    for mut request in analyze(predicate, next)? {
                        if request.level == 1 {
                            seek_step(&mut steps[at].node, &mut steps[at].bindings, &mut request)?;
                        } else {
                            request.level -= 1;
                        }
                        if request.level > 0 {
                            requests.push(request);
                        }
                    }
                }
                for mut request in requests {
                    let mut previous = at;
                    while request.level > 0 && previous > 0 {
                        previous -= 1;
                        while previous > 0
                            && focused(&steps[previous])
                            && focused(&steps[previous - 1])
                        {
                            previous -= 1;
                        }
                        let step = &mut steps[previous];
                        seek_step(&mut step.node, &mut step.bindings, &mut request)?;
                    }
                    if request.level > 0 {
                        pending.push(request);
                    }
                }
            }
            mark(node);
        }
        Kind::Filter(base, predicates) => {
            pending = analyze(base, next)?;
            for predicate in predicates {
                for mut request in analyze(predicate, next)? {
                    seek_node(base, &mut request)?;
                    if request.level > 0 {
                        pending.push(request);
                    }
                }
            }
        }
        Kind::Sort(base, terms) => {
            pending = analyze(base, next)?;
            for (term, _) in terms {
                for mut request in analyze(term, next)? {
                    seek_node(base, &mut request)?;
                    if request.level > 0 {
                        pending.push(request);
                    }
                }
            }
        }
        Kind::Reduce(base, pairs) => {
            pending = analyze(base, next)?;
            // The reference does not derive parents from a group constructor.
            for (key, value) in pairs {
                analyze(key, next)?;
                analyze(value, next)?;
            }
        }
        Kind::Call(target, args) => {
            analyze(target, next)?;
            for arg in args {
                pending.extend(analyze(arg, next)?);
            }
        }
        Kind::Partial(target, args) => {
            analyze(target, next)?;
            for arg in args.iter_mut().flatten() {
                pending.extend(analyze(arg, next)?);
            }
        }
        Kind::Lambda(_, body) => {
            analyze(body, next)?;
        }
        Kind::Transform(d) => {
            analyze(&mut d.pattern, next)?;
            analyze(&mut d.update, next)?;
            if let Some(delete) = &mut d.delete {
                analyze(delete, next)?;
            }
        }
        Kind::Binary(crate::expression::Op::Chain, left, right) => {
            analyze(left, next)?;
            analyze(right, next)?;
        }
        _ => {
            let mut error = None;
            crate::analysis::children(node, &mut |child| {
                if error.is_none() {
                    match analyze(child, next) {
                        Ok(requests) => pending.extend(requests),
                        Err(e) => error = Some(e),
                    }
                }
            });
            if let Some(error) = error {
                return Err(error);
            }
        }
    }
    Ok(pending)
}
fn focused(step: &Step) -> bool {
    step.bindings.as_ref().is_some_and(|b| b.focus.is_some())
}
fn mark(node: &mut Node) {
    if let Kind::Route(steps, focus) = &mut node.kind
        && steps
            .iter()
            .any(|s| s.bindings.is_some() || crate::tuple::active(&s.node))
    {
        node.kind = Kind::Tuples(std::mem::take(steps), *focus);
    }
}
fn seek_step(
    node: &mut Node,
    bindings: &mut Option<Box<crate::tuple::Bindings>>,
    request: &mut Seek,
) -> Result<(), Error> {
    if matches!(&node.kind, Kind::Path(p) if !p.rooted && p.fields.len() == 1)
        || matches!(node.kind, Kind::Wildcard)
    {
        request.level -= 1;
        if request.level == 0 {
            bindings
                .get_or_insert_with(Default::default)
                .ancestors
                .push(request.name.clone());
        }
        Ok(())
    } else {
        seek_node(node, request)
    }
}
fn seek_node(node: &mut Node, request: &mut Seek) -> Result<(), Error> {
    if let Kind::Path(path) = &mut node.kind {
        if path.fields.is_empty() {
            return Err(unsupported(request.offset));
        }
        let mut steps = Vec::new();
        if path.rooted {
            steps.push(step(
                Kind::Path(Path {
                    fields: Box::default(),
                    rooted: true,
                }),
                node.offset,
            ));
        }
        for field in std::mem::take(&mut path.fields) {
            steps.push(step(
                Kind::Path(Path {
                    fields: vec![field].into(),
                    rooted: false,
                }),
                node.offset,
            ));
        }
        if steps.is_empty() {
            return Err(unsupported(request.offset));
        }
        node.kind = Kind::Route(steps.into(), false);
    }
    match &mut node.kind {
        Kind::Route(steps, _) | Kind::Tuples(steps, _) => {
            let mut at = steps.len();
            while request.level > 0 && at > 0 {
                at -= 1;
                while at > 0 && focused(&steps[at]) && focused(&steps[at - 1]) {
                    at -= 1;
                }
                let s = &mut steps[at];
                seek_step(&mut s.node, &mut s.bindings, request)?;
            }
            mark(node);
            Ok(())
        }
        Kind::Group(base) | Kind::Keep(base, _) | Kind::Filter(base, _) => {
            if matches!(base.kind, Kind::Missing) {
                return Ok(());
            }
            seek_node(base, request)
        }
        Kind::Block(items) => {
            if let Some(last) = items.last_mut() {
                seek_node(last, request)
            } else {
                Ok(())
            }
        }
        Kind::Parent(_) => {
            request.level += 1;
            Ok(())
        }
        _ => Err(unsupported(request.offset)),
    }
}
fn step(kind: Kind, offset: usize) -> Step {
    Step {
        node: Node {
            kind,
            offset,
            depth: 1,
            effects: false,
            tail_call: false,
        },
        predicates: Box::default(),
        bindings: None,
        lookup: true,
        effects: false,
    }
}

// Only ancestry crosses a parenthesized path boundary; user @/# bindings do not.
pub(crate) fn captured(node: &Node) -> bool {
    match &node.kind {
        Kind::Tuples(steps, _) => steps.iter().any(|s| {
            s.bindings.as_ref().is_some_and(|b| !b.ancestors.is_empty()) || captured(&s.node)
        }),
        Kind::Group(base) | Kind::Keep(base, _) | Kind::Filter(base, _) | Kind::Sort(base, _) => {
            captured(base)
        }
        Kind::Block(items) => items.last().is_some_and(captured),
        _ => false,
    }
}

// Ancestry resolution can expand a collapsed path after its parent was parsed.
// Keep normalized tree/call budgets consistent without changing ordinary paths.
fn refresh_depth(node: &mut Node) -> Result<(), Error> {
    let mut before = 0;
    let mut after = 0;
    let mut error = None;
    crate::analysis::children(node, &mut |child| {
        before = before.max(child.depth);
        if error.is_none()
            && let Err(e) = refresh_depth(child)
        {
            error = Some(e);
        }
        after = after.max(child.depth);
    });
    if let Some(error) = error {
        return Err(error);
    }
    node.depth = match &node.kind {
        Kind::Route(steps, _) | Kind::Tuples(steps, _) => {
            steps.len()
                + steps
                    .iter()
                    .map(|step| {
                        step.node.depth
                            + step.predicates.len()
                            + step.predicates.iter().map(|n| n.depth).max().unwrap_or(0)
                    })
                    .max()
                    .unwrap_or(0)
        }
        Kind::Filter(base, predicates) => {
            base.depth + predicates.len() + predicates.iter().map(|n| n.depth).max().unwrap_or(0)
        }
        _ => node.depth + after.saturating_sub(before),
    };
    if node.depth > crate::parse::MAX_DEPTH {
        return Err(Error::new(
            ErrorKind::DepthLimit,
            node.offset,
            "resolved ancestry exceeds 128 expression levels",
        ));
    }
    Ok(())
}
