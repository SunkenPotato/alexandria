pub mod stream;

use span::source::{SourceIdx, SourceMap};

pub use internment::Intern;

use crate::stream::TokenStream;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum TokenKind {}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Token {
    pub kind: TokenKind,
    pub symbol: Intern<str>,
}

impl Token {
    pub fn new<T>(kind: TokenKind, symbol: T) -> Self
    where
        Intern<str>: From<T>,
    {
        Self {
            kind,
            symbol: Intern::from(symbol),
        }
    }
}

pub struct Lexer<'s> {
    pub source_idx: SourceIdx,
    pub source_map: &'s SourceMap,
}

impl<'s> Lexer<'s> {
    pub const fn new(source_idx: SourceIdx, source_map: &'s SourceMap) -> Self {
        Self {
            source_idx,
            source_map,
        }
    }

    pub fn lex(self) -> TokenStream {
        todo!()
    }
}
