use super::{Array, Object};
use crate::{Error, ErrorKind, Value, constant::Data};
use std::{fmt, rc::Rc};

pub(crate) const MAX_NESTING: u16 = 256;
const TOO_DEEP: u16 = MAX_NESTING + 1;
const _: () = assert!(crate::MAX_DEPTH < MAX_NESTING as usize);

// Constructed storage caches an upper bound. Raw containers reserve the JSON
// validator's entire depth allowance; refine only if that would reject a value.
pub(super) fn nesting<'a, 'e: 'a, 'i: 'a>(
    items: impl Iterator<Item = &'a Value<'e, 'i>> + Clone,
) -> u16 {
    let maximum = items.clone().map(Value::nesting).max().unwrap_or(0);
    if maximum < MAX_NESTING {
        maximum + 1
    } else if maximum == TOO_DEEP {
        TOO_DEEP
    } else {
        items
            .map(|v| within(v, MAX_NESTING - 1))
            .try_fold(0, |maximum, depth| depth.map(|d| maximum.max(d)))
            .map_or(TOO_DEEP, |d| d + 1)
    }
}

fn within(value: &Value<'_, '_>, remaining: u16) -> Option<u16> {
    let bound = value.nesting();
    if bound <= remaining {
        return Some(bound);
    }
    if remaining == 0 {
        return None;
    }
    let mut maximum = 0;
    if value.is_array() {
        for item in value.elements() {
            maximum = maximum.max(within(&item, remaining - 1)?);
        }
    } else if value.is_object() {
        for (_, item) in value.members() {
            maximum = maximum.max(within(&item, remaining - 1)?);
        }
    } else {
        return Some(0);
    }
    Some(maximum + 1)
}

impl Value<'_, '_> {
    pub(crate) fn checked_nesting(self) -> Result<Self, Error> {
        self.check_nesting(0)?;
        Ok(self)
    }
    pub(crate) fn nesting(&self) -> u16 {
        match self {
            Self::Raw(raw) => {
                if raw.is_array() || raw.as_bytes()[0] == b'{' {
                    crate::MAX_DEPTH as u16
                } else {
                    0
                }
            }
            Self::InputView(view) => Value::Raw(view.raw()).nesting(),
            Self::Array(array) => array.nesting,
            Self::Object(object) => object.nesting,
            Self::Constant(value) => value.data.nesting(),
            Self::Copied(value) => value.source.nesting(),
            _ => 0,
        }
    }
    pub(crate) fn check_nesting(&self, offset: usize) -> Result<(), Error> {
        // Raw/compiled storage and clone views are bounded when created. Only
        // the infallible host container builders can produce oversized values.
        let nesting = match self {
            Self::Array(array) => array.nesting,
            Self::Object(object) => object.nesting,
            _ => return Ok(()),
        };
        if nesting > MAX_NESTING {
            Err(Error::new(
                ErrorKind::EvaluationLimit,
                offset,
                "value nesting limit exceeded",
            ))
        } else {
            Ok(())
        }
    }
}
impl Data {
    pub(crate) fn nesting(&self) -> u16 {
        match self {
            Self::Array(_, _, nesting) | Self::Object { nesting, .. } => *nesting,
            _ => 0,
        }
    }
}

// Infallible host constructors may exceed the evaluation ceiling. Detach unique
// container children before dropping them so those graphs never recurse on drop.
impl Drop for Array<'_, '_> {
    fn drop(&mut self) {
        if self.nesting > MAX_NESTING {
            release(std::mem::take(&mut self.items));
        }
    }
}
impl Drop for Object<'_, '_> {
    fn drop(&mut self) {
        if self.nesting > MAX_NESTING {
            let mut pending = Vec::new();
            for (key, value) in std::mem::take(&mut self.members) {
                pending.push(value);
                pending.push(key);
            }
            release(pending);
        }
    }
}
fn release(mut pending: Vec<Value<'_, '_>>) {
    while let Some(value) = pending.pop() {
        match value {
            Value::Array(array) => {
                if let Ok(mut array) = Rc::try_unwrap(array) {
                    pending.append(&mut array.items);
                }
            }
            Value::Object(object) => {
                if let Ok(mut object) = Rc::try_unwrap(object) {
                    for (key, value) in std::mem::take(&mut object.members) {
                        pending.push(value);
                        pending.push(key);
                    }
                }
            }
            Value::Copied(copy) => {
                if let Ok(copy) = Rc::try_unwrap(copy) {
                    pending.push(copy.source);
                }
            }
            _ => {}
        }
    }
}

impl fmt::Debug for Array<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.nesting > MAX_NESTING {
            return f
                .debug_struct("Array")
                .field("nesting", &self.nesting)
                .finish_non_exhaustive();
        }
        f.debug_struct("Array")
            .field("items", &self.items)
            .field("shape", &self.shape)
            .field("nesting", &self.nesting)
            .finish()
    }
}
impl fmt::Debug for Object<'_, '_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.nesting > MAX_NESTING {
            return f
                .debug_struct("Object")
                .field("nesting", &self.nesting)
                .finish_non_exhaustive();
        }
        f.debug_struct("Object")
            .field("members", &self.members)
            .field("nesting", &self.nesting)
            .finish()
    }
}
