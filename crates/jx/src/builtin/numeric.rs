use crate::Value;
use std::fmt::{self, Write};

// Decimal exponent shifting uses the shortest decimal representation, rather
// than multiplication by 10^p (which changes half-even ties such as 4.525).
struct Decimal {
    bytes: [u8; 64],
    len: usize,
}
impl Write for Decimal {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let end = self.len + text.len();
        let target = self.bytes.get_mut(self.len..end).ok_or(fmt::Error)?;
        target.copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}
fn shift(n: f64, precision: i32) -> f64 {
    let mut text = Decimal {
        bytes: [0; 64],
        len: 0,
    };
    write!(text, "{n:e}").unwrap();
    let source = std::str::from_utf8(&text.bytes[..text.len]).unwrap();
    let Some((mantissa, exponent)) = source.split_once('e') else {
        return f64::NAN;
    };
    let exponent = exponent.parse::<i32>().unwrap() + precision;
    let mut shifted = Decimal {
        bytes: [0; 64],
        len: 0,
    };
    write!(shifted, "{mantissa}e{exponent}").unwrap();
    std::str::from_utf8(&shifted.bytes[..shifted.len])
        .unwrap()
        .parse()
        .unwrap_or(f64::NAN)
}
pub(super) fn round_value<'e, 'i, const N: usize>(
    args: &[Option<Value<'e, 'i>>; N],
) -> Option<Value<'e, 'i>> {
    let n = super::library::number(&args[0])?;
    let p = super::library::number(&args[1]).unwrap_or(0.0);
    let result = round(n, p);
    Some(Value::Number(if result == 0.0 { 0.0 } else { result }))
}

pub(crate) fn round(n: f64, p: f64) -> f64 {
    if p == 0.0 || p.is_nan() {
        n.round_ties_even()
    } else if p.fract() != 0.0 || !p.is_finite() || p.abs() >= 1e21 {
        f64::NAN
    } else if p.abs() > 10000.0 {
        if p < 0.0 || n == 0.0 { 0.0 } else { f64::NAN }
    } else {
        shift(shift(n, p as i32).round_ties_even(), -(p as i32))
    }
}
