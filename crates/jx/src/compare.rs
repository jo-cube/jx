use crate::{
    Error, RawJson, Value,
    evaluate::Operand,
    expression::Op,
    json::string,
    sequence::{Halt, Stream},
    value::type_error,
};
use std::{
    cmp::Ordering,
    collections::HashMap,
    hash::{Hash, Hasher},
};

pub(crate) fn equal<'e, 'i>(
    left: Operand<'e, 'i>,
    right: Operand<'e, 'i>,
    negate: bool,
) -> Result<bool, Error> {
    let equal = match (left, right) {
        (Operand::Missing, _) | (_, Operand::Missing) => return Ok(false),
        (Operand::One(left), Operand::One(right)) => values(left, right),
        (Operand::Many(stream), Operand::One(value))
        | (Operand::One(value), Operand::Many(stream)) => {
            if value.is_array() {
                sequence(stream, value.elements())?
            } else {
                false
            }
        }
        (Operand::Many(left), Operand::Many(right)) => {
            // Equality retains one side so two push streams can be compared in order.
            let mut items = Vec::new();
            right.visit(|value| {
                items.push(value);
                Ok(())
            })?;
            sequence(left, items.into_iter())?
        }
    };
    Ok(equal != negate)
}

fn sequence<'e, 'i>(
    stream: Stream<'_, '_>,
    mut other: impl Iterator<Item = Value<'e, 'i>>,
) -> Result<bool, Error> {
    let result = stream.walk(&mut |item| {
        if other.next().is_some_and(|next| values(item, next)) {
            Ok(())
        } else {
            Err(Halt::Stop)
        }
    });
    match result {
        Ok(()) => Ok(other.next().is_none()),
        Err(Halt::Stop) => Ok(false),
        Err(Halt::Evaluation(error)) => Err(error),
    }
}

pub(crate) fn values(left: Value<'_, '_>, right: Value<'_, '_>) -> bool {
    match (left.atomic(), right.atomic()) {
        (Value::Function(left), Value::Function(right)) => {
            std::rc::Rc::ptr_eq(&left, &right)
                || matches!((&left.kind, &right.kind), (crate::function::FunctionKind::Builtin(a), crate::function::FunctionKind::Builtin(b)) if a == b)
        }
        (Value::Null, Value::Null) | (Value::Undefined, Value::Undefined) => true,
        (Value::Boolean(left), Value::Boolean(right)) => left == right,
        (Value::Number(left), Value::Number(right)) => left == right,
        (left, right) => {
            if left.is_array() && right.is_array() {
                let mut right = right.elements();
                return left
                    .elements()
                    .all(|item| right.next().is_some_and(|next| values(item, next)))
                    && right.next().is_none();
            }
            if left.is_object() && right.is_object() {
                return objects(&left, &right);
            }
            match (left.string_body(), right.string_body()) {
                (Some(left), Some(right)) => string::units(left).eq(string::units(right)),
                _ => false,
            }
        }
    }
}

// Encoded keys compare and hash their UTF-16 units, including lone surrogates.
pub(crate) struct Key<'a>(pub(crate) &'a str);
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

fn objects(left: &Value<'_, '_>, right: &Value<'_, '_>) -> bool {
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
        *equal = Some(values(expected.clone(), value));
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

// `in` uses reference identity for containers, unlike structural `=`.
pub(crate) fn includes(left: Operand<'_, '_>, right: Operand<'_, '_>) -> Result<bool, Error> {
    let Operand::One(left) = left else {
        return Ok(false);
    };
    let mut found = false;
    let mut compare = |right: Value<'_, '_>| {
        found |= match (&left, &right) {
            (Value::Constant(a), Value::Constant(b)) => a.same(b),
            (Value::Array(a), Value::Array(b)) => std::rc::Rc::ptr_eq(a, b),
            (Value::Object(a), Value::Object(b)) => std::rc::Rc::ptr_eq(a, b),
            (Value::Raw(a), Value::Raw(b)) if left.is_array() || left.is_object() => {
                a.as_bytes().as_ptr() == b.as_bytes().as_ptr()
            }
            _ if left.is_array() || left.is_object() || right.is_array() || right.is_object() => {
                false
            }
            _ => values(left.clone(), right),
        };
        Ok(())
    };
    match right {
        Operand::Missing => {}
        Operand::One(value) if value.is_array() => {
            for item in value.elements() {
                compare(item)?;
            }
        }
        Operand::One(value) => compare(value)?,
        Operand::Many(stream) => stream.visit(compare)?,
    }
    Ok(found)
}
