//! Utilities for representing spans and spanned items.
//!
//! See [`Span`] for more information.
#![feature(type_changing_struct_update)]
#![feature(seek_stream_len)]

pub mod source;

/// A source region.
///
/// A [`Span`] is a reference to a region of source code. It does not actually contain the source code, but is rather a pointer to it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Span {
    start: u32,
    stop: u32,
}

impl Span {
    pub const fn new(start: u32, stop: u32) -> Self {
        debug_assert!(stop >= start);

        Self { start, stop }
    }

    pub const fn start(&self) -> u32 {
        self.start
    }

    pub const fn stop(&self) -> u32 {
        self.stop
    }

    pub const fn extend(mut self, other: Self) -> Self {
        debug_assert!(other.stop >= self.stop);
        self.stop = other.stop;
        self
    }
}

/// A spanned item.
pub struct Spanned<T> {
    pub item: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub const fn new(span: Span, item: T) -> Self {
        Self { span, item }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assert_span_size() {
        assert_eq!(size_of::<Span>(), 2 * size_of::<u32>());
    }
}
