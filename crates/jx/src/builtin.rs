mod arrays;
mod collections;
mod diagnostics;
mod encoding;
mod higher;
pub(crate) mod library;
pub(crate) mod numeric;
mod padding;
mod strings;

use crate::{
    Error, Value,
    evaluate::Operand,
    expression::{Aggregate, Node},
    sequence::Context,
    value::type_error,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Builtin {
    Aggregate(Aggregate),
    Boolean,
    Not,
    Exists,
    Lookup,
    Library(library::Library),
    Runtime(crate::dynamic::Builtin),
}
impl Builtin {
    pub fn named(name: &str) -> Option<Self> {
        if let Some(function) = library::Library::named(name) {
            return Some(Self::Library(function));
        }
        Some(match name {
            "average" => Self::Aggregate(Aggregate::Average),
            "count" => Self::Aggregate(Aggregate::Count),
            "sum" => Self::Aggregate(Aggregate::Sum),
            "min" => Self::Aggregate(Aggregate::Min),
            "max" => Self::Aggregate(Aggregate::Max),
            "boolean" => Self::Boolean,
            "not" => Self::Not,
            "exists" => Self::Exists,
            "lookup" => Self::Lookup,
            "eval" => Self::Runtime(crate::dynamic::Builtin::Eval),
            "random" => Self::Runtime(crate::dynamic::Builtin::Random),
            "shuffle" => Self::Runtime(crate::dynamic::Builtin::Shuffle),
            _ => return None,
        })
    }
    pub fn evaluate<'e, 'i>(
        self,
        args: &'e [Node],
        context: &Context<'e, 'i>,
        offset: usize,
    ) -> Result<Operand<'e, 'i>, Error> {
        self.evaluate_in(args, context, context, offset)
    }
    pub fn contextual(self, argc: usize) -> bool {
        match self {
            Self::Library(f) => f.contextual(argc),
            Self::Boolean | Self::Not => argc == 0,
            Self::Lookup => argc == 1,
            Self::Runtime(crate::dynamic::Builtin::Eval) => true,
            _ => false,
        }
    }
    pub fn evaluate_in<'e, 'i>(
        self,
        args: &'e [Node],
        context: &Context<'e, 'i>,
        caller: &Context<'e, 'i>,
        offset: usize,
    ) -> Result<Operand<'e, 'i>, Error> {
        if let Self::Library(function) = self {
            return function.evaluate_in(args, context, caller, offset);
        }
        if matches!(self, Self::Lookup) {
            return crate::lookup::evaluate_in(args, context, caller, offset);
        }
        if let Self::Runtime(function) = self {
            let args = crate::function::Arguments::evaluate(args, context)?;
            return function.values(args.as_slice(), caller, offset);
        }
        if let Self::Aggregate(aggregate) = self {
            return aggregate.evaluate(args, context, offset);
        }
        if args.is_empty() && matches!(self, Self::Boolean | Self::Not) {
            return self.value(Some(caller.value.clone()), offset);
        }
        let [argument] = args else {
            for arg in args {
                crate::retain::materialize(arg, context)?;
            }
            return Err(type_error(offset));
        };
        let mut count = 0;
        let mut defined = false;
        let mut truth = false;
        let mut error = None;
        crate::retain::visit(argument, context, &mut |value| {
            count += 1;
            defined |= !matches!(value, Value::Undefined);
            if !matches!(self, Self::Exists) {
                match value.truth(offset) {
                    Ok(value) => truth |= value,
                    Err(e) => {
                        error.get_or_insert(e);
                    }
                }
            }
        })?;
        if let Some(error) = error {
            return Err(error);
        }
        if matches!(self, Self::Exists) {
            return Ok(Operand::One(Value::Boolean(count > 1 || defined)));
        }
        if count <= 1 && !defined {
            return Ok(Operand::Missing);
        }
        Ok(Operand::One(Value::Boolean(
            truth != matches!(self, Self::Not),
        )))
    }
    pub fn values<'e, 'i>(
        self,
        args: &[Option<Value<'e, 'i>>],
        context: &Context<'e, 'i>,
        offset: usize,
    ) -> Result<Operand<'e, 'i>, Error> {
        match self {
            Self::Library(function) => function.values(args, context, offset),
            Self::Lookup => match args {
                [key] => crate::lookup::values(Some(context.value.clone()), key.clone(), offset),
                [object, key] => crate::lookup::values(object.clone(), key.clone(), offset),
                _ => Err(type_error(offset)),
            },
            Self::Runtime(function) => function.values(args, context, offset),
            Self::Boolean | Self::Not if args.is_empty() => {
                self.value(Some(context.value.clone()), offset)
            }
            _ => match args {
                [value] => self.value(value.clone(), offset),
                _ => Err(type_error(offset)),
            },
        }
    }
    pub fn partial_values<'e, 'i>(
        self,
        args: &[Option<Value<'e, 'i>>],
        context: &Context<'e, 'i>,
        offset: usize,
    ) -> Result<Operand<'e, 'i>, Error> {
        if let Self::Library(function) = self {
            return function.partial_values(args, context, offset);
        }
        if let Self::Runtime(function) = self {
            return function.partial_values(args, context, offset);
        }
        if matches!(self, Self::Aggregate(Aggregate::Count)) {
            let Some(value) = args.first().cloned().flatten() else {
                return self.value(None, offset);
            };
            return match value.atomic() {
                Value::Undefined => self.value(None, offset),
                Value::Null => Err(type_error(offset)),
                value if value.is_array() => self.value(Some(value), offset),
                value if value.string_body().is_some() => Ok(Operand::One(Value::Number(
                    crate::json::string::units(value.string_body().unwrap()).count() as f64,
                ))),
                value if value.is_object() => crate::lookup::values(
                    Some(value),
                    Some(Value::StringLiteral(crate::RawJson("\"length\""))),
                    offset,
                ),
                _ => Ok(Operand::Missing),
            };
        }
        let numeric = matches!(self, Self::Aggregate(_));
        if numeric
            && let Some(value) = args
                .first()
                .and_then(Option::as_ref)
                .filter(|v| !v.is_array())
        {
            if value.is_object() || matches!(value, Value::Function(_)) {
                return Err(crate::Error::new(
                    crate::ErrorKind::UnsupportedExpression,
                    offset,
                    "native array-like object coercion is deferred",
                ));
            }
            if !matches!(self, Self::Aggregate(Aggregate::Sum))
                && value
                    .string_body()
                    .is_some_and(|s| crate::json::string::units(s).next().is_none())
            {
                return Ok(Operand::Missing);
            }
            return Err(type_error(offset));
        }
        self.values(args, context, offset).map_err(|error| {
            if numeric
                && matches!(
                    error.kind,
                    crate::ErrorKind::TypeError | crate::ErrorKind::NumericRange
                )
            {
                crate::Error::new(
                    crate::ErrorKind::UnsupportedExpression,
                    offset,
                    "untyped native aggregate coercion is deferred",
                )
            } else {
                error
            }
        })
    }

    pub fn partial_arity(self, offset: usize) -> Result<usize, Error> {
        if matches!(self, Self::Library(library::Library::String)) {
            return Err(crate::Error::new(
                crate::ErrorKind::UnsupportedExpression,
                offset,
                "native partial application with default parameters is deferred",
            ));
        }
        Ok(self.arity())
    }
    pub fn arity(self) -> usize {
        match self {
            Self::Library(function) => function.arity(),
            Self::Lookup => 2,
            Self::Runtime(function) => function.arity(),
            _ => 1,
        }
    }
    pub fn is_conversion(self) -> bool {
        matches!(
            self,
            Self::Library(library::Library::String | library::Library::Number)
        )
    }

    pub fn constant(self, args: &[Node]) -> bool {
        match self {
            Self::Library(function) => function.constant(args),
            Self::Runtime(_) => false,
            Self::Lookup => args.len() == 2,
            _ => !args.is_empty(),
        }
    }

    pub fn value<'e, 'i>(
        self,
        value: Option<Value<'e, 'i>>,
        offset: usize,
    ) -> Result<Operand<'e, 'i>, Error> {
        let value = value.filter(|value| !matches!(value, Value::Undefined));
        if let Self::Aggregate(aggregate) = self {
            return aggregate.retained(value, offset);
        }
        let result = match self {
            Self::Exists => value.is_some(),
            Self::Boolean | Self::Not => {
                let Some(value) = value else {
                    return Ok(Operand::Missing);
                };
                value.truth(offset)? != matches!(self, Self::Not)
            }
            Self::Aggregate(_) | Self::Runtime(_) | Self::Lookup | Self::Library(_) => {
                unreachable!()
            }
        };
        Ok(Operand::One(Value::Boolean(result)))
    }
}
