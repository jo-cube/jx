//! Conservative input-use and external-binding demand proofs.
use super::{Operation, Program};
use crate::{
    BindingDemand, InputDependency,
    builtin::Builtin,
    expression::{Kind, Node, Step},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum StoredBindingDemand {
    Unused,
    Fields(Box<[Box<str>]>),
    All,
}

impl StoredBindingDemand {
    pub(super) fn as_demand(&self) -> BindingDemand<'_> {
        match self {
            Self::Unused => BindingDemand::Unused,
            Self::Fields(fields) => BindingDemand::Fields(fields),
            Self::All => BindingDemand::All,
        }
    }
}

pub(super) fn classify_bindings(
    root: &Node,
    bindings: &[Box<str>],
) -> Box<[(Box<str>, StoredBindingDemand)]> {
    if bindings.is_empty() {
        return Box::new([]);
    }
    let mut collector = BindingCollector::new(bindings);
    collector.visit(root);
    collector.finish(bindings)
}

enum Demand {
    Unused,
    Fields(BTreeSet<Box<str>>),
    All,
}

struct BindingCollector {
    demands: BTreeMap<Box<str>, Demand>,
    shadowed: BTreeMap<Box<str>, usize>,
}

impl BindingCollector {
    fn new(bindings: &[Box<str>]) -> Self {
        let demands = bindings
            .iter()
            .map(|name| (name.clone(), Demand::Unused))
            .collect();
        Self {
            demands,
            shadowed: BTreeMap::new(),
        }
    }

    fn finish(mut self, bindings: &[Box<str>]) -> Box<[(Box<str>, StoredBindingDemand)]> {
        bindings
            .iter()
            .map(|name| {
                let demand = match self.demands.remove(name).unwrap_or(Demand::Unused) {
                    Demand::Unused => StoredBindingDemand::Unused,
                    Demand::Fields(fields) if fields.is_empty() => StoredBindingDemand::Unused,
                    Demand::Fields(fields) => {
                        StoredBindingDemand::Fields(fields.into_iter().collect())
                    }
                    Demand::All => StoredBindingDemand::All,
                };
                (name.clone(), demand)
            })
            .collect()
    }

    fn is_live_external(&self, name: &str) -> bool {
        self.demands.contains_key(name) && self.shadowed.get(name).copied().unwrap_or(0) == 0
    }

    fn mark_all(&mut self, name: &str) {
        if self.is_live_external(name)
            && let Some(slot) = self.demands.get_mut(name)
        {
            *slot = Demand::All;
        }
    }

    fn mark_all_live(&mut self) {
        for (name, slot) in &mut self.demands {
            if self.shadowed.get(name).copied().unwrap_or(0) == 0 {
                *slot = Demand::All;
            }
        }
    }

    fn mark_field(&mut self, name: &str, field: &str) {
        if !self.is_live_external(name) {
            return;
        }
        if let Some(slot) = self.demands.get_mut(name) {
            match slot {
                Demand::Unused => {
                    let mut fields = BTreeSet::new();
                    fields.insert(field.into());
                    *slot = Demand::Fields(fields);
                }
                Demand::Fields(fields) => {
                    fields.insert(field.into());
                }
                Demand::All => {}
            }
        }
    }

    fn push_shadow(&mut self, name: &str, undo: &mut Vec<Box<str>>) {
        if self.demands.contains_key(name) {
            *self.shadowed.entry(name.into()).or_insert(0) += 1;
            undo.push(name.into());
        }
    }

    fn pop_shadows(&mut self, undo: Vec<Box<str>>) {
        for name in undo {
            if let Some(count) = self.shadowed.get_mut(&name) {
                *count -= 1;
                if *count == 0 {
                    self.shadowed.remove(&name);
                }
            }
        }
    }

    fn visit(&mut self, node: &Node) {
        match &node.kind {
            Kind::Variable(name) => self.mark_all(name),
            Kind::Route(steps, _) => self.visit_route(steps),
            Kind::Tuples(steps, _) => self.visit_steps(steps),
            Kind::Eval(call) => {
                self.mark_all_live();
                for arg in &call.args {
                    self.visit(arg);
                }
            }
            Kind::Builtin(Builtin::Runtime(crate::dynamic::Builtin::Eval), args) => {
                self.mark_all_live();
                for arg in args {
                    self.visit(arg);
                }
            }
            Kind::Builtin(Builtin::Lookup, args) => {
                if let [obj, key] = args.as_ref()
                    && let Kind::Variable(name) = &obj.kind
                    && self.is_live_external(name)
                    && let Kind::String(raw) = &key.kind
                    && let Some(field) = decode_json_string(raw)
                {
                    self.mark_field(name, &field);
                } else {
                    for arg in args {
                        self.visit(arg);
                    }
                }
            }
            Kind::Builtin(_, args) | Kind::Array(args, _) => {
                for arg in args {
                    self.visit(arg);
                }
            }
            Kind::Formatted(call) => {
                for arg in &call.args {
                    self.visit(arg);
                }
            }
            Kind::Block(nodes) => {
                let mut undo = Vec::new();
                for stmt in nodes {
                    if let Kind::Bind(name, value) = &stmt.kind {
                        self.visit(value);
                        self.push_shadow(name, &mut undo);
                    } else {
                        self.visit(stmt);
                    }
                }
                self.pop_shadows(undo);
            }
            Kind::Bind(_, value) => self.visit(value),
            Kind::Lambda(def) => {
                let mut undo = Vec::new();
                for param in &def.params {
                    self.push_shadow(param, &mut undo);
                }
                self.visit(&def.body);
                self.pop_shadows(undo);
            }
            Kind::Filter(base, preds) => {
                self.visit(base);
                for pred in preds {
                    self.visit(pred);
                }
            }
            Kind::Group(inner) | Kind::Keep(inner, _) | Kind::Negate(inner) => {
                self.visit(inner);
            }
            Kind::Range(a, b) | Kind::Binary(_, a, b) => {
                self.visit(a);
                self.visit(b);
            }
            Kind::Reduce(source, pairs) => {
                self.visit(source);
                for (key, value) in pairs {
                    self.visit(key);
                    self.visit(value);
                }
            }
            Kind::Sort(source, terms) => {
                self.visit(source);
                for (term, _) in terms {
                    self.visit(term);
                }
            }
            Kind::Conditional(cond, then_branch, else_branch) => {
                self.visit(cond);
                self.visit(then_branch);
                if let Some(else_branch) = else_branch {
                    self.visit(else_branch);
                }
            }
            Kind::Call(callee, args) => {
                self.visit(callee);
                for arg in args {
                    self.visit(arg);
                }
            }
            Kind::Partial(callee, args) => {
                self.visit(callee);
                for arg in args.iter().flatten() {
                    self.visit(arg);
                }
            }
            Kind::Object(pairs) => {
                for (key, value) in pairs {
                    self.visit(key);
                    self.visit(value);
                }
            }
            Kind::Transform(def) => {
                self.visit(&def.pattern);
                self.visit(&def.update);
                if let Some(delete) = &def.delete {
                    self.visit(delete);
                }
            }
            Kind::StaticLookup(_, key) => self.visit(key),
            Kind::Path(_)
            | Kind::Prepared(_)
            | Kind::BuiltinReference(_)
            | Kind::Parent(_)
            | Kind::Regex(_)
            | Kind::Wildcard
            | Kind::Descendants
            | Kind::Number(_)
            | Kind::Boolean(_)
            | Kind::Null
            | Kind::String(_)
            | Kind::Missing => {}
        }
    }

    fn visit_route(&mut self, steps: &[Step]) {
        if let [first, second, rest @ ..] = steps
            && let Kind::Variable(name) = &first.node.kind
            && self.is_live_external(name)
            && first.predicates.is_empty()
            && first.bindings.is_none()
            && let Some((field, filters)) = static_head_field(&second.node)
        {
            self.mark_field(name, field);
            let mut undo = Vec::new();
            for filter in filters {
                self.visit(filter);
            }
            if let Some(bindings) = &second.bindings {
                self.shadow_step_bindings(bindings, &mut undo);
            }
            for pred in &second.predicates {
                self.visit(pred);
            }
            self.visit_steps_in_scope(rest, &mut undo);
            self.pop_shadows(undo);
            return;
        }
        self.visit_steps(steps);
    }

    fn visit_steps(&mut self, steps: &[Step]) {
        let mut undo = Vec::new();
        self.visit_steps_in_scope(steps, &mut undo);
        self.pop_shadows(undo);
    }

    fn visit_steps_in_scope(&mut self, steps: &[Step], undo: &mut Vec<Box<str>>) {
        for step in steps {
            self.visit(&step.node);
            if let Some(bindings) = &step.bindings {
                self.shadow_step_bindings(bindings, undo);
            }
            for pred in &step.predicates {
                self.visit(pred);
            }
        }
    }

    fn shadow_step_bindings(
        &mut self,
        bindings: &crate::tuple::Bindings,
        undo: &mut Vec<Box<str>>,
    ) {
        if let Some(focus) = &bindings.focus {
            self.push_shadow(focus, undo);
        }
        if let Some(index) = &bindings.index {
            self.push_shadow(index, undo);
        }
        for (_, index) in &bindings.indices {
            self.push_shadow(index, undo);
        }
    }
}

fn static_head_field(node: &Node) -> Option<(&str, &[Node])> {
    match &node.kind {
        Kind::Path(path) if !path.rooted && !path.fields.is_empty() => Some((&path.fields[0], &[])),
        Kind::Keep(inner, _) => static_head_field(inner),
        Kind::Filter(inner, preds) => {
            if let Kind::Path(path) = &inner.kind
                && !path.rooted
                && !path.fields.is_empty()
            {
                Some((&path.fields[0], preds))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn decode_json_string(raw: &str) -> Option<String> {
    let body = raw.strip_prefix('"')?.strip_suffix('"')?;
    if !body.as_bytes().contains(&b'\\') {
        return Some(body.to_owned());
    }
    char::decode_utf16(crate::json::string::units(body))
        .collect::<Result<String, _>>()
        .ok()
}

pub(super) fn classify(program: &Program) -> InputDependency {
    if independent(program, false) {
        InputDependency::Independent
    } else {
        InputDependency::MayRequireInput
    }
}

fn independent(program: &Program, independent_focus: bool) -> bool {
    program
        .code
        .iter()
        .all(|i| operation(&i.kind, independent_focus))
}

fn operation(op: &Operation, independent_focus: bool) -> bool {
    // If every operation passes, register values and lexical writes in this
    // region depend only on bindings/constants or an independent child focus.
    match op {
        Operation::Focus | Operation::Traverse(_) => independent_focus,
        Operation::Read(name) | Operation::Bind(name, _) => {
            name.as_ref() != "$" && !name.starts_with('\0')
        }
        Operation::EnterScope
        | Operation::LeaveScope
        | Operation::Argument(_)
        | Operation::Outer(..)
        | Operation::Literal(_)
        | Operation::Field(..)
        | Operation::Copy(_)
        | Operation::Acquire(..)
        | Operation::Materialize(_)
        | Operation::Negate(_)
        | Operation::Truth(_)
        | Operation::Accept(_)
        | Operation::Binary(..)
        | Operation::Range(..)
        | Operation::Lookup(..)
        | Operation::Object(_)
        | Operation::Array(..)
        | Operation::Keep(..)
        | Operation::JumpFalse(..)
        | Operation::Jump(_) => true,
        Operation::Builtin(Builtin::Runtime(_), ..) => false,
        Operation::Builtin(Builtin::Library(library), ..) if library.calls_functions() => false,
        Operation::Builtin(builtin, args, _) => {
            independent_focus || !builtin.contextual(args.len())
        }
        Operation::Formatted(function, args, _) => {
            independent_focus || !function.contextual(args.len())
        }
        Operation::Construct(body) => independent(body, independent_focus),
        Operation::Path(path) => {
            independent(&path.body, true) && path.predicates.iter().all(|p| independent(p, true))
        }
        Operation::Navigate(navigation) => {
            // An unwrapped first stage can iterate the enclosing focus even
            // when its body does not read it. Absolute heads avoid that edge.
            (independent_focus || navigation.variable || navigation.array_focus)
                && navigation.steps.iter().enumerate().all(|(index, step)| {
                    independent(&step.body, independent_focus || index > 0)
                        && step.predicates.iter().all(|p| independent(&p.body, true))
                })
        }
        Operation::Select(_, predicates) => predicates.iter().all(|p| independent(&p.body, true)),
        Operation::Group(source, pairs) => {
            (source.is_some() || independent_focus)
                && pairs
                    .iter()
                    .all(|(key, value)| independent(key, true) && independent(value, true))
        }
        Operation::Order(_, terms) => terms.iter().all(|(body, _, _)| independent(body, true)),
        // Optimized callbacks retain the enclosing focus; only arguments come from the collection.
        Operation::Each { body, .. } => independent(body, independent_focus),
        Operation::Reduce(_, producer, _) | Operation::Collect(producer) => {
            operation(producer, independent_focus)
        }
        // Closures, unknown/dynamic calls, correlated contexts and unreviewed
        // operations can observe hidden focus/root state. Never guess here.
        _ => false,
    }
}
