use super::*;
pub(crate) fn negate<'e, 'i>(
    value: Operand<'e, 'i>,
    offset: usize,
) -> Result<Operand<'e, 'i>, Error> {
    Ok(value
        .number(offset)?
        .map_or(Operand::Missing, |n| Operand::One(Value::Number(-n))))
}
