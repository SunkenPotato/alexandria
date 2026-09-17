//! TokenStream implementations.

use source::SourceIdx;
use span::Spanned;

use crate::{Token, TokenKind};

#[expect(missing_docs)]
pub type Result<T> = std::result::Result<T, StreamError>;

/// Errors that may occur whilst inspecting a stream.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum StreamError {
    /// The EOF has been reached.
    NoToken,
    /// A token was requested which did not match the actual next token.
    KindMismatch(TokenKind),
}

/// A token stream.
#[derive(Debug, Clone)]
pub struct TokenStream {
    /// The source file these tokens stem from.
    pub source_idx: SourceIdx,
    /// The tokens.
    tokens: Vec<Spanned<Token>>,
    /// An index tracking currently consumed tokens.
    index: usize,
}

impl TokenStream {
    /// Create a new tokenstream.
    pub const fn new(source_idx: SourceIdx, tokens: Vec<Spanned<Token>>) -> Self {
        Self {
            source_idx,
            tokens,
            index: 0,
        }
    }

    /// Obtain a reference to the tokens.
    pub const fn tokens(&self) -> &[Spanned<Token>] {
        self.tokens.as_slice()
    }

    /// Consume the next token.
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

    /// Peek the next token.
    pub fn peek(&self) -> Result<Spanned<Token>> {
        self.tokens
            .get(self.index)
            .copied()
            .ok_or(StreamError::NoToken)
    }

    /// Peek the nth token.
    pub fn peek_n(&self, n: usize) -> Result<Spanned<Token>> {
        self.tokens
            .get(self.index + n)
            .copied()
            .ok_or(StreamError::NoToken)
    }

    /// Consume a token of the given kind. Errors if the token kind does not match or no more tokens are left to consume.
    pub fn next_require(&mut self, expect: TokenKind) -> Result<Spanned<Token>> {
        match self.tokens.get(self.index) {
            Some(t) if t.kind == expect => {
                self.index += 1;
                Ok(*t)
            }
            Some(t) => Err(StreamError::KindMismatch(t.kind)),
            None => Err(StreamError::NoToken),
        }
    }

    /// See [`Self::peek`] and [`Self::next_require`].
    pub fn peek_require(&self, expect: TokenKind) -> Result<Spanned<Token>> {
        match self.tokens.get(self.index) {
            Some(t) if t.kind == expect => Ok(*t),
            Some(t) => Err(StreamError::KindMismatch(t.kind)),
            None => Err(StreamError::NoToken),
        }
    }
}
