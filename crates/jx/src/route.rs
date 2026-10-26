use crate::{
    Value,
    evaluate::Operand,
    sequence::{Halt, Output, View, Walk},
};

pub(crate) fn map_items<'e, 'i>(
    input: View<'_, 'e, 'i>,
    last: bool,
    output: &mut Output<'_, 'e, 'i>,
    mut evaluate: impl FnMut(Value<'e, 'i>, &mut dyn FnMut(View<'_, 'e, 'i>) -> Walk) -> Walk,
) -> Result<Option<Value<'e, 'i>>, Halt> {
    let mut defined = false;
    let mut pending = None;
    input.transform(false, output, |value, output| {
        let mut mapped = |view: View<'_, 'e, 'i>| {
            if matches!(view, View::Operand(Operand::Missing)) {
                return Ok(());
            }
            if let Some(value) = pending.take() {
                emit_mapped(value, output)?;
            }
            if last && !defined {
                defined = true;
                if let View::Operand(Operand::One(value)) = view
                    && value.is_array()
                    && !value.is_sequence()
                {
                    pending = Some(value.clone());
                    return Ok(());
                }
            }
            defined = true;
            match &view {
                View::Operand(Operand::One(value)) if value.preserves_array() => {
                    output(value.clone())
                }
                _ => view.candidates(false, output),
            }
        };
        evaluate(value, &mut |view| view.resolve(&mut mapped))
    })?;
    Ok(pending)
}

fn emit_mapped<'e, 'i>(value: Value<'e, 'i>, output: &mut Output<'_, 'e, 'i>) -> Walk {
    if value.preserves_array() {
        output(value)
    } else {
        View::Operand(&Operand::One(value)).candidates(false, output)
    }
}
