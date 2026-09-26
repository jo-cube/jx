use crate::{
    Error, Value, evaluate::Operand, expression::Node, json::string, sequence::Context,
    value::type_error,
};
use std::cmp::Ordering;

pub(crate) fn evaluate<'e, 'i>(
    base: &'e Node,
    terms: &'e [(Node, bool)],
    context: &Context<'e, 'i>,
    offset: usize,
) -> Result<Operand<'e, 'i>, Error> {
    let mut items = Vec::new();
    crate::retain::visit(base, context, &mut |value| items.push(value))?;
    if items.len() == 1 && items[0].is_array() {
        items = items.pop().unwrap().elements().collect();
    }
    match items.len() {
        0 => return Ok(Operand::Missing),
        1 => {
            return Ok(match items.pop().unwrap() {
                Value::Undefined => Operand::Missing,
                value => Operand::One(value),
            });
        }
        _ => {}
    }
    let mut indices: Vec<_> = (0..items.len()).collect();
    let mut scratch = vec![0; items.len()];
    merge_sort(&mut indices, &mut scratch, &mut |a, b| {
        compare(&items[a], &items[b], terms, context, offset)
    })?;
    Ok(Operand::One(Value::array(
        indices
            .into_iter()
            .map(|index| items[index].clone())
            .collect(),
        false,
    )))
}

fn compare<'e, 'i>(
    left: &Value<'e, 'i>,
    right: &Value<'e, 'i>,
    terms: &'e [(Node, bool)],
    context: &Context<'e, 'i>,
    offset: usize,
) -> Result<Ordering, Error> {
    for (term, descending) in terms {
        let a = crate::retain::materialize(
            term,
            &Context {
                value: left.clone(),
                wrapped: false,
                scope: context.scope.clone(),
            },
        )?;
        let b = crate::retain::materialize(
            term,
            &Context {
                value: right.clone(),
                wrapped: false,
                scope: context.scope.clone(),
            },
        )?;
        let order = match (a, b) {
            (None, None) => continue,
            (None, _) => return Ok(Ordering::Greater),
            (_, None) => return Ok(Ordering::Less),
            (Some(a), Some(b)) => match (a.atomic(), b.atomic()) {
                (Value::Number(a), Value::Number(b)) => {
                    // NaN is unequal and not less in the reference comparator.
                    if a == b {
                        Ordering::Equal
                    } else if a < b {
                        Ordering::Less
                    } else {
                        Ordering::Greater
                    }
                }
                (a, b) => {
                    let (Some(a), Some(b)) = (a.string_body(), b.string_body()) else {
                        return Err(type_error(offset));
                    };
                    string::units(a).cmp(string::units(b))
                }
            },
        };
        if order != Ordering::Equal {
            return Ok(if *descending { order.reverse() } else { order });
        }
    }
    Ok(Ordering::Equal)
}

// Stable, fallible top-down merge order also fixes the observable order of
// comparator expression evaluation. Keys may have lexical effects or fail.
fn merge_sort(
    indices: &mut [usize],
    scratch: &mut [usize],
    compare: &mut impl FnMut(usize, usize) -> Result<Ordering, Error>,
) -> Result<(), Error> {
    if indices.len() < 2 {
        return Ok(());
    }
    let mid = indices.len() / 2;
    let (left, right) = indices.split_at_mut(mid);
    let (sl, sr) = scratch.split_at_mut(mid);
    merge_sort(left, sl, compare)?;
    merge_sort(right, sr, compare)?;
    let (mut l, mut r) = (0, mid);
    for slot in scratch.iter_mut() {
        let from_left = r == indices.len()
            || (l < mid && compare(indices[l], indices[r])? != Ordering::Greater);
        *slot = if from_left {
            let v = indices[l];
            l += 1;
            v
        } else {
            let v = indices[r];
            r += 1;
            v
        };
    }
    indices.copy_from_slice(scratch);
    Ok(())
}
