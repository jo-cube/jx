mod date;
mod decimal;
mod integer;
mod number;
mod words;

use crate::{
    Error, ErrorKind, OwnedString, Value,
    builtin::library::{Library, number as numeric},
    sequence::Context,
};
use std::borrow::Cow;

fn error(offset: usize, code: &'static str) -> Error {
    Error::new(
        if code == "D3110" {
            ErrorKind::DateTimeError
        } else {
            ErrorKind::PictureError
        },
        offset,
        code,
    )
}
fn unsupported(offset: usize, message: &'static str) -> Error {
    Error::new(ErrorKind::UnsupportedExpression, offset, message)
}
fn text<'a>(value: &'a Value<'_, '_>, offset: usize) -> Result<Cow<'a, str>, Error> {
    let body = value
        .string_body()
        .ok_or_else(|| crate::value::type_error(offset))?;
    if !body.contains('\\') {
        return Ok(Cow::Borrowed(body));
    }
    String::from_utf16(&crate::json::string::units(body).collect::<Vec<_>>())
        .map(Cow::Owned)
        .map_err(|_| unsupported(offset, "surrogate units in format pictures are deferred"))
}
#[derive(Clone, Debug)]
pub(crate) struct Program(Spec);
#[derive(Clone, Debug)]
enum Spec {
    Integer(integer::Integer),
    Number(Box<number::Number>),
    Date(date::Date),
    ParseDate(date::Parser),
}
fn program(
    kind: Library,
    picture: Option<&str>,
    options: Option<&Value<'_, '_>>,
    offset: usize,
) -> Result<Program, Error> {
    Ok(Program(match kind {
        Library::FormatInteger | Library::ParseInteger => Spec::Integer(integer::Integer::new(
            picture.ok_or_else(|| crate::value::type_error(offset))?,
            offset,
        )?),
        Library::FormatNumber => Spec::Number(Box::new(number::Number::new(
            picture.ok_or_else(|| crate::value::type_error(offset))?,
            options,
            offset,
        )?)),
        Library::FromMillis | Library::Now => {
            Spec::Date(date::Date::new(picture.unwrap_or(date::ISO), offset)?)
        }
        Library::ToMillis => Spec::ParseDate(date::Parser::new(picture, offset)?),
        _ => unreachable!(),
    }))
}
pub(crate) fn call<'e, 'i>(
    kind: Library,
    args: &[Option<Value<'e, 'i>>; 3],
    context: &Context<'e, 'i>,
    offset: usize,
    prepared: Option<&Result<Program, Error>>,
) -> Result<Option<Value<'e, 'i>>, Error> {
    if kind == Library::Millis {
        return Ok(Some(Value::Number(
            context.scope.as_ref().expect("clock scope").timestamp() as f64,
        )));
    }
    if kind != Library::Now && args[0].is_none() {
        return Ok(None);
    }
    if kind == Library::FormatBase {
        return base(args, offset).map(|v| Some(Value::String(OwnedString::text(v))));
    }
    let (picture, options) = if kind == Library::Now {
        (&args[0], None)
    } else {
        (&args[1], args[2].as_ref())
    };
    let dynamic;
    let spec = if let Some(prepared) = prepared {
        prepared.as_ref().map_err(Clone::clone)?
    } else {
        let picture = picture.as_ref().map(|p| text(p, offset)).transpose()?;
        dynamic = program(
            kind,
            picture.as_deref(),
            if kind == Library::FormatNumber {
                options
            } else {
                None
            },
            offset,
        )?;
        &dynamic
    };
    let result = match (kind, &spec.0) {
        (Library::FormatNumber, Spec::Number(spec)) => {
            spec.format(numeric(&args[0]).unwrap(), offset)?
        }
        (Library::FormatInteger, Spec::Integer(spec)) => {
            spec.format(numeric(&args[0]).unwrap(), offset)?
        }
        (Library::ParseInteger, Spec::Integer(spec)) => {
            return Ok(Some(Value::Number(
                spec.parse(&text(args[0].as_ref().unwrap(), offset)?, offset)?,
            )));
        }
        (Library::FromMillis | Library::Now, Spec::Date(spec)) => {
            let n = if kind == Library::Now {
                context.scope.as_ref().expect("clock scope").timestamp() as f64
            } else {
                numeric(&args[0]).unwrap()
            };
            let timezone = if kind == Library::Now {
                &args[1]
            } else {
                &args[2]
            };
            let timezone = timezone.as_ref().map(|v| text(v, offset)).transpose()?;
            spec.format(n, timezone.as_deref(), offset)?
        }
        (Library::ToMillis, Spec::ParseDate(spec)) => {
            return spec
                .parse(&text(args[0].as_ref().unwrap(), offset)?, context, offset)
                .map(|v| v.map(Value::Number));
        }
        _ => unreachable!(),
    };
    Ok(Some(Value::String(OwnedString::text(result))))
}
fn base(args: &[Option<Value<'_, '_>>; 3], offset: usize) -> Result<String, Error> {
    let n = numeric(&args[0]).unwrap().round_ties_even();
    let radix = numeric(&args[1]).unwrap_or(10.0).round_ties_even();
    if !(2.0..=36.0).contains(&radix) {
        return Err(crate::value::range_error(offset));
    }
    if !n.is_finite() {
        return Ok(if n.is_nan() {
            "NaN"
        } else if n > 0.0 {
            "Infinity"
        } else {
            "-Infinity"
        }
        .into());
    }
    if n.abs() >= 1e21 {
        return Err(unsupported(
            offset,
            "large floating-point radix conversion is deferred",
        ));
    }
    let mut n = n.abs() as u128;
    let mut bytes = [0; 128];
    let mut at = bytes.len();
    loop {
        at -= 1;
        let digit = (n % radix as u128) as u8;
        bytes[at] = if digit < 10 {
            b'0' + digit
        } else {
            b'a' + digit - 10
        };
        n /= radix as u128;
        if n == 0 {
            break;
        }
    }
    if numeric(&args[0]).unwrap().round_ties_even() < 0.0 {
        at -= 1;
        bytes[at] = b'-';
    }
    Ok(std::str::from_utf8(&bytes[at..]).unwrap().into())
}

#[derive(Clone, Debug)]
pub(crate) struct Call {
    pub function: Library,
    pub args: Box<[crate::expression::Node]>,
    picture_index: Option<usize>,
    program: Result<Program, Error>,
}
impl Call {
    pub fn prepare(
        function: Library,
        args: &mut Box<[crate::expression::Node]>,
        offset: usize,
    ) -> Option<Self> {
        use Library::*;
        if !matches!(
            function,
            FormatNumber | FormatInteger | ParseInteger | FromMillis | ToMillis | Now
        ) || args.len() > 3
        {
            return None;
        }
        let shifted = matches!(function, FormatNumber | FormatInteger | FromMillis)
            && args
                .first()
                .and_then(static_value)
                .is_some_and(|v| v.string_body().is_some());
        let picture_parameter = usize::from(function != Now && !shifted);
        let index = if args.len() > picture_parameter {
            Some(picture_parameter)
        } else if matches!(function, FormatInteger | ParseInteger | FormatNumber) && args.len() == 1
        {
            Some(0)
        } else {
            None
        };
        let picture_value = match index {
            Some(i) => Some(static_value(&args[i])?),
            None => None,
        };
        let picture = picture_value
            .as_ref()
            .filter(|v| !matches!(v, Value::Undefined));
        let picture = picture.map(|v| text(v, offset)).transpose();
        let options_index = picture_parameter + 1;
        let options = if function == FormatNumber && args.len() > options_index {
            Some(static_value(&args[options_index])?)
        } else {
            None
        };
        let prepared = picture.and_then(|p| {
            program(
                function,
                p.as_deref(),
                options.as_ref().filter(|v| !matches!(v, Value::Undefined)),
                offset,
            )
        });
        Some(Self {
            function,
            args: std::mem::take(args),
            picture_index: index,
            program: prepared,
        })
    }
    pub fn needs_clock(&self) -> bool {
        match self.function {
            Library::ToMillis => {
                matches!(&self.program,Ok(Program(Spec::ParseDate(parser))) if parser.needs_clock())
            }
            Library::Now => true,
            _ => false,
        }
    }
    pub fn constant(&self) -> bool {
        if self.function == Library::ToMillis {
            !self.args.is_empty() && !self.needs_clock()
        } else {
            self.function.constant(&self.args)
        }
    }
    pub fn evaluate<'e, 'i>(
        &'e self,
        context: &Context<'e, 'i>,
        offset: usize,
    ) -> Result<crate::evaluate::Operand<'e, 'i>, Error> {
        let mut evaluated = [None, None, None];
        for (i, arg) in self.args.iter().enumerate() {
            evaluated[i] = crate::retain::materialize(arg, context)?;
        }
        let mut values = [None, None, None];
        let skip = self.function.arguments::<3>(
            &evaluated[..self.args.len()],
            context,
            offset,
            &mut values,
        )?;
        let parameter = usize::from(self.function != Library::Now);
        let actual_index = if parameter >= skip && self.args.len() > parameter - skip {
            Some(parameter - skip)
        } else {
            None
        };
        let prepared = (actual_index == self.picture_index).then_some(&self.program);
        call(self.function, &values, context, offset, prepared).map(|v| {
            v.map_or(
                crate::evaluate::Operand::Missing,
                crate::evaluate::Operand::One,
            )
        })
    }
}
fn static_value(node: &crate::expression::Node) -> Option<Value<'_, 'static>> {
    use crate::expression::Kind;
    Some(match &node.kind {
        Kind::Prepared(p) => p.data.value(),
        Kind::String(s) => Value::StringLiteral(crate::RawJson(s)),
        Kind::Number(n) => Value::Number(*n),
        Kind::Boolean(v) => Value::Boolean(*v),
        Kind::Null => Value::Null,
        Kind::Missing => Value::Undefined,
        _ => return None,
    })
}
