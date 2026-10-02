use crate::{
    Value,
    constant::Data,
    evaluate::Operand,
    expression::{Kind, Node, Op, Path},
    json::{Captured, Captures, Demand},
    sequence::Context,
};
mod callback;
pub(crate) use callback::Callback;
mod lower;
#[cfg(feature = "jit")]
mod native;
mod object;
mod pipeline;
use lower::Lower;

const SLOTS: usize = crate::json::CAPTURE_SLOTS;

#[derive(Clone, Debug)]
pub(crate) struct Plan {
    pub source: Box<Node>,
    execution: Execution,
}
#[derive(Clone, Debug)]
enum Execution {
    Scalar(Program),
    Fold(pipeline::Pipeline),
    Object(object::Object),
}
#[derive(Clone, Debug)]
struct Program {
    instructions: Box<[Instruction]>,
    paths: Box<[Path]>,
    lookups: Box<[(Box<Data>, Path)]>,
    result: u8,
    capture: Demand,
    inputs: Box<[Option<u8>]>,
    argument_demands: Box<[(Option<u8>, Demand)]>,
    #[cfg(feature = "jit")]
    native: Option<native::Compiled>,
}
#[derive(Clone, Debug)]
enum Instruction {
    Load(u8),
    Lookup(u8),
    Number(f64),
    Boolean(bool),
    Missing,
    Negate(u8),
    Binary(Op, u8, u8),
    Truth(u8),
    Copy(u8),
    Branch(u8, bool, u8),
    Jump(u8),
    Merge(u8, u8),
}
#[derive(Clone, Copy)]
enum Cell {
    Missing,
    Number(f64),
    Boolean(bool),
}
impl Cell {
    fn number(self) -> Option<Option<f64>> {
        match self {
            Self::Missing => Some(None),
            Self::Number(n) if n.is_finite() => Some(Some(n)),
            _ => None,
        }
    }
    fn truth(self) -> Option<bool> {
        match self {
            Self::Missing => Some(false),
            Self::Number(n) if !n.is_infinite() => Some(n != 0.0 && !n.is_nan()),
            Self::Boolean(b) => Some(b),
            _ => None,
        }
    }
    fn operand<'e, 'i>(self) -> Operand<'e, 'i> {
        match self {
            Self::Missing => Operand::Missing,
            Self::Number(n) => Operand::One(Value::Number(n)),
            Self::Boolean(b) => Operand::One(Value::Boolean(b)),
        }
    }
}
impl Program {
    fn run<'e, 'i>(&'e self, context: &Context<'e, 'i>) -> Option<Cell> {
        self.execute(context, None, |slots| slots[usize::from(self.result)])
    }
    // Keep the register frame out of recursive tree evaluation.
    #[inline(never)]
    fn execute<'e, 'i, T>(
        &'e self,
        context: &Context<'e, 'i>,
        captured: Option<&Captures<'i>>,
        finish: impl FnOnce(&[Cell; SLOTS]) -> T,
    ) -> Option<T> {
        let mut local;
        let captured = if captured.is_some() {
            captured
        } else if let Value::Raw(raw) = context.value
            && !self.capture.is_empty()
        {
            local = Captures::default();
            raw.capture(&self.capture, &mut local);
            Some(&local)
        } else {
            None
        };
        self.dispatch(
            |at, path| {
                if let Some(captured) = captured {
                    return captured_operand(captured.get(at));
                }
                path.select_context(context).operand().ok()
            },
            finish,
        )
    }
    #[inline(never)]
    fn dispatch<'e, 'i, T>(
        &'e self,
        mut load: impl FnMut(usize, &'e Path) -> Option<Operand<'e, 'i>>,
        finish: impl FnOnce(&[Cell; SLOTS]) -> T,
    ) -> Option<T> {
        #[cfg(feature = "jit")]
        if let Some(native) = &self.native
            && let Some(value) = native.run(self, &mut load)
        {
            let mut slots = [Cell::Missing; SLOTS];
            slots[usize::from(self.result)] = value;
            return Some(finish(&slots));
        }
        let mut slots = [Cell::Missing; SLOTS];
        let mut at = 0;
        while at < self.instructions.len() {
            slots[at] = match self.instructions[at] {
                Instruction::Load(index) => cell(load(at, &self.paths[usize::from(index)])?)?,
                Instruction::Lookup(index) => {
                    let (data, path) = &self.lookups[usize::from(index)];
                    let key = match load(at, path)? {
                        Operand::Missing => None,
                        Operand::One(value) => Some(value),
                        Operand::Many(_) => return None,
                    };
                    match crate::lookup::constant_data(data, key, 0).ok()? {
                        None | Some(Data::Missing) => Cell::Missing,
                        Some(Data::Number(n)) => Cell::Number(*n),
                        Some(Data::Boolean(b)) => Cell::Boolean(*b),
                        _ => return None,
                    }
                }
                Instruction::Number(n) => Cell::Number(n),
                Instruction::Boolean(b) => Cell::Boolean(b),
                Instruction::Missing => Cell::Missing,
                Instruction::Negate(a) => slots[usize::from(a)]
                    .number()?
                    .map_or(Cell::Missing, |n| Cell::Number(-n)),
                Instruction::Binary(op, a, b) => {
                    binary(op, slots[usize::from(a)], slots[usize::from(b)])?
                }
                Instruction::Truth(a) => Cell::Boolean(slots[usize::from(a)].truth()?),
                Instruction::Copy(a) => slots[usize::from(a)],
                Instruction::Branch(a, truth, target) => {
                    if slots[usize::from(a)].truth()? == truth {
                        at = usize::from(target);
                        continue;
                    }
                    Cell::Missing
                }
                Instruction::Merge(a, target) => {
                    slots[usize::from(target)] = slots[usize::from(a)];
                    Cell::Missing
                }
                Instruction::Jump(target) => {
                    at = usize::from(target);
                    continue;
                }
            };
            at += 1;
        }
        Some(finish(&slots))
    }
}
fn captured_operand<'e, 'i>(value: Captured<'i>) -> Option<Operand<'e, 'i>> {
    match value {
        Captured::Missing => Some(Operand::Missing),
        Captured::Raw(raw) => Some(Operand::One(Value::Raw(raw))),
        Captured::Deferred => None,
    }
}
fn cell(operand: Operand<'_, '_>) -> Option<Cell> {
    match operand {
        Operand::Missing => Some(Cell::Missing),
        Operand::One(value) => match value.atomic() {
            Value::Undefined => Some(Cell::Missing),
            Value::Number(n) => Some(Cell::Number(n)),
            Value::Boolean(b) => Some(Cell::Boolean(b)),
            _ => None,
        },
        Operand::Many(_) => None,
    }
}
fn binary(op: Op, a: Cell, b: Cell) -> Option<Cell> {
    let a = a.number()?;
    let b = b.number()?;
    Some(match (a, b) {
        (Some(a), Some(b)) => match op {
            Op::Add => Cell::Number(a + b),
            Op::Subtract => Cell::Number(a - b),
            Op::Multiply => Cell::Number(a * b),
            Op::Divide => Cell::Number(a / b),
            Op::Remainder => Cell::Number(a % b),
            Op::Equal => Cell::Boolean(a == b),
            Op::NotEqual => Cell::Boolean(a != b),
            Op::Less => Cell::Boolean(a < b),
            Op::LessEqual => Cell::Boolean(a <= b),
            Op::Greater => Cell::Boolean(a > b),
            Op::GreaterEqual => Cell::Boolean(a >= b),
            _ => unreachable!("lowered numeric operation"),
        },
        _ if matches!(op, Op::Equal | Op::NotEqual) => Cell::Boolean(false),
        _ => Cell::Missing,
    })
}
impl Plan {
    pub(crate) fn evaluate<'e, 'i>(
        &'e self,
        input: &'i [u8],
    ) -> Result<Operand<'e, 'i>, crate::Error> {
        let demand = match &self.execution {
            Execution::Scalar(p) => &p.capture,
            Execution::Object(o) => &o.program.capture,
            Execution::Fold(p) => &p.demand,
        };
        let mut captured = Captures::default();
        let raw = crate::json::capture(input, demand, &mut captured)?;
        let context = Context {
            value: Value::Raw(raw),
            wrapped: true,
            scope: None,
        };
        let result = match &self.execution {
            Execution::Scalar(p) => p
                .execute(&context, Some(&captured), |s| s[usize::from(p.result)])
                .map(Cell::operand),
            Execution::Object(o) if raw.as_bytes()[0] != b'[' => {
                o.run_captured(&context, Some(&captured))
            }
            Execution::Object(_) => None,
            Execution::Fold(p) => p.run_captured(captured.get(0)),
        };
        result.map_or_else(|| self.fallback(&context), Ok)
    }
    #[inline(never)]
    pub(crate) fn fallback<'e, 'i>(
        &'e self,
        context: &Context<'e, 'i>,
    ) -> Result<Operand<'e, 'i>, crate::Error> {
        if context.scope.is_some() || !self.source.effects {
            return self.source.run(context);
        }
        let context = Context {
            value: context.value.clone(),
            wrapped: context.wrapped,
            scope: Some(crate::runtime::Scope::with_random(
                context.value.clone(),
                self.source.clock,
                None,
            )),
        };
        self.source.run(&context)
    }
    // Only pure regions may retry: fallback preserves offsets and error precedence.
    pub fn run<'e, 'i>(&'e self, context: &Context<'e, 'i>) -> Option<Operand<'e, 'i>> {
        match &self.execution {
            Execution::Scalar(program) => program.run(context).map(Cell::operand),
            Execution::Fold(pipeline) => pipeline.run(context),
            Execution::Object(object) => object.run(context),
        }
    }
}

pub(crate) fn prepare(node: &mut Node) {
    if let Some(pipeline) = pipeline::lower_callbacks(node) {
        let source = Box::new(node.clone());
        node.kind = Kind::Plan(Box::new(Plan {
            source,
            execution: Execution::Fold(pipeline),
        }));
        return;
    }
    if let Kind::Lambda(definition) = &mut node.kind {
        definition.plan = callback::lower(definition).map(std::sync::Arc::new);
    }
    if !node.effects && !node.clock {
        let execution = pipeline::lower(node)
            .map(Execution::Fold)
            .or_else(|| object::lower(node).map(Execution::Object))
            .or_else(|| {
                if !matches!(
                    node.kind,
                    Kind::Binary(..) | Kind::Group(_) | Kind::Negate(_) | Kind::Conditional(..)
                ) || !lower::computed(node)
                {
                    return None;
                }
                let mut lower = Lower::default();
                let result = lower.node(node)?;
                (lower.operations >= 3).then(|| Execution::Scalar(lower.finish(result)))
            });
        if let Some(execution) = execution {
            let source = Box::new(node.clone());
            node.kind = Kind::Plan(Box::new(Plan { source, execution }));
            return;
        }
    }
    crate::analysis::children(node, &mut prepare);
}

#[cfg(test)]
mod tests;

#[cfg(feature = "jit")]
pub(crate) fn native_prepare(node: &mut Node) -> crate::NativeStats {
    let mut stats = crate::NativeStats::default();
    native::prepare(node, &mut stats);
    stats
}
