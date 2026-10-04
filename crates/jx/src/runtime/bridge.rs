use super::*;
impl<'e, 'i> Scope<'e, 'i> {
    pub(crate) fn dynamic_definitions(&self) -> Vec<Rc<crate::dynamic::Definition>> {
        let mut definitions = Vec::new();
        for frame in self.runtime.frames.borrow().iter() {
            for (_, value) in frame.bindings.entries() {
                crate::dynamic::borrow::collect(value, &mut definitions);
            }
        }
        definitions
    }
    pub(crate) fn loan_dynamic(&self, definitions: &'e [Rc<crate::dynamic::Definition>]) {
        if definitions.is_empty() {
            return;
        }
        let mut frames = self.runtime.frames.borrow_mut();
        for frame in frames.iter_mut() {
            match &mut frame.bindings {
                Bindings::Owned(v) => {
                    for (_, v) in v {
                        *v = crate::dynamic::borrow::value(v, definitions)
                    }
                }
                Bindings::Shared(v)
                    if v.iter().any(|(_, v)| crate::dynamic::borrow::contains(v)) =>
                {
                    for (_, v) in Rc::make_mut(v) {
                        *v = crate::dynamic::borrow::value(v, definitions)
                    }
                }
                Bindings::Shared(_) => {}
                Bindings::Dynamic(v) => {
                    for (_, v) in v {
                        *v = crate::dynamic::borrow::value(v, definitions)
                    }
                }
            }
        }
    }
    // Dynamic code gets a shorter expression lifetime without making the ordinary
    // arena self-referential. Values/JSON leaves remain shared; only scope slots
    // are copied. Unchanged bindings are restored directly on the way out.
    pub(crate) fn bridge<'d>(
        &self,
        value: Value<'d, 'i>,
        wrapped: bool,
        run: impl FnOnce(&Context<'d, 'i>) -> Result<Option<Value<'d, 'i>>, Error>,
    ) -> Result<Option<Value<'e, 'i>>, Error>
    where
        'e: 'd,
    {
        let frames: Vec<Frame<'d, 'i>> = self.runtime.frames.borrow().clone();
        let fork = Scope {
            runtime: Rc::new(Runtime {
                frames: RefCell::new(frames),
                depth: Cell::new(self.runtime.depth.get()),
                tree_depth: Cell::new(self.runtime.tree_depth.get()),
                writes: Cell::new(self.runtime.writes.get()),
                timestamp: Cell::new(self.runtime.timestamp.get()),
                random: RefCell::new(self.runtime.random.borrow().clone()),
                control: self.runtime.control.clone(),
            }),
            frame: self.frame,
        };
        let result = run(&Context {
            value,
            wrapped,
            scope: Some(fork.clone()),
        });
        self.runtime.timestamp.set(fork.runtime.timestamp.get());
        *self.runtime.random.borrow_mut() = fork.runtime.random.borrow().clone();
        self.runtime
            .writes
            .set(self.runtime.writes.get().min(fork.runtime.writes.get()));
        let mut original = self.runtime.frames.borrow_mut();
        let mut retain = crate::dynamic::Retention::default();
        for frame in original.iter() {
            for (_, value) in frame.bindings.entries() {
                retain.seed(value);
            }
        }
        let frames = fork.runtime.frames.take();
        original.truncate(frames.len());
        for (index, frame) in frames.into_iter().enumerate() {
            let unchanged = original.get(index).is_some_and(|old| {
                let mut a = old.bindings.entries();
                let mut b = frame.bindings.entries();
                loop {
                    match (a.next(), b.next()) {
                        (None, None) => return true,
                        (Some((ak, av)), Some((bk, bv)))
                            if ak == bk && crate::dynamic::same(av, bv) => {}
                        _ => return false,
                    }
                }
            });
            if !unchanged && index < original.len() {
                self.runtime
                    .writes
                    .set(self.runtime.writes.get().min(index));
            }
            let bindings = if unchanged {
                std::mem::replace(&mut original[index].bindings, Bindings::Owned(Vec::new()))
            } else {
                Bindings::Dynamic(
                    frame
                        .bindings
                        .entries()
                        .map(|(n, v)| (n.into(), retain.value(v)))
                        .collect(),
                )
            };
            let exported = Frame {
                parent: frame.parent,
                bindings,
                captured: frame.captured,
                vacant: frame.vacant,
            };
            if index < original.len() {
                original[index] = exported;
            } else {
                original.push(exported);
            }
        }
        result.map(|v| v.as_ref().map(|v| retain.value(v)))
    }
}
