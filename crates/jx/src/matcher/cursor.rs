use super::{State, empty_error, groups, record, text::Text};
use crate::{
    Error, Function, Value,
    function::{FunctionKind, invoke},
    sequence::Context,
    value::type_error,
};
use std::rc::Rc;

pub(super) enum Hit<'e, 'i> {
    Native(Rc<State<'e>>, Text<'e, 'i>, regress::Match),
    Custom(Value<'e, 'i>, usize, usize),
}
impl<'e, 'i> Hit<'e, 'i> {
    pub fn start(&self) -> usize {
        match self {
            Self::Native(_, _, m) => m.start(),
            Self::Custom(_, s, _) => *s,
        }
    }
    pub fn end(&self) -> usize {
        match self {
            Self::Native(_, _, m) => m.end(),
            Self::Custom(_, _, e) => *e,
        }
    }
    pub fn matched(&self) -> Value<'e, 'i> {
        match self {
            Self::Native(_, input, m) => input.slice(m.range()),
            Self::Custom(value, _, _) => value.field("match").unwrap(),
        }
    }
    pub fn groups(&self) -> Value<'e, 'i> {
        match self {
            Self::Native(_, input, m) => groups(input, m),
            Self::Custom(value, _, _) => value.field("groups").unwrap(),
        }
    }
    pub fn matched_len(&self) -> usize {
        match self {
            Self::Native(_, _, m) => m.end() - m.start(),
            Self::Custom(value, _, _) => {
                crate::json::string::units(value.field("match").unwrap().string_body().unwrap())
                    .count()
            }
        }
    }
    pub fn group_count(&self) -> usize {
        match self {
            Self::Native(_, _, m) => m.captures.len(),
            Self::Custom(value, _, _) => value.field("groups").unwrap().elements().count(),
        }
    }
    pub fn append_group(&self, index: usize, output: &mut Vec<u16>) {
        match self {
            Self::Native(_, input, m) => {
                if let Some(range) = m.group(index) {
                    input.append(range, output);
                }
            }
            Self::Custom(value, _, _) => {
                let group = if index == 0 {
                    value.field("match")
                } else {
                    value.field("groups").unwrap().elements().nth(index - 1)
                };
                if let Some(value) = group
                    && let Some(body) = value.string_body()
                {
                    output.extend(crate::json::string::units(body));
                }
            }
        }
    }
    pub fn object(&self) -> Value<'e, 'i> {
        match self {
            Self::Native(state, input, m) => record(state, input, m),
            Self::Custom(value, _, _) => value.clone(),
        }
    }
}
pub(super) struct Cursor<'e, 'i> {
    function: Rc<Function<'e, 'i>>,
    input: Text<'e, 'i>,
    first: bool,
}
impl<'e, 'i> Cursor<'e, 'i> {
    pub fn new(function: Rc<Function<'e, 'i>>, input: Text<'e, 'i>) -> Self {
        Self {
            function,
            input,
            first: true,
        }
    }
    pub fn next(
        &mut self,
        context: &Context<'e, 'i>,
        offset: usize,
    ) -> Result<Option<Hit<'e, 'i>>, Error> {
        if let FunctionKind::Matcher(state) = &self.function.kind {
            self.input.check_case(state.pattern.legacy_icase, offset)?;
            let start = if self.first { 0 } else { state.position.get() };
            if !self.first && start >= self.input.len() {
                return Ok(None);
            }
            let found = state.search(&self.input, start);
            if !self.first && found.as_ref().is_some_and(|m| m.start() == m.end()) {
                return Err(empty_error(offset));
            }
            self.first = false;
            return Ok(found.map(|m| Hit::Native(state.clone(), self.input.clone(), m)));
        }
        let args = [self.first.then(|| self.input.value.clone())];
        self.first = false;
        let result = invoke(&self.function, &args, context, offset)?.normalize();
        let value = match result {
            crate::evaluate::Operand::Missing => return Ok(None),
            crate::evaluate::Operand::One(value) => value,
            crate::evaluate::Operand::Many(_) => return Err(type_error(offset)),
        };
        let index = |name| match value.field(name).map(|v| v.atomic()) {
            Some(Value::Number(n)) if n >= 0.0 && n.is_finite() && n.fract() == 0.0 => {
                Ok(n as usize)
            }
            _ => Err(type_error(offset)),
        };
        let start = index("start")?;
        let end = index("end")?;
        if value
            .field("match")
            .is_none_or(|v| v.string_body().is_none())
            || value.field("groups").is_none_or(|v| !v.is_array())
        {
            return Err(type_error(offset));
        }
        let Some(Value::Function(next)) = value.field("next") else {
            return Err(type_error(offset));
        };
        self.function = next;
        Ok(Some(Hit::Custom(value, start, end)))
    }
}
