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
    BindingError,
    HostError,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase {
    Compilation,
    Validation,
    Evaluation,
    Serialization,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Source {
    Expression,
    Input,
    DynamicExpression,
    Result,
    Host,
}
/// Zero-based half-open byte span. Unknown token ends use an empty point span.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

/// Offsets are zero-based bytes in the expression or input record.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Error {
    pub kind: ErrorKind,
    pub offset: usize,
    pub message: std::borrow::Cow<'static, str>,
    pub phase: Phase,
    pub source: Source,
    cause: Option<Box<Error>>,
}

impl Error {
    pub fn user(message: impl Into<String>) -> Self {
        let mut error = Self::custom(ErrorKind::UserError, 0, message.into());
        error.source = Source::Host;
        error
    }
    pub fn span(&self) -> Span {
        Span {
            start: self.offset,
            end: self.offset,
        }
    }
    pub fn cause(&self) -> Option<&Error> {
        self.cause.as_deref()
    }
    pub(crate) fn with_cause(mut self, cause: Error) -> Self {
        self.cause = Some(Box::new(cause));
        self
    }
    pub fn location(&self, source: &str) -> Option<(usize, usize)> {
        let prefix = source.get(..self.offset)?;
        Some((
            prefix.bytes().filter(|&b| b == b'\n').count() + 1,
            prefix.rsplit('\n').next().unwrap().chars().count() + 1,
        ))
    }
    pub(crate) fn result(mut self) -> Self {
        self.source = Source::Result;
        self
    }
    pub(crate) fn compilation(mut self) -> Self {
        self.phase = Phase::Compilation;
        self.source = Source::Expression;
        self
    }
    pub(crate) fn input(mut self) -> Self {
        self.phase = Phase::Validation;
        self.source = Source::Input;
        self
    }

    pub(crate) fn new(kind: ErrorKind, offset: usize, message: &'static str) -> Self {
        Self {
            kind,
            offset,
            message: message.into(),
            phase: if matches!(kind, ErrorKind::InvalidJson) {
                Phase::Validation
            } else {
                Phase::Evaluation
            },
            source: if matches!(kind, ErrorKind::InvalidJson) {
                Source::Input
            } else {
                Source::Expression
            },
            cause: None,
        }
    }
    pub(crate) fn custom(kind: ErrorKind, offset: usize, message: String) -> Self {
        let mut error = Self::new(kind, offset, "");
        error.message = message.into();
        error
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

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.cause.as_deref().map(|e| e as _)
    }
}
