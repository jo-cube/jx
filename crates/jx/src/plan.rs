use crate::{
    Value,
    constant::Data,
    evaluate::Operand,
    expression::{Kind, Node, Op, Path},
    sequence::Context,
};

const SLOTS: usize = 32;

#[derive(Clone, Debug)]
pub(crate) struct Plan {
    pub source: Box<Node>,
    instructions: Box<[Instruction]>,
    paths: Box<[Path]>,
}
#[derive(Clone, Debug)]
enum Instruction {
    Load(u8),
    Number(f64),
    Missing,
    Negate(u8),
    Binary(Op, u8, u8),
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
}

impl Plan {
    // A type/shape miss retries the original pure tree, preserving diagnostic
    // offsets and error precedence. The successful numeric path allocates nothing.
    // Keep the register frame out of recursive tree evaluation.
    #[inline(never)]
    pub fn run<'e, 'i>(&'e self, context: &Context<'e, 'i>) -> Option<Operand<'e, 'i>> {
        let mut slots = [Cell::Missing; SLOTS];
        for (at, instruction) in self.instructions.iter().enumerate() {
            slots[at] = match *instruction {
                Instruction::Load(path) => {
                    let value = self.paths[usize::from(path)]
                        .select_context(context)
                        .operand()
                        .ok()?;
                    value.number(0).ok()?.map_or(Cell::Missing, Cell::Number)
                }
                Instruction::Number(n) => Cell::Number(n),
                Instruction::Missing => Cell::Missing,
                Instruction::Negate(a) => slots[usize::from(a)]
                    .number()?
                    .map_or(Cell::Missing, |n| Cell::Number(-n)),
                Instruction::Binary(op, a, b) => {
                    let a = slots[usize::from(a)].number()?;
                    let b = slots[usize::from(b)].number()?;
                    match (a, b) {
                        (Some(a), Some(b)) => match op {
                            Op::Add => Cell::Number(a + b),
                            Op::Subtract => Cell::Number(a - b),
                            Op::Multiply => Cell::Number(a * b),
                            Op::Divide => Cell::Number(a / b),
                            Op::Remainder => Cell::Number(a % b),
                            Op::Less => Cell::Boolean(a < b),
                            Op::LessEqual => Cell::Boolean(a <= b),
                            Op::Greater => Cell::Boolean(a > b),
                            Op::GreaterEqual => Cell::Boolean(a >= b),
                            _ => unreachable!("lowered numeric operation"),
                        },
                        _ => Cell::Missing,
                    }
                }
            };
        }
        Some(match slots[self.instructions.len() - 1] {
            Cell::Missing => Operand::Missing,
            Cell::Number(n) => Operand::One(Value::Number(n)),
            Cell::Boolean(b) => Operand::One(Value::Boolean(b)),
        })
    }
}

#[derive(Default)]
struct Lower {
    instructions: Vec<Instruction>,
    paths: Vec<(Path, u8)>,
    operations: usize,
}
impl Lower {
    fn emit(&mut self, instruction: Instruction) -> Option<u8> {
        if self.instructions.len() == SLOTS {
            return None;
        }
        let at = self.instructions.len() as u8;
        self.instructions.push(instruction);
        Some(at)
    }
    fn node(&mut self, node: &Node) -> Option<u8> {
        let instruction = match &node.kind {
            Kind::Path(path) => {
                if let Some((_, slot)) = self
                    .paths
                    .iter()
                    .find(|(p, _)| p.fields == path.fields && p.rooted == path.rooted)
                {
                    return Some(*slot);
                }
                let index = self.paths.len() as u8;
                let slot = self.emit(Instruction::Load(index))?;
                self.paths.push((path.clone(), slot));
                return Some(slot);
            }
            Kind::Number(n) => Instruction::Number(*n),
            Kind::Missing => Instruction::Missing,
            Kind::Prepared(p) => match p.data {
                Data::Number(n) => Instruction::Number(n),
                Data::Missing => Instruction::Missing,
                _ => return None,
            },
            Kind::Group(child) => return self.node(child),
            Kind::Negate(child) => {
                let a = self.node(child)?;
                self.operations += 1;
                Instruction::Negate(a)
            }
            Kind::Binary(
                op @ (Op::Add
                | Op::Subtract
                | Op::Multiply
                | Op::Divide
                | Op::Remainder
                | Op::Less
                | Op::LessEqual
                | Op::Greater
                | Op::GreaterEqual),
                a,
                b,
            ) => {
                let a = self.node(a)?;
                let b = self.node(b)?;
                self.operations += 1;
                Instruction::Binary(*op, a, b)
            }
            _ => return None,
        };
        self.emit(instruction)
    }
}

pub(crate) fn prepare(node: &mut Node) {
    if !node.effects
        && matches!(
            node.kind,
            Kind::Binary(..) | Kind::Group(_) | Kind::Negate(_)
        )
    {
        let mut lower = Lower::default();
        if lower.node(node).is_some() && lower.operations >= 3 {
            let source = Box::new(node.clone());
            node.kind = Kind::Plan(Box::new(Plan {
                source,
                instructions: lower.instructions.into_boxed_slice(),
                paths: lower.paths.into_iter().map(|(p, _)| p).collect(),
            }));
            return;
        }
    }
    crate::analysis::children(node, &mut prepare);
}

#[cfg(test)]
mod tests;
