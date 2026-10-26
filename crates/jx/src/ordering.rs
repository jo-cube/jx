mod keys;
use crate::{Error, Value, json::string, value::type_error};
pub(crate) use keys::Keys;
use std::cmp::Ordering;

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

pub(crate) fn indices(
    length: usize,
    mut compare: impl FnMut(usize, usize) -> Result<Ordering, Error>,
) -> Result<Vec<usize>, Error> {
    let mut indices: Vec<_> = (0..length).collect();
    let mut scratch = vec![0; length];
    merge_sort(&mut indices, &mut scratch, &mut compare)?;
    Ok(indices)
}

fn values(
    a: Option<Value<'_, '_>>,
    b: Option<Value<'_, '_>>,
    offset: usize,
) -> Result<Ordering, Error> {
    Ok(match (a, b) {
        (None, None) => Ordering::Equal,
        (None, _) => Ordering::Greater,
        (_, None) => Ordering::Less,
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
    })
}
