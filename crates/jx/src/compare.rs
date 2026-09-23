use crate::{
    Error, RawJson, Value, evaluate::Operand, expression::Op, json::string, path::PathEvaluation,
    value::type_error,
};
use std::{
    cmp::Ordering,
    collections::HashMap,
    hash::{Hash, Hasher},
};

pub(crate) fn equal(left: Operand<'_, '_>, right: Operand<'_, '_>, negate: bool) -> bool {
    let equal = match (left, right) {
        (Operand::Missing, _) | (_, Operand::Missing) => return false,
        (Operand::One(left), Operand::One(right)) => values(left, right),
        (Operand::Many(path), Operand::One(value)) | (Operand::One(value), Operand::Many(path)) => {
            value
                .json()
                .is_some_and(|raw| raw.is_array() && sequence(path, raw.elements()))
        }
        (Operand::Many(left), Operand::Many(right)) => {
            // Two push streams cannot be zipped while suspended. Only this equality
            // case retains one side; ordinary paths and other operators stay streamed.
            let mut items = Vec::new();
            right.for_each(|value| items.push(value));
            sequence(left, items.into_iter())
        }
    };
    equal != negate
}

fn sequence<'a>(
    path: PathEvaluation<'_, '_>,
    mut other: impl Iterator<Item = RawJson<'a>>,
) -> bool {
    let result = path.try_for_each(|item| {
        if other
            .next()
            .is_some_and(|next| values(Value::Raw(item), Value::Raw(next)))
        {
            Ok(())
        } else {
            Err(())
        }
    });
    result.is_ok() && other.next().is_none()
}

fn values(left: Value<'_, '_>, right: Value<'_, '_>) -> bool {
    match (left.atomic(), right.atomic()) {
        (Value::Null, Value::Null) => true,
        (Value::Boolean(left), Value::Boolean(right)) => left == right,
        (Value::Number(left), Value::Number(right)) => left == right,
        (left, right) => {
            let (Some(left), Some(right)) = (left.json(), right.json()) else {
                return false;
            };
            match (left.as_bytes()[0], right.as_bytes()[0]) {
                (b'"', b'"') => units(left).eq(units(right)),
                (b'[', b'[') => {
                    let mut right = right.elements();
                    left.elements().all(|item| {
                        right
                            .next()
                            .is_some_and(|next| values(Value::Raw(item), Value::Raw(next)))
                    }) && right.next().is_none()
                }
                (b'{', b'{') => objects(left, right),
                _ => false,
            }
        }
    }
}

// Encoded keys compare and hash their UTF-16 units, including lone surrogates.
struct Key<'a>(&'a str);
impl PartialEq for Key<'_> {
    fn eq(&self, other: &Self) -> bool {
        string::units(self.0).eq(string::units(other.0))
    }
}
impl Eq for Key<'_> {}
impl Hash for Key<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for unit in string::units(self.0) {
            state.write_u32(u32::from(unit));
        }
        state.write_u32(0x1_0000); // A terminator outside the UTF-16 unit range.
    }
}

fn objects(left: RawJson<'_>, right: RawJson<'_>) -> bool {
    // Retain only borrowed left members. Later duplicate keys overwrite earlier
    // values on both sides, without decoding strings or building a JSON tree.
    let mut members = HashMap::new();
    for (key, value) in left.members() {
        members.insert(Key(key), (value, None));
    }
    for (key, value) in right.members() {
        let Some((expected, equal)) = members.get_mut(&Key(key)) else {
            return false;
        };
        *equal = Some(values(Value::Raw(*expected), Value::Raw(value)));
    }
    members.values().all(|(_, equal)| *equal == Some(true))
}

pub(crate) fn order<'e, 'i>(
    left: Operand<'e, 'i>,
    right: Operand<'e, 'i>,
    op: Op,
    offset: usize,
) -> Result<Operand<'e, 'i>, Error> {
    fn comparable<'e, 'i>(
        operand: Operand<'e, 'i>,
        offset: usize,
    ) -> Result<Option<Value<'e, 'i>>, Error> {
        match operand {
            Operand::Missing => Ok(None),
            Operand::One(value) => {
                let value = value.atomic();
                if matches!(value, Value::Number(_))
                    || value.json().is_some_and(|raw| raw.as_bytes()[0] == b'"')
                {
                    Ok(Some(value))
                } else {
                    Err(type_error(offset))
                }
            }
            Operand::Many(_) => Err(type_error(offset)),
        }
    }
    let left = comparable(left, offset)?;
    let right = comparable(right, offset)?;
    let (Some(left), Some(right)) = (left, right) else {
        return Ok(Operand::Missing);
    };
    let order = match (left, right) {
        (Value::Number(left), Value::Number(right)) => left.partial_cmp(&right),
        (left, right) => {
            let (Some(left), Some(right)) = (left.json(), right.json()) else {
                return Err(type_error(offset));
            };
            Some(units(left).cmp(units(right)))
        }
    };
    let value = match op {
        Op::Less => order == Some(Ordering::Less),
        Op::LessEqual => matches!(order, Some(Ordering::Less | Ordering::Equal)),
        Op::Greater => order == Some(Ordering::Greater),
        Op::GreaterEqual => matches!(order, Some(Ordering::Greater | Ordering::Equal)),
        _ => unreachable!(),
    };
    Ok(Operand::One(Value::Boolean(value)))
}

fn units(raw: RawJson<'_>) -> string::Units<'_> {
    string::units(&raw.as_str()[1..raw.as_str().len() - 1])
}
