use super::*;
use crate::builtin::library::Library;
use crate::expression::{Kind, Node};

type Environment<'e> = Vec<(&'e str, usize)>;
struct Lower<'e> {
    code: Vec<Instruction>,
    registers: usize,
    env: Environment<'e>,
    fields: Vec<(usize, Vec<Box<str>>, bool, usize)>,
    available: Vec<usize>,
    deferred: Vec<bool>,
    scoped: bool,
    lexical: bool,
    wrapped_focus: Option<bool>,
    constructor_body: bool,
    epoch: usize,
}
pub(super) fn compile(
    node: &Node,
    external: bool,
    wrapped_focus: Option<bool>,
) -> Result<Program, Unsupported> {
    let mut lower = Lower::new();
    lower.scoped = external || node.clock || needs_scope(node);
    lower.wrapped_focus = wrapped_focus;
    lower.emit(Operation::Focus, node.offset);
    lower.emit(Operation::Root, node.offset);
    let result = lower.node(node)?;
    let mut program = lower.finish(result);
    program.deferred = streamable(node);
    program.deferred_options = streamable_options(node);
    program.offset = node.offset;
    program.clock = node.clock;
    acquisition::plan_joins(&mut program);
    prepare(&mut program);
    Ok(program)
}
pub(super) fn context_body(node: &Node) -> Result<Program, Unsupported> {
    let mut lower = Lower::new();
    lower.scoped = true;
    lower.emit(Operation::Focus, node.offset);
    lower.emit(Operation::Root, node.offset);
    let result = lower.node(node)?;
    let mut p = lower.finish(result);
    p.offset = node.offset;
    p.call.depth = node.depth;
    Ok(p)
}
// All semantic rewriting and capture remapping precedes physical preparation.
// Parent rewrites may move/merge children; their executable metadata is built once.
pub(super) fn prepare(p: &mut Program) {
    for i in &mut p.code {
        i.kind.children_mut(&mut prepare);
    }
    acquisition::plan_rows(p);
    for i in &mut p.code {
        if let Operation::Navigate(n) = i.kind.producer_mut() {
            n.region = region::plan(n);
        }
    }
    p.storage = registers::layout(p);
    p.scalar = scalar::layout(p);
    p.atomic_loads = optimize::load_transfers(p, p.scalar.as_ref().and_then(|s| s.gate));
    p.delivery = consume::plan(p);
    p.argument_mask = optimize::argument_mask(p);
    p.workspace = p.storage.count;
    for i in &p.code {
        if let Operation::Construct(body) = &i.kind {
            p.workspace = p.workspace.max(body.workspace);
        }
    }
}
impl<'e> Lower<'e> {
    fn new() -> Self {
        Self {
            code: Vec::new(),
            registers: 0,
            env: Vec::new(),
            fields: Vec::new(),
            available: Vec::new(),
            deferred: Vec::new(),
            scoped: false,
            lexical: false,
            wrapped_focus: None,
            constructor_body: false,
            epoch: 0,
        }
    }
    fn finish(self, result: usize) -> Program {
        let mut program = Program {
            code: self.code.into(),
            result,
            registers: self.registers,
            input_template: None,
            row_schema: Arc::new(acquisition::Schema::default()),
            row_reads: Box::new([]),
            key: Arc::new(()),
            scalar: None,
            storage: Default::default(),
            delivery: consume::Delivery::Value,
            argument_mask: 0,
            atomic_loads: Box::new([]),
            workspace: 0,
            scoped: self.scoped,
            clock: false,
            call: Default::default(),
            writes_scope: false,
            deferred: false,
            deferred_options: false,
            constants: Box::new([]),
            bindings: Box::new([]),
            offset: 0,
        };
        optimize::run(&mut program);
        optimize::row_dataflow(&mut program);
        program.writes_scope = optimize::writes_scope(&program);
        program
    }
    fn emit_path(&mut self, path: selection::Selection, node: &Node) -> usize {
        let writes = path.body.writes_scope || path.predicates.iter().any(|p| p.writes_scope);
        let result = self.emit(Operation::Path(Box::new(path)), node.offset);
        if writes {
            self.invalidate();
        }
        if node.effects {
            self.emit(Operation::Materialize(result), node.offset)
        } else {
            result
        }
    }
    fn register(&mut self) -> usize {
        let out = self.registers;
        self.registers += 1;
        self.deferred.push(false);
        out
    }
    fn emit(&mut self, kind: Operation, offset: usize) -> usize {
        let reusable = !matches!(&kind, Operation::Builtin(Builtin::Library(lib), ..) if lib.calls_functions())
            && !matches!(
                &kind,
                Operation::Builtin(
                    Builtin::Library(
                        Library::Clone
                            | Library::Append
                            | Library::Reverse
                            | Library::Distinct
                            | Library::Keys
                            | Library::Spread
                            | Library::Merge
                            | Library::Sort
                            | Library::Zip
                            | Library::Split
                            | Library::Error
                            | Library::Assert
                    ),
                    _,
                    _
                )
            )
            && matches!(
                &kind,
                Operation::Builtin(Builtin::Library(_), _, _)
                    | Operation::Binary(_, _, _)
                    | Operation::Negate(_)
                    | Operation::Truth(_)
                    | Operation::Literal(
                        Literal::Number(_)
                            | Literal::Boolean(_)
                            | Literal::Null
                            | Literal::Missing
                            | Literal::String(_)
                    )
            );
        if reusable {
            for &index in &self.available {
                if same(&self.code[index].kind, &kind) {
                    return self.code[index].out;
                }
            }
        }
        let out = self.register();
        self.deferred[out] = matches!(
            kind,
            Operation::Path(_)
                | Operation::Navigate(_)
                | Operation::Select(..)
                | Operation::Range(..)
                | Operation::Traverse(_)
        );
        if reusable {
            self.available.push(self.code.len());
        }
        self.code.push(Instruction { out, kind, offset });
        out
    }
    fn child_focus(
        &self,
        node: &'e Node,
        params: &'e [Box<str>],
        wrapped_focus: Option<bool>,
    ) -> Result<Program, Unsupported> {
        self.child_scope(node, params, wrapped_focus, self.scoped)
    }
    fn child_scope(
        &self,
        node: &'e Node,
        params: &'e [Box<str>],
        wrapped_focus: Option<bool>,
        scoped: bool,
    ) -> Result<Program, Unsupported> {
        let mut child = Lower::new();
        child.scoped = scoped;
        child.lexical = self.scoped || self.lexical;
        child.wrapped_focus = wrapped_focus;
        child.emit(Operation::Focus, node.offset);
        child.emit(Operation::Root, node.offset);
        let kinds = if self.env.is_empty() {
            Vec::new()
        } else {
            facts::infer(&self.code, self.registers)
        };
        for &(name, reg) in self.env.iter().filter(|_| !scoped) {
            let out = child.emit(Operation::Outer(reg, kinds[reg]), node.offset);
            child.remember(name, out);
        }
        for (i, name) in params.iter().enumerate().filter(|_| !scoped) {
            let reg = child.emit(Operation::Argument(i), node.offset);
            child.remember(name, reg);
        }
        let result = child.node(node)?;
        let mut program = child.finish(result);
        program.offset = node.offset;
        Ok(program)
    }
    fn field(&mut self, base: usize, names: &[Box<str>], focus: bool, offset: usize) -> usize {
        if names.is_empty() {
            return base;
        }
        if let Some((_, _, _, reg)) = self
            .fields
            .iter()
            .find(|(b, n, f, _)| *b == base && n == names && *f == focus)
        {
            return *reg;
        }
        let reg = self.emit(
            Operation::Field(
                base,
                Box::new(crate::expression::Path {
                    fields: names.into(),
                    rooted: false,
                }),
                focus,
                FieldMode::Value,
            ),
            offset,
        );
        self.fields.push((base, names.into(), focus, reg));
        reg
    }
    fn focus(&mut self, root: bool, offset: usize) -> usize {
        let found = self.code.iter().find(|i| {
            if root {
                matches!(i.kind, Operation::Root)
            } else {
                matches!(i.kind, Operation::Focus)
            }
        });
        if let Some(i) = found {
            i.out
        } else {
            self.emit(
                if root {
                    Operation::Root
                } else {
                    Operation::Focus
                },
                offset,
            )
        }
    }
    fn callback(&self, node: &'e Node) -> Result<Program, Unsupported> {
        let Kind::Lambda(d) = &node.kind else {
            return Err(unsupported(node, "immediate lambda required"));
        };
        if d.signature.is_some() || d.params.len() > 3 {
            return Err(unsupported(node, "callback signature/arity"));
        }
        let mut body = self.child_scope(&d.body, &d.params, self.wrapped_focus, false)?;
        body.call = call::Contract {
            depth: d.body.depth,
            tail: d.tail,
        };
        Ok(body)
    }
    fn predicate(&self, node: &'e Node) -> Result<Program, Unsupported> {
        let mut p = self.child_focus(node, &[], Some(false))?;
        p.offset = node.offset;
        Ok(p)
    }
    fn pairs(&self, pairs: &'e [(Node, Node)]) -> Result<Box<[(Program, Program)]>, Unsupported> {
        pairs
            .iter()
            .map(|(k, v)| {
                Ok((
                    self.child_focus(k, &[], Some(false))?,
                    self.child_focus(v, &[], Some(false))?,
                ))
            })
            .collect()
    }
    fn predicates(
        &self,
        predicates: &'e [Node],
    ) -> Result<Box<[navigation::Predicate]>, Unsupported> {
        predicates
            .iter()
            .map(|node| {
                Ok(navigation::Predicate {
                    body: self.predicate(node)?,
                    position: match node.kind {
                        Kind::Number(n) => Some(n),
                        _ => None,
                    },
                    effects: node.effects,
                })
            })
            .collect()
    }
    fn selection(
        &self,
        base: usize,
        predicates: &[Node],
        offset: usize,
    ) -> Result<selection::Selection, Unsupported> {
        let mut body = Lower::new();
        let focus = body.focus(false, offset);
        self.stage(base, body.finish(focus), false, predicates)
    }
    fn stage(
        &self,
        base: usize,
        body: Program,
        mapping: bool,
        predicates: &[Node],
    ) -> Result<selection::Selection, Unsupported> {
        let programs = predicates
            .iter()
            .map(|p| self.predicate(p))
            .collect::<Result<Vec<_>, _>>()?;
        for (node, program) in predicates.iter().zip(&programs).skip(1) {
            if !facts::truth_predicate(program) {
                return Err(unsupported(
                    node,
                    "positional stage requires retained candidate context",
                ));
            }
        }
        Ok(selection::Selection {
            source: base,
            body: Box::new(body),
            mapping,
            candidate: programs.first().is_some_and(|p| !facts::truth_predicate(p)),
            positional: predicates
                .first()
                .is_some_and(|p| matches!(p.kind, Kind::Number(_))),
            preflight: predicates.iter().any(|p| p.effects),
            eager: false,
            predicates: programs.into(),
        })
    }
    fn route_source(&self, step: &crate::expression::Step) -> bool {
        match &step.node.kind {
            Kind::Variable(_) => true,
            Kind::Path(path) if path.rooted && path.fields.is_empty() => true,
            Kind::Path(_) => step.lookup && self.wrapped_focus == Some(true),
            _ => false,
        }
    }
    fn projection(&mut self, node: &'e Node) -> Result<selection::Selection, Unsupported> {
        if let Kind::Route(steps, false) = &node.kind
            && steps.len() == 2
            && self.route_source(&steps[0])
            && steps.iter().all(|s| s.bindings.is_none())
            && steps[1].predicates.is_empty()
        {
            let base = self.node(&steps[0].node)?;
            self.stage(
                base,
                self.child_focus(&steps[1].node, &[], Some(false))?,
                true,
                &steps[0].predicates,
            )
        } else {
            Err(unsupported(node, "sequence producer"))
        }
    }
    fn remember(&mut self, name: &'e str, reg: usize) {
        self.env.retain(|(n, _)| *n != name);
        self.env.push((name, reg));
    }
    fn merge_env(&mut self, incoming: &Environment<'e>) {
        self.env.retain(|value| incoming.contains(value));
    }
    fn fence(&mut self) {
        self.fields.clear();
        self.available.clear();
    }
    fn call_boundary(&mut self, target: usize) {
        if call::writes_caller(&self.code, target) {
            self.invalidate();
        } else {
            self.fence();
        }
    }
    fn invalidate(&mut self) {
        if self.scoped {
            self.env.clear();
            self.epoch += 1;
        }
        self.fence();
    }
    fn node(&mut self, node: &'e Node) -> Result<usize, Unsupported> {
        self.node_in(node, false)
    }
    fn node_in(&mut self, node: &'e Node, tail: bool) -> Result<usize, Unsupported> {
        let offset = node.offset;
        if crate::tuple::active(node) && !matches!(node.kind, Kind::Keep(..)) {
            let region = context::Region::compile(node)?;
            let result = self.emit(Operation::Correlate(Box::new(region)), offset);
            self.invalidate();
            return Ok(result);
        }
        let kind = match &node.kind {
            Kind::Parent(name) => Operation::Read(name.clone()),
            Kind::Path(path) => {
                // '$' is current focus; '$$' is represented by a lexical variable.
                let reg = self.focus(false, offset);
                if path.rooted && !path.fields.is_empty() {
                    return Ok(self.emit(
                        Operation::Field(reg, Box::new(path.clone()), true, FieldMode::Value),
                        offset,
                    ));
                }
                return Ok(self.field(reg, &path.fields, true, offset));
            }
            Kind::Variable(name)
                if name.as_ref() == "$" && !self.env.iter().any(|(n, _)| *n == "$") =>
            {
                let reg = self.emit(Operation::Root, offset);
                self.remember(name, reg);
                return Ok(reg);
            }
            Kind::Variable(name) if self.scoped => {
                if let Some((_, reg)) = self.env.iter().rev().find(|(n, _)| *n == name.as_ref()) {
                    return Ok(*reg);
                }
                let reg = self.emit(Operation::Read(name.clone()), offset);
                // Bound cells are stable until this scope writes them. An
                // unbound builtin fallback creates a fresh function value;
                // preserve that public host-observable allocation identity.
                if Builtin::named(name).is_none() {
                    self.remember(name, reg);
                }
                return Ok(reg);
            }
            Kind::Variable(name) => {
                return self
                    .env
                    .iter()
                    .rev()
                    .find(|(n, _)| *n == name.as_ref())
                    .map(|(_, r)| *r)
                    .map_or_else(
                        || {
                            Ok(self.emit(
                                if self.lexical {
                                    Operation::Read(name.clone())
                                } else {
                                    Operation::Literal(Literal::Missing)
                                },
                                offset,
                            ))
                        },
                        Ok,
                    );
            }
            Kind::Group(child) => {
                let env = self.env.clone();
                let epoch = self.epoch;
                if self.scoped {
                    self.emit(Operation::EnterScope, offset);
                }
                let out = self.node_in(child, tail)?;
                if self.scoped {
                    self.emit(Operation::LeaveScope, offset);
                }
                if self.epoch == epoch {
                    self.env = env;
                } else {
                    self.env.clear();
                }
                return Ok(if self.deferred[out] {
                    self.emit(Operation::Materialize(out), offset)
                } else {
                    out
                });
            }
            Kind::Block(items) => {
                let env = self.env.clone();
                let epoch = self.epoch;
                let mut result = self.emit(Operation::Literal(Literal::Missing), offset);
                if self.scoped {
                    self.emit(Operation::EnterScope, offset);
                }
                for (index, n) in items.iter().enumerate() {
                    result = self.node_in(n, tail && index + 1 == items.len())?;
                }
                if self.scoped {
                    self.emit(Operation::LeaveScope, offset);
                }
                if self.epoch == epoch {
                    self.env = env;
                } else {
                    self.env.clear();
                }
                return Ok(if self.deferred[result] {
                    self.emit(Operation::Materialize(result), offset)
                } else {
                    result
                });
            }
            Kind::Bind(name, child) => {
                let reg = self.node(child)?;
                let reg = if self.deferred[reg] {
                    self.emit(Operation::Materialize(reg), offset)
                } else {
                    reg
                };
                if self.scoped {
                    self.fields.clear();
                    self.available.clear();
                    self.emit(Operation::Bind(name.clone(), reg), offset);
                    self.remember(name, reg);
                    return Ok(reg);
                }
                self.remember(name, reg);
                return Ok(reg);
            }
            Kind::Lambda(d) => {
                let mut body = Lower::new();
                body.scoped = true;
                body.wrapped_focus = self.wrapped_focus;
                for (index, name) in d.params.iter().enumerate() {
                    let reg = body.emit(Operation::Argument(index), offset);
                    body.remember(name, reg);
                }
                let result = body.node_in(&d.body, true)?;
                let mut body = body.finish(result);
                acquisition::plan_joins(&mut body);
                body.call = call::Contract {
                    depth: d.body.depth,
                    tail: d.tail,
                };
                let demands = acquisition::dependencies(&body, d.params.len());
                Operation::Function(Box::new(call::Definition {
                    params: d.params.clone(),
                    signature: d.signature.clone(),
                    body,
                    demands,
                }))
            }
            Kind::BuiltinReference(b) => Operation::FunctionReference(*b),
            Kind::Eval(call) => {
                // Resolve the lexical target before evaluating arguments, as for
                // ordinary calls. A builtin target compiles into this same IR.
                let target = self.emit(Operation::Read("eval".into()), offset);
                let args = call
                    .args
                    .iter()
                    .map(|n| self.node(n))
                    .collect::<Result<_, _>>()?;
                self.invalidate();
                Operation::Invoke(
                    target,
                    args,
                    tail && node.tail_call,
                    node.tail_call,
                    if tail && node.tail_call {
                        None
                    } else {
                        call.program.clone()
                    },
                )
            }
            Kind::Call(target, args) => {
                let target = self.node(target)?;
                let args = args
                    .iter()
                    .map(|n| self.node(n))
                    .collect::<Result<_, _>>()?;
                self.call_boundary(target);
                Operation::Invoke(target, args, tail && node.tail_call, node.tail_call, None)
            }
            Kind::Partial(target, args) => {
                let args = args
                    .iter()
                    .map(|n| n.as_ref().map(|n| self.node(n)).transpose())
                    .collect::<Result<_, _>>()?;
                let target = self.node(target)?;
                Operation::Partial(target, args)
            }
            Kind::Binary(Op::Chain, left, right) => {
                let left = self.node(left)?;
                if let Kind::Call(target, args) = &right.kind {
                    let target = self.node(target)?;
                    let mut registers = vec![left];
                    for arg in args {
                        registers.push(self.node(arg)?);
                    }
                    self.call_boundary(target);
                    Operation::Invoke(
                        target,
                        registers.into(),
                        tail && node.tail_call,
                        node.tail_call,
                        None,
                    )
                } else {
                    let right = self.node(right)?;
                    self.call_boundary(right);
                    Operation::Chain(left, right, node.tail_call)
                }
            }
            Kind::Transform(d) => {
                let body = |node: &'e Node| {
                    let mut program = self.child_focus(node, &[], Some(false))?;
                    program.call.depth = node.depth;
                    Ok::<_, Unsupported>(program)
                };
                Operation::Transform(Box::new(crate::transform::Definition {
                    pattern: body(&d.pattern)?,
                    update: body(&d.update)?,
                    delete: d.delete.as_ref().map(body).transpose()?,
                }))
            }
            Kind::Regex(pattern) => Operation::Matcher(pattern.clone()),
            Kind::Formatted(call) if tail && call.function.contextual(call.args.len()) => {
                let target = self.emit(
                    Operation::FunctionReference(Builtin::Library(call.function)),
                    offset,
                );
                let args = call
                    .args
                    .iter()
                    .map(|n| self.node(n))
                    .collect::<Result<_, _>>()?;
                Operation::Invoke(target, args, true, true, None)
            }
            Kind::Formatted(call) => {
                let regs = call
                    .args
                    .iter()
                    .map(|n| self.node(n))
                    .collect::<Result<_, _>>()?;
                Operation::Formatted(call.function, regs, Arc::new(call.prepared.clone()))
            }
            Kind::Number(n) => Operation::Literal(Literal::Number(*n)),
            Kind::Boolean(b) => Operation::Literal(Literal::Boolean(*b)),
            Kind::Null => Operation::Literal(Literal::Null),
            Kind::Missing => Operation::Literal(Literal::Missing),
            Kind::String(s) => Operation::Literal(Literal::String(s.clone())),
            Kind::Prepared(p) => Operation::Literal(Literal::Data(Arc::new(p.data.clone()))),
            Kind::StaticLookup(data, key) => {
                let key = self.node(key)?;
                Operation::Lookup(Arc::new((**data).clone()), key)
            }
            Kind::Negate(child) => Operation::Negate(self.node(child)?),
            Kind::Conditional(test, yes, no) => {
                return self.choose(node, test, yes, no.as_deref(), tail);
            }
            Kind::Binary(op @ (Op::And | Op::Or), lhs, rhs) => {
                let test = self.node(lhs)?;
                let rhs = |lower: &mut Self| {
                    let value = lower.node(rhs)?;
                    Ok(lower.emit(Operation::Truth(value), offset))
                };
                let constant = |lower: &mut Self| {
                    Ok(lower.emit(Operation::Literal(Literal::Boolean(*op == Op::Or)), offset))
                };
                return if *op == Op::And {
                    self.branch(test, offset, rhs, constant)
                } else {
                    self.branch(test, offset, constant, rhs)
                };
            }
            Kind::Binary(op @ (Op::Default | Op::Coalesce), test, no) => {
                return self.fallback(node, *op, test, no, tail);
            }
            Kind::Binary(op, lhs, rhs) if !matches!(op, Op::Chain) => {
                Operation::Binary(*op, self.node(lhs)?, self.node(rhs)?)
            }
            Kind::Object(pairs) if static_members(pairs) && !self.constructor_body => {
                let mut body = Lower::new();
                body.scoped = self.scoped;
                body.constructor_body = true;
                body.wrapped_focus = Some(false);
                body.emit(Operation::Focus, offset);
                body.emit(Operation::Root, offset);
                // This region executes once in its parent's activation. Known
                // lexical versions remain ports even when live cells also exist.
                let kinds = facts::infer(&self.code, self.registers);
                for &(name, reg) in &self.env {
                    let port = body.emit(Operation::Outer(reg, kinds[reg]), offset);
                    body.remember(name, port);
                }
                let result = body.node(node)?;
                let mut body = body.finish(result);
                body.offset = offset;
                if body.writes_scope {
                    self.invalidate();
                }
                Operation::Construct(Box::new(body))
            }
            Kind::Object(pairs) if !static_members(pairs) => {
                let pairs = self.pairs(pairs)?;
                if pairs.iter().any(|(k, v)| k.writes_scope || v.writes_scope) {
                    self.invalidate();
                }
                Operation::Group(None, pairs)
            }
            Kind::Reduce(base, pairs) if crate::tuple::active(base) => {
                let region = context::Region::compile(base)?;
                let pairs = pairs
                    .iter()
                    .map(|(k, v)| Ok((context::Region::value(k)?, context::Region::value(v)?)))
                    .collect::<Result<_, Unsupported>>()?;
                self.invalidate();
                Operation::CorrelatedGroup(Box::new(region), pairs)
            }
            Kind::Reduce(base, pairs) => {
                let base = self.node(base)?;
                let pairs = self.pairs(pairs)?;
                if pairs.iter().any(|(k, v)| k.writes_scope || v.writes_scope) {
                    self.invalidate();
                }
                Operation::Group(Some(base), pairs)
            }
            Kind::Sort(base, terms) => {
                let base = self.node(base)?;
                let terms = terms
                    .iter()
                    .map(|(n, desc)| Ok((self.child_focus(n, &[], Some(false))?, *desc, n.effects)))
                    .collect::<Result<Box<[_]>, Unsupported>>()?;
                if terms.iter().any(|(p, _, _)| p.writes_scope) {
                    self.invalidate();
                }
                Operation::Order(base, terms)
            }
            Kind::Object(pairs) => {
                self.constructor_body = false;
                let mut members: Vec<(Box<str>, usize)> = Vec::new();
                for (key, value) in pairs {
                    let Kind::String(key) = &key.kind else {
                        return Err(unsupported(node, "dynamic constructor key"));
                    };
                    if members.iter().any(|(s, _)| {
                        crate::json::string::equal(&key[1..key.len() - 1], &s[1..s.len() - 1])
                    }) {
                        return Err(unsupported(node, "duplicate constructor key"));
                    }
                    let reg = self.node(value)?;
                    members.push((key.clone(), reg));
                }
                Operation::Object(members.into())
            }
            Kind::Keep(child, path) => {
                if !*path
                    && let Kind::Path(field) = &child.kind
                    && !field.rooted
                    && field.fields.len() == 1
                {
                    let base = self.focus(false, child.offset);
                    return Ok(self.emit(
                        Operation::Field(base, Box::new(field.clone()), true, FieldMode::Retained),
                        offset,
                    ));
                }
                let force = matches!(&child.kind, Kind::Route(..) | Kind::Tuples(..))
                    || (*path && path_value(child));
                let reg = self.node(child)?;
                // An eager copy does not own cardinality. Preserve the native
                // producer's candidates before its normalization, even when the
                // child required an administrative materialization instruction.
                let mut producer = reg;
                if force || matches!(child.kind, Kind::Filter(..)) {
                    while let Some(Instruction {
                        kind: Operation::Materialize(source),
                        ..
                    }) = self.code.iter().rev().find(|i| i.out == producer)
                    {
                        producer = *source;
                    }
                }
                if (force || matches!(child.kind, Kind::Filter(..)))
                    && let Some(i) = self.code.iter_mut().rev().find(|i| i.out == producer)
                    && matches!(
                        i.kind,
                        Operation::Navigate(_)
                            | Operation::Correlate(_)
                            | Operation::Select(..)
                            | Operation::Path(_)
                            | Operation::Traverse(_)
                            | Operation::Range(..)
                    )
                {
                    let producer =
                        std::mem::replace(&mut i.kind, Operation::Literal(Literal::Missing));
                    i.kind = Operation::Collect(Box::new(producer));
                }
                // Postfix retention consumes a native call result before ordinary
                // call normalization; parentheses already crossed that boundary.
                if matches!(
                    child.kind,
                    Kind::Builtin(..) | Kind::Call(..) | Kind::Binary(Op::Chain, ..)
                ) && let Some(i) = self.code.iter_mut().rev().find(|i| i.out == reg)
                {
                    match &mut i.kind {
                        Operation::Builtin(_, _, native)
                        | Operation::Invoke(_, _, _, native, _)
                        | Operation::Chain(_, _, native)
                        | Operation::Each { native, .. } => *native = true,
                        _ => {}
                    }
                }
                Operation::Keep(reg, force)
            }
            Kind::Array(items, preserve) => {
                let mut regs = Vec::new();
                for n in items {
                    regs.push((self.node(n)?, n.is_array_constructor()));
                }
                Operation::Array(regs.into(), *preserve)
            }
            Kind::Builtin(builtin, args)
                if tail && node.tail_call && builtin.contextual(args.len()) =>
            {
                let target = self.emit(Operation::FunctionReference(*builtin), offset);
                let args = args
                    .iter()
                    .map(|n| self.node(n))
                    .collect::<Result<_, _>>()?;
                Operation::Invoke(target, args, true, true, None)
            }
            Kind::Builtin(Builtin::Aggregate(aggregate), args) if args.len() == 1 => {
                let reg = self.node(&args[0])?;
                Operation::Builtin(
                    Builtin::Aggregate(*aggregate),
                    vec![reg].into(),
                    node.tail_call,
                )
            }
            Kind::Builtin(Builtin::Library(lib @ (Library::Map | Library::Filter)), args)
                if args.len() == 2
                    && matches!(&args[1].kind, Kind::Lambda(d)
                        if d.signature.is_none() && d.params.len() <= 3
                            && d.params.iter().all(|p| &**p != "$")
                            && !needs_scope(&d.body)) =>
            {
                let source = self.node(&args[0])?;
                let mut body = self.callback(&args[1])?;
                body.offset = offset;
                Operation::Each {
                    source,
                    body: Box::new(body),
                    kind: if *lib == Library::Map {
                        Each::Map
                    } else {
                        Each::Filter
                    },
                    native: node.tail_call,
                }
            }
            Kind::Route(steps, false)
                if steps.len() == 1
                    && self.route_source(&steps[0])
                    && steps[0].bindings.is_none()
                    && !steps[0].predicates.is_empty()
                    && steps[0]
                        .predicates
                        .iter()
                        .skip(1)
                        .all(|p| self.predicate(p).is_ok_and(|p| facts::truth_predicate(&p))) =>
            {
                let base = self.node(&steps[0].node)?;
                return Ok(
                    self.emit_path(self.selection(base, &steps[0].predicates, offset)?, node)
                );
            }
            Kind::Route(steps, false)
                if steps.len() > 1
                    && self.route_source(&steps[0])
                    && steps
                        .iter()
                        .all(|s| s.bindings.is_none() && s.predicates.is_empty())
                    && steps[1..]
                        .iter()
                        .all(|s| matches!(s.node.kind, Kind::Path(_))) =>
            {
                let mut reg = self.node(&steps[0].node)?;
                for step in &steps[1..] {
                    let Kind::Path(path) = &step.node.kind else {
                        unreachable!()
                    };
                    reg = self.field(reg, &path.fields, false, step.node.offset);
                }
                return Ok(reg);
            }
            Kind::Route(steps, focus) => {
                if !*focus
                    && steps.len() == 2
                    && self.route_source(&steps[0])
                    && steps.iter().all(|s| s.bindings.is_none())
                    && steps[1].predicates.is_empty()
                    && steps[0]
                        .predicates
                        .iter()
                        .skip(1)
                        .all(|p| self.predicate(p).is_ok_and(|p| facts::truth_predicate(&p)))
                {
                    let path = self.projection(node)?;
                    return Ok(self.emit_path(path, node));
                }
                let mut compiled = Vec::new();
                for step in steps {
                    if step.bindings.is_some() {
                        return Err(unsupported(node, "tuple bindings and ancestry context"));
                    }
                    compiled.push(navigation::Step {
                        body: self.child_focus(&step.node, &[], Some(step.lookup))?,
                        predicates: self.predicates(&step.predicates)?,
                        lookup: step.lookup,
                        effects: step.effects,
                    });
                }
                let variable = steps.first().is_some_and(|s| match &s.node.kind {
                    Kind::Variable(_) | Kind::Sort(..) => true,
                    Kind::Prepared(p) => p.data.is_shared(),
                    Kind::Path(p) => p.rooted && p.fields.is_empty(),
                    _ => false,
                });
                if compiled.iter().any(|s| {
                    s.body.writes_scope || s.predicates.iter().any(|p| p.body.writes_scope)
                }) {
                    self.invalidate();
                }
                Operation::Navigate(Box::new(navigation::Navigation {
                    region: None,
                    steps: compiled.into(),
                    array_focus: *focus,
                    variable,
                }))
            }
            Kind::Range(a, b) => Operation::Range(self.node(a)?, self.node(b)?),
            Kind::Wildcard => Operation::Traverse(false),
            Kind::Descendants => Operation::Traverse(true),
            Kind::Filter(base, predicates) if !predicates.is_empty() => {
                let base = self.node(base)?;
                let tests = self.predicates(predicates)?;
                if tests
                    .iter()
                    .skip(1)
                    .any(|p| !facts::truth_predicate(&p.body))
                {
                    if tests.iter().any(|p| p.body.writes_scope) {
                        self.invalidate();
                    }
                    return Ok(self.emit(Operation::Select(base, tests), offset));
                }
                let mut body = Lower::new();
                let focus = body.focus(false, offset);
                let path = selection::Selection {
                    source: base,
                    body: Box::new(body.finish(focus)),
                    mapping: false,
                    candidate: tests
                        .first()
                        .is_some_and(|p| !facts::truth_predicate(&p.body)),
                    positional: tests.first().is_some_and(|p| p.position.is_some()),
                    preflight: tests.iter().any(|p| p.effects),
                    eager: false,
                    predicates: tests.into_vec().into_iter().map(|p| p.body).collect(),
                };
                return Ok(self.emit_path(path, node));
            }
            Kind::Builtin(builtin, args) => {
                let mut regs = Vec::new();
                for n in args {
                    regs.push(self.node(n)?);
                }
                if call::builtin_writes_caller(&self.code, *builtin, &regs) {
                    self.invalidate();
                } else if matches!(builtin, Builtin::Library(l) if l.calls_functions()) {
                    self.fence();
                }
                Operation::Builtin(*builtin, regs.into(), node.tail_call)
            }
            _ => {
                return Err(unsupported(
                    node,
                    match node.kind {
                        Kind::Tuples(..) | Kind::Parent(_) => "tuple bindings and ancestry context",
                        _ => "unsupported semantic operation",
                    },
                ));
            }
        };
        Ok(self.emit(kind, offset))
    }
    fn fallback(
        &mut self,
        node: &'e Node,
        op: Op,
        test: &'e Node,
        no: &'e Node,
        tail: bool,
    ) -> Result<usize, Unsupported> {
        let yes = if op == Op::Coalesce {
            let args = match &test.kind {
                Kind::Builtin(Builtin::Exists, args) | Kind::Call(_, args) => args,
                _ => return Err(unsupported(node, "coalescing test must be a call")),
            };
            let [value] = args.as_ref() else {
                return Err(unsupported(node, "coalescing arity"));
            };
            value
        } else {
            test
        };
        // Proven replay-safe primaries need one value port, not a duplicated
        // branch tree. Shadowable $exists and effects retain ordinary calls.
        if !yes.effects {
            if op == Op::Default {
                let value = self.node(test)?;
                return self.branch(
                    value,
                    node.offset,
                    |_| Ok(value),
                    |lower| lower.node_in(no, tail),
                );
            }
            if matches!(&test.kind, Kind::Builtin(Builtin::Exists, _)) {
                let value = self.node(yes)?;
                let condition = self.emit(
                    Operation::Builtin(Builtin::Exists, vec![value].into(), false),
                    test.offset,
                );
                return self.branch(
                    condition,
                    node.offset,
                    |_| Ok(value),
                    |lower| lower.node_in(no, tail),
                );
            }
        }
        let condition = self.node(test)?;
        // Both operators re-evaluate their selected primary. Common pure CSE is
        // still legal, while effects and shadowable $exists retain normal calls.
        self.branch(
            condition,
            node.offset,
            |lower| lower.node_in(yes, tail),
            |lower| lower.node_in(no, tail),
        )
    }
    fn choose(
        &mut self,
        node: &'e Node,
        test: &'e Node,
        yes: &'e Node,
        no: Option<&'e Node>,
        tail: bool,
    ) -> Result<usize, Unsupported> {
        let test = self.node(test)?;
        self.branch(
            test,
            node.offset,
            |lower| lower.node_in(yes, tail),
            |lower| {
                if let Some(no) = no {
                    lower.node_in(no, tail)
                } else {
                    Ok(lower.emit(Operation::Literal(Literal::Missing), node.offset))
                }
            },
        )
    }
    // One structured join owns dominance, lexical versions and cardinality for
    // conditionals, short-circuit operators and value joins.
    fn branch(
        &mut self,
        test: usize,
        offset: usize,
        yes: impl FnOnce(&mut Self) -> Result<usize, Unsupported>,
        no: impl FnOnce(&mut Self) -> Result<usize, Unsupported>,
    ) -> Result<usize, Unsupported> {
        let env = self.env.clone();
        let result = self.register();
        let guard = self.code.len();
        self.emit(Operation::JumpFalse(test, 0), offset);
        let fields = self.fields.clone();
        let available = self.available.clone();
        let y = yes(self)?;
        self.code.push(Instruction {
            out: result,
            kind: Operation::Copy(y),
            offset,
        });
        let yes_env = self.env.clone();
        let jump = self.code.len();
        self.emit(Operation::Jump(0), offset);
        let no_at = self.code.len();
        self.env = env;
        self.fields = fields.clone();
        self.available = available.clone();
        let n = no(self)?;
        self.code.push(Instruction {
            out: result,
            kind: Operation::Copy(n),
            offset,
        });
        let end = self.code.len();
        self.code[guard].kind = Operation::JumpFalse(test, no_at);
        self.code[jump].kind = Operation::Jump(end);
        self.fields = fields;
        self.available = available;
        self.merge_env(&yes_env);
        self.deferred[result] = self.deferred[y] || self.deferred[n];
        Ok(if self.deferred[result] {
            self.emit(Operation::Materialize(result), offset)
        } else {
            result
        })
    }
}
fn unsupported(node: &Node, reason: &'static str) -> Unsupported {
    Unsupported {
        offset: node.offset,
        reason,
    }
}
// Only deterministic scalar builtins enter this pool. Their arguments are SSA
// values and focus is immutable in one activation. Pool snapshots enforce
// dominance across branches: no error-producing instruction moves or replays.
pub(super) fn same(a: &Operation, b: &Operation) -> bool {
    match (a, b) {
        (Operation::Builtin(a, x, n), Operation::Builtin(b, y, m)) => a == b && x == y && n == m,
        (Operation::Binary(a, x, y), Operation::Binary(b, u, v)) => a == b && x == u && y == v,
        (Operation::Negate(a), Operation::Negate(b))
        | (Operation::Truth(a), Operation::Truth(b)) => a == b,
        (Operation::Literal(a), Operation::Literal(b)) => match (a, b) {
            (Literal::Number(a), Literal::Number(b)) => a.to_bits() == b.to_bits(),
            (Literal::Boolean(a), Literal::Boolean(b)) => a == b,
            (Literal::String(a), Literal::String(b)) => a == b,
            (Literal::Null, Literal::Null) | (Literal::Missing, Literal::Missing) => true,
            _ => false,
        },
        _ => false,
    }
}

// An escaping/callable value requires live lexical cells. Proven immediate map/filter
// callbacks retain their explicit SSA ports; all other calls use the same value ABI.
fn needs_scope(node: &Node) -> bool {
    // (requires retained cells, writes the current lexical frame). Group/block
    // writes are local. A conditional or row-stage write needs live cells; a
    // straight-line local declaration can stay SSA. Visit each child once.
    fn merge(items: impl Iterator<Item = (bool, bool)>) -> (bool, bool) {
        items.fold((false, false), |(a, b), (c, d)| (a || c, b || d))
    }
    fn analyze(node: &Node) -> (bool, bool) {
        match &node.kind {
            Kind::Regex(_) => (false, false),
            Kind::Transform(_) | Kind::Tuples(..) | Kind::Parent(_) => (true, false),
            Kind::Eval(call) => (true, merge(call.args.iter().map(analyze)).1),
            Kind::Formatted(call) => {
                let (retained, writes) = merge(call.args.iter().map(analyze));
                (retained || call.needs_clock(), writes)
            }
            Kind::Builtin(Builtin::Runtime(_), args) => (true, merge(args.iter().map(analyze)).1),
            Kind::Builtin(Builtin::Library(l), args) if l.uses_clock() => {
                (true, merge(args.iter().map(analyze)).1)
            }
            Kind::Lambda(_) | Kind::Call(..) | Kind::Partial(..) | Kind::BuiltinReference(_) => {
                (true, false)
            }
            Kind::Builtin(Builtin::Library(Library::Map | Library::Filter), args)
                if args.len() == 2 =>
            {
                if let Kind::Lambda(d) = &args[1].kind {
                    let (outer, writes) = analyze(&args[0]);
                    (
                        outer
                            || d.signature.is_some()
                            || d.params.len() > 3
                            || d.params.iter().any(|p| &**p == "$")
                            || analyze(&d.body).0,
                        writes,
                    )
                } else {
                    (true, false)
                }
            }
            Kind::Builtin(_, args) | Kind::Array(args, _) => merge(args.iter().map(analyze)),
            Kind::Block(args) => (merge(args.iter().map(analyze)).0, false),
            Kind::Group(child) => (analyze(child).0, false),
            Kind::Bind(name, child) => (name.as_ref() == "$" || analyze(child).0, true),
            Kind::Negate(child) | Kind::Keep(child, _) => analyze(child),
            Kind::Object(pairs) => {
                let (retained, writes) =
                    merge(pairs.iter().flat_map(|(a, b)| [analyze(a), analyze(b)]));
                (retained || writes, writes)
            }
            Kind::Reduce(base, pairs) => {
                let (retained, writes) = merge(
                    std::iter::once(analyze(base))
                        .chain(pairs.iter().flat_map(|(a, b)| [analyze(a), analyze(b)])),
                );
                (retained || writes, writes)
            }
            Kind::Sort(base, terms) => {
                let (retained, writes) = merge(
                    std::iter::once(analyze(base)).chain(terms.iter().map(|(n, _)| analyze(n))),
                );
                (retained || writes, writes)
            }
            Kind::Binary(op, a, b) => {
                let (retained, writes) = merge([analyze(a), analyze(b)].into_iter());
                (
                    retained
                        || (writes && matches!(op, Op::And | Op::Or | Op::Default | Op::Coalesce)),
                    writes,
                )
            }
            Kind::Range(a, b) => merge([analyze(a), analyze(b)].into_iter()),
            Kind::Conditional(a, b, c) => {
                let (retained, writes) = merge(
                    [
                        analyze(a),
                        analyze(b),
                        c.as_deref().map_or((false, false), analyze),
                    ]
                    .into_iter(),
                );
                (retained || writes, writes)
            }
            Kind::Route(steps, _) => {
                let (retained, writes) = merge(steps.iter().map(|s| {
                    let node = analyze(&s.node);
                    let predicates = merge(s.predicates.iter().map(analyze));
                    (node.0 || predicates.0, node.1 || predicates.1)
                }));
                (retained || writes, writes)
            }
            Kind::Filter(base, predicates) => {
                let base = analyze(base);
                let predicates = merge(predicates.iter().map(analyze));
                (
                    base.0 || predicates.0 || predicates.1,
                    base.1 || predicates.1,
                )
            }
            _ => (false, false),
        }
    }
    analyze(node).0
}

// Delivery is a language/API property, independent of kernel selection. Effectful
// paths and lexical blocks complete before external consumption, as in JSONata.
fn streamable(node: &Node) -> bool {
    if node.effects && !matches!(node.kind, Kind::Tuples(..)) {
        return false;
    }
    match &node.kind {
        Kind::Path(p) => !p.fields.is_empty(),
        Kind::Route(..)
        | Kind::Tuples(..)
        | Kind::Filter(..)
        | Kind::Wildcard
        | Kind::Descendants
        | Kind::Range(..) => true,
        _ => false,
    }
}

fn streamable_options(node: &Node) -> bool {
    if node.effects {
        return false;
    }
    match &node.kind {
        Kind::Group(n) => streamable_options(n),
        _ => streamable(node),
    }
}

fn path_value(node: &Node) -> bool {
    match &node.kind {
        Kind::Path(path) => !path.fields.is_empty(),
        Kind::Keep(child, _) => path_value(child),
        Kind::Wildcard | Kind::Sort(..) => true,
        _ => false,
    }
}

fn static_members(pairs: &[(Node, Node)]) -> bool {
    pairs.iter().enumerate().all(|(at, (key, _))| {
        let Kind::String(key) = &key.kind else { return false; };
        !pairs[..at].iter().any(|(other, _)| matches!(&other.kind, Kind::String(other) if crate::json::string::equal(&key[1..key.len()-1], &other[1..other.len()-1])))
    })
}
