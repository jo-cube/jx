use super::*;
use jx_native::{Binary, Kernel, Operation};

#[derive(Clone, Debug)]
pub(super) struct Compiled {
    kernel: Kernel,
    loads: Box<[(u8, u8)]>,
}
impl Compiled {
    fn compile(program: &Program) -> Option<Result<Self, String>> {
        let mut loads = Vec::new();
        let operations = program
            .instructions
            .iter()
            .enumerate()
            .map(|(at, i)| {
                Some(match *i {
                    Instruction::Load(path) => {
                        loads.push((at as u8, path));
                        Operation::Input
                    }
                    Instruction::Lookup(_) => return None,
                    Instruction::Number(n) => Operation::Number(n),
                    Instruction::Boolean(b) => Operation::Boolean(b),
                    Instruction::Missing => Operation::Missing,
                    Instruction::Negate(a) => Operation::Negate(a),
                    Instruction::Copy(a) => Operation::Copy(a),
                    Instruction::Truth(a) => Operation::Truth(a),
                    Instruction::Branch(a, b, c) => Operation::Branch(a, b, c),
                    Instruction::Jump(a) => Operation::Jump(a),
                    Instruction::Merge(a, b) => Operation::Merge(a, b),
                    Instruction::Binary(op, a, b) => Operation::Binary(
                        match op {
                            Op::Add => Binary::Add,
                            Op::Subtract => Binary::Subtract,
                            Op::Multiply => Binary::Multiply,
                            Op::Divide => Binary::Divide,
                            Op::Equal => Binary::Equal,
                            Op::NotEqual => Binary::NotEqual,
                            Op::Less => Binary::Less,
                            Op::LessEqual => Binary::LessEqual,
                            Op::Greater => Binary::Greater,
                            Op::GreaterEqual => Binary::GreaterEqual,
                            _ => return None,
                        },
                        a,
                        b,
                    ),
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(
            Kernel::compile(&operations, program.result).map(|kernel| Self {
                kernel,
                loads: loads.into_boxed_slice(),
            }),
        )
    }
    pub(super) fn run<'e, 'i>(
        &self,
        program: &'e Program,
        load: &mut impl FnMut(usize, &'e Path) -> Option<Operand<'e, 'i>>,
    ) -> Option<Cell> {
        let mut numbers = [0.0; SLOTS];
        let mut tags = [0; SLOTS];
        for &(at, path) in &self.loads {
            match cell(load(usize::from(at), &program.paths[usize::from(path)])?)? {
                Cell::Missing => {}
                Cell::Number(n) => {
                    numbers[usize::from(at)] = n;
                    tags[usize::from(at)] = 1;
                }
                Cell::Boolean(b) => {
                    numbers[usize::from(at)] = if b { 1.0 } else { 0.0 };
                    tags[usize::from(at)] = 2;
                }
            }
        }
        let (n, t) = self.kernel.run(&numbers, &tags)?;
        match t {
            0 => Some(Cell::Missing),
            1 => Some(Cell::Number(n)),
            2 => Some(Cell::Boolean(n != 0.0)),
            _ => None,
        }
    }
}
fn execution(execution: &mut Execution, stats: &mut crate::NativeStats) {
    let program = match execution {
        Execution::Scalar(p) => p,
        Execution::Fold(p) => &mut p.program,
        // Only single-result programs can use the native ABI. Constructed members
        // read several register results and keep the existing Rust dispatch.
        Execution::Object(_) => return,
    };
    if program.native.is_some() {
        return;
    }
    if let Some(result) = Compiled::compile(program) {
        match result {
            Ok(compiled) => {
                stats.kernels += 1;
                stats.code_bytes += compiled.kernel.code_bytes();
                program.native = Some(compiled)
            }
            Err(_) => stats.failures += 1,
        }
    }
}
pub(super) fn prepare(node: &mut Node, stats: &mut crate::NativeStats) {
    match &mut node.kind {
        Kind::Plan(plan) => execution(&mut plan.execution, stats),
        Kind::Lambda(d) => {
            if let Some(callback) = &mut d.plan {
                execution(&mut std::sync::Arc::make_mut(callback).execution, stats);
            }
            prepare(&mut d.body, stats);
        }
        _ => crate::analysis::children(node, &mut |child| prepare(child, stats)),
    }
}
