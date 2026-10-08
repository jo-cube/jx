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
    // Complete both operands before conversion; neither may be replayed.
    let left = materialize(left)?;
    let right = materialize(right)?;
    crate::convert::concat(left, right, offset)
}
