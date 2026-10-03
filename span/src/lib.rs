//! Utilities for representing source regions in code.
//!
//! For actually representing the source itself, see the `source` crate.

use derive_more::{Deref, DerefMut};

const _ASSERT_USIZE_GREATER_THAN_U32: () = assert!(
    u32::BITS <= usize::BITS,
    "The compiler needs at least 32-bit wide integers."
);

/// A source region.
///
/// A [`Span`] is a reference to a region of source code, expressed in byte offsets.
/// It does not actually contain the source code, but is rather a pointer to it.
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
    /// The wrapped item.
    #[deref]
    #[deref_mut]
    pub item: T,
    /// The region of source the item was taken from.
    pub span: Span,
}

impl<T> Spanned<T> {
    /// Wrap an item with a span.
    pub fn new(span: Span, item: T) -> Self {
        Self { item, span }
    }

    /// Extend the span of this item. See [`Span::extend`].
    pub const fn extend(mut self, span: Span) -> Self {
        self.span = self.span.extend(span);
        self
    }

    /// Map the inner item, keeping the span.
    pub fn map<F, U>(self, f: F) -> Spanned<U>
    where
        F: FnOnce(T) -> U,
    {
        Spanned {
            item: f(self.item),
            span: self.span,
        }
    }

    /// Convert a `&Spanned<T>` into a `Spanned<&T>`.
    pub fn as_ref(&self) -> Spanned<&T> {
        Spanned {
            item: &self.item,
            span: self.span,
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
