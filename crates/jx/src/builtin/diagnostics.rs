use super::library::Library;
use crate::{Error, ErrorKind, Value, json::string};

pub(super) fn call<'e, 'i>(
    function: Library,
    args: &[Option<Value<'e, 'i>>; 3],
    offset: usize,
) -> Result<Option<Value<'e, 'i>>, Error> {
    if function == Library::Assert
        && matches!(
            args[0].as_ref().map(Value::atomic),
            Some(Value::Boolean(true))
        )
    {
        return Ok(None);
    }
    let (kind, message, default) = if function == Library::Error {
        (
            ErrorKind::UserError,
            &args[0],
            "$error() function evaluated",
        )
    } else {
        (
            ErrorKind::AssertionFailed,
            &args[1],
            "$assert() statement failed",
        )
    };
    let body = message
        .as_ref()
        .and_then(Value::string_body)
        .filter(|s| !s.is_empty());
    Err(if let Some(body) = body {
        Error::custom(
            kind,
            offset,
            String::from_utf16_lossy(&string::units(body).collect::<Vec<_>>()),
        )
    } else {
        Error::new(kind, offset, default)
    })
}
