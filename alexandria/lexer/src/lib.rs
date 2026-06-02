pub mod stream;
#[cfg(test)]
mod tests;

use std::str::Chars;

use diagnostic::{Diagnostic, DiagnosticLevel, Diagnostics};
use span::{
    Span, Spanned,
    source::{SourceIdx, SourceMap},
};

pub use internment::Intern;

use crate::stream::TokenStream;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum TokenKind {
    Bang,
    Caret,
    Ampersand,
    Asterisk,
    LParen,
    RParen,
    Plus,
    Equal,
    Minus,
    Slash,
    LessThan,
    GreaterThan,
    Colon,
    Semicolon,
    Comma,
    Dot,
    Question,
    Tilde,
    LBracket,
    RBracket,
    Pipe,
    Invalid,
    Integer,
    StringLit,
    Ident,
}

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

pub struct Lexer<'s, 'd> {
    source_idx: SourceIdx,
    iter: Chars<'s>,
    source: &'s str,
    cursor: Cursor,
    diagnostics: &'d mut Diagnostics,
}

pub struct LexError;

impl<'s, 'd> Lexer<'s, 'd> {
    pub fn new(
        map: &'s SourceMap,
        source_idx: SourceIdx,
        diagnostics: &'d mut Diagnostics,
    ) -> Self {
        let source = map[source_idx].contents();

        Self {
            iter: source.chars(),
            cursor: Cursor::new(),
            source,
            source_idx,
            diagnostics,
        }
    }

    pub fn lex(mut self) -> Result<TokenStream, LexError> {
        use TokenKind::*;

        let mut tokens = vec![];

        while let Some(next) = self.next() {
            let kind = match next {
                '!' => Bang,
                '^' => Caret,
                '&' => Ampersand,
                '*' => Asterisk,
                '(' => LParen,
                ')' => RParen,
                '+' => Plus,
                '=' => Equal,
                '-' => Minus,
                '/' => Slash,
                '<' => LessThan,
                '>' => GreaterThan,
                ':' => Colon,
                ';' => Semicolon,
                ',' => Comma,
                '.' => Dot,
                '?' => Question,
                '~' => Tilde,
                '[' => LBracket,
                ']' => RBracket,
                '|' => Pipe,
                '0'..='9' => {
                    self.lex_int();
                    Integer
                }
                '"' => {
                    self.lex_str();
                    StringLit
                }
                'a'..='z' | 'A'..='Z' | '_' => {
                    self.lex_ident();
                    Ident
                }
                c if c.is_whitespace() => {
                    self.cursor.commit();
                    continue;
                }
                other => {
                    self.emit(
                        DiagnosticLevel::Error,
                        format!("'{other}' is not a recognized token"),
                        None,
                    );
                    return Err(LexError);
                }
            };

            let token = self.commit(kind);
            tokens.push(token);
        }

        Ok(TokenStream::new(self.source_idx, tokens))
    }

    pub fn emit(
        &mut self,
        level: DiagnosticLevel,
        message: impl Into<String>,
        suggestion: Option<String>,
    ) {
        let span = Span::new(self.cursor.committed as u32, self.cursor.cursor as u32);
        self.diagnostics
            .push(Diagnostic::new(span, level, message.into(), suggestion))
    }

    fn lex_int(&mut self) {
        while let Some('0'..='9' | '_') = self.peek() {
            _ = self.next();
        }
    }

    fn lex_str(&mut self) {
        let mut closed = false;
        while let Some(ch) = self.peek() {
            match ch {
                '"' => {
                    closed = true;
                    _ = self.next();
                    break;
                }
                '\\' => _ = self.next(),
                _ => (),
            }

            _ = self.next();
        }

        if !closed {
            self.emit(
                DiagnosticLevel::Error,
                "unclosed string delimiter",
                Some("add a '\"' at the end of the string".to_owned()),
            );
        }
    }

    fn lex_ident(&mut self) {
        while let Some('a'..='z' | 'A'..='Z' | '0'..='9' | '_') = self.peek() {
            _ = self.next();
        }
    }

    #[expect(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<char> {
        self.iter.next().inspect(|_| self.cursor.next())
    }

    pub fn peek(&self) -> Option<char> {
        self.iter.clone().next()
    }

    pub fn commit(&mut self, kind: TokenKind) -> Spanned<Token> {
        let start = self.cursor.committed;
        let stop = self.cursor.cursor;
        let symbol = &self.source[start..stop];
        let span = Span::new(start as u32, stop as u32);

        self.cursor.commit();
        Spanned::new(span, Token::new(kind, symbol))
    }
}

#[derive(Default)]
pub struct Cursor {
    cursor: usize,
    committed: usize,
}

impl Cursor {
    pub const fn new() -> Self {
        Self {
            cursor: 0,
            committed: 0,
        }
    }

    pub const fn next(&mut self) {
        self.cursor += 1;
    }

    pub const fn commit(&mut self) {
        self.committed = self.cursor;
    }

    pub const fn rollback(&mut self) {
        self.cursor = self.committed;
    }

    pub const fn get(&self) -> usize {
        self.cursor
    }
}
