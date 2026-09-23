use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ErrorKind {
    UnsupportedExpression,
    InvalidJson,
    DepthLimit,
    ArrayTraversal,
}

/// Offsets are zero-based bytes in the expression or input record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Error {
    pub kind: ErrorKind,
    pub offset: usize,
    pub message: &'static str,
}

impl Error {
    pub(crate) fn new(kind: ErrorKind, offset: usize, message: &'static str) -> Self {
        Self {
            kind,
            offset,
            message,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at byte {} ({:?})",
            self.message, self.offset, self.kind
        )
    }
}

impl std::error::Error for Error {}
