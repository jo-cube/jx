use crate::{
    Value,
    evaluate::Operand,
    sequence::{Halt, View, Walk},
    value::range_error,
};

pub(crate) fn position<'e, 'i>(
    input: View<'_, 'e, 'i>,
    index: f64,
    output: &mut dyn FnMut(View<'_, 'e, 'i>) -> Walk,
) -> Walk {
    let mut length = 0;
    if index < 0.0 {
        input.candidates(true, &mut |_| {
            length += 1;
            Ok(())
        })?;
    }
    let target = if index < 0.0 {
        length as f64 + index.floor()
    } else {
        index.floor()
    };
    let mut at = 0;
    let mut selected = None;
    // Finish the source even after selection, so errors in upstream
    // predicates are not suppressed by a subsequent positional filter.
    input.candidates(true, &mut |value| {
        if at as f64 == target {
            selected = Some(value);
        }
        at += 1;
        Ok(())
    })?;
    match selected {
        Some(value) if value.is_array() => output(View::Operand(&Operand::One(value))),
        None | Some(Value::Undefined) => output(View::Items(&[])),
        Some(value) => output(View::Items(&[value])),
    }
}

fn numbers(
    operand: &Operand<'_, '_>,
    offset: usize,
    output: &mut dyn FnMut(f64) -> Walk,
) -> Result<bool, Halt> {
    let mut numeric = true;
    let mut item = |value: Value<'_, '_>| {
        match value.atomic() {
            Value::Number(n) if n.is_infinite() => return Err(range_error(offset).into()),
            Value::Number(n) if !n.is_nan() => output(n)?,
            _ => numeric = false,
        }
        Ok(())
    };
    match operand {
        Operand::Missing => numeric = false,
        Operand::One(value) if value.is_array() => {
            for value in value.elements() {
                item(value)?;
            }
        }
        Operand::One(value) => item(value.clone())?,
        Operand::Many(stream) => stream.walk(&mut item)?,
    }
    Ok(numeric)
}

pub(crate) fn select(
    predicate: &Operand<'_, '_>,
    offset: usize,
    index: usize,
    mut length: impl FnMut() -> Result<usize, Halt>,
    output: &mut dyn FnMut() -> Walk,
) -> Walk {
    // Scalar positions finish checking before invoking the consumer. They do
    // not need the array-position enumerator's recursive callback activation.
    if let Operand::One(value) = predicate
        && !value.is_array()
    {
        match value.atomic() {
            Value::Number(n) if n.is_infinite() => return Err(range_error(offset).into()),
            Value::Number(n) if !n.is_nan() => {
                let n = n.floor();
                let target = if n < 0.0 { length()? as f64 + n } else { n };
                return if target == index as f64 {
                    output()
                } else {
                    Ok(())
                };
            }
            _ => {
                return if predicate.truth(offset)? {
                    output()
                } else {
                    Ok(())
                };
            }
        }
    }
    if numbers(predicate, offset, &mut |_| Ok(()))? {
        numbers(predicate, offset, &mut |number| {
            let number = number.floor();
            let target = if number < 0.0 {
                length()? as f64 + number
            } else {
                number
            };
            if target == index as f64 {
                output()?;
            }
            Ok(())
        })?;
    } else if predicate.truth(offset)? {
        output()?;
    }
    Ok(())
}
