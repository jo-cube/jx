use super::*;
use crate::json::{RawJson, string};

#[derive(Clone, Debug)]
pub(super) struct Object {
    pub(super) program: Program,
    members: Box<[(Box<str>, Member)]>,
}
#[derive(Clone, Debug)]
enum Member {
    Slot(u8),
    Constant(Data),
}
impl Object {
    pub(super) fn run<'e, 'i>(&'e self, input: &Context<'e, 'i>) -> Option<Operand<'e, 'i>> {
        self.run_captured(input, None)
    }
    pub(super) fn run_captured<'e, 'i>(
        &'e self,
        input: &Context<'e, 'i>,
        captured: Option<&Captures<'i>>,
    ) -> Option<Operand<'e, 'i>> {
        if !input.wrapped && input.value.is_array() {
            return None;
        }
        let context = Context {
            value: input.value.clone(),
            wrapped: false,
            scope: None,
        };
        self.program.execute(&context, captured, |slots| {
            let mut members = Vec::with_capacity(self.members.len());
            for (key, member) in &self.members {
                let value = match member {
                    Member::Slot(slot) => match slots[usize::from(*slot)].operand() {
                        Operand::Missing => continue,
                        Operand::One(value) => value,
                        Operand::Many(_) => unreachable!(),
                    },
                    Member::Constant(data) => data.value(),
                };
                if !matches!(value, Value::Undefined) {
                    members.push((Value::StringLiteral(RawJson(key)), value));
                }
            }
            Operand::One(Value::object(members))
        })
    }
}
pub(super) fn lower(node: &Node) -> Option<Object> {
    let Kind::Object(pairs) = &node.kind else {
        return None;
    };
    let mut pairs = pairs
        .iter()
        .map(|(key, value)| {
            let key = match &key.kind {
                Kind::String(s) => s.as_ref(),
                Kind::Prepared(p) => match &p.data {
                    Data::String(s) => s.as_ref(),
                    _ => return None,
                },
                _ => return None,
            };
            Some((key, value))
        })
        .collect::<Option<Vec<_>>>()?;
    // Match the existing constructor's key validation and numeric-key order.
    for (i, (key, _)) in pairs.iter().enumerate() {
        if pairs[..i].iter().any(|(other, _)| {
            string::units(&key[1..key.len() - 1]).eq(string::units(&other[1..other.len() - 1]))
        }) {
            return None;
        }
    }
    pairs.sort_by_key(|(key, _)| crate::members::index(&key[1..key.len() - 1]).unwrap_or(u32::MAX));
    let mut lower = Lower::default();
    let mut members = Vec::with_capacity(pairs.len());
    for (key, value) in pairs {
        let member = match &value.kind {
            Kind::Prepared(p) => Member::Constant(p.data.clone()),
            Kind::Null => Member::Constant(Data::Null),
            Kind::String(s) => Member::Constant(Data::String(s.clone())),
            _ if lower::computed(value) => Member::Slot(lower.node(value)?),
            _ => return None,
        };
        members.push((key.into(), member));
    }
    if lower.instructions.is_empty() {
        return None;
    }
    Some(Object {
        program: lower.finish(0),
        members: members.into_boxed_slice(),
    })
}
