use super::RawJson;

pub(crate) const CAPTURE_SLOTS: usize = 32;
#[derive(Default)]
pub(crate) struct Captures<'a> {
    values: [Option<RawJson<'a>>; CAPTURE_SLOTS],
    deferred: u32,
}
impl<'a> Captures<'a> {
    pub(crate) fn get(&self, slot: usize) -> Captured<'a> {
        if self.deferred & (1 << slot) != 0 {
            Captured::Deferred
        } else {
            self.values[slot].map_or(Captured::Missing, Captured::Raw)
        }
    }
    pub(super) fn fill(&mut self, mut mask: u32, value: Captured<'a>) {
        self.deferred &= !mask;
        let raw = match value {
            Captured::Raw(raw) => Some(raw),
            Captured::Missing => None,
            Captured::Deferred => {
                self.deferred |= mask;
                None
            }
        };
        while mask != 0 {
            self.values[mask.trailing_zeros() as usize] = raw;
            mask &= mask - 1;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Captured<'a> {
    Missing,
    Raw(RawJson<'a>),
    // An intermediate array needs JSONata sequence normalization, not field capture.
    Deferred,
}

/// Only expression demands are indexed; input objects never acquire an index.
#[derive(Clone, Debug, Default)]
pub(crate) struct Demand {
    pub(super) slots: u32,
    pub(super) subtree: u32,
    pub(super) children: Vec<(Box<str>, Demand)>,
}
impl Demand {
    pub(crate) fn insert(&mut self, fields: &[Box<str>], slot: usize) {
        assert!(slot < CAPTURE_SLOTS);
        self.subtree |= 1 << slot;
        let Some((name, tail)) = fields.split_first() else {
            self.slots |= 1 << slot;
            return;
        };
        let at = self
            .children
            .iter()
            .position(|(key, _)| key == name)
            .unwrap_or_else(|| {
                self.children.push((name.clone(), Self::default()));
                self.children.len() - 1
            });
        self.children[at].1.insert(tail, slot);
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.subtree == 0
    }
}
