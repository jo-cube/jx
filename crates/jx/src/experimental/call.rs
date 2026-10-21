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
    pub(super) demands: acquisition::Dependencies,
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
    pub(super) fn scope<'e, 'i>(&'e self, input: &Input<'_, 'i, '_>) -> Option<Scope<'e, 'i>> {
        self.scoped.then(|| {
            Scope::with_random(
                input.root().value(input).unwrap_or(Value::Undefined),
                false,
                None,
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
            outer: &[],
            calls: Default::default(),
            scope: context.scope.as_ref(),
        };
        let mut frame = frame::Frame::new();
        if self.scalar.is_some() {
            return Ok(tail::Outcome::Done(
                self.run(activation, &mut frame)?.into_value(input),
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

// Value-based kernels share the function ABI with the reference engine. A
// program domain must never enter AST-backed definitions or dynamic tree eval,
// including through unbound declared names, host results, partials or callbacks.
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
        FunctionKind::Program { .. }
        | FunctionKind::Host(_)
        | FunctionKind::Matcher(_)
        | FunctionKind::MatchNext(_)
        | FunctionKind::Partial { .. }
        | FunctionKind::Chain(..) => true,
        FunctionKind::Builtin(b) => !matches!(b, Builtin::Runtime(_)),
        FunctionKind::Dynamic(c) => matches!(
            c.definition.as_ref(),
            crate::dynamic::Definition::Program(_)
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
