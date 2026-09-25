use crate::{
    Value,
    evaluate::Operand,
    expression::{Kind, Node},
    sequence::{Context, Halt, Output, View, Walk},
    value::range_error,
};
use std::cell::Cell;

pub(crate) fn with_filters<'e, 'i>(
    base: &'e Node,
    predicates: &'e [Node],
    context: &Context<'e, 'i>,
    output: &mut dyn FnMut(View<'_, 'e, 'i>) -> Walk,
) -> Walk {
    let Some((predicate, previous)) = predicates.split_last() else {
        return output(View::Operand(&base.run(context)?));
    };
    with_filters(base, previous, context, &mut |input| {
        if let Kind::Number(index) = predicate.kind {
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
        } else {
            let filter = Filter {
                input,
                predicate,
                length: Cell::new(None),
                context,
            };
            if predicate.effects {
                let mut items = Vec::new();
                filter.walk(&mut |value| {
                    items.push(value);
                    Ok(())
                })?;
                output(View::Items(&items))
            } else {
                output(View::Filter(&filter))
            }
        }
    })
}
pub(crate) struct Filter<'s, 'e, 'i> {
    input: View<'s, 'e, 'i>,
    predicate: &'e Node,
    length: Cell<Option<usize>>,
    context: &'s Context<'e, 'i>,
}
impl<'e, 'i> Filter<'_, 'e, 'i> {
    pub fn walk(&self, output: &mut Output<'_, 'e, 'i>) -> Walk {
        let mut index = 0;
        self.input.transform(true, output, |value, output| {
            let context = Context {
                value,
                wrapped: false,
                scope: self.context.scope.clone(),
            };
            let predicate = self.predicate.run(&context)?;
            let numeric = numbers(&predicate, self.predicate.offset, &mut |_| Ok(()))?;
            if numeric {
                numbers(&predicate, self.predicate.offset, &mut |number| {
                    let number = number.floor();
                    let target = if number < 0.0 {
                        self.length()? as f64 + number
                    } else {
                        number
                    };
                    if target == index as f64 {
                        output(context.value.clone())?;
                    }
                    Ok(())
                })?;
            } else if predicate.truth(self.predicate.offset)? {
                output(context.value)?;
            }
            index += 1;
            Ok(())
        })
    }
    fn length(&self) -> Result<usize, Halt> {
        if let Some(length) = self.length.get() {
            return Ok(length);
        }
        let mut length = 0;
        self.input.candidates(true, &mut |_| {
            length += 1;
            Ok(())
        })?;
        self.length.set(Some(length));
        Ok(length)
    }
}

// Numeric lists preserve duplicate matching positions. Inspect the entire list
// before choosing positional versus effective-boolean semantics (mixed lists).
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
