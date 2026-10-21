use super::run::Cell;
use std::ops::{Deref, DerefMut};

// Inline storage changes only allocation, never eligibility or semantics. Larger
// activations grow normally. One frame is reused across all rows of an operator.
pub(super) struct Frame<'e, 'i> {
    inline: [Cell<'e, 'i>; 16],
    heap: Vec<Cell<'e, 'i>>,
    len: usize,
    repeated: bool,
    region: Option<Box<Self>>,
}
impl<'e, 'i> Frame<'e, 'i> {
    pub fn new() -> Self {
        Self {
            inline: [const { Cell::Missing }; 16],
            heap: Vec::new(),
            len: 0,
            repeated: false,
            region: None,
        }
    }
    pub fn repeated() -> Self {
        Self {
            repeated: true,
            ..Self::new()
        }
    }
    // A region executes serially inside this activation. Repeated operators own
    // its register workspace; one-shot activations keep an ordinary stack frame.
    // Clear every value before another iteration, retaining capacities only.
    pub fn region<T>(
        &mut self,
        workspace: usize,
        run: impl FnOnce(&[Cell<'e, 'i>], &mut Self) -> T,
    ) -> T {
        if !self.repeated || workspace <= self.inline.len() {
            return run(self, &mut Self::new());
        }
        let Self {
            inline,
            heap,
            len,
            region,
            ..
        } = self;
        let parent = if *len <= inline.len() {
            &inline[..*len]
        } else {
            &heap[..]
        };
        let child = region.get_or_insert_with(|| Box::new(Self::repeated()));
        let result = run(parent, child);
        child.reset(0);
        result
    }
    // Complete execution transfers its result. Scratch values follow the usual
    // reset/drop path; stages that still need their result read the frame directly.
    pub fn take_result(&mut self, result: usize) -> Cell<'e, 'i> {
        std::mem::replace(&mut self[result], Cell::Missing)
    }
    pub fn reset(&mut self, len: usize) {
        self.deref_mut().fill(Cell::Missing);
        self.len = len;
        if len > self.inline.len() {
            self.heap.resize(len, Cell::Missing);
        }
    }
}
impl<'e, 'i> Deref for Frame<'e, 'i> {
    type Target = [Cell<'e, 'i>];
    fn deref(&self) -> &Self::Target {
        if self.len <= self.inline.len() {
            &self.inline[..self.len]
        } else {
            &self.heap
        }
    }
}
impl DerefMut for Frame<'_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        if self.len <= self.inline.len() {
            &mut self.inline[..self.len]
        } else {
            &mut self.heap
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_region_workspace_releases_values_between_iterations() {
        let value = crate::Value::array(vec![crate::Value::Number(1.0)], false);
        let crate::Value::Array(array) = &value else {
            unreachable!()
        };
        let weak = std::rc::Rc::downgrade(array);
        let mut frame = Frame::repeated();
        frame.region(32, |_, child| {
            child.reset(32);
            child[0] = Cell::Value(value);
        });
        assert!(
            weak.upgrade().is_none(),
            "iteration retained a result graph"
        );
        frame.region(32, |_, child| {
            child.reset(32);
            assert!(child.iter().all(|c| matches!(c, Cell::Missing)));
        });
    }
}
