//! Utilities for representing source regions in code.
//!
//! For actually representing the source itself, see the `source` crate.

#![feature(type_changing_struct_update)]

use derive_more::{Deref, DerefMut};

const _ASSERT_USIZE_GREATER_THAN_U32: () = assert!(
    u32::BITS <= usize::BITS,
    "The compiler needs at least 32-bit wide integers."
);

/// A source region.
///
/// A [`Span`] is a reference to a region of source code. It does not actually contain the source code, but is rather a pointer to it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Span {
    start: u32,
    stop: u32,
}

impl Span {
    /// Create a new a span.
    pub fn new(start: u32, stop: u32) -> Self {
        debug_assert!(stop >= start);

        Self { start, stop }
    }

    /// The start of the span.
    #[inline]
    pub const fn start(&self) -> u32 {
        self.start
    }

    /// The end of the span.
    #[inline]
    pub const fn stop(&self) -> u32 {
        self.stop
    }

    /// Extend this span by setting the end to the given position.
    pub const fn extend(mut self, other: Self) -> Self {
        debug_assert!(other.stop >= self.stop);
        self.stop = other.stop;
        self
    }
}

/// A wrapper around a span and an item.
///
/// This dereferences to `T`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash, Deref, DerefMut)]
pub struct Spanned<T> {
    #[deref]
    #[deref_mut]
    pub item: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub fn new(span: Span, item: T) -> Self {
        Self { item, span }
    }

    pub const fn extend(mut self, span: Span) -> Self {
        self.span = self.span.extend(span);
        self
    }

    pub fn map<F, U>(self, f: F) -> Spanned<U>
    where
        F: FnOnce(T) -> U,
    {
        Spanned {
            item: f(self.item),
            ..self
        }
    }

    pub fn as_ref(&self) -> Spanned<&T> {
        Spanned {
            item: &self.item,
            ..*self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assert_span_size() {
        assert_eq!(size_of::<Span>(), 2 * size_of::<u32>());
    }
}
