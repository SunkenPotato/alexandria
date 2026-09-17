//! The lexer (tokenizer) for the alexandria language.

pub mod stream;
#[cfg(test)]
mod tests;

use std::str::Chars;

use diagnostic::{Diagnostic, DiagnosticLevel, Diagnostics};
use source::{SourceIdx, SourceMap};
use span::{Span, Spanned};

pub use internment::Intern;

use crate::stream::TokenStream;

/// An exhaustive list of token kinds.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum TokenKind {
    /// `!`.
    Bang,
    /// `^`.
    Caret,
    /// `&`.
    Ampersand,
    /// `*`.
    Asterisk,
    /// `(`
    LParen,
    /// `)`.
    RParen,
    /// `+`.
    Plus,
    /// `=`.
    Equal,
    /// `-`.
    Minus,
    /// `/`.
    Slash,
    /// `<`
    LessThan,
    /// `>`.
    GreaterThan,
    /// `:`.
    Colon,
    /// `;`.
    Semicolon,
    /// `,`.
    Comma,
    /// `.`.
    Dot,
    /// `?`.
    Question,
    /// `~`.
    Tilde,
    /// `%`.
    Percent,
    /// `[`.
    LBracket,
    /// `]`.
    RBracket,
    /// `{`.
    LCurly,
    /// `}`.
    RCurly,
    /// `|`.
    Pipe,
    /// An invalid token kind. Used internally only.
    Invalid,
    /// An integer. This excludes any sign.
    Integer,
    /// A string literal.
    StringLit,
    /// An identifier.
    Ident,
}

impl TokenKind {
    /// Check whether this is an atom, i.e., a constant token.
    pub const fn is_atom(&self) -> bool {
        !matches!(
            self,
            TokenKind::Integer | TokenKind::StringLit | TokenKind::Ident
        )
    }
}

/// A token. Contains the token
// TODO: investigate whether this is compressible with pointer tagging
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Token {
    /// The kind of token.
    pub kind: TokenKind,
    /// The source of this token.
    pub symbol: Intern<str>,
}

impl Token {
    /// Create a new token.
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

/// The lexer machinery.
pub struct Lexer<'s, 'd> {
    source_idx: SourceIdx,
    iter: Chars<'s>,
    source: &'s str,
    cursor: Cursor,
    diagnostics: &'d mut Diagnostics,
}

/// A lexer error. This contains no information about the actual error.
#[derive(Clone, Debug)]
pub struct LexError;

impl<'s, 'd> Lexer<'s, 'd> {
    /// Construct a new lexer.
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

    /// Tokenize the given inputs.
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
                '{' => LCurly,
                '}' => RCurly,
                '|' => Pipe,
                '%' => Percent,
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

    /// Emit the given diagnostic.
    pub(crate) fn emit(
        &mut self,
        level: DiagnosticLevel,
        message: impl Into<String>,
        suggestion: Option<String>,
    ) {
        let span = Span::new(self.cursor.committed as u32, self.cursor.cursor as u32);
        self.diagnostics.push(Diagnostic::new(
            span,
            level,
            message.into(),
            suggestion,
            self.source_idx,
        ))
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

    /// Advance the cursor of the lexer.
    #[expect(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<char> {
        self.iter.next().inspect(|_| self.cursor.next())
    }

    /// Peek the next character.
    pub fn peek(&self) -> Option<char> {
        self.iter.clone().next()
    }

    /// Commit the text consumed so far and create a new token from it.
    pub fn commit(&mut self, kind: TokenKind) -> Spanned<Token> {
        let start = self.cursor.committed;
        let stop = self.cursor.cursor;
        let symbol = &self.source[start..stop];
        let span = Span::new(start as u32, stop as u32);

        self.cursor.commit();
        Spanned::new(span, Token::new(kind, symbol))
    }
}

/// A commitable cursor.
#[derive(Default)]
pub struct Cursor {
    cursor: usize,
    committed: usize,
}

impl Cursor {
    /// Construct a new cursor.
    pub const fn new() -> Self {
        Self {
            cursor: 0,
            committed: 0,
        }
    }

    /// Advance the cursor.
    pub const fn next(&mut self) {
        self.cursor += 1;
    }

    /// Commit the cursor.
    pub const fn commit(&mut self) {
        self.committed = self.cursor;
    }

    /// Rollback to the last commit.
    pub const fn rollback(&mut self) {
        self.cursor = self.committed;
    }

    /// Retrieve the current cursor.
    pub const fn get(&self) -> usize {
        self.cursor
    }
}
