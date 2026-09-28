use super::*;
use crate::{aggregate::Fold, builtin::Builtin};

#[derive(Clone, Debug)]
pub(super) struct Pipeline {
    source: Box<[Box<str>]>,
    pub(super) demand: Demand,
    program: Program,
    aggregate: crate::expression::Aggregate,
    offset: usize,
}
impl Pipeline {
    pub(super) fn run<'e, 'i>(&'e self, input: &Context<'e, 'i>) -> Option<Operand<'e, 'i>> {
        // General sequence boundaries, nested candidate arrays and missing source
        // contexts retain the tree's normalization and error-ordering machinery.
        if !input.wrapped && input.value.is_array() {
            return None;
        }
        if let Value::Raw(raw) = input.value {
            let mut captured = Captures::default();
            raw.capture(&self.demand, &mut captured);
            return self.run_captured(captured.get(0));
        }
        let mut source = input.value.clone();
        for field in &self.source {
            if source.is_array() {
                return None;
            }
            source = source.field(field)?;
        }
        self.consume(source)
    }
    pub(super) fn run_captured<'e, 'i>(&'e self, source: Captured<'i>) -> Option<Operand<'e, 'i>> {
        let Captured::Raw(raw) = source else {
            return None;
        };
        self.consume(Value::Raw(raw))
    }
    fn consume<'e, 'i>(&'e self, source: Value<'e, 'i>) -> Option<Operand<'e, 'i>> {
        let mut fold = Fold::new(self.aggregate);
        let mut defined = false;
        let mut consume = |value: Value<'e, 'i>, captured: Option<&Captures<'i>>| -> Option<()> {
            if value.is_array() || matches!(value, Value::Undefined) {
                return None;
            }
            let context = Context {
                value,
                wrapped: false,
                scope: None,
            };
            match self
                .program
                .execute(&context, captured, |s| s[usize::from(self.program.result)])?
            {
                Cell::Missing => {}
                Cell::Number(n) => {
                    defined = true;
                    fold.push(Value::Number(n));
                }
                Cell::Boolean(_) => return None,
            }
            Some(())
        };
        if let Value::Raw(raw) = source
            && raw.is_array()
        {
            let mut elements = raw.elements();
            let mut captured = Captures::default();
            while let Some(raw) = elements.next_captured(&self.program.capture, &mut captured) {
                consume(Value::Raw(raw), Some(&captured))?;
            }
        } else if source.is_array() {
            for value in source.elements() {
                consume(value, None)?;
            }
        } else {
            consume(source, None)?;
        }
        fold.finish(defined, self.offset).ok()
    }
}
pub(super) fn lower(node: &Node) -> Option<Pipeline> {
    let Kind::Builtin(Builtin::Aggregate(aggregate), args) = &node.kind else {
        return None;
    };
    let [argument] = args.as_ref() else {
        return None;
    };
    let Kind::Route(steps, false) = &argument.kind else {
        return None;
    };
    let (mapped, prefix) = steps.split_last()?;
    let (source, parents) = prefix.split_last()?;
    let mut fields = Vec::new();
    for step in prefix {
        let Kind::Path(path) = &step.node.kind else {
            return None;
        };
        if path.rooted || path.fields.is_empty() {
            return None;
        }
        fields.extend(path.fields.iter().cloned());
    }
    if parents.iter().any(|step| !step.predicates.is_empty()) || !mapped.predicates.is_empty() {
        return None;
    }
    let mut lower = Lower::default();
    let mut branches = Vec::new();
    for predicate in &source.predicates {
        if !boolean(predicate) {
            return None;
        }
        let test = lower.node(predicate)?;
        branches.push(lower.emit(Instruction::Branch(test, false, 0))?);
    }
    let value = lower.node(&mapped.node)?;
    let result = if branches.is_empty() {
        value
    } else {
        let result = lower.emit(Instruction::Copy(value))?;
        let jump = lower.emit(Instruction::Jump(0))?;
        let skip = lower.instructions.len() as u8;
        let missing = lower.emit(Instruction::Missing)?;
        lower.emit(Instruction::Merge(missing, result))?;
        let end = lower.instructions.len() as u8;
        lower.instructions[usize::from(jump)] = Instruction::Jump(end);
        for branch in branches {
            let Instruction::Branch(test, false, _) = lower.instructions[usize::from(branch)]
            else {
                unreachable!()
            };
            lower.instructions[usize::from(branch)] = Instruction::Branch(test, false, skip);
        }
        result
    };
    let mut demand = Demand::default();
    demand.insert(&fields, 0);
    Some(Pipeline {
        demand,
        source: fields.into_boxed_slice(),
        program: lower.finish(result),
        aggregate: *aggregate,
        offset: node.offset,
    })
}
fn boolean(node: &Node) -> bool {
    match &node.kind {
        Kind::Boolean(_) => true,
        Kind::Group(n) => boolean(n),
        Kind::Binary(
            Op::And
            | Op::Or
            | Op::Equal
            | Op::NotEqual
            | Op::Less
            | Op::LessEqual
            | Op::Greater
            | Op::GreaterEqual,
            _,
            _,
        ) => true,
        _ => false,
    }
}
