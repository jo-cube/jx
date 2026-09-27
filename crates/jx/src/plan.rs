use crate::{
    Value,
    constant::Data,
    evaluate::Operand,
    expression::{Kind, Node, Op, Path},
    sequence::Context,
};
mod lower;
mod object;
mod pipeline;
use lower::Lower;

const SLOTS: usize = 32;

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
    capture: Box<[(Box<str>, u8)]>,
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
    Unsupported,
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
            Self::Unsupported => unreachable!("loads guard unsupported values"),
            Self::Missing => Operand::Missing,
            Self::Number(n) => Operand::One(Value::Number(n)),
            Self::Boolean(b) => Operand::One(Value::Boolean(b)),
        }
    }
}
impl Program {
    fn run<'e, 'i>(&'e self, context: &Context<'e, 'i>) -> Option<Cell> {
        self.execute(context, |slots| slots[usize::from(self.result)])
    }
    // Keep the register frame out of recursive tree evaluation.
    #[inline(never)]
    fn execute<'e, 'i, T>(
        &'e self,
        context: &Context<'e, 'i>,
        finish: impl FnOnce(&[Cell; SLOTS]) -> T,
    ) -> Option<T> {
        let mut slots = [Cell::Missing; SLOTS];
        let captured = if let Value::Raw(raw) = context.value
            && raw.as_bytes()[0] == b'{'
            && !self.capture.is_empty()
        {
            // Populate only compiled demands. Unsupported cells are checked when
            // loaded, so an unselected branch cannot force a fallback or error.
            for (key, value) in raw.members() {
                for (name, slot) in &self.capture {
                    if crate::json::string::matches(key, name) {
                        slots[usize::from(*slot)] =
                            cell(Operand::One(Value::Raw(value))).unwrap_or(Cell::Unsupported);
                    }
                }
            }
            true
        } else {
            false
        };
        let mut at = 0;
        while at < self.instructions.len() {
            slots[at] = match self.instructions[at] {
                Instruction::Load(_) if captured => match slots[at] {
                    Cell::Unsupported => return None,
                    value => value,
                },
                Instruction::Load(path) => cell(
                    self.paths[usize::from(path)]
                        .select_context(context)
                        .operand()
                        .ok()?,
                )?,
                Instruction::Lookup(index) => {
                    let (data, path) = &self.lookups[usize::from(index)];
                    let key = match path.select_context(context).operand().ok()? {
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
    if !node.effects {
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
