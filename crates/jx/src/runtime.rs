use crate::{Error, ErrorKind, Value, expression::Node, sequence::Context};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

#[derive(Debug)]
struct Frame<'e, 'i> {
    parent: Option<usize>,
    bindings: Vec<(&'e str, Value<'e, 'i>)>,
    captured: bool,
}

// Frames own values; closures contain frame indices, never an owning back-edge.
// The arena lives for one record and is dropped even after errors/cancellation.
#[derive(Debug)]
pub(crate) struct Runtime<'e, 'i> {
    frames: RefCell<Vec<Frame<'e, 'i>>>,
    depth: Cell<usize>,
    tree_depth: Cell<usize>,
}
#[derive(Clone, Debug)]
pub(crate) struct Scope<'e, 'i> {
    runtime: Rc<Runtime<'e, 'i>>,
    pub frame: usize,
}
impl<'e, 'i> Scope<'e, 'i> {
    pub fn new(root: Value<'e, 'i>) -> Self {
        Self {
            runtime: Rc::new(Runtime {
                frames: RefCell::new(vec![Frame {
                    parent: None,
                    bindings: vec![("$", root)],
                    captured: false,
                }]),
                depth: Cell::new(0),
                tree_depth: Cell::new(0),
            }),
            frame: 0,
        }
    }
    pub fn lookup(&self, name: &str) -> Option<Value<'e, 'i>> {
        let frames = self.runtime.frames.borrow();
        let mut at = Some(self.frame);
        while let Some(index) = at {
            let frame = &frames[index];
            if let Some((_, value)) = frame.bindings.iter().rev().find(|(key, _)| *key == name) {
                return Some(value.clone());
            }
            at = frame.parent;
        }
        None
    }
    pub fn bind(&self, name: &'e str, value: Value<'e, 'i>) {
        let mut frames = self.runtime.frames.borrow_mut();
        let bindings = &mut frames[self.frame].bindings;
        if let Some((_, previous)) = bindings.iter_mut().find(|(key, _)| *key == name) {
            *previous = value;
        } else {
            bindings.push((name, value));
        }
    }
    pub fn child(&self, parent: usize) -> Self {
        let mut frames = self.runtime.frames.borrow_mut();
        let frame = frames.len();
        frames.push(Frame {
            parent: Some(parent),
            bindings: Vec::new(),
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
