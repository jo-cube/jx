mod groups;
use crate::Value;
pub(crate) use groups::Groups;

pub(crate) fn group_value<'e, 'i>(
    values: (Option<Value<'e, 'i>>, Vec<Value<'e, 'i>>),
) -> Value<'e, 'i> {
    let (first, rest) = values;
    let first = first.unwrap_or(Value::Undefined);
    if rest.is_empty() {
        first
    } else {
        let mut items = Vec::with_capacity(1 + rest.len());
        for value in std::iter::once(first).chain(rest) {
            if value.is_array() {
                items.extend(value.elements());
            } else if !matches!(value, Value::Undefined) {
                items.push(value);
            }
        }
        Value::array(items, false)
    }
}
