//! A physical scalar kernel selected from semantic operations. It consumes the
//! same acquired inputs and reports checked errors once; there is no guard replay
//! or syntax recognizer. Registers preserve raw spelling until arithmetic.
use super::{
    frame::Frame,
    run::{Activation, Cell},
    *,
};
use crate::{Error, RawJson};
pub(super) fn supported(p: &Program) -> bool {
    p.code.iter().all(|i| match &i.kind {
        Operation::Focus
        | Operation::Root
        | Operation::Argument(_)
        | Operation::Outer(..)
        | Operation::Field(_, _, _, FieldMode::Value)
        | Operation::Accept(_)
        | Operation::Copy(_)
        | Operation::Literal(_)
        | Operation::Negate(_)
        | Operation::Lookup(..) => true,
        Operation::Binary(op, ..) => matches!(
            op,
            Op::Add
                | Op::Subtract
                | Op::Multiply
                | Op::Divide
                | Op::Remainder
                | Op::Equal
                | Op::NotEqual
                | Op::Less
                | Op::LessEqual
                | Op::Greater
                | Op::GreaterEqual
        ),
        _ => false,
    })
}
impl Program {
    #[inline(always)]
    pub(super) fn run_scalar<'e, 'i>(
        &'e self,
        a: Activation<'_, 'e, 'i, '_>,
        frame: &mut Frame<'e, 'i>,
    ) -> Result<Cell<'e, 'i>, Error> {
        let layout = self.scalar.as_ref().expect("scalar layout");
        self.run_scalar_frame(&a, frame)?;
        Ok(frame.take_result(layout.slots[self.result]))
    }
    pub(super) fn run_scalar_frame<'e, 'i>(
        &'e self,
        a: &Activation<'_, 'e, 'i, '_>,
        frame: &mut Frame<'e, 'i>,
    ) -> Result<(), Error> {
        let layout = self.scalar.as_ref().expect("scalar layout");
        frame.reset(layout.count);
        self.scalar_range(a, frame, 0..self.code.len(), layout.reads[0])
    }
    pub(super) fn run_row<'e, 'i>(
        &'e self,
        a: Activation<'_, 'e, 'i, '_>,
        frame: &mut Frame<'e, 'i>,
        pending: &mut Option<Error>,
        predicate_only: bool,
    ) -> Result<bool, Error> {
        let layout = self.scalar.as_ref().expect("scalar layout");
        let gate = layout.gate.expect("ordered row stage");
        frame.reset(layout.count);
        self.scalar_range(&a, frame, 0..gate, layout.reads[0])?;
        let Operation::Accept(test) = self.code[gate].kind else {
            unreachable!()
        };
        if !frame[layout.slots[test]].truth(a.input, self.code[gate].offset)?
            || pending.is_some()
            || predicate_only
        {
            return Ok(false);
        }
        // Only mapper failures are deferred. No per-instruction stage state or
        // error interception enters the common dispatch loop.
        if let Err(error) = self.scalar_range(&a, frame, gate + 1..self.code.len(), layout.reads[1])
        {
            *pending = Some(error);
            return Ok(false);
        }
        Ok(true)
    }
    #[inline(always)]
    fn scalar_range<'e, 'i>(
        &'e self,
        a: &Activation<'_, 'e, 'i, '_>,
        frame: &mut Frame<'e, 'i>,
        range: std::ops::Range<usize>,
        needs_reads: bool,
    ) -> Result<(), Error> {
        let layout = self.scalar.as_ref().expect("scalar layout");
        let slots = &layout.slots;
        let reads = if needs_reads {
            a.input.reads(self)
        } else {
            None
        };
        let controlled = a.scope.filter(|s| s.controlled());
        for i in &self.code[range] {
            if let Some(scope) = controlled {
                scope.checkpoint(i.offset)?;
            }
            // Only unobserved input representations may decode here. No checked
            // conversion occurs: the original operator still checks type/range.
            let load = |value: Cell<'e, 'i>| {
                if self.atomic_loads[i.out] {
                    value.atomic(a.input)
                } else {
                    value
                }
            };
            let value = match &i.kind {
                Operation::Focus => load(a.focus.read()),
                Operation::Root => load(if let Some(scope) = a.scope {
                    a.input.restore(scope.lookup("$"))
                } else {
                    a.input.root()
                }),
                Operation::Argument(n) => load(a.args.get(*n).map_or(Cell::Missing, Cell::read)),
                Operation::Outer(n, _) => load(a.outer[*n].read()),
                Operation::Copy(n) => load(frame[slots[*n]].clone()),
                Operation::Field(base, path, contextual, FieldMode::Value) => {
                    let value = a
                        .input
                        .select_field(
                            &frame[slots[*base]],
                            path,
                            *contextual && a.wrapped,
                            reads.and_then(|r| r.get(i.out)),
                        )
                        .retain()?;
                    load(value)
                }
                Operation::Literal(l) => match l {
                    Literal::Missing => Cell::Missing,
                    Literal::Null => Cell::Value(Value::Null),
                    Literal::Number(n) => Cell::Value(Value::Number(*n)),
                    Literal::Boolean(b) => Cell::Value(Value::Boolean(*b)),
                    Literal::String(s) => Cell::Value(Value::StringLiteral(RawJson(s))),
                    Literal::Data(d) => {
                        if matches!(&***d, Data::Missing) {
                            Cell::Missing
                        } else {
                            Cell::Value(d.value())
                        }
                    }
                },
                Operation::Negate(n) => frame[slots[*n]]
                    .number(a.input, i.offset)?
                    .map_or(Cell::Missing, |n| Cell::Value(Value::Number(-n))),
                Operation::Binary(op @ (Op::Equal | Op::NotEqual), l, r) => {
                    Cell::Value(Value::Boolean(crate::compare::equal(
                        frame[slots[*l]].operand(a.input),
                        frame[slots[*r]].operand(a.input),
                        *op == Op::NotEqual,
                    )?))
                }
                Operation::Binary(
                    op @ (Op::Less | Op::LessEqual | Op::Greater | Op::GreaterEqual),
                    l,
                    r,
                ) => match (&frame[slots[*l]], &frame[slots[*r]]) {
                    (Cell::Value(Value::Number(l)), Cell::Value(Value::Number(r))) => {
                        Cell::Value(Value::Boolean(match op {
                            Op::Less => l < r,
                            Op::LessEqual => l <= r,
                            Op::Greater => l > r,
                            Op::GreaterEqual => l >= r,
                            _ => unreachable!(),
                        }))
                    }
                    _ => Cell::from_operand(crate::compare::order(
                        frame[slots[*l]].operand(a.input),
                        frame[slots[*r]].operand(a.input),
                        *op,
                        i.offset,
                    )?)?,
                },
                Operation::Binary(op, l, r) => {
                    frame[slots[*l]].arithmetic(*op, &frame[slots[*r]], a.input, i.offset)?
                }
                Operation::Lookup(data, key) => load(
                    crate::lookup::constant(data, frame[slots[*key]].value(a.input), i.offset)?
                        .map_or(Cell::Missing, Cell::Value),
                ),
                _ => unreachable!("scalar backend validated at compilation"),
            };
            frame[slots[i.out]] = value;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(super) struct Layout {
    pub slots: Box<[usize]>,
    pub count: usize,
    pub gate: Option<usize>,
    pub number_result: bool,
    reads: [bool; 2],
}
pub(super) fn layout(p: &Program) -> Option<Layout> {
    if !supported(p) {
        return None;
    }
    // Stage boundaries and input demand determine ordered range execution.
    // Shared semantic-use analysis chooses load representations after layout.
    let gate = p
        .code
        .iter()
        .position(|i| matches!(i.kind, Operation::Accept(_)));
    let fields = |code: &[Instruction]| code.iter().any(|i| matches!(i.kind, Operation::Field(..)));
    let reads = gate.map_or_else(
        || [fields(&p.code), false],
        |at| [fields(&p.code[..at]), fields(&p.code[at + 1..])],
    );
    let mut last = vec![usize::MAX; p.registers];
    for (pc, i) in p.code.iter().enumerate() {
        super::optimize::operands(&i.kind, &mut |r| last[r] = pc);
    }
    last[p.result] = p.code.len();
    let mut slots = vec![usize::MAX; p.registers];
    let mut free = Vec::new();
    let mut count = 0;
    for (pc, i) in p.code.iter().enumerate() {
        // The result is written after all operands have been read, so a cell
        // whose last read is this instruction can also hold its output.
        super::optimize::operands(&i.kind, &mut |r| {
            if last[r] == pc && !free.contains(&slots[r]) {
                free.push(slots[r]);
            }
        });
        let out = free.pop().unwrap_or_else(|| {
            let n = count;
            count += 1;
            n
        });
        slots[i.out] = out;
        if last[i.out] == usize::MAX {
            free.push(out);
        }
    }
    let number_result = p.code.iter().any(|i| {
        i.out == p.result
            && matches!(
                i.kind,
                Operation::Negate(_)
                    | Operation::Literal(Literal::Number(_))
                    | Operation::Binary(
                        Op::Add | Op::Subtract | Op::Multiply | Op::Divide | Op::Remainder,
                        ..
                    )
            )
    });
    Some(Layout {
        slots: slots.into(),
        count,
        number_result,
        gate,
        reads,
    })
}
