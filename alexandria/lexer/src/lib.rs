use std::ops::{Deref, DerefMut};

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

pub struct Spanned<T> {
    pub t: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub const fn new(span: Span, t: T) -> Self {
        Self { t, span }
    }

    pub fn map<F, U>(self, f: F) -> Spanned<U>
    where
        F: FnOnce(T) -> U,
    {
        Spanned {
            t: f(self.t),
            span: self.span,
        }
    }
}

impl<T> Deref for Spanned<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.t
    }
}

impl<T> DerefMut for Spanned<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.t
    }
}
