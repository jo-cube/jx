use super::arguments::Arguments;
use crate::{Error, ErrorKind, Value};

const NUMBER: u8 = 1;
const STRING: u8 = 2;
const BOOLEAN: u8 = 4;
const NULL: u8 = 8;
const OBJECT: u8 = 16;
const ARRAY: u8 = 32;
const FUNCTION: u8 = 64;
const MISSING: u8 = 128;

#[derive(Clone, Debug)]
pub(crate) struct Signature {
    params: Box<[Param]>,
    fixed: bool,
}
#[derive(Clone, Debug)]
struct Param {
    mask: u8,
    repeat: Repeat,
    context: bool,
    array: bool,
    subtype: Option<Box<str>>,
}
#[derive(Clone, Copy, Debug)]
enum Repeat {
    One,
    Optional,
    Many,
}

impl Signature {
    pub(crate) fn compile(source: &str, offset: usize) -> Result<Self, Error> {
        let bytes = source.as_bytes();
        let mut at = 1;
        let mut params: Vec<Param> = Vec::new();
        while at < bytes.len() - 1 && bytes[at] != b':' {
            let ch = bytes[at];
            if matches!(ch, b'?' | b'+' | b'-') {
                let param = params.last_mut().ok_or_else(|| syntax(offset + at))?;
                param.repeat = if ch == b'+' {
                    Repeat::Many
                } else {
                    Repeat::Optional
                };
                param.context |= ch == b'-';
                at += 1;
                continue;
            }
            if ch == b'<' {
                let param = params.last_mut().ok_or_else(|| syntax(offset + at))?;
                if !param.array && param.mask != FUNCTION {
                    return Err(Error::new(
                        ErrorKind::SignatureError,
                        offset + at,
                        "S0401: only arrays and functions accept a type parameter",
                    ));
                }
                let end = closing(bytes, at, b'<', b'>').ok_or_else(|| syntax(offset + at))?;
                param.subtype = Some(source[at + 1..end].into());
                at = end + 1;
                continue;
            }
            let (mask, array) = if ch == b'(' {
                let end = closing(bytes, at, b'(', b')').ok_or_else(|| syntax(offset + at))?;
                if bytes[at + 1..end].contains(&b'<') {
                    return Err(Error::new(
                        ErrorKind::SignatureError,
                        offset + at,
                        "S0402: parameterized types in a union are not supported",
                    ));
                }
                let mut mask = MISSING;
                for &ch in &bytes[at + 1..end] {
                    mask |= symbol(ch).ok_or_else(|| syntax(offset + at))?;
                }
                at = end;
                (mask, false)
            } else {
                let mask = match ch {
                    b'a' | b'x' => u8::MAX,
                    b'j' => u8::MAX ^ FUNCTION,
                    b'u' => NUMBER | STRING | BOOLEAN | NULL | MISSING,
                    b'f' => FUNCTION,
                    _ => symbol(ch).ok_or_else(|| syntax(offset + at))? | MISSING,
                };
                (mask, ch == b'a')
            };
            params.push(Param {
                mask,
                repeat: Repeat::One,
                context: false,
                array,
                subtype: None,
            });
            if params.len() > 128 {
                return Err(syntax(offset + at));
            }
            at += 1;
        }
        // Return types and function sub-signatures describe intent; the reference
        // validates neither. Array subtypes use its shallow homogeneous rule.
        let fixed = params.iter().all(|p| matches!(p.repeat, Repeat::One));
        Ok(Self {
            params: params.into_boxed_slice(),
            fixed,
        })
    }
    pub(super) fn validate<'e, 'i>(
        &self,
        args: &[Option<Value<'e, 'i>>],
        focus: &Value<'e, 'i>,
        offset: usize,
    ) -> Result<Arguments<'e, 'i>, Error> {
        let mut counts = [0; 128];
        let matched = if self.fixed {
            counts[..self.params.len()].fill(1);
            args.len() == self.params.len()
                && args
                    .iter()
                    .zip(&self.params)
                    .all(|(v, p)| p.mask & kind(v.as_ref()) != 0)
        } else {
            match_arguments(&self.params, args, &mut counts)
        };
        if !matched {
            return Err(invalid(
                offset,
                "T0410: arguments do not match function signature",
            ));
        }
        let mut result = Arguments::new(args.len().max(self.params.len()));
        let mut at = 0;
        for (param, count) in self.params.iter().zip(counts) {
            if count == 0 {
                if param.context {
                    if param.mask & kind(Some(focus)) == 0 {
                        return Err(invalid(
                            offset,
                            "T0411: context does not match function signature",
                        ));
                    }
                    result.push(Some(focus.clone()));
                } else {
                    // A skipped optional parameter still advances the argument
                    // index in the pinned reference; later bindings may shift.
                    result.push(args.get(at).cloned().flatten());
                    at += 1;
                }
                continue;
            }
            for _ in 0..count {
                let arg = args.get(at).cloned().flatten();
                at += 1;
                if param.array && kind(arg.as_ref()) != MISSING {
                    let value = arg.unwrap();
                    if let Some(subtype) = &param.subtype {
                        let valid = if value.is_array() {
                            let mut items = value.elements();
                            match items.next() {
                                None => true,
                                Some(first) => {
                                    let first = kind(Some(&first));
                                    subtype.as_bytes().first().copied().and_then(symbol)
                                        == Some(first)
                                        && items.all(|item| kind(Some(&item)) == first)
                                }
                            }
                        } else {
                            count == 1
                                && subtype.len() == 1
                                && subtype.as_bytes().first().copied().and_then(symbol)
                                    == Some(kind(Some(&value)))
                        };
                        if !valid {
                            return Err(invalid(
                                offset,
                                "T0412: array elements do not match function signature",
                            ));
                        }
                    }
                    result.push(Some(if value.is_array() {
                        value
                    } else {
                        Value::array(vec![value], false)
                    }));
                } else {
                    result.push(arg);
                }
            }
        }
        Ok(result)
    }
}
// Suffix reachability keeps ambiguous optional/variadic signatures bounded.
// Small signatures use stack scratch; only unusually wide calls allocate it.
fn match_arguments(params: &[Param], args: &[Option<Value<'_, '_>>], counts: &mut [usize]) -> bool {
    let width = args.len() + 1;
    let size = (params.len() + 1) * width;
    let mut small = [false; 256];
    let mut large;
    let table = if size <= small.len() {
        &mut small[..size]
    } else {
        large = vec![false; size];
        &mut large
    };
    table[params.len() * width + args.len()] = true;
    for i in (0..params.len()).rev() {
        let param = &params[i];
        for at in (0..=args.len()).rev() {
            let accepts = args
                .get(at)
                .is_some_and(|v| param.mask & kind(v.as_ref()) != 0);
            table[i * width + at] = match param.repeat {
                Repeat::One => accepts && table[(i + 1) * width + at + 1],
                Repeat::Optional => {
                    table[(i + 1) * width + at] || accepts && table[(i + 1) * width + at + 1]
                }
                Repeat::Many => {
                    accepts && (table[(i + 1) * width + at + 1] || table[i * width + at + 1])
                }
            };
        }
    }
    if !table[0] {
        return false;
    }
    let mut at = 0;
    for (i, param) in params.iter().enumerate() {
        let max = match param.repeat {
            Repeat::One | Repeat::Optional => usize::from(
                args.get(at)
                    .is_some_and(|v| param.mask & kind(v.as_ref()) != 0),
            ),
            Repeat::Many => args[at..]
                .iter()
                .take_while(|v| param.mask & kind(v.as_ref()) != 0)
                .count(),
        };
        let min = usize::from(!matches!(param.repeat, Repeat::Optional));
        let n = (min..=max)
            .rev()
            .find(|n| table[(i + 1) * width + at + n])
            .unwrap();
        counts[i] = n;
        at += n;
    }
    true
}
fn kind(value: Option<&Value<'_, '_>>) -> u8 {
    let Some(value) = value else {
        return MISSING;
    };
    match value {
        Value::Raw(raw) => match raw.as_bytes()[0] {
            b'n' => NULL,
            b't' | b'f' => BOOLEAN,
            b'-' | b'0'..=b'9' => NUMBER,
            b'"' => STRING,
            b'[' => ARRAY,
            b'{' => OBJECT,
            _ => unreachable!("validated JSON value"),
        },
        Value::Undefined => MISSING,
        Value::Null => NULL,
        Value::Boolean(_) => BOOLEAN,
        Value::Number(_) => NUMBER,
        Value::Function(_) => FUNCTION,
        value if value.string_body().is_some() => STRING,
        value if value.is_array() => ARRAY,
        _ => OBJECT,
    }
}
fn symbol(ch: u8) -> Option<u8> {
    Some(match ch {
        b'n' => NUMBER,
        b's' => STRING,
        b'b' => BOOLEAN,
        b'l' => NULL,
        b'o' => OBJECT,
        b'a' => ARRAY,
        b'f' => FUNCTION,
        b'x' => u8::MAX,
        b'j' => u8::MAX ^ FUNCTION,
        b'u' => NUMBER | STRING | BOOLEAN | NULL,
        _ => return None,
    })
}
fn closing(bytes: &[u8], start: usize, open: u8, close: u8) -> Option<usize> {
    let mut depth = 0;
    for (at, &ch) in bytes.iter().enumerate().skip(start) {
        if ch == open {
            depth += 1;
        }
        if ch == close {
            depth -= 1;
            if depth == 0 {
                return Some(at);
            }
        }
    }
    None
}
fn invalid(offset: usize, message: &'static str) -> Error {
    Error::new(ErrorKind::TypeError, offset, message)
}
fn syntax(offset: usize) -> Error {
    Error::new(
        ErrorKind::SignatureError,
        offset,
        "invalid function signature",
    )
}
