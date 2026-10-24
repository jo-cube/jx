//! Local semantic rewrites over the common IR. No source syntax is consulted.
//! Fusion requires a single use and adjacent producer/consumer so neither error
//! order nor lexical lifetime moves across another instruction.
use super::*;
use crate::builtin::Builtin;
pub(super) fn run(p: &mut Program) {
    copies(p);
    compose_fields(p);
    let mut definitions = vec![0; p.registers];
    for i in &p.code {
        definitions[i.out] += 1;
    }
    let mut data = vec![None; p.registers];
    for i in &mut p.code {
        if let Operation::Builtin(Builtin::Lookup, args, _) = &i.kind
            && let [base, key] = args.as_ref()
            && let Some(d) = &data[*base]
        {
            i.kind = Operation::Lookup(Arc::clone(d), *key);
        }
        data[i.out] = if definitions[i.out] != 1 {
            None
        } else {
            match &i.kind {
                Operation::Literal(Literal::Data(d)) => Some(Arc::clone(d)),
                Operation::Copy(r) => data[*r].clone(),
                _ => None,
            }
        };
    }
    // Preserve eager selection as membership before aggregate fusion can
    // consume its mapper. This avoids retaining an intermediate value sequence
    // while keeping singleton-array normalization in the semantic Path region.
    selection_ownership(p);
    let mut uses = vec![0usize; p.registers];
    uses[p.result] += 1;
    for i in &p.code {
        operands(&i.kind, &mut |r| uses[r] += 1);
    }
    for n in 1..p.code.len() {
        let (left, right) = p.code.split_at_mut(n);
        let consumer = &mut right[0];
        let Operation::Builtin(Builtin::Aggregate(aggregate), args, _) = &consumer.kind else {
            continue;
        };
        let aggregate = *aggregate;
        let mut producer_index = n - 1;
        if let Operation::Materialize(r) = left[n - 1].kind {
            if n < 2
                || args.as_ref() != [left[n - 1].out]
                || uses[left[n - 1].out] != 1
                || left[n - 2].out != r
                || uses[r] != 1
            {
                continue;
            }
            producer_index = n - 2;
        } else if args.as_ref() != [left[n - 1].out] || uses[left[n - 1].out] != 1 {
            continue;
        }
        let producer = &mut left[producer_index];
        let eligible = match &producer.kind {
            Operation::Each {
                kind: Each::Map, ..
            } => true,
            Operation::Path(path) => {
                !path.eager
                    && !path.candidate
                    && path.predicates.len() <= 1
                    && facts::non_array_result(&path.body)
                    && (path.predicates.is_empty()
                        || (reorderable(&path.body) && path.predicates.iter().all(reorderable)))
            }
            _ => false,
        };
        if !eligible {
            if matches!(
                producer.kind,
                Operation::Field(_, _, _, FieldMode::Value)
                    | Operation::Path(_)
                    | Operation::Correlate(_)
                    | Operation::Navigate(_)
                    | Operation::Select(..)
                    | Operation::Range(..)
                    | Operation::Traverse(_)
            ) {
                let offset = producer.offset;
                let op =
                    std::mem::replace(&mut producer.kind, Operation::Literal(Literal::Missing));
                if producer_index != n - 1 {
                    left[n - 1].kind = Operation::Literal(Literal::Missing);
                }
                consumer.kind = Operation::Reduce(aggregate, Box::new(op), offset);
            }
            continue;
        }
        let (source, body, predicate, callback) =
            match std::mem::replace(&mut producer.kind, Operation::Literal(Literal::Missing)) {
                Operation::Each { source, body, .. } => (source, body, None, true),
                Operation::Path(path) => {
                    let selection::Selection {
                        source,
                        body,
                        predicates,
                        eager: _,
                        mapping: _,
                        candidate: _,
                        positional: _,
                        preflight: _,
                    } = *path;
                    (
                        source,
                        body,
                        predicates.into_vec().pop().map(Box::new),
                        false,
                    )
                }
                _ => unreachable!(),
            };
        if producer_index != n - 1 {
            left[n - 1].kind = Operation::Literal(Literal::Missing);
        }
        consumer.kind = Operation::Fold {
            source,
            body,
            predicate,
            aggregate,
            callback,
        };
    }

    loop {
        let mut uses = vec![0usize; p.registers];
        uses[p.result] += 1;
        for i in &p.code {
            operands(&i.kind, &mut |r| uses[r] += 1);
        }
        let keep: Vec<bool> = p
            .code
            .iter()
            .map(|i| uses[i.out] > 0 || !infallible(&i.kind))
            .collect();
        if keep.iter().all(|k| *k) {
            break;
        }
        let mut pc = vec![0; p.code.len() + 1];
        for n in 0..p.code.len() {
            pc[n + 1] = pc[n] + usize::from(keep[n]);
        }
        let mut n = 0;
        let mut code = std::mem::take(&mut p.code).into_vec();
        code.retain_mut(|i| {
            let k = keep[n];
            n += 1;
            if let Operation::Jump(to) | Operation::JumpFalse(_, to) = &mut i.kind {
                *to = pc[*to];
            }
            k
        });
        p.code = code.into_boxed_slice();
    }
    compact(p);
}

pub(super) fn operands(op: &Operation, emit: &mut impl FnMut(usize)) {
    match op {
        Operation::Reduce(_, producer, _) => return operands(producer, emit),
        Operation::Field(r, ..)
        | Operation::Copy(r)
        | Operation::Acquire(r, _)
        | Operation::Materialize(r)
        | Operation::Keep(r, _)
        | Operation::Negate(r)
        | Operation::Truth(r)
        | Operation::Accept(r)
        | Operation::Lookup(_, r)
        | Operation::JumpFalse(r, _)
        | Operation::Select(r, _)
        | Operation::Order(r, _) => emit(*r),
        Operation::Range(a, b) | Operation::Binary(_, a, b) | Operation::Chain(a, b, _) => {
            emit(*a);
            emit(*b);
        }
        Operation::Builtin(_, args, _) | Operation::Formatted(_, args, _) => {
            args.iter().for_each(|r| emit(*r))
        }
        Operation::Bind(_, r) => emit(*r),
        Operation::Invoke(target, args, _, _) => {
            emit(*target);
            args.iter().for_each(|r| emit(*r));
        }
        Operation::Partial(target, args) => {
            emit(*target);
            args.iter().flatten().for_each(|r| emit(*r));
        }
        Operation::Object(m) => m.iter().for_each(|(_, r)| emit(*r)),
        Operation::Array(m, _) => m.iter().for_each(|(r, _)| emit(*r)),
        Operation::Group(Some(source), _) => emit(*source),
        Operation::Path(path) => emit(path.source),
        Operation::Each { source, .. } | Operation::Fold { source, .. } => emit(*source),
        _ => {}
    }
    // Function definitions capture live names, not parent register numbers.
    if !matches!(op, Operation::Function(_) | Operation::Transform(_)) {
        op.children(&mut |p| captures(p, emit));
    }
}

fn captures(p: &Program, emit: &mut impl FnMut(usize)) {
    for i in &p.code {
        if let Operation::Outer(r, _) = i.kind {
            emit(r);
        }
    }
}

// An eager filter is a cardinality boundary: a sole selected array becomes
// the mapper's input sequence. Do not erase that boundary when forming a fold.
// Stage fusion can defer checked failures, but cannot move calls or writes
// across earlier predicates. Reads alone observe immutable state when both stages
// pass this proof. Keep the default conservative as new operations are introduced.
fn reorderable(p: &Program) -> bool {
    p.code.iter().all(|i| match i.kind.producer() {
        Operation::Path(path) => reorderable(&path.body) && path.predicates.iter().all(reorderable),
        Operation::Each { body, .. } => reorderable(body),
        Operation::Fold {
            body, predicate, ..
        } => reorderable(body) && predicate.as_deref().is_none_or(reorderable),
        Operation::Builtin(Builtin::Library(l), ..) => !l.calls_functions(),
        Operation::Builtin(Builtin::Runtime(_), ..) => false,
        Operation::Builtin(..)
        | Operation::Focus
        | Operation::Root
        | Operation::Argument(_)
        | Operation::Outer(..)
        | Operation::Read(_)
        | Operation::Copy(_)
        | Operation::Literal(_)
        | Operation::Field(..)
        | Operation::Materialize(_)
        | Operation::Array(..)
        | Operation::Object(_)
        | Operation::Binary(..)
        | Operation::Negate(_)
        | Operation::Truth(_)
        | Operation::Lookup(..)
        | Operation::Keep(..)
        | Operation::Jump(_)
        | Operation::JumpFalse(..)
        | Operation::Acquire(..)
        | Operation::Accept(_) => true,
        _ => false,
    })
}

// A materialized identity filter used once by the next path needs membership,
// not retained values. The eager predicate barrier is preserved explicitly.
fn selection_ownership(p: &mut Program) {
    let mut uses = vec![0; p.registers];
    uses[p.result] += 1;
    for i in &p.code {
        operands(&i.kind, &mut |r| uses[r] += 1);
    }
    for n in 2..p.code.len() {
        let Operation::Path(next) = &p.code[n].kind else {
            continue;
        };
        let Operation::Materialize(r) = p.code[n - 1].kind else {
            continue;
        };
        if next.source != p.code[n - 1].out
            || r != p.code[n - 2].out
            || uses[p.code[n - 1].out] != 1
            || uses[r] != 1
            || !next.predicates.is_empty()
            || !matches!(&p.code[n-2].kind, Operation::Path(path) if path.identity() && !path.predicates.is_empty() && !path.eager)
        {
            continue;
        }
        let Operation::Path(previous) = std::mem::replace(
            &mut p.code[n - 2].kind,
            Operation::Literal(Literal::Missing),
        ) else {
            unreachable!()
        };
        p.code[n - 1].kind = Operation::Literal(Literal::Missing);
        let Operation::Path(next) = &mut p.code[n].kind else {
            unreachable!()
        };
        next.source = previous.source;
        next.predicates = previous.predicates;
        next.candidate = previous.candidate;
        next.positional = previous.positional;
        next.eager = true;
    }
}

// Compact semantic identities after DCE; generic frames must not reserve cells
// for removed declarations. Child captures name the immediate parent's IDs.
fn compact(p: &mut Program) {
    let mut ids = vec![usize::MAX; p.registers];
    let mut count = 0;
    for i in &p.code {
        if ids[i.out] == usize::MAX {
            ids[i.out] = count;
            count += 1;
        }
    }
    let id = |r: usize| {
        assert_ne!(ids[r], usize::MAX, "live register definition");
        ids[r]
    };
    for i in &mut p.code {
        i.out = id(i.out);
        remap(&mut i.kind, &id);
    }
    p.result = id(p.result);
    p.registers = count;
}
fn remap(op: &mut Operation, id: &impl Fn(usize) -> usize) {
    match op {
        Operation::Reduce(_, producer, _) => return remap(producer, id),
        Operation::Field(r, ..)
        | Operation::Copy(r)
        | Operation::Acquire(r, _)
        | Operation::Materialize(r)
        | Operation::Keep(r, _)
        | Operation::Negate(r)
        | Operation::Truth(r)
        | Operation::Accept(r)
        | Operation::Lookup(_, r)
        | Operation::JumpFalse(r, _)
        | Operation::Select(r, _)
        | Operation::Order(r, _)
        | Operation::Bind(_, r) => *r = id(*r),
        Operation::Range(a, b) | Operation::Binary(_, a, b) | Operation::Chain(a, b, _) => {
            *a = id(*a);
            *b = id(*b);
        }
        Operation::Builtin(_, args, _) | Operation::Formatted(_, args, _) => {
            args.iter_mut().for_each(|r| *r = id(*r))
        }
        Operation::Invoke(target, args, _, _) => {
            *target = id(*target);
            args.iter_mut().for_each(|r| *r = id(*r));
        }
        Operation::Partial(target, args) => {
            *target = id(*target);
            args.iter_mut().flatten().for_each(|r| *r = id(*r));
        }
        Operation::Object(members) => members.iter_mut().for_each(|(_, r)| *r = id(*r)),
        Operation::Array(items, _) => items.iter_mut().for_each(|(r, _)| *r = id(*r)),
        Operation::Group(Some(source), _) => *source = id(*source),
        Operation::Path(path) => path.source = id(path.source),
        Operation::Each { source, .. } | Operation::Fold { source, .. } => *source = id(*source),
        _ => {}
    }
    if !matches!(op, Operation::Function(_) | Operation::Transform(_)) {
        op.children_mut(&mut |p| {
            for i in &mut p.code {
                if let Operation::Outer(r, _) = &mut i.kind {
                    *r = id(*r);
                }
            }
        });
    }
}

// This proof is deliberately smaller than purity: unused checked arithmetic, conversions, and grouping still run.
fn infallible(op: &Operation) -> bool {
    let program = |p: &Program| p.code.iter().all(|i| infallible(&i.kind));
    match op {
        Operation::Focus
        | Operation::Root
        | Operation::Argument(_)
        | Operation::Outer(..)
        | Operation::Copy(_)
        | Operation::Literal(_)
        | Operation::Field(..)
        | Operation::Materialize(_)
        | Operation::Array(..) => true,
        Operation::Path(path) => {
            program(&path.body)
                && path
                    .predicates
                    .iter()
                    .all(|p| program(p) && facts::truth_predicate(p))
        }
        Operation::Each { body, kind, .. } => {
            program(body) && (*kind == Each::Map || facts::truth_predicate(body))
        }
        _ => false,
    }
}

// Collapse copy identities, including a join whose incoming values are identical.
// Branch tests and checked instructions remain in place: only dataflow changes.
fn copies(p: &mut Program) {
    if !p.code.iter().any(|i| matches!(i.kind, Operation::Copy(_))) {
        return;
    }
    let mut sources = vec![None; p.registers];
    let mut other = vec![false; p.registers];
    for i in &p.code {
        if let Operation::Copy(r) = i.kind {
            if sources[i.out].is_some_and(|s| s != r) {
                other[i.out] = true;
            }
            sources[i.out] = Some(r);
        } else {
            other[i.out] = true;
        }
    }
    fn resolve(mut r: usize, sources: &[Option<usize>], other: &[bool]) -> usize {
        while !other[r] {
            let Some(s) = sources[r] else { break };
            r = s;
        }
        r
    }
    // Lowering emits acyclic copies. Resolve each once in definition order;
    // this keeps chained aliases linear rather than repeatedly walking prefixes.
    let mut ids: Vec<_> = (0..p.registers).collect();
    for i in &p.code {
        if !other[i.out]
            && let Some(r) = sources[i.out]
        {
            ids[i.out] = resolve(r, &sources, &other);
            sources[i.out] = Some(ids[i.out]);
        }
    }
    for i in &mut p.code {
        remap(&mut i.kind, &|r| ids[r]);
    }
    p.result = ids[p.result];
}

// Fuse the already-proven fold stages in semantic IR. No expression syntax or
// numeric shape is recognized. The acceptance boundary preserves error priority;
// physical scalar lowering and row acquisition see one activation/dataflow.
pub(super) fn row_dataflow(p: &mut Program) {
    for instruction in &mut p.code {
        let Operation::Fold {
            body,
            predicate,
            callback: false,
            ..
        } = &mut instruction.kind
        else {
            continue;
        };
        if !super::scalar::supported(body)
            || !predicate.as_deref().is_some_and(super::scalar::supported)
        {
            continue;
        }
        compose_row(body, *predicate.take().unwrap(), instruction.offset);
    }
}

// Both physical iteration regions and direct folds use this same IR composition.
pub(super) fn compose_row(body: &mut Program, predicate: Program, offset: usize) {
    // A derived program has new logical IDs; it cannot use a source program's
    // root binding table when no acquired row is active.
    body.key = Arc::new(());
    let shift = predicate.registers;
    let mut code = predicate.code.into_vec();
    code.push(Instruction {
        out: shift + body.registers,
        kind: Operation::Accept(predicate.result),
        offset,
    });
    let mut ids = vec![usize::MAX; body.registers];
    for mut i in std::mem::take(&mut body.code).into_vec() {
        remap(&mut i.kind, &|r| ids[r]);
        let equivalent = code.iter().find(|before| match (&before.kind, &i.kind) {
            (Operation::Focus, Operation::Focus) | (Operation::Root, Operation::Root) => true,
            (Operation::Outer(a, _), Operation::Outer(b, _)) => a == b,
            (Operation::Field(a, x, c, m), Operation::Field(b, y, d, n)) => {
                a == b && x.fields == y.fields && c == d && m == n
            }
            _ => super::lower::same(&before.kind, &i.kind),
        });
        if let Some(before) = equivalent {
            ids[i.out] = before.out;
        } else {
            let original = i.out;
            i.out += shift;
            ids[original] = i.out;
            code.push(i);
        }
    }
    body.result = ids[body.result];
    body.registers += shift + 1;
    body.code = code.into();
    // Compact away inputs shared across stages; replan acquisition and the
    // physical register layout over this single semantic program.
    compact(body);
}

// Static explicit-base navigation composes with its defining field path. The
// first selection's context wrapping is retained; contextual second selections
// and value/materialization/control boundaries are not crossed. Navigation is
// infallible, and unused intermediate reads are removed by the existing DCE.
fn compose_fields(p: &mut Program) {
    let mut definitions = vec![0; p.registers];
    for i in &p.code {
        definitions[i.out] += 1;
    }
    type FieldOrigin = (usize, Vec<Box<str>>, bool);
    let mut paths: Vec<Option<FieldOrigin>> = vec![None; p.registers];
    for i in &mut p.code {
        if let Operation::Field(base, path, false, FieldMode::Value) = &mut i.kind
            && let Some((origin, prefix, wrapped)) = &paths[*base]
        {
            let mut fields = prefix.clone();
            fields.extend(path.fields.iter().cloned());
            i.kind = Operation::Field(
                *origin,
                Box::new(crate::expression::Path {
                    fields: fields.into(),
                    rooted: false,
                }),
                *wrapped,
                FieldMode::Value,
            );
        }
        paths[i.out] = if definitions[i.out] == 1 {
            match &i.kind {
                Operation::Field(base, path, wrapped, FieldMode::Value) => {
                    Some((*base, path.fields.to_vec(), *wrapped))
                }
                _ => None,
            }
        } else {
            None
        };
    }
}

pub(super) fn argument_mask(p: &Program) -> u8 {
    p.code.iter().fold(0, |mask, i| match i.kind {
        Operation::Argument(n) if n < 3 => mask | (1_u8 << n),
        _ => mask,
    })
}

// Physical representation is a property of semantic uses, shared by every
// executor. Only repeated checked numeric uses (or an atomic stage crossing)
// amortize decoding. 0/1/2 are use cardinalities; 3 marks an observable value.
pub(super) fn load_transfers(p: &Program, gate: Option<usize>) -> Box<[bool]> {
    let atomic: Box<[bool]> = if gate.is_some() {
        let mut before = vec![false; p.registers];
        let mut after = vec![false; p.registers];
        let mut accepted = false;
        for i in &p.code {
            if matches!(i.kind, Operation::Accept(_)) {
                accepted = true;
            }
            operands(&i.kind, &mut |r| {
                if accepted {
                    after[r] = true;
                } else {
                    before[r] = true;
                }
            });
        }
        let mut atomic: Vec<_> = before.iter().zip(&after).map(|(a, b)| *a && *b).collect();
        atomic[p.result] = false;
        for i in &p.code {
            if !matches!(
                i.kind,
                Operation::Binary(..) | Operation::Negate(_) | Operation::Accept(_)
            ) {
                operands(&i.kind, &mut |r| atomic[r] = false);
            }
        }
        atomic.into_boxed_slice()
    } else {
        Box::new([])
    };
    let mut uses = vec![0_u8; p.registers];
    uses[p.result] = 3;
    for i in &p.code {
        let numeric = matches!(
            i.kind,
            Operation::Negate(_)
                | Operation::Binary(
                    Op::Add | Op::Subtract | Op::Multiply | Op::Divide | Op::Remainder,
                    ..
                )
        );
        operands(&i.kind, &mut |r| {
            if numeric {
                if uses[r] < 2 {
                    uses[r] += 1;
                }
            } else {
                uses[r] = 3;
            }
        });
    }
    uses.iter()
        .enumerate()
        .map(|(r, use_)| *use_ == 2 || atomic.get(r).copied().unwrap_or(false))
        .collect()
}

// This is a lexical invalidation fact, not the source scheduling/effects flag
// (which also includes reads). Compute it once, bottom-up, over the common IR.
pub(super) fn writes_scope(p: &Program) -> bool {
    fn operation(op: &Operation, code: &[Instruction]) -> bool {
        match op {
            Operation::Invoke(target, ..) => call::writes_caller(code, *target),
            Operation::Chain(_, target, _) => call::writes_caller(code, *target),
            Operation::Function(_) | Operation::Transform(_) => false,
            Operation::Builtin(b, args, _) => call::builtin_writes_caller(code, *b, args),
            _ => {
                let mut writes = false;
                op.children(&mut |p| writes |= p.writes_scope);
                writes
            }
        }
    }
    let mut depth = 0;
    for i in &p.code {
        match i.kind {
            Operation::EnterScope => depth += 1,
            Operation::LeaveScope => depth -= 1,
            Operation::Bind(..) if depth == 0 => return true,
            _ => {
                if operation(&i.kind, &p.code) {
                    return true;
                }
            }
        }
    }
    false
}
