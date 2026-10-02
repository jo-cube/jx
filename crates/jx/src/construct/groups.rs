use crate::{
    Error, ErrorKind, Value, evaluate::Operand, expression::Node, json::string, sequence::Context,
    value::type_error,
};
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
};

struct Group<'e, 'i, T> {
    key: Value<'e, 'i>,
    pair: usize,
    first: Option<T>,
    rest: Vec<T>,
}
pub(crate) struct Groups<'e, 'i, T> {
    groups: Vec<Group<'e, 'i, T>>,
    index: Option<HashMap<Key<'e, 'i>, usize>>,
}

// Encoded spellings can differ while their JSONata UTF-16 keys are equal.
#[derive(Clone)]
struct Key<'e, 'i>(Value<'e, 'i>);
impl PartialEq for Key<'_, '_> {
    fn eq(&self, other: &Self) -> bool {
        string::units(self.0.string_body().unwrap())
            .eq(string::units(other.0.string_body().unwrap()))
    }
}
impl Eq for Key<'_, '_> {}
impl Hash for Key<'_, '_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        for unit in string::units(self.0.string_body().unwrap()) {
            unit.hash(state);
        }
    }
}
impl<'e, 'i, T: Clone> Groups<'e, 'i, T> {
    pub fn new(capacity: usize) -> Self {
        Self {
            groups: Vec::with_capacity(capacity),
            index: None,
        }
    }
    pub fn add(
        &mut self,
        pairs: &'e [(Node, Node)],
        context: &Context<'e, 'i>,
        item: Option<T>,
        offset: usize,
    ) -> Result<(), Error> {
        for (pair, (key, _)) in pairs.iter().enumerate() {
            let key = match key.run(context)? {
                Operand::Missing => continue,
                Operand::One(key) if key.string_body().is_some() => key,
                _ => return Err(type_error(offset)),
            };
            let found = if let Some(index) = &self.index {
                index.get(&Key(key.clone())).copied()
            } else {
                self.groups.iter().position(|g| {
                    string::units(g.key.string_body().unwrap())
                        .eq(string::units(key.string_body().unwrap()))
                })
            };
            if let Some(found) = found {
                let group = &mut self.groups[found];
                if group.pair != pair {
                    return Err(Error::new(
                        ErrorKind::DuplicateKey,
                        offset,
                        "duplicate object key from different members",
                    ));
                }
                if group.first.is_none() {
                    group.first = item.clone();
                } else if let Some(item) = &item {
                    group.rest.push(item.clone());
                }
            } else {
                if let Some(index) = &mut self.index {
                    index.insert(Key(key.clone()), self.groups.len());
                }
                self.groups.push(Group {
                    key,
                    pair,
                    first: item.clone(),
                    rest: Vec::new(),
                });
                // Keep small constructors linear; only wide dynamic groups need
                // a key index. This index preserves first-seen/member ordering.
                if self.index.is_none() && self.groups.len() == 32 {
                    self.index = Some(
                        self.groups
                            .iter()
                            .enumerate()
                            .map(|(i, g)| (Key(g.key.clone()), i))
                            .collect(),
                    );
                }
            }
        }
        Ok(())
    }
    pub fn finish(
        mut self,
        pairs: &'e [(Node, Node)],
        mut evaluate: impl FnMut((Option<T>, Vec<T>), &'e Node) -> Result<Option<Value<'e, 'i>>, Error>,
    ) -> Result<Value<'e, 'i>, Error> {
        drop(self.index.take());
        self.groups.sort_by_key(|g| {
            crate::members::index(g.key.string_body().unwrap()).unwrap_or(u32::MAX)
        });
        let mut members = Vec::with_capacity(self.groups.len());
        for group in self.groups {
            if let Some(value) = evaluate((group.first, group.rest), &pairs[group.pair].1)? {
                members.push((group.key, value));
            }
        }
        Ok(Value::object(members))
    }
}
