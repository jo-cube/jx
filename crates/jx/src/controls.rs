use crate::{Error, ErrorKind, Value};
use std::{
    cell::Cell,
    io::{self, Write},
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

/// Cooperative controls, not a process memory quota. Existing parser/JSON depth
/// and hard stack ceilings remain in force; limits can only tighten those ceilings.
#[derive(Clone, Debug)]
pub struct Limits {
    pub max_work: usize,
    pub max_items: usize,
    pub max_results: usize,
    pub max_output_bytes: usize,
    pub max_calls: usize,
    pub max_tail_calls: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            max_work: 1_000_000,
            max_items: 1_000_000,
            max_results: 1_000_000,
            max_output_bytes: 16 * 1024 * 1024,
            max_calls: 64,
            max_tail_calls: 1_000_000,
        }
    }
}
/// Clone into another thread to request cancellation. A token stays cancelled.
#[derive(Clone, Debug, Default)]
pub struct Cancellation(Arc<AtomicBool>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}
#[derive(Debug)]
pub(crate) struct State {
    limits: Option<Limits>,
    cancellation: Option<Cancellation>,
    deadline: Option<Instant>,
    work: Cell<usize>,
    items: Cell<usize>,
    results: Cell<usize>,
    bytes: Cell<usize>,
}
#[derive(Clone, Debug)]
pub(crate) struct Control(Rc<State>);
impl Control {
    pub fn new(
        limits: Option<Limits>,
        cancellation: Option<Cancellation>,
        deadline: Option<Instant>,
    ) -> Option<Self> {
        if limits.is_none() && cancellation.is_none() && deadline.is_none() {
            return None;
        }
        Some(Self(Rc::new(State {
            limits,
            cancellation,
            deadline,
            work: Cell::new(0),
            items: Cell::new(0),
            results: Cell::new(0),
            bytes: Cell::new(0),
        })))
    }
    pub fn checkpoint(&self, offset: usize) -> Result<(), Error> {
        if self
            .0
            .cancellation
            .as_ref()
            .is_some_and(Cancellation::is_cancelled)
        {
            return Err(Error::new(
                ErrorKind::Cancelled,
                offset,
                "evaluation cancelled",
            ));
        }
        if self.0.deadline.is_some_and(|d| Instant::now() >= d) {
            return Err(limit(offset, "evaluation deadline exceeded"));
        }
        if let Some(limits) = &self.0.limits {
            charge(
                &self.0.work,
                1,
                limits.max_work,
                offset,
                "evaluation work limit exceeded",
            )?;
        }
        Ok(())
    }
    pub fn item(&self, offset: usize) -> Result<(), Error> {
        self.checkpoint(offset)?;
        if let Some(limits) = &self.0.limits {
            charge(
                &self.0.items,
                1,
                limits.max_items,
                offset,
                "intermediate item limit exceeded",
            )?;
        }
        Ok(())
    }
    pub fn inspect(&self, value: &Value<'_, '_>, offset: usize) -> Result<(), Error> {
        self.inspect_at(value, offset, 0)
    }
    fn inspect_at(&self, value: &Value<'_, '_>, offset: usize, depth: usize) -> Result<(), Error> {
        self.item(offset)?;
        if depth > 512 {
            return Err(limit(offset, "value nesting limit exceeded"));
        }
        if value.is_array() {
            for item in value.elements() {
                self.inspect_at(&item, offset, depth + 1)?;
            }
        } else if value.is_object() {
            for (_, item) in value.members() {
                self.inspect_at(&item, offset, depth + 1)?;
            }
        }
        Ok(())
    }
    pub fn max_calls(&self) -> usize {
        self.0.limits.as_ref().map_or(64, |l| l.max_calls.min(64))
    }
    pub fn max_tail_calls(&self) -> usize {
        self.0
            .limits
            .as_ref()
            .map_or(1_000_000, |l| l.max_tail_calls.min(1_000_000))
    }
    pub fn result(&self, value: &Value<'_, '_>) -> Result<(), Error> {
        self.checkpoint(0)?;
        if let Some(limits) = &self.0.limits {
            charge(
                &self.0.results,
                1,
                limits.max_results,
                0,
                "result count limit exceeded",
            )?;
            let mut writer = Count {
                bytes: 0,
                limit: limits.max_output_bytes.saturating_sub(self.0.bytes.get()),
            };
            if let Err(error) = value.write_compact(&mut writer) {
                // Byte accounting does not make JSON encoding a requirement for
                // library results. Functions remain inspectable by the consumer.
                if error.kind() == io::ErrorKind::InvalidInput {
                    return Ok(());
                }
                let mut error = if error.kind() == io::ErrorKind::FileTooLarge {
                    limit(0, "output byte limit exceeded")
                } else {
                    Error::custom(ErrorKind::TypeError, 0, error.to_string())
                };
                error.phase = crate::Phase::Serialization;
                error.source = crate::Source::Result;
                return Err(error);
            }
            self.0.bytes.set(self.0.bytes.get() + writer.bytes);
        }
        Ok(())
    }
}
fn charge(
    counter: &Cell<usize>,
    n: usize,
    max: usize,
    offset: usize,
    message: &'static str,
) -> Result<(), Error> {
    if n > max.saturating_sub(counter.get()) {
        return Err(limit(offset, message));
    }
    counter.set(counter.get() + n);
    Ok(())
}
fn limit(offset: usize, message: &'static str) -> Error {
    Error::new(ErrorKind::EvaluationLimit, offset, message)
}
struct Count {
    bytes: usize,
    limit: usize,
}
impl Write for Count {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if data.len() > self.limit.saturating_sub(self.bytes) {
            return Err(io::ErrorKind::FileTooLarge.into());
        }
        self.bytes += data.len();
        Ok(data.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
