use crate::{
    Error, Value,
    evaluate::Operand,
    expression::{Kind, Node},
    sequence::Context,
    value::{range_error, type_error},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Library {
    FormatNumber,
    FormatInteger,
    ParseInteger,
    FormatBase,
    FromMillis,
    ToMillis,
    Now,
    Millis,
    Round,
    Pad,
    Sort,
    Zip,
    Single,
    EncodeUrl,
    EncodeComponent,
    DecodeUrl,
    DecodeComponent,
    Base64Encode,
    Base64Decode,
    Error,
    Assert,
    String,
    Number,
    Clone,
    Length,
    Uppercase,
    Lowercase,
    Trim,
    Substring,
    Before,
    After,
    Contains,
    Split,
    Match,
    Replace,
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
    Container,
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
            Self::Container => value.is_object() || value.is_array(),
            Self::Function => matches!(value, Value::Function(_)),
            Self::Json => !matches!(value, Value::Function(_)),
        }
    }
}
impl Library {
    pub fn uses_clock(self) -> bool {
        matches!(self, Self::ToMillis | Self::Now | Self::Millis)
    }
    pub fn clock_call(self, args: &[Node]) -> bool {
        match self {
            Self::ToMillis => args.get(1).is_some_and(|n| match &n.kind {
                Kind::Missing => false,
                Kind::Prepared(p) => !matches!(p.data, crate::constant::Data::Missing),
                _ => true,
            }),
            _ => self.uses_clock(),
        }
    }
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "formatNumber" => Self::FormatNumber,
            "formatInteger" => Self::FormatInteger,
            "parseInteger" => Self::ParseInteger,
            "formatBase" => Self::FormatBase,
            "fromMillis" => Self::FromMillis,
            "toMillis" => Self::ToMillis,
            "now" => Self::Now,
            "millis" => Self::Millis,
            "round" => Self::Round,
            "pad" => Self::Pad,
            "sort" => Self::Sort,
            "zip" => Self::Zip,
            "single" => Self::Single,
            "encodeUrl" => Self::EncodeUrl,
            "encodeUrlComponent" => Self::EncodeComponent,
            "decodeUrl" => Self::DecodeUrl,
            "decodeUrlComponent" => Self::DecodeComponent,
            "base64encode" => Self::Base64Encode,
            "base64decode" => Self::Base64Decode,
            "error" => Self::Error,
            "assert" => Self::Assert,
            "string" => Self::String,
            "number" => Self::Number,
            "clone" => Self::Clone,
            "length" => Self::Length,
            "uppercase" => Self::Uppercase,
            "lowercase" => Self::Lowercase,
            "trim" => Self::Trim,
            "substring" => Self::Substring,
            "substringBefore" => Self::Before,
            "substringAfter" => Self::After,
            "contains" => Self::Contains,
            "split" => Self::Split,
            "match" => Self::Match,
            "replace" => Self::Replace,
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
            Self::FormatNumber => (&[Number, String, Object], 2, true),
            Self::FormatInteger => (&[Number, String], 2, true),
            Self::ParseInteger => (&[String, String], 2, true),
            Self::FormatBase => (&[Number, Number], 1, true),
            Self::FromMillis => (&[Number, String, String], 1, true),
            Self::ToMillis => (&[String, String], 1, true),
            Self::Now => (&[String, String], 0, false),
            Self::Millis => (&[], 0, false),
            Self::Round => (&[Number, Number], 1, true),
            Self::Pad => (&[String, Number, String], 2, true),
            Self::Sort | Self::Single => (&[Array, Function], 1, false),
            Self::Zip => (&[Array], 1, false),
            Self::Error => (&[String], 0, false),
            Self::Assert => (&[Boolean, String], 1, false),
            Self::EncodeUrl
            | Self::EncodeComponent
            | Self::DecodeUrl
            | Self::DecodeComponent
            | Self::Base64Encode
            | Self::Base64Decode => (&[String], 1, true),
            Self::String => (&[Any, Boolean], 1, true),
            Self::Number => (&[Numeric], 1, true),
            Self::Clone => (&[Container], 1, true),
            Self::Length | Self::Uppercase | Self::Lowercase | Self::Trim => (&[String], 1, true),
            Self::Substring => (&[String, Number, Number], 2, true),
            Self::Before | Self::After => (&[String, String], 2, true),
            Self::Contains => (&[String, Pattern], 2, true),
            Self::Split => (&[String, Pattern, Number], 2, true),
            Self::Match => (&[String, Function, Number], 2, true),
            Self::Replace => (&[String, Pattern, Pattern, Number], 3, true),
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
        if self == Self::Zip {
            return Ok(Operand::One(
                if args
                    .iter()
                    .any(|v| v.as_ref().is_none_or(|v| !v.is_array()))
                {
                    Value::array(Vec::new(), false)
                } else {
                    super::arrays::zip(args)
                },
            ));
        }
        // Native partials bypass signatures: an omitted optional comparator is
        // still its default. Direct calls with an explicit missing callback fail.
        let args =
            if matches!(self, Self::Sort | Self::Single) && args.len() == 2 && args[1].is_none() {
                &args[..1]
            } else {
                args
            };
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
        if self == Self::ToMillis
            && args.len() == 1
            && matches!(
                &args[0].kind,
                crate::expression::Kind::String(_) | crate::expression::Kind::Missing
            )
        {
            return true;
        }
        if matches!(self, Self::Sort | Self::Single) {
            return args.len() == 1;
        }
        if matches!(
            self,
            Self::FormatNumber | Self::FormatBase | Self::FromMillis
        ) && args.first().is_some_and(|n| match &n.kind {
            Kind::Number(_) | Kind::Missing => true,
            Kind::Prepared(p) => matches!(
                p.data,
                crate::constant::Data::Number(_) | crate::constant::Data::Missing
            ),
            _ => false,
        }) {
            return true;
        }
        if self == Self::Round && args.len() == 1 {
            return true;
        }
        let (params, _, context) = self.signature();
        // Optional context matching can depend on argument types, not only count.
        !matches!(
            self,
            Self::Now
                | Self::Millis
                | Self::ToMillis
                | Self::Clone
                | Self::Map
                | Self::Filter
                | Self::Reduce
                | Self::Each
                | Self::Sift
                | Self::Error
                | Self::Assert
        ) && (!context || args.len() == params.len() || self == Self::String && args.len() == 1)
    }
    pub fn contextual(self, argc: usize) -> bool {
        let (params, _, context) = self.signature();
        context && argc < params.len()
    }
    pub fn evaluate_in<'e, 'i>(
        self,
        args: &'e [Node],
        context: &Context<'e, 'i>,
        caller: &Context<'e, 'i>,
        offset: usize,
    ) -> Result<Operand<'e, 'i>, Error> {
        if self == Self::Zip {
            let values = args
                .iter()
                .map(|arg| crate::retain::materialize(arg, context))
                .collect::<Result<Vec<_>, _>>()?;
            return self.values(&values, caller, offset);
        }
        if self == Self::Replace {
            let mut values = [None, None, None, None];
            for (i, arg) in args.iter().enumerate() {
                let value = crate::retain::materialize(arg, context)?;
                if i < values.len() {
                    values[i] = value;
                }
            }
            if args.len() > values.len() {
                return Err(type_error(offset));
            }
            return self.values(&values[..args.len()], caller, offset);
        }
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
        self.values(&values[..args.len()], caller, offset)
    }
    pub fn values<'e, 'i>(
        self,
        args: &[Option<Value<'e, 'i>>],
        context: &Context<'e, 'i>,
        offset: usize,
    ) -> Result<Operand<'e, 'i>, Error> {
        if self == Self::Zip {
            if args.is_empty() {
                return Err(type_error(offset));
            }
            return Ok(Operand::One(super::arrays::zip(args)));
        }
        if self == Self::Replace {
            self.validated::<4>(args, context, offset)
        } else {
            self.validated::<3>(args, context, offset)
        }
    }
    pub(crate) fn arguments<'e, 'i, const N: usize>(
        self,
        args: &[Option<Value<'e, 'i>>],
        context: &Context<'e, 'i>,
        offset: usize,
        values: &mut [Option<Value<'e, 'i>>; N],
    ) -> Result<usize, Error> {
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
        Ok(skip)
    }
    fn validated<'e, 'i, const N: usize>(
        self,
        args: &[Option<Value<'e, 'i>>],
        context: &Context<'e, 'i>,
        offset: usize,
    ) -> Result<Operand<'e, 'i>, Error> {
        let mut values = std::array::from_fn(|_| None);
        self.arguments::<N>(args, context, offset, &mut values)?;
        let result = match self {
            Self::FormatNumber
            | Self::FormatInteger
            | Self::ParseInteger
            | Self::FormatBase
            | Self::FromMillis
            | Self::ToMillis
            | Self::Now
            | Self::Millis => {
                crate::format::call(self, values[..3].try_into().unwrap(), context, offset, None)?
            }
            Self::Round => super::numeric::round_value(&values),
            Self::Pad => super::padding::pad(values[..3].try_into().unwrap(), offset)?,
            Self::Sort | Self::Single => {
                super::arrays::call(self, values[..3].try_into().unwrap(), context, offset)?
            }
            Self::EncodeUrl
            | Self::EncodeComponent
            | Self::DecodeUrl
            | Self::DecodeComponent
            | Self::Base64Encode
            | Self::Base64Decode => super::encoding::call(self, values[0].clone(), offset)?,
            Self::Error | Self::Assert => {
                super::diagnostics::call(self, values[..3].try_into().unwrap(), offset)?
            }
            Self::Clone => crate::transform::clone(values[0].clone(), offset)?,
            Self::String => crate::convert::string(
                values[0].clone(),
                matches!(
                    values[1].as_ref().map(Value::atomic),
                    Some(Value::Boolean(true))
                ),
                offset,
            )?,
            Self::Number => crate::convert::number(values[0].clone(), offset)?,
            Self::Match | Self::Replace => crate::matcher::call(self, &values, context, offset)?,
            Self::Contains | Self::Split if matches!(values[1], Some(Value::Function(_))) => {
                crate::matcher::call(self, &values, context, offset)?
            }
            Self::Length
            | Self::Uppercase
            | Self::Lowercase
            | Self::Trim
            | Self::Substring
            | Self::Before
            | Self::After
            | Self::Contains
            | Self::Split
            | Self::Join => super::strings::call(self, values[..3].try_into().unwrap(), offset)?,
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
                super::higher::call(self, values[..3].try_into().unwrap(), context, offset)?
            }
            _ => super::collections::call(self, values[..3].try_into().unwrap(), offset)?,
        };
        Ok(result.map_or(Operand::Missing, Operand::One))
    }
}
pub(crate) fn number(value: &Option<Value<'_, '_>>) -> Option<f64> {
    match value.as_ref()?.atomic() {
        Value::Number(n) => Some(n),
        _ => unreachable!("validated numeric argument"),
    }
}
