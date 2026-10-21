//! Retained grouping and ordering boundaries. Keys/values are ordinary compiled
//! programs; shared kernels own key equality, duplicates and comparison order.
use super::{
    frame::Frame,
    run::{Activation, Cell},
    *,
};
use crate::Error;
pub(super) fn execute<'e, 'i>(
    op: &'e Operation,
    a: Activation<'_, 'e, 'i, '_>,
    registers: &[Cell<'e, 'i>],
    offset: usize,
) -> Result<Cell<'e, 'i>, Error> {
    let run = |body: &'e Program, value: Value<'e, 'i>, frame: &mut Frame<'e, 'i>| {
        let focus = a.input.restore(Some(value));
        body.run(
            Activation {
                focus: &focus,
                wrapped: false,
                ..a
            },
            frame,
        )
        .map(|v| v.into_value(a.input))
    };
    match op {
        Operation::Group(source, pairs) => {
            let value = source
                .map_or_else(|| a.focus.value(a.input), |r| registers[r].value(a.input))
                .unwrap_or(Value::Undefined);
            let mut groups = crate::construct::Groups::new(pairs.len());
            let mut frame = Frame::repeated();
            let mut add = |value: Value<'e, 'i>| {
                groups.add_with(
                    pairs.len(),
                    (!matches!(value, Value::Undefined)).then_some(value.clone()),
                    offset,
                    |pair| {
                        run(&pairs[pair].0, value.clone(), &mut frame).map(|v| {
                            v.map_or(
                                crate::evaluate::Operand::Missing,
                                crate::evaluate::Operand::One,
                            )
                        })
                    },
                )
            };
            if value.is_array() && (source.is_some() || !a.wrapped) {
                let mut empty = true;
                for item in value.elements() {
                    empty = false;
                    add(item)?;
                }
                if empty {
                    add(Value::Undefined)?;
                }
            } else {
                add(value)?;
            }
            Ok(Cell::Value(groups.finish_with(|values, pair| {
                run(
                    &pairs[pair].1,
                    crate::construct::group_value(values),
                    &mut frame,
                )
            })?))
        }
        Operation::Order(source, terms) => {
            let Some(value) = registers[*source].value(a.input) else {
                return Ok(Cell::Missing);
            };
            let mut items = if value.is_array() {
                value.elements().collect::<Vec<_>>()
            } else {
                vec![value]
            };
            if items.len() < 2 {
                return Ok(items
                    .pop()
                    .filter(|v| !matches!(v, Value::Undefined))
                    .map_or(Cell::Missing, Cell::Value));
            }
            let mut keys = crate::ordering::Keys::with_terms(
                items.len(),
                terms.len(),
                terms.iter().any(|(_, _, effects)| !effects),
            );
            let mut frame = Frame::repeated();
            let indices = crate::ordering::indices(items.len(), |left, right| {
                keys.compare_with(
                    left,
                    right,
                    terms
                        .iter()
                        .map(|(_, descending, effects)| (*effects, *descending)),
                    offset,
                    |row, term| run(&terms[term].0, items[row].clone(), &mut frame),
                )
            })?;
            Ok(Cell::Value(Value::array(
                indices.into_iter().map(|i| items[i].clone()).collect(),
                false,
            )))
        }
        _ => unreachable!("retained candidate boundary"),
    }
}
