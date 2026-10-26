//! Physical storage for logical IR values. Forward control flow permits compact,
//! conservative live intervals: joins keep one location across all definitions,
//! and nested regions count their captures as reads at the enclosing operation.
use super::{Operation, Program, optimize};
use std::{cmp::Reverse, collections::BinaryHeap};

#[derive(Clone, Debug, Default)]
pub(super) struct Layout {
    pub slots: Box<[usize]>,
    pub count: usize,
}
pub(super) fn layout(p: &Program) -> Layout {
    let mut first = vec![usize::MAX; p.registers];
    let mut last = vec![0; p.registers];
    for (pc, i) in p.code.iter().enumerate() {
        // Writes occur after all reads, including synchronous child captures.
        let write = pc * 2 + 1;
        first[i.out] = first[i.out].min(write);
        last[i.out] = last[i.out].max(write);
        optimize::operands(&i.kind, &mut |r| last[r] = last[r].max(pc * 2));
        if matches!(i.kind, Operation::Jump(to) | Operation::JumpFalse(_, to) if to <= pc) {
            // Currently lowering emits only forward branches. A future backedge
            // needs CFG liveness before storage can be reused across that edge.
            return Layout {
                slots: (0..p.registers).collect(),
                count: p.registers,
            };
        }
    }
    last[p.result] = p.code.len() * 2;
    let mut slots = vec![usize::MAX; p.registers];
    let mut active = BinaryHeap::new();
    let mut free = Vec::new();
    let mut count = 0;
    for i in &p.code {
        if slots[i.out] != usize::MAX {
            continue;
        }
        while let Some(&Reverse((end, slot))) = active.peek() {
            if end >= first[i.out] {
                break;
            }
            active.pop();
            free.push(slot);
        }
        let slot = free.pop().unwrap_or_else(|| {
            let n = count;
            count += 1;
            n
        });
        slots[i.out] = slot;
        active.push(Reverse((last[i.out], slot)));
    }
    Layout {
        slots: slots.into(),
        count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn joins_and_child_captures_keep_their_values() {
        for source in [
            "($x := a + 1; $y := flag ? b + 2 : c + 3; [$x, $y, $x + $y])",
            "($x := a; items.($y := $x + v; {\"a\": $x, \"b\": $y}))",
            "($x := {\"v\": a}; $f := function($y){[$x, $y]}; $f(b))",
        ] {
            let e = crate::compile(source).unwrap();
            let p = Program::compile(&e).unwrap();
            let mut a = super::super::Acquisition::new([&p]);
            for flag in ["true", "false"] {
                let input = format!(
                    "{{\"a\":2,\"b\":3,\"c\":4,\"flag\":{flag},\"items\":[{{\"v\":5}},{{\"v\":6}}]}}"
                );
                let d = a.prepare(input.as_bytes()).unwrap();
                let mut actual = Vec::new();
                p.write_json(&d, &mut actual).unwrap();
                let expected = e
                    .evaluate(input.as_bytes())
                    .unwrap()
                    .collect_owned()
                    .unwrap();
                let found = p.evaluate(&d).unwrap().collect_owned().unwrap();
                assert_eq!(format!("{found:?}"), format!("{expected:?}"), "{source}");
            }
        }
    }
    #[test]
    fn branching_predicate_reuses_logical_lifetimes() {
        let p = Program::compile(
            &crate::compile("a='x' and b in ['one','two','three'] and c>0 and d='y'").unwrap(),
        )
        .unwrap();
        assert!(p.registers > 16);
        assert!(p.storage.count < 8, "{:?}", p.storage);
        // Every interval-sharing pair is also exercised through the full pinned
        // and generated semantic campaigns; this guard records the spill cause.
    }
}
