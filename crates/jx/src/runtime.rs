mod bridge;
mod region;
use crate::{Error, ErrorKind, Value, expression::Node, sequence::Context};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

#[derive(Debug, Clone)]
struct Frame<'e, 'i> {
    parent: Option<usize>,
    bindings: Bindings<'e, 'i>,
    captured: bool,
    vacant: bool,
}

pub(crate) type BindingValues<'e, 'i> = Vec<(&'e str, Value<'e, 'i>)>;
#[derive(Clone, Debug)]
enum Bindings<'e, 'i> {
    Owned(BindingValues<'e, 'i>),
    Shared(Rc<BindingValues<'e, 'i>>),
    Dynamic(Vec<(Box<str>, Value<'e, 'i>)>),
}
impl<'e, 'i> Bindings<'e, 'i> {
    fn lookup(&self, name: &str) -> Option<&Value<'e, 'i>> {
        match self {
            Self::Owned(v) => v.iter().rev().find(|(n, _)| *n == name).map(|(_, v)| v),
            Self::Shared(v) => v.iter().rev().find(|(n, _)| *n == name).map(|(_, v)| v),
            Self::Dynamic(v) => v
                .iter()
                .rev()
                .find(|(n, _)| n.as_ref() == name)
                .map(|(_, v)| v),
        }
    }
    fn bind(&mut self, name: &'e str, value: Value<'e, 'i>) {
        let values = match self {
            Self::Owned(v) => v,
            Self::Shared(v) => Rc::make_mut(v),
            Self::Dynamic(v) => {
                if let Some((_, previous)) = v.iter_mut().find(|(n, _)| n.as_ref() == name) {
                    *previous = value;
                } else {
                    v.push((name.into(), value));
                }
                return;
            }
        };
        if let Some((_, previous)) = values.iter_mut().find(|(n, _)| *n == name) {
            *previous = value;
        } else {
            values.push((name, value));
        }
    }
    fn clear(&mut self) {
        match self {
            Self::Owned(v) => v.clear(),
            Self::Shared(_) => *self = Self::Owned(Vec::new()),
            Self::Dynamic(v) => v.clear(),
        }
    }
    fn entries(&self) -> impl Iterator<Item = (&str, &Value<'e, 'i>)> {
        type Borrowed<'a, 'e, 'i> = &'a [(&'e str, Value<'e, 'i>)];
        type Owned<'a, 'e, 'i> = &'a [(Box<str>, Value<'e, 'i>)];
        let (borrowed, owned): (Borrowed<'_, '_, '_>, Owned<'_, '_, '_>) = match self {
            Self::Owned(v) => (v, &[]),
            Self::Shared(v) => (v, &[]),
            Self::Dynamic(v) => (&[], v),
        };
        borrowed
            .iter()
            .map(|(k, v)| (*k, v))
            .chain(owned.iter().map(|(k, v)| (k.as_ref(), v)))
    }
}

// Frames own values; closures contain frame indices, never an owning back-edge.
// The arena lives for one record and is dropped even after errors/cancellation.
#[derive(Debug)]
pub(crate) struct Runtime<'e, 'i> {
    frames: RefCell<Vec<Frame<'e, 'i>>>,
    depth: Cell<usize>,
    tree_depth: Cell<usize>,
    writes: Cell<usize>,
    timestamp: Cell<i64>,
    random: RefCell<Option<crate::Random>>,
}
#[derive(Clone, Debug)]
pub(crate) struct Scope<'e, 'i> {
    runtime: Rc<Runtime<'e, 'i>>,
    pub frame: usize,
}
impl<'e, 'i> Scope<'e, 'i> {
    pub fn with_random(root: Value<'e, 'i>, clock: bool, random: Option<&crate::Random>) -> Self {
        Self {
            runtime: Rc::new(Runtime {
                frames: RefCell::new(vec![Frame {
                    parent: None,
                    bindings: Bindings::Owned(vec![("$", root)]),
                    captured: false,
                    vacant: false,
                }]),
                depth: Cell::new(0),
                tree_depth: Cell::new(0),
                writes: Cell::new(usize::MAX),
                timestamp: Cell::new(if clock { timestamp() } else { i64::MIN }),
                random: RefCell::new(random.cloned()),
            }),
            frame: 0,
        }
    }
    pub fn timestamp(&self) -> i64 {
        let mut value = self.runtime.timestamp.get();
        if value == i64::MIN {
            value = timestamp();
            self.runtime.timestamp.set(value);
        }
        value
    }
    pub fn random(&self) -> crate::Random {
        self.runtime
            .random
            .borrow_mut()
            .get_or_insert_with(crate::Random::default)
            .clone()
    }
    pub fn lookup(&self, name: &str) -> Option<Value<'e, 'i>> {
        let frames = self.runtime.frames.borrow();
        let mut at = Some(self.frame);
        while let Some(index) = at {
            let frame = &frames[index];
            if let Some(value) = frame.bindings.lookup(name) {
                return Some(value.clone());
            }
            at = frame.parent;
        }
        None
    }
    pub fn bind(&self, name: &'e str, value: Value<'e, 'i>) {
        let mut frames = self.runtime.frames.borrow_mut();
        frames[self.frame].bindings.bind(name, value);
        self.runtime
            .writes
            .set(self.runtime.writes.get().min(self.frame));
    }
    pub fn at(&self, frame: usize) -> Self {
        Self {
            runtime: self.runtime.clone(),
            frame,
        }
    }
    pub fn child(&self, parent: usize) -> Self {
        self.frame(parent, Bindings::Owned(Vec::new()))
    }
    pub fn shared(&self, values: Rc<BindingValues<'e, 'i>>) -> Self {
        self.frame(self.frame, Bindings::Shared(values))
    }
    fn frame(&self, parent: usize, bindings: Bindings<'e, 'i>) -> Self {
        let mut frames = self.runtime.frames.borrow_mut();
        let frame = if frames.last().is_some_and(|f| f.vacant) {
            let last = frames.last_mut().unwrap();
            if !matches!((&last.bindings, &bindings), (Bindings::Owned(_), Bindings::Owned(v)) if v.is_empty())
            {
                last.bindings = bindings;
            }
            last.parent = Some(parent);
            last.vacant = false;
            frames.len() - 1
        } else {
            let index = frames.len();
            frames.push(Frame {
                parent: Some(parent),
                bindings,
                captured: false,
                vacant: false,
            });
            index
        };
        Self {
            runtime: self.runtime.clone(),
            frame,
        }
    }
    pub fn capture(&self) -> usize {
        let mut frames = self.runtime.frames.borrow_mut();
        let mut at = Some(self.frame);
        while let Some(index) = at {
            if frames[index].captured {
                break;
            }
            frames[index].captured = true;
            at = frames[index].parent;
        }
        self.frame
    }
    pub fn reset(&self, parent: usize) -> bool {
        let mut frames = self.runtime.frames.borrow_mut();
        if frames[self.frame].captured {
            return false;
        }
        if self.frame + 1 != frames.len()
            && !(self.frame + 2 == frames.len() && frames.last().is_some_and(|f| f.vacant))
        {
            return false;
        }
        let frame = &mut frames[self.frame];
        frame.parent = Some(parent);
        frame.bindings.clear();
        true
    }
    pub fn bind_arguments(&self, params: &'e [Box<str>], arguments: &[Option<Value<'e, 'i>>]) {
        let mut frames = self.runtime.frames.borrow_mut();
        let bindings = &mut frames[self.frame].bindings;
        self.runtime
            .writes
            .set(self.runtime.writes.get().min(self.frame));
        for (index, param) in params.iter().enumerate() {
            bindings.bind(
                param,
                arguments
                    .get(index)
                    .cloned()
                    .flatten()
                    .unwrap_or(Value::Undefined),
            );
        }
    }
    // Released scopes must no longer be used. Individual releases keep captures;
    // retained regions can later retire them when their values cannot escape.
    // Keep at most one vacant terminal slot, including its parameter capacity.
    pub fn release(&self) {
        let mut frames = self.runtime.frames.borrow_mut();
        if frames[self.frame].captured {
            return;
        }
        trim_vacant(&mut frames, self.frame);
        if self.frame + 1 == frames.len() {
            let frame = &mut frames[self.frame];
            frame.bindings.clear();
            frame.parent = None;
            frame.vacant = true;
        }
    }
    pub fn call<T>(
        &self,
        offset: usize,
        body_depth: usize,
        body: impl FnOnce() -> Result<T, Error>,
    ) -> Result<T, Error> {
        let depth = self.runtime.depth.get();
        let tree_depth = self.runtime.tree_depth.get();
        if depth >= 64 || tree_depth + body_depth > 512 {
            return Err(Error::new(
                ErrorKind::DepthLimit,
                offset,
                "function stack exceeds 64 calls or 512 expression levels",
            ));
        }
        self.runtime.depth.set(depth + 1);
        self.runtime.tree_depth.set(tree_depth + body_depth);
        let result = body();
        self.runtime.depth.set(depth);
        self.runtime.tree_depth.set(tree_depth);
        result
    }
}

fn trim_vacant(frames: &mut Vec<Frame<'_, '_>>, parent: usize) {
    if frames.len() == parent + 2 && frames.last().is_some_and(|f| f.vacant) {
        frames.pop();
    }
}

pub(crate) fn block<'e, 'i>(
    nodes: &'e [Node],
    context: &Context<'e, 'i>,
) -> Result<Option<Value<'e, 'i>>, Error> {
    let Some(scope) = &context.scope else {
        let mut result = None;
        for node in nodes {
            result = crate::retain::materialize(node, context)?;
        }
        return Ok(result);
    };
    scope.retained(|| {
        let child = scope.child(scope.frame);
        let context = Context {
            scope: Some(child.clone()),
            ..context.clone()
        };
        let result = (|| {
            let mut result = None;
            for node in nodes {
                result = crate::retain::materialize(node, &context)?;
            }
            Ok(result)
        })();
        child.release();
        result
    })
}

fn timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or_else(
            |e| -(e.duration().as_millis() as i64),
            |d| d.as_millis() as i64,
        )
}

#[cfg(test)]
mod tests;
