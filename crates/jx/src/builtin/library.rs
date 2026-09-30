use crate::{
    Error, Value,
    evaluate::Operand,
    expression::Node,
    sequence::Context,
    value::{range_error, type_error},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Library {
    String,
    Number,
    Length,
    Uppercase,
    Lowercase,
    Trim,
    Substring,
    Before,
    After,
    Contains,
    Split,
    Join,
    Abs,
    Floor,
    Ceil,
    Sqrt,
    Power,
    Append,
    Reverse,
    Distinct,
    Keys,
    Spread,
    Merge,
    Type,
    Map,
    Filter,
    Reduce,
    Each,
    Sift,
}
#[derive(Clone, Copy)]
enum Param {
    Any,
    Boolean,
    Numeric,
    Number,
    String,
    Pattern,
    Array,
    Strings,
    Objects,
    Object,
    Function,
    Json,
}
impl Param {
    fn accepts(self, value: &Option<Value<'_, '_>>) -> bool {
        let Some(value) = value.as_ref().filter(|v| !matches!(v, Value::Undefined)) else {
            return !matches!(self, Self::Function);
        };
        match self {
            Self::Any | Self::Array | Self::Strings | Self::Objects => true,
            Self::Boolean => matches!(value.atomic(), Value::Boolean(_)),
            Self::Numeric => {
                matches!(value.atomic(), Value::Number(_) | Value::Boolean(_))
                    || value.string_body().is_some()
            }
            Self::Number => matches!(value.atomic(), Value::Number(_)),
            Self::String => value.string_body().is_some(),
            Self::Pattern => value.string_body().is_some() || matches!(value, Value::Function(_)),
            Self::Object => value.is_object(),
            Self::Function => matches!(value, Value::Function(_)),
            Self::Json => !matches!(value, Value::Function(_)),
        }
    }
}
impl Library {
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "string" => Self::String,
            "number" => Self::Number,
            "length" => Self::Length,
            "uppercase" => Self::Uppercase,
            "lowercase" => Self::Lowercase,
            "trim" => Self::Trim,
            "substring" => Self::Substring,
            "substringBefore" => Self::Before,
            "substringAfter" => Self::After,
            "contains" => Self::Contains,
            "split" => Self::Split,
            "join" => Self::Join,
            "abs" => Self::Abs,
            "floor" => Self::Floor,
            "ceil" => Self::Ceil,
            "sqrt" => Self::Sqrt,
            "power" => Self::Power,
            "append" => Self::Append,
            "reverse" => Self::Reverse,
            "distinct" => Self::Distinct,
            "keys" => Self::Keys,
            "spread" => Self::Spread,
            "merge" => Self::Merge,
            "type" => Self::Type,
            "map" => Self::Map,
            "filter" => Self::Filter,
            "reduce" => Self::Reduce,
            "each" => Self::Each,
            "sift" => Self::Sift,
            _ => return None,
        })
    }
    // Fixed signatures only: parameters, required count, optional leading context.
    // This is not the language's user-defined function signature system.
    fn signature(self) -> (&'static [Param], usize, bool) {
        use Param::*;
        match self {
            Self::String => (&[Any, Boolean], 1, true),
            Self::Number => (&[Numeric], 1, true),
            Self::Length | Self::Uppercase | Self::Lowercase | Self::Trim => (&[String], 1, true),
            Self::Substring => (&[String, Number, Number], 2, true),
            Self::Before | Self::After => (&[String, String], 2, true),
            Self::Contains => (&[String, Pattern], 2, true),
            Self::Split => (&[String, Pattern, Number], 2, true),
            Self::Join => (&[Strings, String], 1, false),
            Self::Abs | Self::Floor | Self::Ceil | Self::Sqrt => (&[Number], 1, true),
            Self::Power => (&[Number, Number], 2, true),
            Self::Append => (&[Any, Any], 2, false),
            Self::Reverse => (&[Array], 1, false),
            Self::Distinct | Self::Type => (&[Any], 1, false),
            Self::Keys | Self::Spread => (&[Any], 1, true),
            Self::Merge => (&[Objects], 1, false),
            Self::Map | Self::Filter => (&[Array, Function], 2, false),
            Self::Reduce => (&[Array, Function, Json], 2, false),
            Self::Each => (&[Object, Function], 2, true),
            Self::Sift => (&[Object, Function], 1, true),
        }
    }
    pub fn partial_values<'e, 'i>(
        self,
        args: &[Option<Value<'e, 'i>>],
        context: &Context<'e, 'i>,
        offset: usize,
    ) -> Result<Operand<'e, 'i>, Error> {
        if self == Self::Number {
            return crate::convert::number(args.first().cloned().flatten(), offset)
                .map(|value| value.map_or(Operand::Missing, Operand::One));
        }
        let (params, _, _) = self.signature();
        // Upstream native partials bypass signatures and array promotion. Keep
        // supported typed calls exact; do not emulate arbitrary JavaScript coercion.
        if args.iter().zip(params).any(|(value, param)| {
            !param.accepts(value)
                || matches!(param, Param::Array | Param::Strings | Param::Objects)
                    && value.as_ref().is_some_and(|v| !v.is_array())
        }) {
            return Err(crate::Error::new(
                crate::ErrorKind::UnsupportedExpression,
                offset,
                "untyped native partial application is deferred",
            ));
        }
        self.values(args, context, offset)
    }
    pub fn arity(self) -> usize {
        if self == Self::String {
            1
        } else {
            self.signature().0.len()
        }
    }
    pub fn constant(self, args: &[Node]) -> bool {
        let (params, _, context) = self.signature();
        // Optional context matching can depend on argument types, not only count.
        !matches!(
            self,
            Self::Map | Self::Filter | Self::Reduce | Self::Each | Self::Sift
        ) && (!context || args.len() == params.len() || self == Self::String && args.len() == 1)
    }
    pub fn evaluate<'e, 'i>(
        self,
        args: &'e [Node],
        context: &Context<'e, 'i>,
        offset: usize,
    ) -> Result<Operand<'e, 'i>, Error> {
        let mut values = [None, None, None];
        for (i, arg) in args.iter().enumerate() {
            let value = crate::retain::materialize(arg, context)?;
            if i < values.len() {
                values[i] = value;
            }
        }
        if args.len() > values.len() {
            return Err(type_error(offset));
        }
        self.values(&values[..args.len()], context, offset)
    }
    pub fn values<'e, 'i>(
        self,
        args: &[Option<Value<'e, 'i>>],
        context: &Context<'e, 'i>,
        offset: usize,
    ) -> Result<Operand<'e, 'i>, Error> {
        let (params, required, contextual) = self.signature();
        let fits = |skip: usize| {
            args.len() + skip >= required
                && args.len() + skip <= params.len()
                && args.iter().zip(&params[skip..]).all(|(v, p)| p.accepts(v))
        };
        let skip = if fits(0) {
            0
        } else if contextual && fits(1) {
            1
        } else {
            return Err(type_error(offset));
        };
        let mut values = [None, None, None];
        if skip == 1 {
            values[0] = Some(context.value.clone());
            if !params[0].accepts(&values[0]) {
                return Err(type_error(offset));
            }
        }
        for (i, arg) in args.iter().enumerate() {
            values[i + skip] = arg.clone();
        }
        for (value, param) in values.iter_mut().zip(params) {
            if matches!(value, Some(Value::Undefined)) {
                *value = None;
            }
            if let Some(value) = value {
                let item_type = match param {
                    Param::Strings => Some(Param::String),
                    Param::Objects => Some(Param::Object),
                    _ => None,
                };
                if let Some(item_type) = item_type {
                    let valid = |v: Value<'e, 'i>| {
                        !matches!(v, Value::Undefined) && item_type.accepts(&Some(v))
                    };
                    if !(if value.is_array() {
                        value.elements().all(valid)
                    } else {
                        valid(value.clone())
                    }) {
                        return Err(type_error(offset));
                    }
                }
            }
        }
        let result = match self {
            Self::String => crate::convert::string(
                values[0].clone(),
                matches!(
                    values[1].as_ref().map(Value::atomic),
                    Some(Value::Boolean(true))
                ),
                offset,
            )?,
            Self::Number => crate::convert::number(values[0].clone(), offset)?,
            Self::Length
            | Self::Uppercase
            | Self::Lowercase
            | Self::Trim
            | Self::Substring
            | Self::Before
            | Self::After
            | Self::Contains
            | Self::Split
            | Self::Join => super::strings::call(self, &values, offset)?,
            Self::Abs | Self::Floor | Self::Ceil | Self::Sqrt | Self::Power => {
                let Some(value) = &values[0] else {
                    return Ok(Operand::Missing);
                };
                let Value::Number(n) = value.atomic() else {
                    unreachable!()
                };
                let result = match self {
                    Self::Abs => n.abs(),
                    Self::Floor => n.floor(),
                    Self::Ceil => n.ceil(),
                    Self::Sqrt if n < 0.0 => return Err(range_error(offset)),
                    Self::Sqrt => n.sqrt(),
                    Self::Power => n.powf(number(&values[1]).unwrap_or(f64::NAN)),
                    _ => unreachable!(),
                };
                if matches!(self, Self::Power) && !result.is_finite() {
                    return Err(range_error(offset));
                }
                Some(Value::Number(result))
            }
            Self::Map | Self::Filter | Self::Reduce | Self::Each | Self::Sift => {
                super::higher::call(self, &values, context, offset)?
            }
            _ => super::collections::call(self, &values, offset)?,
        };
        Ok(result.map_or(Operand::Missing, Operand::One))
    }
}
pub(super) fn number(value: &Option<Value<'_, '_>>) -> Option<f64> {
    match value.as_ref()?.atomic() {
        Value::Number(n) => Some(n),
        _ => unreachable!("validated numeric argument"),
    }
}
