use crate::{Error, ErrorKind, Value, expression::Node, sequence::Context};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

#[derive(Debug)]
struct Frame<'e, 'i> {
    parent: Option<usize>,
    bindings: Bindings<'e, 'i>,
    captured: bool,
}

pub(crate) type BindingValues<'e, 'i> = Vec<(&'e str, Value<'e, 'i>)>;
#[derive(Debug)]
enum Bindings<'e, 'i> {
    Owned(BindingValues<'e, 'i>),
    Shared(Rc<BindingValues<'e, 'i>>),
}
impl<'e, 'i> Bindings<'e, 'i> {
    fn values(&self) -> &BindingValues<'e, 'i> {
        match self {
            Self::Owned(values) => values,
            Self::Shared(values) => values,
        }
    }
    fn mutable(&mut self) -> &mut BindingValues<'e, 'i> {
        match self {
            Self::Owned(values) => values,
            Self::Shared(values) => Rc::make_mut(values),
        }
    }
}

// Frames own values; closures contain frame indices, never an owning back-edge.
// The arena lives for one record and is dropped even after errors/cancellation.
#[derive(Debug)]
pub(crate) struct Runtime<'e, 'i> {
    frames: RefCell<Vec<Frame<'e, 'i>>>,
    depth: Cell<usize>,
    tree_depth: Cell<usize>,
    timestamp: Option<i64>,
}
#[derive(Clone, Debug)]
pub(crate) struct Scope<'e, 'i> {
    runtime: Rc<Runtime<'e, 'i>>,
    pub frame: usize,
}
impl<'e, 'i> Scope<'e, 'i> {
    pub fn new(root: Value<'e, 'i>, clock: bool) -> Self {
        Self {
            runtime: Rc::new(Runtime {
                frames: RefCell::new(vec![Frame {
                    parent: None,
                    bindings: Bindings::Owned(vec![("$", root)]),
                    captured: false,
                }]),
                depth: Cell::new(0),
                tree_depth: Cell::new(0),
                timestamp: clock.then(timestamp),
            }),
            frame: 0,
        }
    }
    pub fn timestamp(&self) -> i64 {
        self.runtime.timestamp.expect("clock analysis")
    }
    pub fn lookup(&self, name: &str) -> Option<Value<'e, 'i>> {
        let frames = self.runtime.frames.borrow();
        let mut at = Some(self.frame);
        while let Some(index) = at {
            let frame = &frames[index];
            if let Some((_, value)) = frame
                .bindings
                .values()
                .iter()
                .rev()
                .find(|(key, _)| *key == name)
            {
                return Some(value.clone());
            }
            at = frame.parent;
        }
        None
    }
    pub fn bind(&self, name: &'e str, value: Value<'e, 'i>) {
        let mut frames = self.runtime.frames.borrow_mut();
        let bindings = frames[self.frame].bindings.mutable();
        if let Some((_, previous)) = bindings.iter_mut().find(|(key, _)| *key == name) {
            *previous = value;
        } else {
            bindings.push((name, value));
        }
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
        let frame = frames.len();
        frames.push(Frame {
            parent: Some(parent),
            bindings,
            captured: false,
        });
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
    pub fn release(&self) {
        let mut frames = self.runtime.frames.borrow_mut();
        if self.frame + 1 == frames.len() && !frames[self.frame].captured {
            frames.pop();
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
}

fn timestamp() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or_else(
            |e| -(e.duration().as_millis() as i64),
            |d| d.as_millis() as i64,
        )
}
