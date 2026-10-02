use super::*;
use crate::function::{FunctionKind, composition::Argument};

impl<'e, 'i> Scope<'e, 'i> {
    // Calls/blocks retain their output before leaving. New frames have no outside
    // roots unless returned values reach them or a write enters an older frame.
    pub(crate) fn retained(
        &self,
        run: impl FnOnce() -> Result<Option<Value<'e, 'i>>, Error>,
    ) -> Result<Option<Value<'e, 'i>>, Error> {
        self.region(run, |value, start| {
            value.as_ref().is_some_and(|v| reaches(v, start))
        })
    }
    pub(crate) fn evaluated(
        &self,
        run: impl FnOnce() -> Result<crate::evaluate::Operand<'e, 'i>, Error>,
    ) -> Result<crate::evaluate::Operand<'e, 'i>, Error> {
        self.region(run, |value, start| match value {
            crate::evaluate::Operand::Missing => false,
            crate::evaluate::Operand::One(value) => reaches(value, start),
            crate::evaluate::Operand::Many(_) => true,
        })
    }
    fn region<T>(
        &self,
        run: impl FnOnce() -> Result<T, Error>,
        escapes: impl FnOnce(&T, usize) -> bool,
    ) -> Result<T, Error> {
        let start = {
            let frames = self.runtime.frames.borrow();
            frames.len() - usize::from(frames.last().is_some_and(|f| f.vacant))
        };
        let previous = self.runtime.writes.replace(usize::MAX);
        let result = run();
        let writes = self.runtime.writes.replace(previous);
        self.runtime.writes.set(previous.min(writes));
        let mut frames = self.runtime.frames.borrow_mut();
        if writes < start
            || start >= frames.len()
            || (start + 1 == frames.len() && frames[start].vacant)
            || result.as_ref().is_ok_and(|v| escapes(v, start))
        {
            return result;
        }
        frames.truncate(start + 1);
        let frame = &mut frames[start];
        frame.bindings.clear();
        frame.parent = None;
        frame.captured = false;
        frame.vacant = true;
        result
    }
}
fn reaches(value: &Value<'_, '_>, start: usize) -> bool {
    match value {
        Value::Function(f) => function(f, start),
        Value::Array(a) => a.items.iter().any(|v| reaches(v, start)),
        Value::Object(o) => o
            .members
            .iter()
            .any(|(k, v)| reaches(k, start) || reaches(v, start)),
        Value::Copied(c) => reaches(&c.source, start),
        _ => false, // Raw/compiled JSON contains no lexical references.
    }
}
fn function(f: &crate::Function<'_, '_>, start: usize) -> bool {
    match &f.kind {
        FunctionKind::Lambda { frame, focus, .. } => *frame >= start || reaches(focus, start),
        FunctionKind::Transform { frame, .. } => *frame >= start,
        FunctionKind::Dynamic(c) => c.frame >= start || reaches(&c.focus, start),
        FunctionKind::Partial { target, arguments } => {
            function(target, start)
                || arguments
                    .iter()
                    .any(|a| matches!(a,Argument::Value(Some(v)) if reaches(v,start)))
        }
        FunctionKind::Chain(a, b) => function(a, start) || function(b, start),
        FunctionKind::MatchNext(next) => reaches(next.value(), start),
        FunctionKind::Builtin(_) | FunctionKind::Matcher(_) => false,
    }
}
