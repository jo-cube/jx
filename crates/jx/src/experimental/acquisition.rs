//! Schema-directed borrowed captures. Storage is proportional to compiled static
//! paths, never to input rows or unused object names. A schema is a hint: reads
//! outside it navigate validated bytes rather than claiming a missing value.
use super::{Arc, Operation, Program};
use crate::{Error, RawJson};
use std::collections::BTreeMap;

#[derive(Debug, Default)]
pub(crate) struct Schema {
    pub nodes: Vec<Node>,
}
#[derive(Debug)]
pub(crate) struct Node {
    pub children: Box<[(Box<str>, usize)]>,
    pub end: usize,
}
#[derive(Default)]
struct Builder {
    children: BTreeMap<String, Builder>,
}
#[derive(Clone, Copy, Default, Debug)]
pub(crate) struct Span {
    pub start: usize,
    pub end: usize,
}
impl Schema {
    fn build(paths: Vec<Vec<String>>) -> Self {
        let mut root = Builder::default();
        for path in paths {
            let mut at = &mut root;
            for name in path {
                at = at.children.entry(name).or_default();
            }
        }
        fn flatten(b: Builder, nodes: &mut Vec<Node>) -> usize {
            let id = nodes.len();
            nodes.push(Node {
                children: Box::new([]),
                end: 0,
            });
            let children = b
                .children
                .into_iter()
                .map(|(name, child)| (name.into_boxed_str(), flatten(child, nodes)))
                .collect();
            nodes[id] = Node {
                children,
                end: nodes.len(),
            };
            id
        }
        let mut nodes = Vec::new();
        flatten(root, &mut nodes);
        Self { nodes }
    }
    pub fn child(&self, id: usize, name: &str) -> Option<usize> {
        let children = &self.nodes[id].children;
        if let [(key, child)] = children.as_ref() {
            return (key.as_ref() == name).then_some(*child);
        }
        children
            .binary_search_by(|(key, _)| key.as_ref().cmp(name))
            .ok()
            .map(|i| children[i].1)
    }
    pub fn encoded_child(&self, id: usize, name: &str) -> Option<usize> {
        if name.as_bytes().contains(&b'\\') {
            self.nodes[id]
                .children
                .iter()
                .find(|(key, _)| crate::json::string::matches(name, key))
                .map(|(_, i)| *i)
        } else {
            self.child(id, name)
        }
    }
    pub(super) fn covers(&self, base: usize, wanted: &Schema) -> bool {
        fn visit(have: &Schema, at: usize, wanted: &Schema, node: usize) -> bool {
            wanted.nodes[node].children.iter().all(|(name, child)| {
                have.child(at, name)
                    .is_some_and(|next| visit(have, next, wanted, *child))
            })
        }
        visit(self, base, wanted, 0)
    }
    pub fn has_fields(&self) -> bool {
        self.nodes.len() > 1
    }
}

// A join is an acquisition namespace, not a union of all paths that could
// reach it. Each value has one symbolic origin; branches never form products.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Origin {
    pub(super) scope: usize,
    pub(super) path: Vec<String>,
}
#[derive(Clone)]
struct Callee {
    demands: Arc<Dependencies>,
    focus: Option<Origin>,
    captured_result: Option<Origin>,
}
impl Callee {
    fn join(mut self, other: Self) -> Self {
        Arc::make_mut(&mut self.demands).join(&other.demands);
        if self.focus != other.focus {
            self.focus = None;
        }
        if self.captured_result != other.captured_result {
            self.captured_result = None;
        }
        self
    }
}
// A register carries both value provenance and a possible call target through
// nested IR regions. Neither fact is a runtime substitution.
#[derive(Clone, Default)]
struct Port {
    origin: Option<Origin>,
    // Unseen, unknown, or a resolved summary (including joined summaries).
    callee: Option<Option<Callee>>,
}
struct Demands {
    paths: Vec<Vec<Vec<String>>>,
    joins: Vec<(Arc<()>, usize, usize)>,
    local: bool,
    shared: Vec<bool>,
    lexical: BTreeMap<String, Origin>,
    captures: Option<BTreeMap<String, usize>>,
    lexical_functions: BTreeMap<String, Callee>,
    contexts: Option<BTreeMap<String, usize>>,
}
impl Demands {
    // Generated ancestry slots are immutable semantic ports. Track one origin
    // per slot through the same demand analysis as lexical/call ports. Paths
    // beneath arrays remain row demands rather than a per-record graph.
    fn context(
        &mut self,
        region: &super::context::Region,
        root: Option<Origin>,
        focus: Option<Origin>,
        outer: &[Port],
    ) -> Option<Origin> {
        use super::context::Kind;
        match &region.kind {
            Kind::Value(p) => self.collect(p, root, focus, &[], outer),
            Kind::Route { steps, .. } => {
                let mut current = focus;
                for step in steps {
                    let next = self.context(&step.node, root.clone(), current.clone(), outer);
                    if let Some(bindings) = &step.bindings {
                        for name in &bindings.ancestors {
                            if let Some(contexts) = &mut self.contexts {
                                let scope =
                                    *contexts.entry(name.to_string()).or_insert_with(|| {
                                        let scope = self.paths.len();
                                        self.paths.push(Vec::new());
                                        self.shared.push(false);
                                        scope
                                    });
                                self.lexical.insert(
                                    name.to_string(),
                                    Origin {
                                        scope,
                                        path: Vec::new(),
                                    },
                                );
                            } else if let Some(origin) = &current {
                                self.lexical.insert(name.to_string(), origin.clone());
                            } else {
                                self.lexical.remove(name.as_ref());
                            }
                        }
                    }
                    if step.bindings.as_ref().is_none_or(|b| b.focus.is_none()) {
                        current = next;
                    }
                    for p in &step.predicates {
                        self.context(p, root.clone(), current.clone(), outer);
                    }
                }
                current
            }
            Kind::Scope(nodes) => {
                let mut result = None;
                for n in nodes {
                    result = self.context(n, root.clone(), focus.clone(), outer);
                }
                result
            }
            Kind::Keep(n) => self.context(n, root, focus, outer),
            Kind::Filter(n, tests) => {
                let result = self.context(n, root.clone(), focus, outer);
                for p in tests {
                    self.context(p, root.clone(), result.clone(), outer);
                }
                result
            }
            Kind::Order(n, terms) => {
                let result = self.context(n, root.clone(), focus, outer);
                for (p, _) in terms {
                    self.context(p, root.clone(), result.clone(), outer);
                }
                result
            }
        }
    }
    fn collect(
        &mut self,
        p: &Program,
        root: Option<Origin>,
        focus: Option<Origin>,
        arguments: &[Option<Origin>],
        outer: &[Port],
    ) -> Option<Origin> {
        let saved_lexical = self.lexical.clone();
        let saved_functions = self.lexical_functions.clone();
        let mut lexical_scopes = Vec::new();
        let mut definitions = vec![0; p.registers];
        for i in &p.code {
            definitions[i.out] += 1;
        }
        // None is unseen; Some(None) is an unknown target. Joining known
        // targets unions shared demands, never a list/product of callees.
        let mut ports = vec![Port::default(); p.registers];
        let mut joins = vec![None; p.registers];
        let mut used: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        for i in &p.code {
            if let Operation::Invoke(target, args, ..) = &i.kind
                && let Some(Some(definition)) = &ports[*target].callee
            {
                let mut demand = |origin: Option<&Origin>, paths: &[Vec<String>]| {
                    if let Some(origin) = origin {
                        for path in paths {
                            let mut full = origin.path.clone();
                            full.extend(path.iter().cloned());
                            self.paths[origin.scope].push(full);
                        }
                        if self.local && paths.len() > 1 {
                            self.shared[origin.scope] = true;
                        }
                    }
                };
                demand(root.as_ref(), &definition.demands.root);
                demand(definition.focus.as_ref(), &definition.demands.focus);
                for (reg, paths) in args.iter().zip(&definition.demands.arguments) {
                    demand(ports[*reg].origin.as_ref(), paths);
                }
            }
            let function = match &i.kind {
                Operation::Function(d) => {
                    // Definitions capture live frames. These are acquisition
                    // hints at definition time, not substitutions for a future
                    // lexical read. Rebinding/shadowing still reads the actual
                    // value and byte-identity guards select its active port.
                    for (name, paths) in &d.demands.captures {
                        let origin = self.lexical_origin(name);
                        if let Some(origin) = origin {
                            for path in paths {
                                let mut full = origin.path.clone();
                                full.extend(path.iter().cloned());
                                self.paths[origin.scope].push(full);
                            }
                        }
                    }
                    let captured_result = d.demands.result.as_ref().and_then(|r| {
                        let name = d.demands.capture_name(r.scope)?;
                        let mut origin = self.lexical_origin(name)?;
                        origin.path.extend(r.path.iter().cloned());
                        Some(origin)
                    });
                    Some(Callee {
                        demands: Arc::clone(&d.demands),
                        focus: focus.clone(),
                        captured_result,
                    })
                }
                Operation::Copy(r) | Operation::Materialize(r) | Operation::Bind(_, r) => {
                    ports[*r].callee.clone().flatten()
                }
                Operation::Read(name) => self.lexical_functions.get(name.as_ref()).cloned(),
                Operation::Outer(n, _) => outer.get(*n).and_then(|p| p.callee.clone().flatten()),
                _ => None,
            };
            let function = if definitions[i.out] > 1 {
                // Only shared preparation consumes conditional call summaries here;
                // local acquisition remains conservative across branch joins.
                if self.local {
                    None
                } else {
                    match (ports[i.out].callee.take(), function) {
                        (None, incoming) => incoming,
                        (Some(Some(before)), Some(incoming)) => Some(before.join(incoming)),
                        _ => None,
                    }
                }
            } else {
                function
            };
            ports[i.out].callee = Some(function);
            if matches!(i.kind, Operation::Jump(_) | Operation::JumpFalse(..)) {
                used.clear();
            }
            let origin = match i.kind.producer() {
                Operation::Invoke(target, args, ..) => ports[*target]
                    .callee
                    .as_ref()
                    .and_then(Option::as_ref)
                    .and_then(|d| {
                        let result = d.demands.result.as_ref()?;
                        let mut origin = match result.scope {
                            0 => root.clone(),
                            1 => d.focus.clone(),
                            n if n < d.demands.arguments.len() + 2 => {
                                args.get(n - 2).and_then(|r| ports[*r].origin.clone())
                            }
                            _ => return d.captured_result.clone(),
                        }?;
                        origin.path.extend(result.path.iter().cloned());
                        Some(origin)
                    }),
                Operation::EnterScope => {
                    lexical_scopes.push((self.lexical.clone(), self.lexical_functions.clone()));
                    None
                }
                Operation::LeaveScope => {
                    (self.lexical, self.lexical_functions) =
                        lexical_scopes.pop().expect("balanced lexical scope");
                    None
                }
                Operation::Bind(name, r) => {
                    if let Some(function) = ports[*r].callee.clone().flatten() {
                        self.lexical_functions.insert(name.to_string(), function);
                    } else {
                        self.lexical_functions.remove(name.as_ref());
                    }
                    if let Some(origin) = ports[*r].origin.clone() {
                        self.lexical.insert(name.to_string(), origin.clone());
                        Some(origin)
                    } else {
                        self.lexical.remove(name.as_ref());
                        None
                    }
                }
                Operation::Read(name) => self.lexical_origin(name),
                Operation::Focus => focus.clone(),
                Operation::Root => root.clone(),
                Operation::Argument(n) => arguments.get(*n).cloned().flatten(),
                Operation::Outer(n, _) => outer.get(*n).and_then(|p| p.origin.clone()),
                Operation::Copy(n) | Operation::Materialize(n) | Operation::Acquire(n, _) => {
                    ports[*n].origin.clone()
                }
                Operation::Field(base, fields, _, _) => {
                    ports[*base].origin.clone().map(|mut origin: Origin| {
                        for name in &fields.fields {
                            origin.path.push(name.to_string());
                            self.paths[origin.scope].push(origin.path.clone());
                        }
                        // Indexing benefits independent reads co-used in a straight
                        // line region. A single navigation chain, or mutually
                        // exclusive reads, can borrow/navigate without an owner.
                        if !self.local {
                            return origin;
                        }
                        if let Some(previous) = used.get_mut(&origin.scope) {
                            if !previous.starts_with(&origin.path)
                                && !origin.path.starts_with(previous.as_slice())
                            {
                                self.shared[origin.scope] = true;
                            } else if origin.path.len() > previous.len() {
                                *previous = origin.path.clone();
                            }
                        } else {
                            used.insert(origin.scope, origin.path.clone());
                        }
                        origin
                    })
                }
                Operation::Correlate(region) => {
                    let saved = self.lexical.clone();
                    let origin = self.context(region, root.clone(), focus.clone(), &ports);
                    self.lexical = saved;
                    origin
                }
                Operation::CorrelatedGroup(..)
                | Operation::Group(..)
                | Operation::Order(..)
                | Operation::Select(..) => {
                    i.kind.children(&mut |p| {
                        self.collect(p, root.clone(), None, &[], &ports);
                    });
                    None
                }
                Operation::Navigate(n) => {
                    let mut current = focus.clone();
                    for step in &n.steps {
                        current = self.collect(&step.body, root.clone(), current, &[], &ports);
                        // A stage may change cardinality or enumerate an array.
                        // Local acquisition does not infer an object origin
                        // across that general value boundary.
                        if self.local {
                            current = None;
                        }
                        for predicate in &step.predicates {
                            self.collect(
                                &predicate.body,
                                root.clone(),
                                current.clone(),
                                &[],
                                &ports,
                            );
                        }
                    }
                    current
                }
                Operation::Construct(body) => {
                    self.collect(body, root.clone(), focus.clone(), &[], &ports)
                }
                Operation::Path(path) => {
                    self.collect(&path.body, root.clone(), None, &[], &ports);
                    for p in &path.predicates {
                        self.collect(p, root.clone(), None, &[], &ports);
                    }
                    None
                }
                Operation::Each { body, .. } => {
                    self.collect(body, root.clone(), focus.clone(), &[], &ports);
                    None
                }
                Operation::Fold {
                    body,
                    predicate,
                    callback,
                    ..
                } => {
                    self.collect(
                        body,
                        root.clone(),
                        if *callback { focus.clone() } else { None },
                        &[],
                        &ports,
                    );
                    if let Some(p) = predicate {
                        self.collect(p, root.clone(), None, &[], &ports);
                    }
                    None
                }
                _ => None,
            };
            let opaque = match &i.kind {
                Operation::Read(_) | Operation::Chain(..) => true,
                Operation::Invoke(target, ..) => ports[*target]
                    .callee
                    .as_ref()
                    .and_then(Option::as_ref)
                    .is_none_or(|d| d.demands.result.is_none()),
                _ => false,
            };
            ports[i.out].origin = if definitions[i.out] > 1
                || matches!(i.kind, Operation::Reduce(..))
                || (self.local && opaque && origin.is_none())
            {
                if self.local
                    && matches!(
                        i.kind,
                        Operation::Copy(_)
                            | Operation::Acquire(..)
                            | Operation::Read(_)
                            | Operation::Invoke(..)
                            | Operation::Chain(..)
                    )
                {
                    let scope = *joins[i.out].get_or_insert_with(|| {
                        let scope = self.paths.len();
                        self.paths.push(Vec::new());
                        self.shared.push(false);
                        self.joins.push((Arc::clone(&p.key), i.out, scope));
                        scope
                    });
                    Some(Origin {
                        scope,
                        path: Vec::new(),
                    })
                } else {
                    None
                }
            } else {
                origin
            };
        }
        let result = ports[p.result].origin.clone();
        self.lexical = saved_lexical;
        self.lexical_functions = saved_functions;
        result
    }
    fn lexical_origin(&mut self, name: &str) -> Option<Origin> {
        if let Some(origin) = self.lexical.get(name) {
            return Some(origin.clone());
        }
        let captures = self.captures.as_mut()?;
        let scope = *captures.entry(name.to_string()).or_insert_with(|| {
            let scope = self.paths.len();
            self.paths.push(Vec::new());
            self.shared.push(false);
            scope
        });
        Some(Origin {
            scope,
            path: Vec::new(),
        })
    }
}
fn collect(p: &Program, root: bool, focus: bool, argument: bool, paths: &mut Vec<Vec<String>>) {
    let origin = |known: bool| {
        known.then(|| Origin {
            scope: 0,
            path: Vec::new(),
        })
    };
    let mut demands = Demands {
        paths: vec![Vec::new()],
        joins: Vec::new(),
        local: false,
        shared: vec![false],
        lexical: BTreeMap::new(),
        captures: None,
        lexical_functions: BTreeMap::new(),
        contexts: None,
    };
    demands.collect(p, origin(root), origin(focus), &[origin(argument)], &[]);
    paths.append(&mut demands.paths[0]);
}
#[derive(Clone, Debug)]
pub(super) struct Dependencies {
    root: Vec<Vec<String>>,
    focus: Vec<Vec<String>>,
    arguments: Box<[Vec<Vec<String>>]>,
    captures: Vec<(String, Vec<Vec<String>>)>,
    // One proven return port/path; joins never expand into alternatives.
    pub(super) result: Option<Origin>,
}
impl Dependencies {
    fn capture_name(&self, scope: usize) -> Option<&str> {
        self.captures
            .get(scope.checked_sub(self.arguments.len() + 2)?)
            .map(|(name, _)| name.as_str())
    }
    fn join(&mut self, other: &Self) {
        fn union(paths: &mut Vec<Vec<String>>, incoming: &[Vec<String>]) {
            paths.extend_from_slice(incoming);
            paths.sort_unstable();
            paths.dedup();
        }
        // Capture port numbers are local to each summary's argument count.
        // Test them before widening arity; otherwise a capture could appear
        // to be a positional argument after a conditional target join.
        let shared_result = self.result == other.result
            && self.result.as_ref().is_none_or(|r| {
                r.scope < self.arguments.len() + 2 && r.scope < other.arguments.len() + 2
            });
        union(&mut self.root, &other.root);
        union(&mut self.focus, &other.focus);
        if self.arguments.len() < other.arguments.len() {
            let mut args = std::mem::take(&mut self.arguments).into_vec();
            args.resize_with(other.arguments.len(), Vec::new);
            self.arguments = args.into();
        }
        for (paths, incoming) in self.arguments.iter_mut().zip(&other.arguments) {
            union(paths, incoming);
        }
        for (name, incoming) in &other.captures {
            if let Some((_, paths)) = self.captures.iter_mut().find(|(n, _)| n == name) {
                union(paths, incoming);
            } else {
                self.captures.push((name.clone(), incoming.clone()));
            }
        }
        if !shared_result {
            self.result = None;
        }
    }
}
pub(super) fn dependencies(p: &Program, count: usize) -> Arc<Dependencies> {
    let origin = |scope| {
        Some(Origin {
            scope,
            path: Vec::new(),
        })
    };
    let mut demands = Demands {
        paths: vec![Vec::new(); count + 2],
        joins: Vec::new(),
        local: false,
        shared: vec![false; count + 2],
        lexical: BTreeMap::new(),
        captures: Some(BTreeMap::new()),
        lexical_functions: BTreeMap::new(),
        contexts: None,
    };
    let arguments = (2..count + 2).map(origin).collect::<Vec<_>>();
    let result = demands.collect(p, origin(0), origin(1), &arguments, &[]);
    // Summaries are shared facts, not call-body expansion. Canonicalize paths
    // once so repeated invocation cannot multiply identical dependencies.
    for paths in &mut demands.paths {
        paths.sort_unstable();
        paths.dedup();
    }
    let mut captures = demands
        .captures
        .take()
        .unwrap()
        .into_iter()
        .collect::<Vec<_>>();
    captures.sort_unstable_by_key(|(_, scope)| *scope);
    let captures = captures
        .into_iter()
        .map(|(name, scope)| (name, std::mem::take(&mut demands.paths[scope])))
        .collect();
    demands.paths.truncate(count + 2);
    let mut ports = demands.paths.into_iter();
    Arc::new(Dependencies {
        root: ports.next().unwrap(),
        focus: ports.next().unwrap(),
        arguments: ports.collect(),
        result,
        captures,
    })
}
pub(super) fn plan_joins(p: &mut Program) {
    fn contains_join(p: &Program) -> bool {
        p.code.iter().any(|i| {
            if matches!(
                i.kind.producer(),
                Operation::Copy(_)
                    | Operation::Read(_)
                    | Operation::Invoke(..)
                    | Operation::Chain(..)
            ) {
                return true;
            }
            let mut found = false;
            // Definition summaries already own their local acquisition decisions.
            if !matches!(i.kind, Operation::Function(_)) {
                i.kind.children(&mut |p| found |= contains_join(p));
            }
            found
        })
    }
    if !contains_join(p) {
        return;
    }

    let mut demands = Demands {
        paths: vec![Vec::new()],
        joins: Vec::new(),
        local: true,
        shared: vec![false],
        lexical: BTreeMap::new(),
        captures: None,
        lexical_functions: BTreeMap::new(),
        contexts: None,
    };
    demands.collect(p, None, None, &[], &[]);
    let schemas = demands
        .joins
        .into_iter()
        .filter_map(|(key, register, scope)| {
            let paths = std::mem::take(&mut demands.paths[scope]);
            demands.shared[scope].then(|| (key, register, Arc::new(Schema::build(paths))))
        })
        .map(|(key, r, schema)| ((Arc::as_ptr(&key) as usize, r), schema))
        .collect::<BTreeMap<_, _>>();
    fn apply(p: &mut Program, schemas: &BTreeMap<(usize, usize), Arc<Schema>>) {
        let key = Arc::as_ptr(&p.key) as usize;
        let mut pc = vec![0; p.code.len() + 1];
        for (n, i) in p.code.iter().enumerate() {
            let extra = schemas.contains_key(&(key, i.out))
                && matches!(
                    i.kind,
                    Operation::Read(_) | Operation::Invoke(..) | Operation::Chain(..)
                );
            pc[n + 1] = pc[n] + 1 + usize::from(extra);
        }
        let mut code = Vec::with_capacity(*pc.last().unwrap());
        for mut i in std::mem::take(&mut p.code).into_vec() {
            if let Operation::Jump(to) | Operation::JumpFalse(_, to) = &mut i.kind {
                *to = pc[*to];
            }
            let schema = schemas.get(&(key, i.out));
            if !matches!(i.kind, Operation::Function(_)) {
                i.kind.children_mut(&mut |p| apply(p, schemas));
            }

            if let Some(schema) = schema {
                if let Operation::Copy(r) = i.kind {
                    i.kind = Operation::Acquire(r, Arc::clone(schema));
                } else if matches!(
                    i.kind,
                    Operation::Read(_) | Operation::Invoke(..) | Operation::Chain(..)
                ) {
                    let out = i.out;
                    let offset = i.offset;
                    i.out = p.registers;
                    p.registers += 1;
                    let source = i.out;
                    code.push(i);
                    code.push(super::Instruction {
                        out,
                        offset,
                        kind: Operation::Acquire(source, Arc::clone(schema)),
                    });
                    continue;
                }
            }
            code.push(i);
        }
        p.code = code.into();
    }
    apply(p, &schemas);
}
// The existing demand collector gives each demanded ancestor its own input
// port. Only those sparse schemas survive stage/lexical lifetimes; arrays and
// unrelated object names are not indexed.
fn plan_context(region: &mut super::context::Region) {
    use super::context::{Kind, Region};
    let mut demands = Demands {
        paths: vec![Vec::new()],
        joins: Vec::new(),
        local: false,
        shared: vec![false],
        lexical: BTreeMap::new(),
        captures: None,
        lexical_functions: BTreeMap::new(),
        contexts: Some(BTreeMap::new()),
    };
    demands.context(region, None, None, &[]);
    let schemas = demands
        .contexts
        .take()
        .unwrap()
        .into_iter()
        .map(|(name, scope)| (name, std::mem::take(&mut demands.paths[scope])))
        .collect::<BTreeMap<_, _>>();
    fn install(region: &mut Region, schemas: &BTreeMap<String, Vec<Vec<String>>>) {
        match &mut region.kind {
            Kind::Route { steps, .. } => {
                for step in steps {
                    if let Some(bindings) = &step.bindings {
                        let paths: Vec<_> = bindings
                            .ancestors
                            .iter()
                            .filter_map(|n| schemas.get(n.as_ref()))
                            .flatten()
                            .cloned()
                            .collect();
                        step.node.parent_schema =
                            (!paths.is_empty()).then(|| Arc::new(Schema::build(paths)));
                    }
                    install(&mut step.node, schemas);
                    for p in &mut step.predicates {
                        install(p, schemas)
                    }
                }
            }
            Kind::Scope(nodes) => {
                for n in nodes {
                    install(n, schemas)
                }
            }
            Kind::Keep(n) => install(n, schemas),
            Kind::Filter(n, tests) => {
                install(n, schemas);
                for p in tests {
                    install(p, schemas)
                }
            }
            Kind::Order(n, terms) => {
                install(n, schemas);
                for (p, _) in terms {
                    install(p, schemas)
                }
            }
            Kind::Value(_) => {}
        }
    }
    install(region, &schemas);
}
pub(super) fn plan_rows(p: &mut Program) {
    for i in &mut p.code {
        if let Operation::Correlate(region) | Operation::CorrelatedGroup(region, _) =
            i.kind.producer_mut()
        {
            plan_context(region);
        }
        match i.kind.producer_mut() {
            Operation::Correlate(_)
            | Operation::CorrelatedGroup(..)
            | Operation::Group(..)
            | Operation::Order(..)
            | Operation::Navigate(_)
            | Operation::Select(..) => {
                i.kind.children_mut(&mut |body| {
                    let mut paths = Vec::new();
                    collect(body, false, true, false, &mut paths);
                    body.row_schema = Arc::new(Schema::build(paths));
                    super::bindings::bind_rows(body, false, true, false);
                });
            }
            Operation::Path(path) => {
                if path.eager || path.preflight || path.positional {
                    let mut body_paths = Vec::new();
                    collect(&path.body, false, true, true, &mut body_paths);
                    for p in &mut path.predicates {
                        let mut paths = Vec::new();
                        collect(p, false, true, false, &mut paths);
                        p.row_schema = Arc::new(Schema::build(paths));
                        super::bindings::bind_rows(p, false, true, false);
                    }
                    path.body.row_schema = Arc::new(Schema::build(body_paths));
                } else {
                    let mut paths = Vec::new();
                    collect(&path.body, false, true, true, &mut paths);
                    for p in &path.predicates {
                        collect(p, false, true, false, &mut paths);
                    }
                    path.body.row_schema = Arc::new(Schema::build(paths));
                    for p in &mut path.predicates {
                        p.row_schema = Arc::clone(&path.body.row_schema);
                        super::bindings::bind_rows(p, false, true, false);
                    }
                }
                super::bindings::bind_rows(&mut path.body, false, true, true);
            }
            Operation::Each { body, .. } => {
                let mut paths = Vec::new();
                collect(body, false, false, true, &mut paths);
                body.row_schema = Arc::new(Schema::build(paths));
                super::bindings::bind_rows(body, false, false, true);
            }
            Operation::Fold {
                body,
                predicate,
                callback,
                ..
            } => {
                let mut paths = Vec::new();
                collect(body, false, !*callback, true, &mut paths);
                if let Some(p) = predicate {
                    collect(p, false, true, false, &mut paths);
                }
                body.row_schema = Arc::new(Schema::build(paths));
                super::bindings::bind_rows(body, false, !*callback, true);
                if let Some(p) = predicate {
                    p.row_schema = Arc::clone(&body.row_schema);
                    super::bindings::bind_rows(p, false, true, false);
                }
            }
            _ => {}
        }
    }
}

/// A union of static root paths for any number of compiled transformations.
/// Immutable schema and caller-owned span storage are separate from input bytes.
#[derive(Debug, Clone)]
pub struct Acquisition {
    schema: Arc<Schema>,
    spans: Vec<Span>,
    bindings: Arc<[super::bindings::Bound]>,
}
impl Acquisition {
    pub fn new<'e>(programs: impl IntoIterator<Item = &'e Program>) -> Self {
        let programs = programs.into_iter().collect::<Vec<_>>();
        let mut paths = Vec::new();
        for p in &programs {
            collect(p, true, true, false, &mut paths);
        }
        let schema = Arc::new(Schema::build(paths));
        let bindings = super::bindings::bind_tree(&programs, &schema).into();
        Self {
            spans: vec![Span::default(); schema.nodes.len()],
            schema,
            bindings,
        }
    }
    pub fn prepare<'i>(&mut self, input: &'i [u8]) -> Result<Prepared<'i, '_>, Error> {
        let text = crate::json::capture_schema(input, &self.schema, &mut self.spans)?;
        Ok(Prepared {
            text,
            schema: &self.schema,
            spans: &self.spans,
            bindings: &self.bindings,
            owner: Some((&self.schema, &self.bindings)),
        })
    }
    /// Acquire an already validated document without repeating JSON validation.
    /// This still walks token boundaries to capture the shared schema's fields.
    pub fn prepare_validated<'i>(&mut self, raw: RawJson<'i>) -> Prepared<'i, '_> {
        crate::json::capture_schema_validated(raw, &self.schema, &mut self.spans);
        Prepared {
            text: raw.as_str(),
            schema: &self.schema,
            spans: &self.spans,
            bindings: &self.bindings,
            owner: Some((&self.schema, &self.bindings)),
        }
    }
    /// Record spans only. Immutable schema and caller's input/output are excluded.
    pub fn retained_bytes(&self) -> usize {
        self.spans.capacity() * std::mem::size_of::<Span>()
    }
}
#[derive(Debug)]
pub struct Prepared<'i, 's> {
    pub(crate) text: &'i str,
    pub(crate) schema: &'s Schema,
    pub(crate) spans: &'s [Span],
    pub(crate) bindings: &'s [super::bindings::Bound],
    pub(crate) owner: Option<(&'s Arc<Schema>, &'s Arc<[super::bindings::Bound]>)>,
}
impl<'i> Prepared<'i, '_> {
    pub fn raw(&self) -> RawJson<'i> {
        self.at(0).expect("validated root")
    }
    pub(crate) fn at(&self, id: usize) -> Option<RawJson<'i>> {
        let span = self.spans[id];
        (span.end > span.start).then(|| RawJson(&self.text[span.start..span.end]))
    }
}

// A deferred public evaluation may outlive/reuse the caller's acquisition buffer.
// Copy query-sized spans, share the immutable metadata, and keep input bytes borrowed.
#[derive(Debug)]
pub(super) enum Snapshot<'i> {
    Raw(Option<RawJson<'i>>),
    Captured {
        text: &'i str,
        schema: Arc<Schema>,
        spans: Box<[Span]>,
        bindings: Arc<[super::bindings::Bound]>,
    },
}
impl<'i> Snapshot<'i> {
    pub fn prepared(&self) -> Option<Prepared<'i, '_>> {
        match self {
            Self::Captured {
                text,
                schema,
                spans,
                bindings,
            } => Some(Prepared {
                text,
                schema,
                spans,
                bindings,
                owner: Some((schema, bindings)),
            }),
            Self::Raw(_) => None,
        }
    }
}
