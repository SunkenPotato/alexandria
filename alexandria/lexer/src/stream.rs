use span::{Spanned, source::SourceIdx};

use crate::{Token, TokenKind};

pub type Result<T> = std::result::Result<T, StreamError>;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum StreamError {
    NoToken,
    KindMismatch(TokenKind),
}

pub struct TokenStream {
    source_idx: SourceIdx,
    tokens: Vec<Spanned<Token>>,
    index: usize,
}

impl TokenStream {
    pub const fn new(source_idx: SourceIdx, tokens: Vec<Spanned<Token>>) -> Self {
        Self {
            source_idx,
            tokens,
            index: 0,
        }
    }

    #[expect(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Spanned<Token>> {
        match self.tokens.get(self.index) {
            Some(v) => {
                self.index += 1;
                Ok(*v)
            }
            None => Err(StreamError::NoToken),
        }
    }

    pub fn peek(&self) -> Result<Spanned<Token>> {
        self.tokens
            .get(self.index)
            .copied()
            .ok_or(StreamError::NoToken)
    }

    pub fn peek_n(&self, n: usize) -> Result<Spanned<Token>> {
        self.tokens
            .get(self.index + n)
            .copied()
            .ok_or(StreamError::NoToken)
    }

    pub fn next_require(&mut self, expect: TokenKind) -> Result<Spanned<Token>> {
        match self.tokens.get(self.index) {
            Some(t) if t.item.kind == expect => {
                self.index += 1;
                Ok(*t)
            }
            Some(t) => Err(StreamError::KindMismatch(t.item.kind)),
            None => Err(StreamError::NoToken),
        }
    }

    pub fn peek_require(&self, expect: TokenKind) -> Result<Spanned<Token>> {
        match self.tokens.get(self.index) {
            Some(t) if t.item.kind == expect => Ok(*t),
            Some(t) => Err(StreamError::KindMismatch(t.item.kind)),
            None => Err(StreamError::NoToken),
        }
    }

    pub fn with<'stream, F, T, E>(&'stream mut self, f: F) -> std::result::Result<T, E>
    where
        F: for<'guard, 'index> FnOnce(
            &'guard mut Guard<'stream, 'index>,
        ) -> std::result::Result<T, E>,
    {
        let mut index = self.index;
        let mut guard = Guard {
            tokens: &self.tokens,
            index: &mut index,
        };

        let result = f(&mut guard);

        if result.is_ok() {
            self.index = index;
        }

        result
    }
}

pub struct Guard<'stream, 'index> {
    tokens: &'stream [Spanned<Token>],
    index: &'index mut usize,
}

impl<'stream, 'index> Guard<'stream, 'index> {
    #[expect(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Spanned<Token>> {
        match self.tokens.get(*self.index) {
            Some(v) => {
                *self.index += 1;
                Ok(*v)
            }
            None => Err(StreamError::NoToken),
        }
    }

    pub fn peek(&self) -> Result<Spanned<Token>> {
        self.tokens
            .get(*self.index)
            .copied()
            .ok_or(StreamError::NoToken)
    }

    pub fn peek_n(&self, n: usize) -> Result<Spanned<Token>> {
        self.tokens
            .get(*self.index + n)
            .copied()
            .ok_or(StreamError::NoToken)
    }

    pub fn next_require(&mut self, expect: TokenKind) -> Result<Spanned<Token>> {
        match self.tokens.get(*self.index) {
            Some(t) if t.item.kind == expect => {
                *self.index += 1;
                Ok(*t)
            }
            Some(t) => Err(StreamError::KindMismatch(t.item.kind)),
            None => Err(StreamError::NoToken),
        }
    }

    pub fn peek_require(&self, expect: TokenKind) -> Result<Spanned<Token>> {
        match self.tokens.get(*self.index) {
            Some(t) if t.item.kind == expect => Ok(*t),
            Some(t) => Err(StreamError::KindMismatch(t.item.kind)),
            None => Err(StreamError::NoToken),
        }
    }

    pub fn with<F, T, E>(&'stream mut self, f: F) -> std::result::Result<T, E>
    where
        F: for<'guard, 'index2> FnOnce(
            &'guard mut Guard<'stream, 'index2>,
        ) -> std::result::Result<T, E>,
    {
        let mut index = *self.index;
        let mut guard = Guard {
            tokens: self.tokens,
            index: &mut index,
        };

        let result = f(&mut guard);

        if result.is_ok() {
            *self.index = index;
        }

        result
    }
}
