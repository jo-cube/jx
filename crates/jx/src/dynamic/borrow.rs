use super::Definition;
use crate::{
    Function, Value,
    container::{Array, Object},
    function::{FunctionKind, composition::Argument},
};
use std::rc::Rc;

pub(crate) fn collect(v: &Value<'_, '_>, definitions: &mut Vec<Rc<Definition>>) {
    match v {
        Value::Function(f) => match &f.kind {
            FunctionKind::Dynamic(c) => {
                if !definitions.iter().any(|d| Rc::ptr_eq(d, &c.definition)) {
                    definitions.push(c.definition.clone());
                }
                collect(&c.focus, definitions);
            }
            FunctionKind::Lambda { focus, .. } => collect(focus, definitions),
            FunctionKind::Partial { target, arguments } => {
                collect(&Value::Function(target.clone()), definitions);
                for a in arguments {
                    if let Argument::Value(Some(v)) = a {
                        collect(v, definitions)
                    }
                }
            }
            FunctionKind::Chain(a, b) => {
                collect(&Value::Function(a.clone()), definitions);
                collect(&Value::Function(b.clone()), definitions)
            }
            _ => {}
        },
        Value::Array(a) => {
            for v in &a.items {
                collect(v, definitions)
            }
        }
        Value::Object(o) => {
            for (_, v) in &o.members {
                collect(v, definitions)
            }
        }
        Value::Copied(c) => collect(&c.source, definitions),
        _ => {}
    }
}
pub(crate) fn contains(v: &Value<'_, '_>) -> bool {
    match v {
        Value::Function(f) => match &f.kind {
            FunctionKind::Dynamic(_) => true,
            FunctionKind::Lambda { focus, .. } => contains(focus),
            FunctionKind::Partial { target, arguments } => {
                contains(&Value::Function(target.clone()))
                    || arguments
                        .iter()
                        .any(|a| matches!(a,Argument::Value(Some(v)) if contains(v)))
            }
            FunctionKind::Chain(a, b) => {
                contains(&Value::Function(a.clone())) || contains(&Value::Function(b.clone()))
            }
            _ => false,
        },
        Value::Array(a) => a.items.iter().any(contains),
        Value::Object(o) => o.members.iter().any(|(_, v)| contains(v)),
        Value::Copied(c) => contains(&c.source),
        _ => false,
    }
}
// Loan owned definitions for the entire dynamic region. Recursive references
// then use the existing call/tail loop instead of opening a bridge per tail hop.
pub(crate) fn value<'d, 'e: 'd, 'i>(
    v: &Value<'e, 'i>,
    definitions: &'d [Rc<Definition>],
) -> Value<'d, 'i> {
    if !contains(v) {
        return v.clone();
    }
    match v {
        Value::Function(f) => {
            let kind = match &f.kind {
                FunctionKind::Dynamic(c) => {
                    let d = definitions
                        .iter()
                        .find(|d| Rc::ptr_eq(d, &c.definition))
                        .expect("collected dynamic definition");
                    match d.as_ref() {
                        Definition::Lambda(definition) => FunctionKind::Lambda {
                            definition,
                            focus: value(&c.focus, definitions),
                            wrapped: c.wrapped,
                            frame: c.frame,
                        },
                        Definition::Transform(definition) => FunctionKind::Transform {
                            definition,
                            frame: c.frame,
                        },
                    }
                }
                FunctionKind::Lambda {
                    definition,
                    focus,
                    wrapped,
                    frame,
                } => FunctionKind::Lambda {
                    definition,
                    focus: value(focus, definitions),
                    wrapped: *wrapped,
                    frame: *frame,
                },
                FunctionKind::Partial { target, arguments } => FunctionKind::Partial {
                    target: function(target, definitions),
                    arguments: arguments
                        .iter()
                        .map(|a| match a {
                            Argument::Hole => Argument::Hole,
                            Argument::Value(v) => {
                                Argument::Value(v.as_ref().map(|v| value(v, definitions)))
                            }
                        })
                        .collect(),
                },
                FunctionKind::Chain(a, b) => {
                    FunctionKind::Chain(function(a, definitions), function(b, definitions))
                }
                _ => unreachable!(),
            };
            Value::Function(Rc::new(Function { kind }))
        }
        Value::Array(a) => Value::Array(Rc::new(Array {
            items: a.items.iter().map(|v| value(v, definitions)).collect(),
            shape: a.shape,
        })),
        Value::Object(o) => Value::Object(Rc::new(Object {
            members: o
                .members
                .iter()
                .map(|(k, v)| (k.clone(), value(v, definitions)))
                .collect(),
        })),
        Value::Copied(c) => Value::Copied(Rc::new(crate::CopiedValue {
            source: value(&c.source, definitions),
            identity: c.identity.clone(),
        })),
        _ => unreachable!(),
    }
}
fn function<'d, 'e: 'd, 'i>(
    f: &Rc<Function<'e, 'i>>,
    definitions: &'d [Rc<Definition>],
) -> Rc<Function<'d, 'i>> {
    let Value::Function(f) = value(&Value::Function(f.clone()), definitions) else {
        unreachable!()
    };
    f
}
