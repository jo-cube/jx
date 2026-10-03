use super::{Callable, Definition};
use crate::{
    Function, Value,
    container::{Array, Object},
    function::{FunctionKind, composition::Argument},
};
use std::{collections::HashMap, rc::Rc};

type Key = (u8, usize, usize);
fn key(v: &Value<'_, '_>) -> Option<Key> {
    Some(match v {
        Value::Array(v) => (0, Rc::as_ptr(v) as usize, 0),
        Value::Object(v) => (1, Rc::as_ptr(v) as usize, 0),
        Value::Function(v) => (2, Rc::as_ptr(v) as usize, 0),
        Value::Copied(v) => (3, Rc::as_ptr(v) as usize, 0),
        Value::Constant(v) => (
            4,
            v.data as *const _ as usize,
            Rc::as_ptr(&v.identity) as usize,
        ),
        Value::StringLiteral(v) => (5, v.as_bytes().as_ptr() as usize, v.as_bytes().len()),
        _ => return None,
    })
}
fn loan(v: &Value<'_, '_>) -> Option<(usize, usize)> {
    let Value::Function(f) = v else {
        return None;
    };
    match &f.kind {
        FunctionKind::Lambda {
            definition, frame, ..
        } => Some((*definition as *const _ as usize, *frame)),
        FunctionKind::Transform { definition, frame } => {
            Some((*definition as *const _ as usize, *frame))
        }
        FunctionKind::Dynamic(c) => Some((
            match c.definition.as_ref() {
                Definition::Lambda(d) => d as *const _ as usize,
                Definition::Transform(d) => d as *const _ as usize,
            },
            c.frame,
        )),
        _ => None,
    }
}
fn same_loan(a: &Value<'_, '_>, b: &Value<'_, '_>) -> bool {
    if loan(a).is_none() || loan(a) != loan(b) {
        return false;
    }
    fn focus<'a, 'e, 'i>(v: &'a Value<'e, 'i>) -> Option<(&'a Value<'e, 'i>, bool)> {
        let Value::Function(f) = v else {
            return None;
        };
        match &f.kind {
            FunctionKind::Lambda { focus, wrapped, .. } => Some((focus, *wrapped)),
            FunctionKind::Dynamic(c) if matches!(c.definition.as_ref(), Definition::Lambda(_)) => {
                Some((&c.focus, c.wrapped))
            }
            _ => None,
        }
    }
    match (focus(a), focus(b)) {
        (Some((a, aw)), Some((b, bw))) => aw == bw && same(a, b),
        (None, None) => true,
        _ => false,
    }
}
pub(crate) fn same(a: &Value<'_, '_>, b: &Value<'_, '_>) -> bool {
    match (a, b) {
        (Value::Raw(a), Value::Raw(b)) => {
            a.as_bytes().as_ptr() == b.as_bytes().as_ptr()
                && a.as_bytes().len() == b.as_bytes().len()
        }
        (Value::Number(a), Value::Number(b)) => a.to_bits() == b.to_bits(),
        (Value::Boolean(a), Value::Boolean(b)) => a == b,
        (Value::Null, Value::Null) | (Value::Undefined, Value::Undefined) => true,
        (Value::String(a), Value::String(b)) => a.body_pointer() == b.body_pointer(),
        _ => (key(a).is_some() && key(a) == key(b)) || same_loan(a, b),
    }
}
#[derive(Default)]
pub(crate) struct Retention<'e, 'i> {
    values: HashMap<Key, Value<'e, 'i>>,
    states: HashMap<usize, Rc<crate::matcher::State<'e>>>,
}
impl<'e, 'i> Retention<'e, 'i> {
    // Imported values already have the caller's lifetime and retain identity.
    pub fn seed(&mut self, v: &Value<'e, 'i>) {
        let Some(k) = key(v) else { return };
        if self.values.insert(k, v.clone()).is_some() {
            return;
        }
        if matches!(v,Value::Function(f) if matches!(f.kind,FunctionKind::Dynamic(_)))
            && let Some((definition, frame)) = loan(v)
        {
            self.values.insert((6, definition, frame), v.clone());
        }
        match v {
            Value::Array(a) => {
                for v in &a.items {
                    self.seed(v)
                }
            }
            Value::Object(o) => {
                for (k, v) in &o.members {
                    self.seed(k);
                    self.seed(v)
                }
            }
            Value::Copied(c) => self.seed(&c.source),
            Value::Function(f) => match &f.kind {
                FunctionKind::Lambda { focus, .. } => self.seed(focus),
                FunctionKind::Dynamic(c) => self.seed(&c.focus),
                FunctionKind::Partial { target, arguments } => {
                    self.seed(&Value::Function(target.clone()));
                    for arg in arguments {
                        if let Argument::Value(Some(v)) = arg {
                            self.seed(v)
                        }
                    }
                }
                FunctionKind::Chain(a, b) => {
                    self.seed(&Value::Function(a.clone()));
                    self.seed(&Value::Function(b.clone()));
                }
                FunctionKind::Matcher(s) => {
                    self.states.insert(Rc::as_ptr(s) as usize, s.clone());
                }
                FunctionKind::MatchNext(n) => {
                    self.states
                        .insert(Rc::as_ptr(n.state()) as usize, n.state().clone());
                    self.seed(n.value());
                }
                _ => {}
            },
            _ => {}
        }
    }
    pub fn value(&mut self, v: &Value<'_, 'i>) -> Value<'e, 'i> {
        if let Some(old) = key(v).and_then(|k| self.values.get(&k)).or_else(|| {
            loan(v).and_then(|(d, f)| self.values.get(&(6, d, f)).filter(|old| same_loan(old, v)))
        }) {
            return old.clone();
        }
        let result = match v {
            Value::Raw(v) => Value::Raw(*v),
            Value::Number(v) => Value::Number(*v),
            Value::Boolean(v) => Value::Boolean(*v),
            Value::Null => Value::Null,
            Value::Undefined => Value::Undefined,
            Value::String(v) => Value::String(v.clone()),
            Value::StringLiteral(v) => Value::String(crate::OwnedString::body(
                &v.as_str()[1..v.as_str().len() - 1],
            )),
            Value::Array(a) => Value::Array(Rc::new(Array {
                items: a.items.iter().map(|v| self.value(v)).collect(),
                shape: a.shape,
            })),
            Value::Object(o) => Value::Object(Rc::new(Object {
                members: o
                    .members
                    .iter()
                    .map(|(k, v)| (self.value(k), self.value(v)))
                    .collect(),
            })),
            Value::Copied(c) => Value::Copied(Rc::new(crate::CopiedValue {
                source: self.value(&c.source),
                identity: c.identity.clone(),
            })),
            Value::Constant(c) => {
                if let Some(shape) = c.shape() {
                    Value::Array(Rc::new(Array {
                        items: v.elements().map(|v| self.value(&v)).collect(),
                        shape,
                    }))
                } else {
                    Value::object(
                        v.members()
                            .map(|(k, v)| {
                                (Value::String(crate::OwnedString::body(k)), self.value(&v))
                            })
                            .collect(),
                    )
                }
            }
            Value::Function(f) => Value::Function(Rc::new(Function {
                kind: match &f.kind {
                    FunctionKind::Builtin(b) => FunctionKind::Builtin(*b),
                    FunctionKind::Host(f) => FunctionKind::Host(f.clone()),
                    FunctionKind::Lambda {
                        definition,
                        focus,
                        wrapped,
                        frame,
                    } => FunctionKind::Dynamic(Box::new(Callable {
                        definition: Rc::new(Definition::Lambda((*definition).clone())),
                        focus: self.value(focus),
                        wrapped: *wrapped,
                        frame: *frame,
                    })),
                    FunctionKind::Transform { definition, frame } => {
                        FunctionKind::Dynamic(Box::new(Callable {
                            definition: Rc::new(Definition::Transform((*definition).clone())),
                            focus: Value::Undefined,
                            wrapped: false,
                            frame: *frame,
                        }))
                    }
                    FunctionKind::Dynamic(c) => FunctionKind::Dynamic(Box::new(Callable {
                        definition: c.definition.clone(),
                        focus: self.value(&c.focus),
                        wrapped: c.wrapped,
                        frame: c.frame,
                    })),
                    FunctionKind::Partial { target, arguments } => FunctionKind::Partial {
                        target: self.function(target),
                        arguments: arguments
                            .iter()
                            .map(|a| match a {
                                Argument::Hole => Argument::Hole,
                                Argument::Value(v) => {
                                    Argument::Value(v.as_ref().map(|v| self.value(v)))
                                }
                            })
                            .collect(),
                    },
                    FunctionKind::Chain(a, b) => {
                        FunctionKind::Chain(self.function(a), self.function(b))
                    }
                    FunctionKind::Matcher(s) => FunctionKind::Matcher(self.state(s)),
                    FunctionKind::MatchNext(n) => {
                        let state = self.state(n.state());
                        let value = self.value(n.value());
                        FunctionKind::MatchNext(Rc::new(n.retained(state, value)))
                    }
                },
            })),
        };
        if let Some(k) = key(v) {
            self.values.insert(k, result.clone());
        }
        result
    }
    fn function(&mut self, f: &Rc<Function<'_, 'i>>) -> Rc<Function<'e, 'i>> {
        let Value::Function(f) = self.value(&Value::Function(f.clone())) else {
            unreachable!()
        };
        f
    }
    fn state(&mut self, s: &Rc<crate::matcher::State<'_>>) -> Rc<crate::matcher::State<'e>> {
        let key = Rc::as_ptr(s) as usize;
        self.states
            .entry(key)
            .or_insert_with(|| Rc::new(s.retained()))
            .clone()
    }
}
