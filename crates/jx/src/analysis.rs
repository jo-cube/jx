use crate::{
    builtin::Builtin,
    expression::{Kind, Node, Op},
};
use std::collections::HashSet;

// Resolve only builtins that cannot be rebound anywhere in this expression.
// Conservative across scopes: compile cost is cheap, observable rebinding is not.
// Return whether unresolved reads/dynamic code need the constant environment.
pub(crate) fn prepare(
    root: &mut Node,
    dynamic: bool,
    external: &[Box<str>],
    constants: &crate::constant::Bindings,
) -> Result<bool, crate::Error> {
    let mut invalid = None;
    let mut transform_binding = None;
    visit(root, &mut |node| {
        if let Kind::Transform(definition) = &node.kind
            && (mutates_scope(&definition.pattern) || mutates_scope(&definition.update))
        {
            transform_binding = Some(node.offset);
        }
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
    if let Some(offset) = transform_binding {
        return Err(crate::Error::new(
            crate::ErrorKind::UnsupportedExpression,
            offset,
            "unscoped transform bindings are deferred",
        ));
    }
    if let Some(offset) = invalid {
        return Err(crate::Error::new(
            crate::ErrorKind::UnsupportedExpression,
            offset,
            "unscoped binding in concurrent constructor members is deferred; use a block",
        ));
    }
    let mut bound = HashSet::<String>::new();
    visit(root, &mut |node| match &node.kind {
        Kind::Bind(name, _) => {
            bound.insert(name.to_string());
        }
        Kind::Lambda(d) => bound.extend(d.params.iter().map(|p| p.to_string())),
        Kind::Route(steps, _) | Kind::Tuples(steps, _) => {
            for b in steps.iter().filter_map(|s| s.bindings.as_deref()) {
                bound.extend(b.names().map(str::to_owned));
            }
        }
        _ => {}
    });
    let mut eval = dynamic;
    visit(root, &mut |node| {
        eval |= matches!(&node.kind, Kind::Variable(name) if name.as_ref() == "eval");
    });
    // Scope analysis is deliberately conservative across the whole expression.
    // Dynamic evaluation can read/rebind any external name; transforms can observe
    // container identity. Both retain the immutable lexical environment.
    let mut identity = false;
    visit(root, &mut |node| {
        identity |= matches!(node.kind, Kind::Transform(_) | Kind::Parent(_))
    });
    let mut environment = eval || identity;
    visit(root, &mut |node| {
        if let Kind::Variable(name) = &node.kind
            && let Some((_, value)) = constants.iter().find(|(n, _)| n == name)
        {
            if eval || identity || bound.contains(name.as_ref()) {
                environment = true;
            } else {
                node.kind = Kind::Prepared(Box::new(crate::constant::Prepared {
                    data: crate::constant::Storage::Shared(value.clone()),
                    array_syntax: false,
                }));
            }
        }
    });
    bound.extend(external.iter().map(|name| name.to_string()));
    bound.extend(constants.iter().map(|(name, _)| name.to_string()));
    visit(root, &mut |node| {
        if let Kind::Call(target, args) = &mut node.kind {
            let builtin = match &target.kind {
                Kind::BuiltinReference(builtin) => Some(*builtin),
                Kind::Variable(name)
                    if !bound.contains(name.as_ref())
                        && (!eval || (!dynamic && name.as_ref() == "eval")) =>
                {
                    Builtin::named(name)
                }
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
                    clock: false,
                    tail_call: false,
                },
            ));
            arguments.extend(std::mem::take(args));
            node.kind = Kind::Builtin(*builtin, arguments.into_boxed_slice());
        }
        if let Kind::Variable(name) = &node.kind
            && !bound.contains(name.as_ref())
            && (!eval || (!dynamic && name.as_ref() == "eval"))
            && let Some(builtin) = Builtin::named(name)
        {
            node.kind = Kind::BuiltinReference(builtin);
        }
        node.clock = own_clock(&node.kind);
        node.effects = matches!(
            node.kind,
            Kind::Transform(_)
                | Kind::Variable(_)
                | Kind::Bind(..)
                | Kind::Lambda(..)
                | Kind::Call(..)
                | Kind::Partial(..)
                | Kind::Binary(Op::Chain, ..)
        );
        node.effects |= matches!(
            node.kind,
            Kind::Builtin(Builtin::Runtime(_), _) | Kind::BuiltinReference(Builtin::Runtime(_))
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
        let mut clock = node.clock;
        children(node, &mut |child| {
            effects |= child.effects;
            clock |= child.clock;
        });
        node.effects = effects;
        node.clock = clock;
    });
    visit(root, &mut |node| {
        if let Kind::Lambda(d) = &mut node.kind {
            tail_calls(&mut d.body);
            d.tail = crate::function::has_tail_calls(&d.body);
        }
    });
    Ok(environment)
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
        Kind::Transform(d) => {
            f(&mut d.pattern);
            f(&mut d.update);
            if let Some(delete) = &mut d.delete {
                f(delete);
            }
        }
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
        | Kind::Bind(_, n) => f(n),
        Kind::Lambda(d) => f(&mut d.body),
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
        Kind::Eval(call) => {
            for arg in &mut call.args {
                f(arg);
            }
        }
        Kind::Formatted(call) => {
            for arg in &mut call.args {
                f(arg);
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

pub(crate) fn own_clock(kind: &Kind) -> bool {
    match kind {
        Kind::Eval(call) => call.needs_clock(),
        Kind::Builtin(Builtin::Runtime(crate::dynamic::Builtin::Eval), _)
        | Kind::BuiltinReference(Builtin::Runtime(crate::dynamic::Builtin::Eval)) => true,
        Kind::Formatted(call) => call.needs_clock(),
        Kind::Builtin(Builtin::Library(function), args) => function.clock_call(args),
        Kind::BuiltinReference(Builtin::Library(function)) => function.uses_clock(),
        Kind::Variable(name) => {
            name.as_ref() == "eval"
                || matches!(Builtin::named(name),Some(Builtin::Library(f)) if f.uses_clock())
        }
        _ => false,
    }
}

// Effects control replay; runtime storage is a separate requirement. A prepared
// eval with no lexical reads/writes, calls or clock can execute without an arena.
pub(crate) fn requires_runtime(root: &mut Node) -> bool {
    let mut runtime = false;
    visit(root, &mut |node| {
        runtime |= node.clock
            || matches!(
                node.kind,
                Kind::Variable(_)
                    | Kind::Parent(_)
                    | Kind::Bind(..)
                    | Kind::Lambda(_)
                    | Kind::Transform(_)
                    | Kind::Call(..)
                    | Kind::Partial(..)
                    | Kind::Tuples(..)
                    | Kind::Binary(Op::Chain, ..)
                    | Kind::Builtin(Builtin::Runtime(_), _)
                    | Kind::BuiltinReference(Builtin::Runtime(_))
            );
        if let Kind::Eval(call) = &node.kind {
            runtime |= call.needs_runtime();
        }
        if let Kind::Route(steps, _) = &node.kind {
            runtime |= steps.iter().any(|s| s.bindings.is_some());
        }
    });
    runtime
}
