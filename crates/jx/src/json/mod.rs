mod scan;
pub(crate) mod string;

pub use scan::MAX_DEPTH;
pub(crate) use scan::{Elements, Members, Selection, select};

/// A validated JSON value borrowing its original UTF-8 encoding.
/// Arrays here are JSON values, not JSONata result sequences.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RawJson<'a>(pub(crate) &'a str);

impl<'a> RawJson<'a> {
    pub fn as_bytes(self) -> &'a [u8] {
        self.0.as_bytes()
    }

    pub fn as_str(self) -> &'a str {
        self.0
    }

    /// Remove only whitespace outside strings; preserve number and escape spelling.
    pub fn write_compact(self, mut output: impl std::io::Write) -> std::io::Result<()> {
        let bytes = self.as_bytes();
        let mut in_string = false;
        let mut escaped = false;
        let mut start = 0;
        for (at, &byte) in bytes.iter().enumerate() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if byte == b'\\' {
                    escaped = true;
                } else if byte == b'"' {
                    in_string = false;
                }
            } else if byte == b'"' {
                in_string = true;
            } else if matches!(byte, b' ' | b'\n' | b'\r' | b'\t') {
                output.write_all(&bytes[start..at])?;
                start = at + 1;
            }
        }
        output.write_all(&bytes[start..])
    }
}

/// Validate one complete UTF-8 JSON text without allocating a JSON tree.
pub fn validate(input: &[u8]) -> Result<RawJson<'_>, crate::Error> {
    match select(input, &[])? {
        Selection::Value(value) => Ok(value),
        _ => unreachable!("identity selects the validated root"),
    }
}
