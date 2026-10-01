use std::fmt::{self, Write};
// Callers bound integer magnitude below 1e21 and fractional precision to 100:
// fixed output needs at most 122 bytes, including its decimal separator.
pub(super) struct Decimal {
    bytes: [u8; 128],
    len: usize,
}
impl Decimal {
    pub fn integer(n: f64) -> Self {
        let mut value = Self {
            bytes: [0; 128],
            len: 0,
        };
        write!(value, "{n}").unwrap();
        value
    }
    pub fn fixed(n: f64, precision: usize) -> Self {
        let mut value = Self {
            bytes: [0; 128],
            len: 0,
        };
        write!(value, "{n:.precision$}").unwrap();
        value
    }
    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len]).unwrap()
    }
}
impl Write for Decimal {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        let end = self.len + text.len();
        self.bytes
            .get_mut(self.len..end)
            .ok_or(fmt::Error)?
            .copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}
