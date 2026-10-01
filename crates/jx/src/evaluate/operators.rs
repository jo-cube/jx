use super::*;

pub(super) fn binary<'e, 'i>(
    op: &Op,
    lhs: &'e Node,
    rhs: &'e Node,
    input: &Context<'e, 'i>,
    offset: usize,
) -> Result<Operand<'e, 'i>, Error> {
    let left = lhs.run(input)?;
    if matches!(op, Op::And | Op::Or) {
        let truth = left.truth(offset)?;
        let value = match op {
            Op::And => truth && rhs.run(input)?.truth(offset)?,
            Op::Or => truth || rhs.run(input)?.truth(offset)?,
            _ => unreachable!(),
        };
        return Ok(Operand::One(Value::Boolean(value)));
    }
    let right = rhs.run(input)?;
    let value = match op {
        Op::In => Value::Boolean(crate::compare::includes(left, right)?),
        Op::Equal | Op::NotEqual => Value::Boolean(crate::compare::equal(
            left,
            right,
            matches!(op, Op::NotEqual),
        )?),
        Op::Less | Op::LessEqual | Op::Greater | Op::GreaterEqual => {
            return crate::compare::order(left, right, *op, offset);
        }
        _ => {
            // Type-check both operands before propagating missing.
            let left = left.number(offset)?;
            let right = right.number(offset)?;
            let (Some(left), Some(right)) = (left, right) else {
                return Ok(Operand::Missing);
            };
            Value::Number(match op {
                Op::Add => left + right,
                Op::Subtract => left - right,
                Op::Multiply => left * right,
                Op::Divide => left / right,
                Op::Remainder => left % right,
                _ => unreachable!(),
            })
        }
    };
    Ok(Operand::One(value))
}
