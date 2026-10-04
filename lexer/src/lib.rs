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
    /// An integer. This excludes any sign.
    Integer,
    /// A string literal.
    StringLit,
    /// A character literal.
    CharLit,
    /// An identifier.
    Ident,
}

impl TokenKind {
    /// Check whether this is an atom, i.e., a token whose text is always the same.
    pub const fn is_atom(&self) -> bool {
        !matches!(
            self,
            TokenKind::Integer | TokenKind::StringLit | TokenKind::Ident
        )
    }
}

impl std::fmt::Display for TokenKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use TokenKind::*;

        let text = match self {
            Bang => "`!`",
            Caret => "`^`",
            Ampersand => "`&`",
            Asterisk => "`*`",
            LParen => "`(`",
            RParen => "`)`",
            Plus => "`+`",
            Equal => "`=`",
            Minus => "`-`",
            Slash => "`/`",
            LessThan => "`<`",
            GreaterThan => "`>`",
            Colon => "`:`",
            Semicolon => "`;`",
            Comma => "`,`",
            Dot => "`.`",
            Question => "`?`",
            Tilde => "`~`",
            Percent => "`%`",
            LBracket => "`[`",
            RBracket => "`]`",
            LCurly => "`{`",
            RCurly => "`}`",
            Pipe => "`|`",
            CharLit => "character literal",
            Integer => "integer literal",
            StringLit => "string literal",
            Ident => "identifier",
        };

        f.write_str(text)
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
pub struct Lexer<'s> {
    source_idx: SourceIdx,
    iter: Chars<'s>,
    source: &'s str,
    cursor: Cursor,
    diagnostics: Diagnostics,
}

/// A lexer error. This contains no information about the actual error.
#[derive(Clone, Debug)]
pub struct LexError;

impl<'s> Lexer<'s> {
    /// Construct a new lexer.
    pub fn new(map: &'s SourceMap, source_idx: SourceIdx, diagnostics: Diagnostics) -> Self {
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
                '/' => {
                    if self.peek().is_some_and(|x| matches!(x, '/' | '*')) {
                        self.consume_comment();
                        self.cursor.commit();
                        continue;
                    } else {
                        Slash
                    }
                }
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
                '\'' => {
                    self.lex_char();
                    CharLit
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
    fn emit(&self, level: DiagnosticLevel, message: impl Into<String>, suggestion: Option<String>) {
        let span = Span::new(self.cursor.committed as u32, self.cursor.cursor as u32);
        self.diagnostics.push(Diagnostic::new(
            Some(span),
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

    fn lex_char(&mut self) {
        let mut ctr = 0;
        let mut closed = false;

        while let Some(next) = self.next() {
            match next {
                '\'' => {
                    closed = true;
                    break;
                }
                '\"' => {
                    self.emit(
                        DiagnosticLevel::Error,
                        "character literals must be terminated with `'`, not `\"`",
                        Some("replace the `'` with a `\"`".to_owned()),
                    );
                    break;
                }
                _ => (),
            }
            ctr += 1;
        }

        if ctr == 0 {
            self.emit(
                DiagnosticLevel::Error,
                "character literals must not be empty",
                None,
            );
        } else if ctr > 1 {
            self.emit(
                DiagnosticLevel::Error,
                "character literals may only contain one grapheme",
                None,
            );
        }

        if !closed {
            self.emit(
                DiagnosticLevel::Error,
                "character literals must be terminated by a `'`",
                None,
            );
        }
    }

    fn consume_comment(&mut self) {
        match self.next().unwrap() {
            '/' => {
                while let Some(x) = self.peek()
                    && x != '\n'
                {
                    _ = self.next();
                }
            }
            '*' => loop {
                let Some(first) = self.next() else {
                    self.emit(
                        DiagnosticLevel::Error,
                        "unterminated comment",
                        Some("add a `*/` at the end".to_owned()),
                    );
                    return;
                };

                if first == '*' && self.next() == Some('/') {
                    break;
                }
            },
            _ => unreachable!(),
        };
    }

    /// Advance the cursor of the lexer.
    fn next(&mut self) -> Option<char> {
        self.iter
            .next()
            .inspect(|c| self.cursor.advance(c.len_utf8()))
    }

    /// Peek the next character.
    fn peek(&self) -> Option<char> {
        self.iter.clone().next()
    }

    /// Commit the text consumed so far and create a new token from it.
    fn commit(&mut self, kind: TokenKind) -> Spanned<Token> {
        let start = self.cursor.committed;
        let stop = self.cursor.cursor;
        let symbol = &self.source[start..stop];
        let span = Span::new(start as u32, stop as u32);

        self.cursor.commit();
        Spanned::new(span, Token::new(kind, symbol))
    }
}

/// A commitable cursor. Positions are byte offsets into the source.
#[derive(Default)]
struct Cursor {
    cursor: usize,
    committed: usize,
}

impl Cursor {
    /// Construct a new cursor.
    const fn new() -> Self {
        Self {
            cursor: 0,
            committed: 0,
        }
    }

    /// Advance the cursor by `len` bytes.
    const fn advance(&mut self, len: usize) {
        self.cursor += len;
    }

    /// Commit the cursor.
    const fn commit(&mut self) {
        self.committed = self.cursor;
    }
}
