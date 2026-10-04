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
