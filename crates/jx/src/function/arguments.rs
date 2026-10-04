use crate::{Error, Value, expression::Node, sequence::Context};

// Ordinary JSONata calls are small. Retention needs owned slots, not a heap
// allocation; wide calls spill without changing the value representation.
pub(crate) enum Arguments<'e, 'i> {
    Small([Option<Value<'e, 'i>>; 3], usize),
    Large(Vec<Option<Value<'e, 'i>>>),
}
impl<'e, 'i> Arguments<'e, 'i> {
    pub fn new(capacity: usize) -> Self {
        if capacity <= 3 {
            Self::Small([None, None, None], 0)
        } else {
            Self::Large(Vec::with_capacity(capacity))
        }
    }
    pub fn push(&mut self, value: Option<Value<'e, 'i>>) {
        match self {
            Self::Small(values, len) if *len < values.len() => {
                values[*len] = value;
                *len += 1;
            }
            Self::Small(values, len) => {
                let mut wide = Vec::with_capacity(*len + 1);
                wide.extend(values.iter_mut().map(Option::take));
                wide.push(value);
                *self = Self::Large(wide);
            }
            Self::Large(values) => values.push(value),
        }
    }
    pub fn as_slice(&self) -> &[Option<Value<'e, 'i>>] {
        match self {
            Self::Small(values, len) => &values[..*len],
            Self::Large(values) => values,
        }
    }
    pub fn evaluate(nodes: &'e [Node], context: &Context<'e, 'i>) -> Result<Self, Error> {
        let mut args = Self::new(nodes.len());
        for node in nodes {
            args.push(crate::retain::materialize(node, context)?);
        }
        Ok(args)
    }
}
