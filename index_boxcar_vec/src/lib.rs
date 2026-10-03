//! Implementation of a concurrent index Vec.

use std::marker::PhantomData;

use index_vec::Idx;

/// A concurrent [`IndexVec`].
#[derive(Debug, Clone)]
pub struct IndexBoxcarVec<I: Idx, T> {
    raw: boxcar::Vec<T>,
    _marker: PhantomData<fn(I)>,
}

impl<I: Idx, T> Default for IndexBoxcarVec<I, T> {
    fn default() -> Self {
        Self {
            raw: boxcar::Vec::default(),
            _marker: PhantomData,
        }
    }
}

impl<I: Idx, T> IndexBoxcarVec<I, T> {
    /// Create a new `IndexBoxcarVec<I, T>`.
    pub fn new() -> Self {
        Self {
            raw: boxcar::Vec::new(),
            _marker: PhantomData,
        }
    }

    /// Pushes a value, returning its stable, typed index.
    pub fn push(&self, value: T) -> I {
        let idx = self.raw.push(value);
        I::from_usize(idx)
    }

    /// Get the item at the given index.
    pub fn get(&self, idx: I) -> Option<&T> {
        self.raw.get(idx.index())
    }

    /// The length of this vector.
    pub fn len(&self) -> usize {
        self.raw.count()
    }

    /// Check whether the vector is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns an iterator over both the index and the item.
    pub fn iter(&self) -> impl Iterator<Item = (I, &T)> {
        self.raw.iter().map(|(i, v)| (I::from_usize(i), v))
    }
}

impl<I: Idx, T> std::ops::Index<I> for IndexBoxcarVec<I, T> {
    type Output = T;
    fn index(&self, idx: I) -> &T {
        &self.raw[idx.index()]
    }
}
