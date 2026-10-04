use crate::{
    Value,
    compare::Key,
    json::string,
    sequence::{Output, Walk},
};
use std::collections::HashMap;

// Object enumeration needs last-key-wins values in first-key order. Integer
// keys precede other keys, matching the reference's ECMAScript object ordering.
// This table holds one object's borrowed members, never a descendant result tree.
pub(crate) fn visit<'e, 'i>(value: &Value<'e, 'i>, output: &mut Output<'_, 'e, 'i>) -> Walk {
    for (_, value) in entries(value) {
        output(value)?;
    }
    Ok(())
}
pub(crate) fn entries<'a, 'e: 'a, 'i: 'a>(
    value: &'a Value<'e, 'i>,
) -> Vec<(&'a str, Value<'e, 'i>)> {
    let mut positions = HashMap::new();
    let mut entries = Vec::new();
    for (key, value) in value.members() {
        match positions.entry(Key(key)) {
            std::collections::hash_map::Entry::Occupied(entry) => {
                entries[*entry.get()] = (key, value)
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(entries.len());
                entries.push((key, value));
            }
        }
    }
    entries.sort_by_key(|(key, _)| index(key).unwrap_or(u32::MAX));
    entries
}
pub(crate) fn index(key: &str) -> Option<u32> {
    let mut number = 0u32;
    let mut count = 0;
    for unit in string::units(key) {
        if !(48..=57).contains(&unit) || (count != 0 && number == 0) {
            return None;
        }
        number = number.checked_mul(10)?.checked_add(u32::from(unit - 48))?;
        count += 1;
    }
    (count != 0 && number != u32::MAX).then_some(number)
}
