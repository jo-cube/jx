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
    Deferred(&'static str),
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
            _ => Self::Deferred(DEFERRED.iter().find(|&&candidate| candidate == name)?),
        })
    }
    pub fn evaluate<'e, 'i>(
        self,
        args: &'e [Node],
        context: &Context<'e, 'i>,
        offset: usize,
    ) -> Result<Operand<'e, 'i>, Error> {
        if let Self::Library(function) = self {
            return function.evaluate(args, context, offset);
        }
        if matches!(self, Self::Lookup) {
            return crate::lookup::evaluate(args, context, offset);
        }
        if let Self::Deferred(name) = self {
            return Err(crate::Error::new(
                crate::ErrorKind::UnsupportedExpression,
                offset,
                name,
            ));
        }
        if let Self::Aggregate(aggregate) = self {
            return aggregate.evaluate(args, context, offset);
        }
        if args.is_empty() && matches!(self, Self::Boolean | Self::Not) {
            return self.value(Some(context.value.clone()), offset);
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
            Self::Deferred(_) => self.value(None, offset),
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
        let numeric = matches!(self, Self::Aggregate(aggregate) if aggregate != Aggregate::Count);
        let unsupported = || {
            crate::Error::new(
                crate::ErrorKind::UnsupportedExpression,
                offset,
                "untyped native partial application is deferred",
            )
        };
        if matches!(self, Self::Aggregate(_))
            && args
                .first()
                .and_then(Option::as_ref)
                .is_some_and(|v| !v.is_array())
        {
            return Err(unsupported());
        }
        self.values(args, context, offset).map_err(|error| {
            // The fold validates items while consuming them; no separate array scan.
            if numeric
                && matches!(
                    error.kind,
                    crate::ErrorKind::TypeError | crate::ErrorKind::NumericRange
                )
            {
                unsupported()
            } else {
                error
            }
        })
    }

    pub fn partial_arity(self, offset: usize) -> Result<usize, Error> {
        if matches!(
            self,
            Self::Library(library::Library::String | library::Library::Zip) | Self::Deferred(_)
        ) {
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
            Self::Deferred(_) => false,
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
        if let Self::Deferred(name) = self {
            return Err(crate::Error::new(
                crate::ErrorKind::UnsupportedExpression,
                offset,
                name,
            ));
        }
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
            Self::Aggregate(_) | Self::Deferred(_) | Self::Lookup | Self::Library(_) => {
                unreachable!()
            }
        };
        Ok(Operand::One(Value::Boolean(result)))
    }
}

// Known standard names must not silently behave like unbound user variables.
const DEFERRED: &[&str] = &["random", "shuffle", "eval"];
