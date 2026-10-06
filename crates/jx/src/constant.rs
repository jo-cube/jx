use crate::{
    Value,
    container::Shape,
    json::{RawJson, string},
};
use std::{
    io::{self, Write},
    rc::Rc,
    sync::Arc,
};

#[derive(Clone, Debug)]
pub(crate) enum Data {
    Missing,
    Null,
    Boolean(bool),
    Number(f64),
    String(Box<str>),
    Array(Box<[Data]>, Shape),
    Object {
        members: Box<[(Box<str>, Data)]>,
        index: Box<[(u64, usize)]>,
    },
}

/// A constructed value backed by immutable expression data. Each construction
/// has fresh identity; descendants share its token but have distinct data addresses.
#[derive(Clone, Debug)]
pub struct ConstantValue<'e> {
    pub(crate) data: &'e Data,
    pub(crate) identity: Rc<()>,
}

#[derive(Clone, Debug)]
pub(crate) struct Prepared {
    pub data: Storage,
    pub array_syntax: bool,
}

pub(crate) type Bindings = [(Box<str>, Arc<crate::OwnedValue>)];

// Ordinary compiled constants keep their existing owned representation. External
// snapshots are shared across references and expressions without copying data.
#[derive(Clone, Debug)]
pub(crate) enum Storage {
    Owned(Data),
    Shared(Arc<crate::OwnedValue>),
}
impl From<Data> for Storage {
    fn from(data: Data) -> Self {
        Self::Owned(data)
    }
}
impl std::ops::Deref for Storage {
    type Target = Data;
    fn deref(&self) -> &Data {
        match self {
            Self::Owned(data) => data,
            Self::Shared(value) => &value.data,
        }
    }
}

impl Data {
    pub(crate) fn capture(value: &Value<'_, '_>) -> Option<Self> {
        Some(match value {
            Value::Undefined => Self::Missing,
            Value::Null => Self::Null,
            Value::Number(n) => Self::Number(*n),
            Value::Boolean(b) => Self::Boolean(*b),
            Value::StringLiteral(s) => Self::String(s.as_str().into()),
            Value::String(_) => Self::String(value.json()?.as_str().into()),
            Value::Constant(c) => c.data.clone(),
            Value::Array(a) => Self::Array(
                a.items
                    .iter()
                    .map(Self::capture)
                    .collect::<Option<Box<[_]>>>()?,
                a.shape,
            ),
            Value::Object(o) => {
                let members = o
                    .members
                    .iter()
                    .map(|(k, v)| Some((k.json()?.as_str().into(), Self::capture(v)?)))
                    .collect::<Option<Box<[(Box<str>, Self)]>>>()?;
                let mut index = members
                    .iter()
                    .enumerate()
                    .map(|(i, (key, _))| (string::fingerprint(body(key)), i))
                    .collect::<Vec<_>>();
                index.sort_unstable();
                Self::Object {
                    members,
                    index: index.into_boxed_slice(),
                }
            }
            _ => return None,
        })
    }
    pub(crate) fn value<'e, 'i>(&'e self) -> Value<'e, 'i> {
        self.with_identity(None)
    }
    pub(crate) fn with_identity<'e, 'i>(&'e self, identity: Option<&Rc<()>>) -> Value<'e, 'i> {
        match self {
            Self::Missing => Value::Undefined,
            Self::Null => Value::Null,
            Self::Boolean(b) => Value::Boolean(*b),
            Self::Number(n) => Value::Number(*n),
            Self::String(s) => Value::StringLiteral(RawJson(s)),
            Self::Array(..) | Self::Object { .. } => Value::Constant(ConstantValue {
                data: self,
                identity: identity.cloned().unwrap_or_else(|| Rc::new(())),
            }),
        }
    }
    pub(crate) fn find(
        &self,
        fingerprint: impl FnOnce() -> u64,
        mut equal: impl FnMut(&str) -> bool,
    ) -> Option<&Data> {
        let Self::Object { members, index } = self else {
            return None;
        };
        if members.len() <= 1 {
            let (key, value) = members.first()?;
            return equal(body(key)).then_some(value);
        }
        let fingerprint = fingerprint();
        let start = index.partition_point(|&(hash, _)| hash < fingerprint);
        for &(hash, i) in &index[start..] {
            if hash != fingerprint {
                break;
            }
            if equal(body(&members[i].0)) {
                return Some(&members[i].1);
            }
        }
        None
    }
    pub(crate) fn write(&self, output: &mut dyn Write) -> io::Result<()> {
        match self {
            Self::Array(items, _) => {
                output.write_all(b"[")?;
                for (i, v) in items.iter().enumerate() {
                    if i != 0 {
                        output.write_all(b",")?;
                    }
                    v.write(output)?;
                }
                output.write_all(b"]")
            }
            Self::Object { members, .. } => {
                output.write_all(b"{")?;
                for (i, (key, v)) in members.iter().enumerate() {
                    if i != 0 {
                        output.write_all(b",")?;
                    }
                    output.write_all(key.as_bytes())?;
                    output.write_all(b":")?;
                    v.write(output)?;
                }
                output.write_all(b"}")
            }
            _ => self.value().write_compact(output),
        }
    }
}
impl<'e> ConstantValue<'e> {
    pub(crate) fn same(&self, other: &Self) -> bool {
        std::ptr::eq(self.data, other.data) && Rc::ptr_eq(&self.identity, &other.identity)
    }
    pub(crate) fn shape(&self) -> Option<Shape> {
        if let Data::Array(_, shape) = self.data {
            Some(*shape)
        } else {
            None
        }
    }
    pub(crate) fn element<'i>(&self, index: usize) -> Option<Value<'e, 'i>> {
        let Data::Array(items, _) = self.data else {
            return None;
        };
        items
            .get(index)
            .map(|v| v.with_identity(Some(&self.identity)))
    }
    pub(crate) fn member<'i>(&self, index: usize) -> Option<(&'e str, Value<'e, 'i>)> {
        let Data::Object { members, .. } = self.data else {
            return None;
        };
        members
            .get(index)
            .map(|(k, v)| (body(k), v.with_identity(Some(&self.identity))))
    }
    pub(crate) fn field<'i>(
        &self,
        fingerprint: impl FnOnce() -> u64,
        equal: impl FnMut(&str) -> bool,
    ) -> Option<Value<'e, 'i>> {
        self.data
            .find(fingerprint, equal)
            .map(|v| v.with_identity(Some(&self.identity)))
    }
}
fn body(s: &str) -> &str {
    &s[1..s.len() - 1]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fingerprint_collisions_require_exact_key_equality() {
        let hash = string::fingerprint("wanted");
        let data = Data::Object {
            members: vec![
                ("\"other\"".into(), Data::Number(1.0)),
                ("\"wanted\"".into(), Data::Number(2.0)),
            ]
            .into_boxed_slice(),
            index: vec![(hash, 0), (hash, 1)].into_boxed_slice(),
        };
        assert!(matches!(
            data.find(|| hash, |k| string::equal(k, "wanted")),
            Some(Data::Number(2.0))
        ));
        assert!(
            data.find(|| hash, |k| string::equal(k, "missing"))
                .is_none()
        );
    }
}
