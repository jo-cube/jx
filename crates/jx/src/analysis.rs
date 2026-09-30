use crate::{
    builtin::Builtin,
    expression::{Kind, Node, Op},
};
use std::collections::HashSet;

// Resolve only builtins that cannot be rebound anywhere in this expression.
// Conservative across scopes: compile cost is cheap, observable rebinding is not.
pub(crate) fn prepare(root: &mut Node) -> Result<(), crate::Error> {
    let mut invalid = None;
    visit(root, &mut |node| {
        if let Kind::Array(items, _) = &node.kind
            && items.len() > 1
            && items.iter().any(mutates_scope)
        {
            invalid = Some(node.offset);
        }
        if let Kind::Object(pairs) | Kind::Reduce(_, pairs) = &node.kind
            && pairs
                .iter()
                .any(|(key, value)| mutates_scope(key) || mutates_scope(value))
        {
            invalid = Some(node.offset);
        }
    });
    if let Some(offset) = invalid {
        return Err(crate::Error::new(
            crate::ErrorKind::UnsupportedExpression,
            offset,
            "unscoped binding in concurrent constructor members is deferred; use a block",
        ));
    }
    let mut bound = HashSet::new();
    visit(root, &mut |node| match &node.kind {
        Kind::Bind(name, _) => {
            bound.insert(name.to_string());
        }
        Kind::Lambda(params, _) => bound.extend(params.iter().map(|p| p.to_string())),
        Kind::Route(steps, _) | Kind::Tuples(steps, _) => {
            for b in steps.iter().filter_map(|s| s.bindings.as_deref()) {
                bound.extend(b.names().map(str::to_owned));
            }
        }
        _ => {}
    });
    visit(root, &mut |node| {
        if let Kind::Call(target, args) = &mut node.kind {
            let builtin = match &target.kind {
                Kind::BuiltinReference(builtin) => Some(*builtin),
                Kind::Variable(name) if !bound.contains(name.as_ref()) => Builtin::named(name),
                _ => None,
            };
            if let Some(builtin) = builtin {
                node.kind = Kind::Builtin(builtin, std::mem::take(args));
            }
        }
        if let Kind::Binary(Op::Chain, left, right) = &mut node.kind
            && let Kind::Builtin(builtin, args) = &mut right.kind
        {
            let mut arguments = Vec::with_capacity(args.len() + 1);
            arguments.push(std::mem::replace(
                left.as_mut(),
                Node {
                    kind: Kind::Missing,
                    offset: 0,
                    depth: 1,
                    effects: false,
                    tail_call: false,
                },
            ));
            arguments.extend(std::mem::take(args));
            node.kind = Kind::Builtin(*builtin, arguments.into_boxed_slice());
        }
        if let Kind::Variable(name) = &node.kind
            && !bound.contains(name.as_ref())
            && let Some(builtin) = Builtin::named(name)
        {
            node.kind = Kind::BuiltinReference(builtin);
        }
        node.effects = matches!(
            node.kind,
            Kind::Variable(_)
                | Kind::Bind(..)
                | Kind::Lambda(..)
                | Kind::Call(..)
                | Kind::Partial(..)
                | Kind::Binary(Op::Chain, ..)
        );
        if let Kind::Route(steps, _) | Kind::Tuples(steps, _) = &mut node.kind {
            for step in steps {
                step.effects = step.bindings.is_some()
                    || step.node.effects
                    || step.predicates.iter().any(|p| p.effects);
                node.effects |= step.bindings.is_some();
            }
        }
        let mut effects = node.effects;
        children(node, &mut |child| effects |= child.effects);
        node.effects = effects;
    });
    visit(root, &mut |node| {
        if let Kind::Lambda(_, body) = &mut node.kind {
            tail_calls(body);
        }
    });
    Ok(())
}
// The reference returns native sequences through tail calls without normalizing
// them inside the lambda. This affects nested higher-order results even without TCO.
fn tail_calls(node: &mut Node) {
    match &mut node.kind {
        Kind::Call(..) | Kind::Builtin(..) | Kind::Binary(Op::Chain, ..) => node.tail_call = true,
        Kind::Group(body) => tail_calls(body),
        Kind::Block(items) => {
            if let Some(last) = items.last_mut() {
                tail_calls(last);
            }
        }
        Kind::Conditional(_, yes, no) => {
            tail_calls(yes);
            if let Some(no) = no {
                tail_calls(no);
            }
        }
        _ => {}
    }
}
fn visit(node: &mut Node, f: &mut impl FnMut(&mut Node)) {
    children(node, &mut |child| visit(child, f));
    f(node);
}
pub(crate) fn children(node: &mut Node, f: &mut impl FnMut(&mut Node)) {
    match &mut node.kind {
        Kind::Route(steps, _) | Kind::Tuples(steps, _) => {
            for step in steps {
                f(&mut step.node);
                for p in &mut step.predicates {
                    f(p);
                }
            }
        }
        Kind::Filter(base, args) | Kind::Call(base, args) => {
            f(base);
            for n in args {
                f(n);
            }
        }
        Kind::Partial(base, args) => {
            f(base);
            for n in args.iter_mut().flatten() {
                f(n);
            }
        }
        Kind::StaticLookup(_, n)
        | Kind::Keep(n, _)
        | Kind::Group(n)
        | Kind::Negate(n)
        | Kind::Bind(_, n)
        | Kind::Lambda(_, n) => f(n),
        Kind::Binary(_, l, r) | Kind::Range(l, r) => {
            f(l);
            f(r);
        }
        Kind::Conditional(test, yes, no) => {
            f(test);
            f(yes);
            if let Some(no) = no {
                f(no);
            }
        }
        Kind::Array(args, _) | Kind::Builtin(_, args) | Kind::Block(args) => {
            for n in args {
                f(n);
            }
        }
        Kind::Reduce(base, pairs) => {
            f(base);
            for (key, value) in pairs {
                f(key);
                f(value);
            }
        }
        Kind::Sort(base, terms) => {
            f(base);
            for (term, _) in terms {
                f(term);
            }
        }
        Kind::Object(pairs) => {
            for (key, value) in pairs {
                f(key);
                f(value);
            }
        }
        _ => {}
    }
}

fn mutates_scope(node: &Node) -> bool {
    match &node.kind {
        Kind::Bind(..) => true,
        Kind::Group(_) | Kind::Block(_) | Kind::Lambda(..) => false,
        Kind::Binary(_, left, right) | Kind::Range(left, right) => {
            mutates_scope(left) || mutates_scope(right)
        }
        Kind::Conditional(test, yes, no) => {
            mutates_scope(test) || mutates_scope(yes) || no.as_deref().is_some_and(mutates_scope)
        }
        Kind::Filter(base, args) | Kind::Call(base, args) => {
            mutates_scope(base) || args.iter().any(mutates_scope)
        }
        Kind::Partial(base, args) => {
            mutates_scope(base) || args.iter().flatten().any(mutates_scope)
        }
        Kind::Route(steps, _) | Kind::Tuples(steps, _) => steps
            .iter()
            .any(|s| mutates_scope(&s.node) || s.predicates.iter().any(mutates_scope)),
        Kind::Array(args, _) | Kind::Builtin(_, args) => args.iter().any(mutates_scope),
        Kind::Object(pairs) => pairs
            .iter()
            .any(|(k, v)| mutates_scope(k) || mutates_scope(v)),
        Kind::Negate(n) | Kind::Keep(n, _) => mutates_scope(n),
        Kind::Reduce(base, pairs) => {
            mutates_scope(base)
                || pairs
                    .iter()
                    .any(|(k, v)| mutates_scope(k) || mutates_scope(v))
        }
        Kind::Sort(base, terms) => {
            mutates_scope(base) || terms.iter().any(|(n, _)| mutates_scope(n))
        }
        _ => false,
    }
}

// Scoped sorting in the pinned reference loses the tuple marker until the next
// map. Keep that host-internal representation out of the value model explicitly.
pub(crate) fn check_composition(root: &mut Node) -> Result<(), crate::Error> {
    fn literal(node: &Node) -> bool {
        matches!(
            node.kind,
            Kind::Number(_)
                | Kind::Boolean(_)
                | Kind::Null
                | Kind::String(_)
                | Kind::Missing
                | Kind::Prepared(_)
        )
    }
    let mut invalid = None;
    visit(root, &mut |node| {
        let unsupported = match &node.kind {
            Kind::Reduce(base, _) | Kind::Sort(base, _) => crate::tuple::ends_sorted(base),
            Kind::Filter(base, predicates) => {
                crate::tuple::ends_sorted(base) && predicates.iter().any(|p| !literal(p))
            }
            Kind::Tuples(steps, _) => steps.iter().any(|step| {
                crate::tuple::ends_sorted(&step.node) && step.predicates.iter().any(|p| !literal(p))
            }),
            _ => false,
        };
        if unsupported {
            invalid = Some(node.offset);
        }
    });
    if let Some(offset) = invalid {
        Err(crate::Error::new(
            crate::ErrorKind::UnsupportedExpression,
            offset,
            "context operations immediately after tuple sorting are deferred; insert .$ first",
        ))
    } else {
        Ok(())
    }
}
