use super::*;

// Root acquisition and tree evaluation share operations, not another dispatcher.
pub(crate) fn conditional<'e, 'i>(
    test: &'e Node,
    yes: &'e Node,
    no: Option<&'e Node>,
    offset: usize,
    run: impl Fn(&'e Node) -> Result<Operand<'e, 'i>, Error>,
) -> Result<Operand<'e, 'i>, Error> {
    if run(test)?.truth(offset)? {
        run(yes)
    } else if let Some(no) = no {
        run(no)
    } else {
        Ok(Operand::Missing)
    }
}
pub(crate) fn fallback<'e, 'i>(
    op: &Op,
    test: &'e Node,
    no: &'e Node,
    offset: usize,
    run: impl Fn(&'e Node) -> Result<Operand<'e, 'i>, Error>,
) -> Result<Operand<'e, 'i>, Error> {
    if run(test)?.truth(offset)? {
        // Coalescing stores its left expression as the shadowable $exists
        // argument. Both fallbacks re-evaluate the selected branch.
        let yes = if matches!(op, Op::Coalesce) {
            match &test.kind {
                Kind::Call(_, args) | Kind::Builtin(_, args) => &args[0],
                _ => unreachable!("coalescing test is an exists call"),
            }
        } else {
            test
        };
        run(yes)
    } else {
        run(no)
    }
}
pub(crate) fn negate<'e, 'i>(
    value: Operand<'e, 'i>,
    offset: usize,
) -> Result<Operand<'e, 'i>, Error> {
    Ok(value
        .number(offset)?
        .map_or(Operand::Missing, |n| Operand::One(Value::Number(-n))))
}
pub(crate) fn concat<'e, 'i>(
    left: &'e Node,
    right: &'e Node,
    offset: usize,
    materialize: impl Fn(&'e Node) -> Result<Option<Value<'e, 'i>>, Error>,
) -> Result<Value<'e, 'i>, Error> {
    if !matches!(left.kind, Kind::Binary(Op::Concat, ..)) {
        // Complete both operands before conversion; neither may be replayed.
        return crate::convert::concat(materialize(left)?, materialize(right)?, offset);
    }
    // Retain a bounded left spine without creating its intermediate strings.
    // Each operator still converts both operands after its right side completes,
    // and before any later operand runs. Grouped/right branches stay independent.
    let mut rights = [None; 7];
    rights[0] = Some((right, offset));
    let mut count = 1;
    let mut first = left;
    while count < rights.len() {
        let Kind::Binary(Op::Concat, left, right) = &first.kind else {
            break;
        };
        rights[count] = Some((right.as_ref(), first.offset));
        count += 1;
        first = left;
    }
    let mut first = materialize(first)?;
    let mut parts: [Option<Value<'e, 'i>>; 8] = std::array::from_fn(|_| None);
    for (index, &(node, offset)) in rights[..count].iter().rev().flatten().enumerate() {
        let right = materialize(node)?;
        if index == 0 {
            parts[0] = crate::convert::string(first.take(), false, offset)?;
        }
        parts[index + 1] = crate::convert::string(right, false, offset)?;
    }
    let nonempty = parts
        .iter()
        .filter_map(|v| v.as_ref())
        .filter(|v| !v.string_body().unwrap().is_empty())
        .count();
    if nonempty < 2 {
        // Empty/missing operands must not detach the remaining borrowed value.
        let value = if nonempty == 0 {
            parts.into_iter().flatten().next_back()
        } else {
            parts
                .into_iter()
                .flatten()
                .find(|v| !v.string_body().unwrap().is_empty())
        };
        return Ok(value.unwrap_or(Value::StringLiteral(RawJson(r#""""#))));
    }
    Ok(Value::String(crate::OwnedString::concat(
        parts.iter().flatten().map(|v| v.string_body().unwrap()),
    )))
}
