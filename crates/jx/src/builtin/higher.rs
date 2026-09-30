use super::{
    collections::{items, key},
    library::Library,
};
use crate::{Error, Value, evaluate::Operand, function, sequence::Context, value::type_error};

pub(super) fn call<'e, 'i>(
    function: Library,
    args: &[Option<Value<'e, 'i>>; 3],
    context: &Context<'e, 'i>,
    offset: usize,
) -> Result<Option<Value<'e, 'i>>, Error> {
    let Some(input) = &args[0] else {
        return Ok((function == Library::Each).then(|| Value::sequence(Vec::new())));
    };
    let Some(Value::Function(callback)) = &args[1] else {
        if function == Library::Sift && input.members().next().is_none() {
            return Ok(None);
        }
        return Err(type_error(offset));
    };
    let arity = function::arity(callback);
    let invoke = |arguments: &[Option<Value<'e, 'i>>]| -> Result<Option<Value<'e, 'i>>, Error> {
        match function::invoke(callback, arguments, context, offset)? {
            Operand::Missing => Ok(None),
            Operand::One(value) => Ok(Some(value)),
            Operand::Many(stream) => crate::retain::collect(|emit| {
                stream.visit(|v| {
                    emit(v);
                    Ok(())
                })
            }),
        }
    };
    if function == Library::Reduce {
        if arity < 2 {
            return Err(type_error(offset));
        }
        let mut result = args[2].clone();
        // Only callbacks requesting the original array need a scalar wrapper.
        let whole = (arity >= 4).then(|| array(input));
        for (index, item) in items(input).enumerate() {
            if index == 0 && result.is_none() {
                result = Some(item);
                continue;
            }
            let arguments = [
                result,
                Some(item),
                Some(Value::Number(index as f64)),
                whole.clone(),
            ];
            result = invoke(&arguments[..arity.clamp(2, 4)])?;
        }
        return Ok(result);
    }
    let mut output = Vec::new();
    if matches!(function, Library::Each | Library::Sift) {
        let mut members = Vec::new();
        for (name, value) in crate::members::entries(input) {
            let name = key(name);
            let arguments = [Some(value.clone()), Some(name.clone()), Some(input.clone())];
            let result = invoke(&arguments[..arity.clamp(1, 3)])?;
            if function == Library::Sift {
                if let Some(result) = result
                    && result.truth(offset)?
                {
                    members.push((name, value));
                }
            } else if let Some(value) = result {
                output.push(value);
            }
        }
        return Ok(if function == Library::Sift {
            (!members.is_empty()).then(|| Value::object(members))
        } else {
            Some(Value::sequence(output))
        });
    }
    let whole = (arity >= 3).then(|| array(input));
    for (index, value) in items(input).enumerate() {
        let arguments = [
            Some(value.clone()),
            Some(Value::Number(index as f64)),
            whole.clone(),
        ];
        let result = invoke(&arguments[..arity.clamp(1, 3)])?;
        if function == Library::Filter {
            if let Some(result) = result
                && result.truth(offset)?
            {
                output.push(value);
            }
        } else if let Some(value) = result {
            output.push(value);
        }
    }
    Ok(Some(Value::sequence(output)))
}
fn array<'e, 'i>(value: &Value<'e, 'i>) -> Value<'e, 'i> {
    if value.is_array() {
        value.clone()
    } else {
        Value::array(vec![value.clone()], false)
    }
}
