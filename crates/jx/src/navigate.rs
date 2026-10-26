use crate::{
    Error, Value,
    evaluate::Operand,
    sequence::{Output, Walk},
    value::{range_error, type_error},
};

pub(crate) fn wildcard<'e, 'i>(value: &Value<'e, 'i>, output: &mut Output<'_, 'e, 'i>) -> Walk {
    fn flatten<'e, 'i>(value: Value<'e, 'i>, output: &mut Output<'_, 'e, 'i>) -> Walk {
        if value.is_array() {
            if let Value::Raw(raw) = value {
                return raw.try_for_each_flattened(|item| output(Value::Raw(item)));
            }
            for item in value.elements() {
                flatten(item, output)?;
            }
            Ok(())
        } else {
            output(value)
        }
    }
    fn members<'e, 'i, I: Iterator<Item = Value<'e, 'i>>>(
        items: impl Fn() -> I,
        output: &mut Output<'_, 'e, 'i>,
    ) -> Walk {
        // Appending any array-valued member makes the reference wildcard's
        // result an array value, even when that member is empty.
        if items().any(|item| item.is_array()) {
            let mut values = Vec::new();
            for item in items() {
                flatten(item, &mut |value| {
                    values.push(value);
                    Ok(())
                })?;
            }
            output(Value::array(values, false))
        } else {
            for item in items() {
                output(item)?;
            }
            Ok(())
        }
    }
    if value.is_array() {
        members(|| value.elements(), output)
    } else if value.is_object() {
        let entries = crate::members::entries(value);
        members(|| entries.iter().map(|(_, v)| v.clone()), output)
    } else {
        Ok(())
    }
}

pub(crate) fn descendants<'e, 'i>(value: Value<'e, 'i>, output: &mut Output<'_, 'e, 'i>) -> Walk {
    if value.is_array() {
        if let Value::Raw(raw) = value {
            return raw.try_for_each_flattened(|item| descendants(Value::Raw(item), output));
        }
        for item in value.elements() {
            descendants(item, output)?;
        }
    } else if !matches!(value, Value::Undefined) {
        output(value.clone())?;
        if value.is_object() {
            crate::members::visit(&value, &mut |item| descendants(item, output))?;
        }
    }
    Ok(())
}

pub(crate) fn range_values<'e, 'i>(
    lhs: Operand<'e, 'i>,
    rhs: Operand<'e, 'i>,
    offset: usize,
    output: &mut Output<'_, 'e, 'i>,
) -> Walk {
    fn integer(value: Operand<'_, '_>, offset: usize) -> Result<Option<f64>, Error> {
        match value {
            Operand::Missing => Ok(None),
            Operand::One(value) => match value.atomic() {
                Value::Number(n) if n.is_finite() && n.fract() == 0.0 => Ok(Some(n)),
                _ => Err(type_error(offset)),
            },
            _ => Err(type_error(offset)),
        }
    }
    let lhs = integer(lhs, offset)?;
    let rhs = integer(rhs, offset)?;
    let (Some(lhs), Some(rhs)) = (lhs, rhs) else {
        return Ok(());
    };
    if lhs > rhs {
        return Ok(());
    }
    let length = rhs - lhs + 1.0;
    // Match upstream's range width bound. Restrict endpoints to exactly
    // incrementable integers, avoiding binary64's non-progressing +1 boundary.
    if length > 10_000_000.0
        || lhs.abs() > 9_007_199_254_740_991.0
        || rhs.abs() > 9_007_199_254_740_991.0
    {
        return Err(range_error(offset).into());
    }
    for at in 0..length as u32 {
        output(Value::Number(lhs + f64::from(at)))?;
    }
    Ok(())
}
