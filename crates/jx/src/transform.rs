mod copy;
mod rewrite;
mod update;
use crate::{Error, Value, expression::Node, sequence::Context};
pub use copy::CopiedValue;
pub(crate) use copy::clone;
use std::rc::Rc;

#[derive(Clone, Debug)]
pub(crate) struct Definition {
    pub pattern: Node,
    pub update: Node,
    pub delete: Option<Node>,
}
impl Definition {
    pub fn depth(&self) -> usize {
        self.pattern
            .depth
            .max(self.update.depth)
            .max(self.delete.as_ref().map_or(0, |d| d.depth))
    }
}
pub(crate) fn literal<'e, 'i>(
    definition: &'e Definition,
    context: &Context<'e, 'i>,
) -> Value<'e, 'i> {
    Value::Function(Rc::new(crate::Function {
        kind: crate::function::FunctionKind::Transform {
            definition,
            frame: context.scope.as_ref().expect("transform runtime").capture(),
        },
    }))
}
pub(crate) fn invoke<'e, 'i>(
    definition: &'e Definition,
    frame: usize,
    args: &[Option<Value<'e, 'i>>],
    context: &Context<'e, 'i>,
    offset: usize,
) -> Result<crate::evaluate::Operand<'e, 'i>, Error> {
    let [value] = args else {
        return Err(crate::value::type_error(offset));
    };
    let Some(value) = value.as_ref().filter(|v| !matches!(v, Value::Undefined)) else {
        return Ok(crate::evaluate::Operand::Missing);
    };
    if !value.is_array() && !value.is_object() {
        return Err(crate::value::type_error(offset));
    }
    let scope = context.scope.as_ref().expect("transform runtime");
    scope.call(offset, definition.depth(), || {
        let child = scope.at(frame);
        let result = (|| {
            if let Some(cloner) = child.lookup("clone") {
                let Value::Function(function) = cloner else {
                    return Err(crate::value::type_error(offset));
                };
                if !matches!(
                    function.kind,
                    crate::function::FunctionKind::Builtin(crate::builtin::Builtin::Library(
                        crate::builtin::library::Library::Clone
                    ))
                ) {
                    return Err(Error::new(
                        crate::ErrorKind::UnsupportedExpression,
                        offset,
                        "overridden transform cloning is deferred",
                    ));
                }
            }
            let copied = clone(Some(value.clone()), offset)?.unwrap();
            let local = Context {
                value: copied.clone(),
                wrapped: false,
                scope: Some(child.clone()),
            };
            rewrite::apply(definition, copied, &local)
        })();
        result.map(crate::evaluate::Operand::One)
    })
}

pub(crate) fn write(
    value: &CopiedValue<'_, '_>,
    output: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    if value.source.is_array() {
        output.write_all(b"[")?;
        for (i, item) in value.source.elements().enumerate() {
            if i != 0 {
                output.write_all(b",")?;
            }
            value.child(item).write_compact(&mut *output)?;
        }
        output.write_all(b"]")
    } else {
        output.write_all(b"{")?;
        for (i, (key, item)) in crate::members::entries(&value.source)
            .into_iter()
            .filter(|(_, v)| !matches!(v, Value::Undefined))
            .enumerate()
        {
            if i != 0 {
                output.write_all(b",")?;
            }
            output.write_all(b"\"")?;
            output.write_all(key.as_bytes())?;
            output.write_all(b"\":")?;
            value.child(item).write_compact(&mut *output)?;
        }
        output.write_all(b"}")
    }
}
