use std::fmt;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
pub enum ErrorKind {
    UnsupportedExpression,
    InvalidJson,
    DepthLimit,
    TypeError,
    DuplicateKey,
    NumericRange,
    RegexError,
    UserError,
    AssertionFailed,
    CardinalityError,
    EncodingError,
    PictureError,
    DateTimeError,
    SignatureError,
    EvaluationLimit,
    EvalSyntax,
    EvalError,
}

/// Offsets are zero-based bytes in the expression or input record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Error {
    pub kind: ErrorKind,
    pub offset: usize,
    pub message: std::borrow::Cow<'static, str>,
}

impl Error {
    pub(crate) fn new(kind: ErrorKind, offset: usize, message: &'static str) -> Self {
        Self {
            kind,
            offset,
            message: message.into(),
        }
    }
    pub(crate) fn custom(kind: ErrorKind, offset: usize, message: String) -> Self {
        Self {
            kind,
            offset,
            message: message.into(),
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
