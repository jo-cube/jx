mod copy;
mod rewrite;
mod update;
use crate::{Error, Value, expression::Node, sequence::Context};
pub use copy::CopiedValue;
pub(crate) use copy::clone;
use std::rc::Rc;

#[derive(Clone, Debug)]
pub(crate) struct Definition<B = Node> {
    pub pattern: B,
    pub update: B,
    pub delete: Option<B>,
}
// Selection, update and deletion are evaluated by their owning backend. Identity,
// copy membership, rewrite order and validation stay in these common kernels.
pub(crate) trait Body {
    fn offset(&self) -> usize;
    fn depth(&self) -> usize;
    fn evaluate<'e, 'i>(
        &'e self,
        context: &Context<'e, 'i>,
    ) -> Result<Option<Value<'e, 'i>>, Error>;
}
impl Body for Node {
    fn offset(&self) -> usize {
        self.offset
    }
    fn depth(&self) -> usize {
        self.depth
    }
    fn evaluate<'e, 'i>(
        &'e self,
        context: &Context<'e, 'i>,
    ) -> Result<Option<Value<'e, 'i>>, Error> {
        crate::retain::materialize(self, context)
    }
}
impl<B: Body> Definition<B> {
    pub fn depth(&self) -> usize {
        self.pattern
            .depth()
            .max(self.update.depth())
            .max(self.delete.as_ref().map_or(0, |d| d.depth()))
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
pub(crate) fn invoke<'e, 'i, B: Body>(
    definition: &'e Definition<B>,
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
