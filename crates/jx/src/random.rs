use std::{
    cell::Cell,
    hash::{BuildHasher, Hasher},
    rc::Rc,
};

/// Shared pseudorandom stream for independent evaluations. Initialization is lazy;
/// a seeded stream is reproducible, but is not a cryptographic random source.
#[derive(Clone, Debug, Default)]
pub struct Random(Rc<Cell<Option<u64>>>);
impl Random {
    pub fn seeded(seed: u64) -> Self {
        Self(Rc::new(Cell::new(Some(seed))))
    }
    pub(crate) fn draw(&self) -> f64 {
        let state = self.0.get().unwrap_or_else(|| {
            std::collections::hash_map::RandomState::new()
                .build_hasher()
                .finish()
        });
        // SplitMix64: one state word, with 53 random bits for binary64's [0, 1).
        let state = state.wrapping_add(0x9e3779b97f4a7c15);
        self.0.set(Some(state));
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        ((z ^ (z >> 31)) >> 11) as f64 * (1.0 / 9007199254740992.0)
    }
}
