use crate::{Value, json};
use std::{
    io::{self, Write},
    rc::Rc,
};

/// Constructed array storage. Members may still borrow input or expression bytes.
#[derive(Debug)]
pub struct Array<'e, 'i> {
    pub(crate) items: Vec<Value<'e, 'i>>,
    pub(crate) shape: Shape,
}
#[derive(Clone, Copy, Debug)]
pub(crate) enum Shape {
    Array,
    Preserved,
    Sequence,
    Kept,
}

impl<'e, 'i> Array<'e, 'i> {
    pub fn as_slice(&self) -> &[Value<'e, 'i>] {
        &self.items
    }
    pub(crate) fn write_compact(&self, output: &mut dyn Write) -> io::Result<()> {
        output.write_all(b"[")?;
        for (index, value) in self.items.iter().enumerate() {
            if index != 0 {
                output.write_all(b",")?;
            }
            value.write_compact(&mut *output)?;
        }
        output.write_all(b"]")
    }
}

/// Constructed object storage. Keys retain their validated JSON string encoding.
#[derive(Debug)]
pub struct Object<'e, 'i> {
    pub(crate) members: Vec<(Value<'e, 'i>, Value<'e, 'i>)>,
}
impl<'e, 'i> Object<'e, 'i> {
    pub fn as_slice(&self) -> &[(Value<'e, 'i>, Value<'e, 'i>)] {
        &self.members
    }
    pub(crate) fn write_compact(&self, output: &mut dyn Write) -> io::Result<()> {
        output.write_all(b"{")?;
        for (index, (key, value)) in self.members.iter().enumerate() {
            if index != 0 {
                output.write_all(b",")?;
            }
            key.write_compact(&mut *output)?;
            output.write_all(b":")?;
            value.write_compact(&mut *output)?;
        }
        output.write_all(b"}")
    }
}

pub(crate) enum Elements<'a, 'e, 'i> {
    Raw(json::Elements<'i>),
    Constant(&'a crate::ConstantValue<'e>, usize),
    Constructed(std::slice::Iter<'a, Value<'e, 'i>>),
}
impl<'e, 'i> Iterator for Elements<'_, 'e, 'i> {
    type Item = Value<'e, 'i>;
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Raw(items) => items.next().map(Value::Raw),
            Self::Constant(value, index) => {
                let item = value.element(*index);
                *index += 1;
                item
            }
            Self::Constructed(items) => items.next().cloned(),
        }
    }
    // Exhaustive consumers dispatch on storage once, outside the item loop.
    fn for_each<F>(self, mut f: F)
    where
        F: FnMut(Self::Item),
    {
        match self {
            Self::Raw(items) => items.for_each(|v| f(Value::Raw(v))),
            Self::Constructed(items) => items.cloned().for_each(f),
            Self::Constant(value, mut index) => {
                while let Some(item) = value.element(index) {
                    f(item);
                    index += 1;
                }
            }
        }
    }
}
impl<'e, 'i> Value<'e, 'i> {
    pub(crate) fn array(items: Vec<Self>, preserve: bool) -> Self {
        Self::Array(Rc::new(Array {
            items,
            shape: if preserve {
                Shape::Preserved
            } else {
                Shape::Array
            },
        }))
    }
    pub(crate) fn sequence(items: Vec<Self>) -> Self {
        Self::Array(Rc::new(Array {
            items,
            shape: Shape::Sequence,
        }))
    }
    pub(crate) fn kept(items: Vec<Self>) -> Self {
        Self::Array(Rc::new(Array {
            items,
            shape: Shape::Kept,
        }))
    }
    pub(crate) fn unpacks_sequence(&self) -> bool {
        matches!(self, Self::Array(array) if matches!(array.shape, Shape::Sequence))
            || matches!(self, Self::Constant(c) if matches!(c.shape(),Some(Shape::Sequence)))
    }
    pub(crate) fn is_sequence(&self) -> bool {
        matches!(self, Self::Array(array) if matches!(array.shape, Shape::Sequence | Shape::Kept))
            || matches!(self, Self::Constant(c) if matches!(c.shape(),Some(Shape::Sequence | Shape::Kept)))
    }
    pub(crate) fn object(members: Vec<(Self, Self)>) -> Self {
        Self::Object(Rc::new(Object { members }))
    }
    pub(crate) fn is_array(&self) -> bool {
        matches!(self, Self::Constant(c) if c.shape().is_some())
            || matches!(self, Self::Array(_))
            || matches!(self, Self::Raw(raw) if raw.is_array())
    }
    pub(crate) fn preserves_array(&self) -> bool {
        matches!(self, Self::Array(array) if matches!(array.shape, Shape::Preserved))
            || matches!(self, Self::Constant(c) if matches!(c.shape(),Some(Shape::Preserved)))
    }
    pub(crate) fn elements(&self) -> Elements<'_, 'e, 'i> {
        match self {
            Self::Raw(raw) => Elements::Raw(raw.elements()),
            Self::Constant(c) => Elements::Constant(c, 0),
            Self::Array(array) => Elements::Constructed(array.items.iter()),
            _ => unreachable!("array value"),
        }
    }
    pub(crate) fn field(&self, field: &str) -> Option<Self> {
        match self {
            Self::Raw(raw) => raw.field(field).map(Self::Raw),
            Self::Constant(c) => c.field(|k| json::string::units(k).cmp(field.encode_utf16())),
            Self::Object(object) => object
                .members
                .iter()
                .find(|(key, _)| json::string::matches(key.string_body().unwrap(), field))
                .map(|(_, value)| value.clone()),
            _ => None,
        }
    }
    pub(crate) fn string_body(&self) -> Option<&str> {
        let raw = self.json()?;
        let text = raw.as_str();
        (text.as_bytes()[0] == b'"').then(|| &text[1..text.len() - 1])
    }
    pub(crate) fn members(&self) -> Members<'_, 'e, 'i> {
        match self {
            Self::Raw(raw) => Members::Raw(raw.members()),
            Self::Constant(c) => Members::Constant(c, 0),
            Self::Object(object) => Members::Constructed(object.members.iter()),
            _ => unreachable!("object value"),
        }
    }
    pub(crate) fn is_object(&self) -> bool {
        matches!(self, Self::Constant(c) if c.shape().is_none())
            || matches!(self, Self::Object(_))
            || matches!(self, Self::Raw(raw) if raw.as_bytes()[0] == b'{')
    }
}

pub(crate) enum Members<'a, 'e, 'i> {
    Raw(json::Members<'i>),
    Constant(&'a crate::ConstantValue<'e>, usize),
    Constructed(std::slice::Iter<'a, (Value<'e, 'i>, Value<'e, 'i>)>),
}
impl<'a, 'e: 'a, 'i: 'a> Iterator for Members<'a, 'e, 'i> {
    type Item = (&'a str, Value<'e, 'i>);
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Raw(items) => items.next().map(|(key, value)| (key, Value::Raw(value))),
            Self::Constant(value, index) => {
                let item = value.member(*index);
                *index += 1;
                item
            }
            Self::Constructed(items) => items
                .next()
                .map(|(key, value)| (key.string_body().unwrap(), value.clone())),
        }
    }
}
