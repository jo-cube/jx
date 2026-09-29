use crate::{Error, Value, expression::Node, sequence::Context};

// Containers and lexical storage retain evaluated values; already retained
// sequences pass through without copying their members.
pub(crate) fn visit<'e, 'i>(
    node: &'e Node,
    context: &Context<'e, 'i>,
    output: &mut dyn FnMut(Value<'e, 'i>),
) -> Result<(), Error> {
    let mut emit = |value| {
        output(value);
        Ok(())
    };
    let result = node.consume(context, &mut emit);
    match result {
        Ok(()) => Ok(()),
        Err(crate::sequence::Halt::Evaluation(error)) => Err(error),
        Err(crate::sequence::Halt::Stop) => unreachable!("construction consumes its members"),
    }
}
pub(crate) fn materialize<'e, 'i>(
    node: &'e Node,
    context: &Context<'e, 'i>,
) -> Result<Option<Value<'e, 'i>>, Error> {
    let stream = match node.stream(context) {
        Some(stream) => stream,
        None => match node.run(context)? {
            crate::evaluate::Operand::Missing => return Ok(None),
            crate::evaluate::Operand::One(value) => return Ok(Some(value)),
            crate::evaluate::Operand::Many(stream) => stream,
        },
    };
    collect(|output| {
        stream.visit(|value| {
            output(value);
            Ok(())
        })
    })
}

pub(crate) fn collect<'e, 'i>(
    walk: impl FnOnce(&mut dyn FnMut(Value<'e, 'i>)) -> Result<(), Error>,
) -> Result<Option<Value<'e, 'i>>, Error> {
    let mut first = None;
    let mut values = Vec::new();
    walk(&mut |value| {
        if !values.is_empty() {
            values.push(value);
        } else if let Some(first) = first.take() {
            values.push(first);
            values.push(value);
        } else {
            first = Some(value);
        }
    })?;
    Ok(if !values.is_empty() {
        Some(Value::sequence(values))
    } else {
        first.filter(|value| !matches!(value, Value::Undefined))
    })
}
