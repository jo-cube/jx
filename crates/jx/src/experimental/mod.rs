//! Opt-in compiled execution foundation. Unsupported semantics are explicit;
//! programs never retry through the tree evaluator. Calls, retained scopes and
//! checked value kernels share the engine's semantic infrastructure.
pub(crate) mod acquisition;
mod bindings;
pub(crate) mod call;
mod consume;
mod context;
pub(crate) use consume::Deferred;
mod embedding;
mod facts;
#[cfg(test)]
mod foundation_tests;
mod frame;
mod grouping;
pub(crate) mod input;
mod iteration;
mod lower;
#[cfg(test)]
mod migration_tests;
mod navigation;
mod optimize;
mod rows;
mod run;
mod scalar;
mod selection;
#[cfg(test)]
mod semantic_tests;
#[cfg(test)]
mod tests;
use crate::{
    Expression, Value,
    builtin::Builtin,
    constant::{Data, Storage},
    expression::{Aggregate, Op},
};
pub use acquisition::{Acquisition, Prepared};
pub use input::BorrowedInput;
use std::{fmt, sync::Arc};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unsupported {
    pub offset: usize,
    pub reason: &'static str,
}
impl fmt::Display for Unsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} at {}", self.reason, self.offset)
    }
}
impl std::error::Error for Unsupported {}

/// Semantic instructions use unbounded logical registers. Bindings become register
/// references; immediate callbacks have explicit argument and capture inputs.
/// This representation is independent of physical JSON acquisition.
#[derive(Clone, Debug)]
pub struct Program {
    code: Box<[Instruction]>,
    result: usize,
    pub(crate) registers: usize,
    row_schema: Arc<acquisition::Schema>,
    row_reads: Box<[Option<bindings::Binding>]>,
    key: Arc<()>,
    scalar: Option<scalar::Layout>,
    delivery: consume::Delivery,
    argument_mask: u8,
    atomic_loads: Box<[bool]>,
    workspace: usize,
    scoped: bool,
    clock: bool,
    pub(crate) call: call::Contract,
    writes_scope: bool,
    deferred: bool,
    deferred_options: bool,
    constants: Box<crate::constant::Bindings>,
    bindings: Box<[Box<str>]>,
    offset: usize,
}
#[derive(Clone, Debug)]
struct Instruction {
    out: usize,
    kind: Operation,
    offset: usize,
}
#[derive(Clone, Debug)]
enum Operation {
    Focus,
    Read(Box<str>),
    Bind(Box<str>, usize),
    EnterScope,
    LeaveScope,
    Function(Box<call::Definition>),
    FunctionReference(Builtin),
    Matcher(Box<crate::matcher::Pattern>),
    Transform(Box<crate::transform::Definition<Program>>),
    Formatted(
        crate::builtin::library::Library,
        Box<[usize]>,
        Arc<crate::format::Prepared>,
    ),
    Invoke(usize, Box<[usize]>, bool, bool),
    Partial(usize, Box<[Option<usize>]>),
    Chain(usize, usize, bool),
    Keep(usize, bool),
    Root,
    Argument(usize),
    Outer(usize, u16),
    Literal(Literal),
    Field(usize, Box<crate::expression::Path>, bool, FieldMode),
    Copy(usize),
    Acquire(usize, Arc<acquisition::Schema>),
    Materialize(usize),
    Negate(usize),
    Truth(usize),
    // Ordered row stage: rejected rows produce missing. Mapper failures after
    // acceptance remain pending while preceding predicates finish other rows.
    Accept(usize),
    Binary(Op, usize, usize),
    Builtin(Builtin, Box<[usize]>, bool),
    Lookup(Arc<Storage>, usize),
    Object(Box<[(Box<str>, usize)]>),
    Array(Box<[(usize, bool)]>, bool),
    JumpFalse(usize, usize),
    Jump(usize),
    Path(Box<selection::Selection>),
    Navigate(Box<navigation::Navigation>),
    Correlate(Box<context::Region>),
    CorrelatedGroup(
        Box<context::Region>,
        Box<[(context::Region, context::Region)]>,
    ),
    Select(usize, Box<[navigation::Predicate]>),
    Range(usize, usize),
    Traverse(bool),
    Construct(Box<Program>),
    Group(Option<usize>, Box<[(Program, Program)]>),
    Order(usize, Box<[(Program, bool, bool)]>),
    Each {
        source: usize,
        body: Box<Program>,
        kind: Each,
        native: bool,
    },
    // A single-use producer feeds a complete consumer without retaining its
    // result graph. Producer errors precede deferred aggregate argument checks.
    Reduce(Aggregate, Box<Operation>, usize),
    Fold {
        source: usize,
        body: Box<Program>,
        predicate: Option<Box<Program>>,
        aggregate: Aggregate,
        callback: bool,
    },
}
// Retained lookup observes field matches before path cardinality normalization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FieldMode {
    Value,
    Retained,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Each {
    Map,
    Filter,
}
#[derive(Clone, Debug)]
enum Literal {
    Missing,
    Null,
    Boolean(bool),
    Number(f64),
    String(Box<str>),
    Data(Arc<Storage>),
}
impl Program {
    pub fn compile(expression: &Expression) -> Result<Self, Unsupported> {
        let mut program = lower::compile(
            &expression.root,
            !expression.constants.is_empty() || !expression.bindings.is_empty(),
            Some(true),
        )?;
        program.constants = expression.constants.clone();
        program.bindings = expression.bindings.clone();
        Ok(program)
    }
    pub(crate) fn compile_dynamic(expression: &Expression) -> Result<Self, crate::Error> {
        let mut program = lower::compile(&expression.root, true, None).map_err(|e| {
            let mut error =
                crate::Error::new(crate::ErrorKind::UnsupportedExpression, e.offset, e.reason);
            error.source = crate::Source::DynamicExpression;
            error
        })?;
        program.call.depth = expression.root.depth;
        Ok(program)
    }
    pub fn layout_sizes() -> (usize, usize) {
        (
            std::mem::size_of::<Instruction>(),
            std::mem::size_of::<run::Cell<'_, '_>>(),
        )
    }
    pub fn instruction_count(&self) -> usize {
        self.code.len()
    }
    pub fn register_count(&self) -> usize {
        self.registers
    }
    /// Evaluate with no input. Focus and root are missing, distinct from JSON null.
    pub fn evaluate_without_input(&self) -> Result<crate::Evaluation<'_, '_>, crate::Error> {
        self.evaluate_validated_with(None, Default::default())
    }
    /// Equal-validation control without constructing capture spans.
    pub fn evaluate_bytes<'e, 'i>(
        &'e self,
        input: &'i [u8],
    ) -> Result<crate::Evaluation<'e, 'i>, crate::Error> {
        self.evaluate_input(run::Input::Raw(Some(crate::validate(input)?)))
    }
    /// Evaluate against a document already validated and acquired for this or
    /// other programs. Values borrow input/program storage, never capture buffers.
    pub fn evaluate<'e, 'i>(
        &'e self,
        document: &Prepared<'i, '_>,
    ) -> Result<crate::Evaluation<'e, 'i>, crate::Error> {
        self.evaluate_input(run::Input::Captured(document))
    }
    /// Already-validated scan control; no repeated grammar validation.
    pub fn evaluate_validated<'e, 'i>(
        &'e self,
        input: crate::RawJson<'i>,
    ) -> Result<crate::Evaluation<'e, 'i>, crate::Error> {
        self.evaluate_input(run::Input::Raw(Some(input)))
    }
    fn evaluate_input<'e, 'i>(
        &'e self,
        input: run::Input<'_, 'i, '_>,
    ) -> Result<crate::Evaluation<'e, 'i>, crate::Error> {
        self.evaluate_input_random(input, None)
    }
    fn evaluate_input_random<'e, 'i>(
        &'e self,
        input: run::Input<'_, 'i, '_>,
        random: Option<&crate::Random>,
    ) -> Result<crate::Evaluation<'e, 'i>, crate::Error> {
        let focus = input.root();
        let scope = self.scope(&input, random);
        if self.deferred {
            return Ok(crate::Evaluation {
                result: crate::evaluate::Results::Execution(Box::new(Deferred::new(
                    self, &input, focus, scope,
                ))),
            });
        }
        let result = self.run(
            run::Activation {
                input: &input,
                focus: &focus,
                wrapped: true,
                args: &[],
                outer: &[],
                calls: Default::default(),
                scope: scope.as_ref(),
            },
            &mut frame::Frame::new(),
        )?;
        Ok(crate::Evaluation {
            result: crate::evaluate::Results::Scalar(result.into_value(&input)),
        })
    }
}

impl Operation {
    fn children(&self, visit: &mut impl FnMut(&Program)) {
        match self {
            Self::Reduce(_, p, _) => p.children(visit),
            Self::Correlate(p) => p.children(visit),
            Self::CorrelatedGroup(p, pairs) => {
                p.children(visit);
                for (k, v) in pairs {
                    k.children(visit);
                    v.children(visit);
                }
            }
            Self::Function(d) => visit(&d.body),
            Self::Transform(d) => {
                visit(&d.pattern);
                visit(&d.update);
                if let Some(delete) = &d.delete {
                    visit(delete);
                }
            }
            Self::Construct(body) => visit(body),
            Self::Path(p) => {
                visit(&p.body);
                for p in &p.predicates {
                    visit(p);
                }
            }
            Self::Navigate(n) => {
                for s in &n.steps {
                    visit(&s.body);
                    for p in &s.predicates {
                        visit(&p.body);
                    }
                }
            }
            Self::Select(_, p) => {
                for p in p {
                    visit(&p.body);
                }
            }
            Self::Group(_, pairs) => {
                for (key, value) in pairs {
                    visit(key);
                    visit(value);
                }
            }
            Self::Order(_, terms) => {
                for (body, _, _) in terms {
                    visit(body);
                }
            }
            Self::Each { body, .. } => visit(body),
            Self::Fold {
                body, predicate, ..
            } => {
                visit(body);
                if let Some(p) = predicate {
                    visit(p);
                }
            }
            _ => {}
        }
    }
    fn children_mut(&mut self, visit: &mut impl FnMut(&mut Program)) {
        match self {
            Self::Reduce(_, p, _) => p.children_mut(visit),
            Self::Correlate(p) => p.children_mut(visit),
            Self::CorrelatedGroup(p, pairs) => {
                p.children_mut(visit);
                for (k, v) in pairs {
                    k.children_mut(visit);
                    v.children_mut(visit);
                }
            }
            Self::Function(d) => visit(&mut d.body),
            Self::Transform(d) => {
                visit(&mut d.pattern);
                visit(&mut d.update);
                if let Some(delete) = &mut d.delete {
                    visit(delete);
                }
            }
            Self::Construct(body) => visit(body),
            Self::Path(p) => {
                visit(&mut p.body);
                for p in &mut p.predicates {
                    visit(p);
                }
            }
            Self::Navigate(n) => {
                for s in &mut n.steps {
                    visit(&mut s.body);
                    for p in &mut s.predicates {
                        visit(&mut p.body);
                    }
                }
            }
            Self::Select(_, p) => {
                for p in p {
                    visit(&mut p.body);
                }
            }
            Self::Group(_, pairs) => {
                for (key, value) in pairs {
                    visit(key);
                    visit(value);
                }
            }
            Self::Order(_, terms) => {
                for (body, _, _) in terms {
                    visit(body);
                }
            }
            Self::Each { body, .. } => visit(body),
            Self::Fold {
                body, predicate, ..
            } => {
                visit(body);
                if let Some(p) = predicate {
                    visit(p);
                }
            }
            _ => {}
        }
    }
    fn producer(&self) -> &Self {
        match self {
            Self::Reduce(_, producer, _) => producer,
            _ => self,
        }
    }
    fn producer_mut(&mut self) -> &mut Self {
        match self {
            Self::Reduce(_, producer, _) => producer,
            _ => self,
        }
    }
}
