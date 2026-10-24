//! General calls use the shared value ABI, lexical arena and tail-call loop.
//! Definitions contain IR programs, never executable AST nodes.
use super::{
    run::{Activation, Cell, Input},
    *,
};
use crate::{Error, Value, function::tail, runtime::Scope, sequence::Context};

// Semantic call obligations also apply to immediate callbacks and fused folds.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Contract {
    pub depth: usize,
    pub tail: bool,
}
#[derive(Clone, Copy, Default)]
pub(super) struct Depth {
    calls: usize,
    tree: usize,
}
pub(super) struct Callback<'a, 'e, 'i> {
    scope: Option<&'a Scope<'e, 'i>>,
    parent: Depth,
    next: Option<Depth>,
}
impl<'a, 'e, 'i> Callback<'a, 'e, 'i> {
    pub fn new(scope: Option<&'a Scope<'e, 'i>>, parent: Depth) -> Self {
        Self {
            scope,
            parent,
            next: None,
        }
    }
    pub fn run<T>(
        &mut self,
        body: &Program,
        run: impl FnOnce(Depth) -> Result<T, Error>,
    ) -> Result<T, Error> {
        if let Some(scope) = self.scope {
            if body.call.tail && scope.max_tail_calls() == 0 {
                return Err(Error::new(
                    crate::ErrorKind::EvaluationLimit,
                    body.offset,
                    "tail-call execution exceeds one million iterations",
                ));
            }
            return scope.call(body.offset, body.call.depth, || run(self.parent));
        }
        let next = if let Some(next) = self.next {
            next
        } else {
            crate::runtime::check_call(
                self.parent.calls,
                self.parent.tree,
                body.call.depth,
                64,
                body.offset,
            )?;
            let next = Depth {
                calls: self.parent.calls + 1,
                tree: self.parent.tree + body.call.depth,
            };
            self.next = Some(next);
            next
        };
        run(next)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Definition {
    pub params: Box<[Box<str>]>,
    pub signature: Option<crate::function::Signature>,
    pub body: Program,
    pub(super) demands: Arc<acquisition::Dependencies>,
}
pub(super) enum Exit<'e, 'i> {
    Done,
    Stop(usize, Scopes<'e, 'i>),
    Call(tail::Call<'e, 'i>),
}
// Scope handles own the arena, while closures retain frame indices. This avoids
// reference cycles even for recursion, forward references and mutable captures.
pub(super) struct Scopes<'e, 'i> {
    pub current: Option<Scope<'e, 'i>>,
    parents: Vec<Scope<'e, 'i>>,
}
impl<'e, 'i> Scopes<'e, 'i> {
    pub fn new(scope: Option<&Scope<'e, 'i>>) -> Self {
        Self {
            current: scope.cloned(),
            parents: Vec::new(),
        }
    }
    pub fn enter(&mut self) {
        let parent = self.current.take().expect("lexical scope");
        self.current = Some(parent.child(parent.frame));
        self.parents.push(parent);
    }
    pub fn leave(&mut self) {
        self.current.take().expect("lexical scope").release();
        self.current = self.parents.pop();
    }
}
impl Drop for Scopes<'_, '_> {
    fn drop(&mut self) {
        while !self.parents.is_empty() {
            self.leave();
        }
    }
}
impl Program {
    pub(super) fn scope<'e, 'i>(
        &'e self,
        input: &Input<'_, 'i, '_>,
        random: Option<&crate::Random>,
    ) -> Option<Scope<'e, 'i>> {
        self.scoped.then(|| {
            Scope::with_random(
                input.root().value(input).unwrap_or(Value::Undefined),
                self.clock,
                random,
            )
            .for_program()
            .constants(&self.constants)
        })
    }
    pub(crate) fn run_function<'e, 'i>(
        &'e self,
        context: &Context<'e, 'i>,
        arguments: &[Option<Value<'e, 'i>>],
        prepared: Option<&Input<'_, 'i, '_>>,
    ) -> Result<tail::Outcome<'e, 'i>, Error> {
        let root = context
            .scope
            .as_ref()
            .and_then(|s| s.lookup("$"))
            .and_then(|v| v.as_raw());
        let raw = Input::Raw(root);
        let input = prepared.unwrap_or(&raw);
        let focus = input.restore(Some(context.value.clone()));
        let mut inline = [const { Cell::Missing }; 8];
        let mut heap = Vec::new();
        let args = if arguments.len() <= inline.len() {
            &mut inline[..arguments.len()]
        } else {
            heap.resize(arguments.len(), Cell::Missing);
            heap.as_mut_slice()
        };
        for (cell, value) in args.iter_mut().zip(arguments) {
            *cell = input.restore(value.clone());
        }
        let activation = Activation {
            input,
            focus: &focus,
            wrapped: context.wrapped,
            args,
            outer: Default::default(),
            calls: Default::default(),
            scope: context.scope.as_ref(),
        };
        let mut frame = frame::Frame::new();
        if self.scalar.is_some() {
            // The invocation/region owns capture lifetime for either backend.
            return Ok(tail::Outcome::Done(
                self.run_scalar(activation, &mut frame)?.into_value(input),
            ));
        }
        match self.run_prefix(activation, &mut frame, None)? {
            Exit::Call(call) => Ok(tail::Outcome::Call(call)),
            Exit::Done => Ok(tail::Outcome::Done(
                frame.take_result(self.result).into_value(input),
            )),
            Exit::Stop(..) => unreachable!("function body has no delivery stop"),
        }
    }
}

// Non-tail regions retain their result before retiring temporary captures.
// Dynamic source and transform parts have the same lifetime obligation.
impl Program {
    pub(crate) fn evaluate_region<'e, 'i>(
        &'e self,
        context: &Context<'e, 'i>,
    ) -> Result<Option<Value<'e, 'i>>, Error> {
        context.scope.as_ref().expect("region scope").retained(|| {
            match self.run_function(context, &[], None)? {
                crate::function::tail::Outcome::Done(value) => Ok(value),
                crate::function::tail::Outcome::Call(_) => {
                    unreachable!("region is not a tail body")
                }
            }
        })
    }
}

impl crate::transform::Body for Program {
    fn offset(&self) -> usize {
        self.offset
    }
    fn depth(&self) -> usize {
        self.call.depth
    }
    fn evaluate<'e, 'i>(
        &'e self,
        context: &Context<'e, 'i>,
    ) -> Result<Option<Value<'e, 'i>>, Error> {
        self.evaluate_region(context)
    }
}

// Definitions own child frames, but captured calls may write older frames.
// Reuse their conservative region-write summary; unknown/recursive targets stay
// effectful. This one fact drives lexical ports, callbacks and region summaries.
pub(super) fn writes_caller(code: &[Instruction], target: usize) -> bool {
    fn read_only(op: &Operation) -> bool {
        match op {
            Operation::Function(d) => !d.body.writes_scope,
            Operation::Matcher(_)
            | Operation::Literal(_)
            | Operation::Object(_)
            | Operation::Array(..) => true,
            Operation::FunctionReference(b) => {
                !matches!(b, Builtin::Runtime(crate::dynamic::Builtin::Eval))
            }
            _ => false,
        }
    }
    let mut found = false;
    for i in code.iter().filter(|i| i.out == target) {
        found = true;
        if matches!(i.kind, Operation::Copy(_)) {
            // Lowering emits forward, acyclic register flow. Join effects once
            // in definition order; recursive traversal of alias diamonds would
            // multiply planning work. Unknown/forward values stay effectful.
            let mut effects = vec![None; code.iter().map(|i| i.out).max().map_or(0, |r| r + 1)];
            for i in code {
                let writes = match i.kind {
                    Operation::Copy(r) => effects.get(r).copied().flatten().unwrap_or(true),
                    _ => !read_only(&i.kind),
                };
                effects[i.out] = Some(effects[i.out].unwrap_or(false) || writes);
            }
            return effects.get(target).copied().flatten().unwrap_or(true);
        }
        if !read_only(&i.kind) {
            return true;
        }
    }
    !found
}

pub(super) fn builtin_writes_caller(
    code: &[Instruction],
    builtin: Builtin,
    args: &[usize],
) -> bool {
    match builtin {
        Builtin::Runtime(crate::dynamic::Builtin::Eval) => true,
        Builtin::Library(l) => args
            .iter()
            .enumerate()
            .any(|(i, r)| l.callback_argument(args.len(), i) && writes_caller(code, *r)),
        _ => false,
    }
}

// Value-based kernels share the function ABI with the reference engine. A
// program domain must never enter AST-backed definitions,
// including through unbound declared names, host results, partials or callbacks.
// Dynamic source compilation selects IR before executing any expression body.
pub(crate) fn check(
    function: &crate::Function<'_, '_>,
    context: &Context<'_, '_>,
    offset: usize,
) -> Result<(), Error> {
    use crate::function::FunctionKind;
    if !context.scope.as_ref().is_some_and(Scope::is_program) {
        return Ok(());
    }
    let supported = match &function.kind {
        FunctionKind::ProgramTransform { .. }
        | FunctionKind::Program { .. }
        | FunctionKind::Host(_)
        | FunctionKind::Matcher(_)
        | FunctionKind::MatchNext(_)
        | FunctionKind::Partial { .. }
        | FunctionKind::Chain(..) => true,
        FunctionKind::Builtin(_) => true,
        FunctionKind::Dynamic(c) => matches!(
            c.definition.as_ref(),
            crate::dynamic::Definition::Program(_)
                | crate::dynamic::Definition::ProgramTransform(_)
        ),
        FunctionKind::Lambda { .. } | FunctionKind::Transform { .. } => false,
    };
    if supported {
        Ok(())
    } else {
        Err(Error::new(
            crate::ErrorKind::UnsupportedExpression,
            offset,
            "call target is outside the execution foundation",
        ))
    }
}
