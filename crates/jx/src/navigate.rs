use crate::{
    Error, Value,
    evaluate::Operand,
    expression::{Kind, Node},
    sequence::{Context, Output, Walk},
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

pub(crate) fn range<'e, 'i>(
    left: &'e Node,
    right: &'e Node,
    input: &Context<'e, 'i>,
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
    let lhs = left.run(input)?;
    let rhs = right.run(input)?;
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

pub(crate) fn keep<'e, 'i>(
    node: &'e Node,
    path: bool,
    input: &Context<'e, 'i>,
) -> Result<Operand<'e, 'i>, Error> {
    if let Kind::Tuples(steps, focus) = &node.kind {
        let mut items = Vec::new();
        match crate::tuple::route_values(steps, *focus, input, &mut |value| {
            items.push(value);
            Ok(())
        }) {
            Err(crate::sequence::Halt::Evaluation(error)) => return Err(error),
            Err(crate::sequence::Halt::Stop) => unreachable!(),
            Ok(()) => {}
        }
        return Ok(if items.is_empty() {
            Operand::Missing
        } else {
            Operand::One(Value::kept(items))
        });
    }
    if let Kind::Route(steps, focus) = &node.kind {
        return crate::route::keep(steps, *focus, input);
    }
    if let Kind::Filter(base, predicates) = &node.kind {
        let mut result = Operand::Missing;
        let walk = crate::filter::with_filters(base, predicates, input, &mut |view| {
            if let crate::sequence::View::Operand(Operand::One(value)) = view
                && value.is_array()
                && !value.is_sequence()
            {
                result = Operand::One(value.clone());
            } else {
                let mut items = Vec::new();
                view.walk(&mut |value| {
                    items.push(value);
                    Ok(())
                })?;
                if !items.is_empty() {
                    result = Operand::One(Value::kept(items));
                }
            }
            Ok(())
        });
        return match walk {
            Ok(()) => Ok(result),
            Err(crate::sequence::Halt::Evaluation(e)) => Err(e),
            Err(crate::sequence::Halt::Stop) => unreachable!(),
        };
    }
    if !path
        && let Kind::Path(field) = &node.kind
        && !field.rooted
        && field.fields.len() == 1
        && input.value.is_array()
    {
        let mut items = Vec::new();
        crate::path::lookup(&input.value, &field.fields[0], &mut |value| {
            items.push(value);
            Ok::<_, Error>(())
        })?;
        return Ok(if items.is_empty() {
            Operand::Missing
        } else {
            Operand::One(Value::kept(items))
        });
    }
    fn is_path(node: &Node) -> bool {
        match &node.kind {
            Kind::Path(path) => !path.fields.is_empty(),
            Kind::Keep(child, _) => is_path(child),
            Kind::Wildcard | Kind::Sort(..) => true,
            _ => false,
        }
    }
    let force = path && is_path(node);
    // A call's sequence is normalized after this postfix operator, while a
    // parenthesized/retained value has already crossed that boundary.
    let result = match &node.kind {
        Kind::Builtin(builtin, args) => builtin.evaluate(args, input, node.offset)?,
        Kind::Call(target, args) => crate::function::call(target, args, input, node.offset)?,
        Kind::Binary(crate::expression::Op::Chain, left, right) => {
            crate::function::chain(left, right, input, node.offset)?
        }
        _ => node.run(input)?,
    };
    Ok(match result {
        Operand::Missing => Operand::Missing,
        Operand::One(value) if force && (!value.is_array() || value.preserves_array()) => {
            Operand::One(Value::kept(vec![value]))
        }
        Operand::One(value) if value.unpacks_sequence() => {
            let items: Vec<_> = value.elements().collect();
            if items.is_empty() {
                Operand::Missing
            } else {
                Operand::One(Value::kept(items))
            }
        }
        Operand::Many(stream) => {
            let mut items = Vec::new();
            stream.visit(|v| {
                items.push(v);
                Ok(())
            })?;
            Operand::One(Value::kept(items))
        }
        other => other,
    })
}
