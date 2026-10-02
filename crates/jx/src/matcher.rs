mod cursor;
mod pattern;
mod processing;
mod text;
use crate::{
    Error, ErrorKind, Function, Value, evaluate::Operand, function::FunctionKind, value::type_error,
};
pub(crate) use processing::call;
use std::{cell::Cell, rc::Rc};
use text::Text;

#[derive(Clone, Debug)]
pub(crate) struct Pattern {
    regex: regress::Regex,
    legacy_icase: bool,
}
impl Pattern {
    pub fn compile(source: &str, flags: &str, offset: usize) -> Result<Self, Error> {
        if source.is_empty() || flags.matches('i').count() > 1 || flags.matches('m').count() > 1 {
            return Err(Error::new(
                ErrorKind::UnsupportedExpression,
                offset,
                "invalid regex literal",
            ));
        }
        // regress does not exclude non-ASCII -> ASCII uppercase folds in legacy
        // /i mode. Restrict patterns that could introduce such equivalences.
        let legacy_icase = flags.contains('i');
        if legacy_icase && (!source.is_ascii() || source.contains(r"\u")) {
            return Err(legacy_case_error(offset));
        }
        regress::Regex::from_unicode(pattern::units(source).into_iter(), flags)
            .map(|regex| Self {
                regex,
                legacy_icase,
            })
            .map_err(|_| {
                Error::new(
                    ErrorKind::UnsupportedExpression,
                    offset,
                    "invalid or unsupported ECMAScript regex",
                )
            })
    }
}
#[derive(Debug)]
enum PatternRef<'e> {
    Borrowed(&'e Pattern),
    Owned(Rc<Pattern>),
}
impl std::ops::Deref for PatternRef<'_> {
    type Target = Pattern;
    fn deref(&self) -> &Pattern {
        match self {
            Self::Borrowed(v) => v,
            Self::Owned(v) => v,
        }
    }
}
#[derive(Debug)]
pub(crate) struct State<'e> {
    pattern: PatternRef<'e>,
    position: Cell<usize>,
}
#[derive(Debug)]
pub(crate) struct Continuation<'e, 'i> {
    state: Rc<State<'e>>,
    input: Text<'e, 'i>,
}
pub(crate) fn literal<'e, 'i>(pattern: &'e Pattern) -> Value<'e, 'i> {
    Value::Function(Rc::new(Function {
        kind: FunctionKind::Matcher(Rc::new(State {
            pattern: PatternRef::Borrowed(pattern),
            position: Cell::new(0),
        })),
    }))
}
impl State<'_> {
    pub(crate) fn retained<'e>(&self) -> State<'e> {
        let pattern = match &self.pattern {
            PatternRef::Borrowed(v) => Rc::new((*v).clone()),
            PatternRef::Owned(v) => v.clone(),
        };
        State {
            pattern: PatternRef::Owned(pattern),
            position: Cell::new(self.position.get()),
        }
    }
    fn search(&self, input: &Text<'_, '_>, start: usize) -> Option<regress::Match> {
        let found = input.search(&self.pattern.regex, start);
        self.position.set(found.as_ref().map_or(0, |m| m.end()));
        found
    }
}
pub(crate) fn invoke<'e, 'i>(
    state: &Rc<State<'e>>,
    args: &[Option<Value<'e, 'i>>],
    offset: usize,
) -> Result<Operand<'e, 'i>, Error> {
    let value = args
        .first()
        .cloned()
        .flatten()
        .filter(|v| v.string_body().is_some())
        .ok_or_else(|| {
            Error::new(
                ErrorKind::UnsupportedExpression,
                offset,
                "native regex argument coercion is deferred",
            )
        })?;
    let input = Text::new(value, offset)?;
    input.check_case(state.pattern.legacy_icase, offset)?;
    let start = match args.get(1).and_then(Option::as_ref).map(Value::atomic) {
        None | Some(Value::Undefined) => 0,
        Some(Value::Number(n)) => {
            if n.is_nan() {
                0
            } else {
                n.max(0.0) as usize
            }
        }
        _ => return Err(type_error(offset)),
    };
    Ok(state
        .search(&input, start)
        .map_or(Operand::Missing, |found| {
            Operand::One(record(state, &input, &found))
        }))
}
impl<'e, 'i> Continuation<'e, 'i> {
    pub(crate) fn state(&self) -> &Rc<State<'e>> {
        &self.state
    }
    pub(crate) fn value(&self) -> &Value<'e, 'i> {
        &self.input.value
    }
    pub(crate) fn retained<'d>(
        &self,
        state: Rc<State<'d>>,
        value: Value<'d, 'i>,
    ) -> Continuation<'d, 'i> {
        Continuation {
            state,
            input: self.input.retained(value),
        }
    }
    pub fn invoke(&self, offset: usize) -> Result<Operand<'e, 'i>, Error> {
        if self.state.position.get() >= self.input.len() {
            return Ok(Operand::Missing);
        }
        let found = self.state.search(&self.input, self.state.position.get());
        if found.as_ref().is_some_and(|m| m.start() == m.end()) {
            return Err(empty_error(offset));
        }
        Ok(found.map_or(Operand::Missing, |found| {
            Operand::One(record(&self.state, &self.input, &found))
        }))
    }
}
fn empty_error(offset: usize) -> Error {
    Error::new(
        ErrorKind::RegexError,
        offset,
        "matcher continuation produced an empty match",
    )
}
fn key<'e, 'i>(encoded: &'static str) -> Value<'e, 'i> {
    Value::StringLiteral(crate::RawJson(encoded))
}
fn groups<'e, 'i>(input: &Text<'e, 'i>, found: &regress::Match) -> Value<'e, 'i> {
    Value::array(
        found
            .captures
            .iter()
            .map(|range| range.clone().map_or(Value::Undefined, |r| input.slice(r)))
            .collect(),
        false,
    )
}
fn record<'e, 'i>(
    state: &Rc<State<'e>>,
    input: &Text<'e, 'i>,
    found: &regress::Match,
) -> Value<'e, 'i> {
    Value::object(vec![
        (key("\"match\""), input.slice(found.range())),
        (key("\"start\""), Value::Number(found.start() as f64)),
        (key("\"end\""), Value::Number(found.end() as f64)),
        (key("\"groups\""), groups(input, found)),
        (
            key("\"next\""),
            Value::Function(Rc::new(Function {
                kind: FunctionKind::MatchNext(Rc::new(Continuation {
                    state: state.clone(),
                    input: input.clone(),
                })),
            })),
        ),
    ])
}

fn legacy_case_error(offset: usize) -> Error {
    Error::new(
        ErrorKind::UnsupportedExpression,
        offset,
        "legacy regex case folding for non-ASCII patterns or dotless-i/long-s is deferred",
    )
}
