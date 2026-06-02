pub mod expr;

use diagnostic::Diagnostics;
use lexer::{Intern, Token, TokenKind, stream::TokenStream};
use span::{Span, Spanned, source::SourceIdx};

pub type ParseResult<T> = std::result::Result<T, ParseError>;

#[derive(Clone, Debug)]
pub enum ParseError {
    Eof,
    // TODO: replace with smallvec or something
    TokenMismatch(Vec<TokenKind>, Span),
}

pub struct Parser<'s, 'd> {
    tokens: &'s [Spanned<Token>],
    pub source_idx: SourceIdx,
    diagnostics: &'d mut Diagnostics,
}

impl<'s, 'd> Parser<'s, 'd> {
    pub const fn new(tokens: &'s TokenStream, diagnostics: &'d mut Diagnostics) -> Self {
        Self {
            tokens: tokens.tokens(),
            source_idx: tokens.source_idx,
            diagnostics,
        }
    }

    pub fn parse<T: Parse>(&mut self) -> ParseResult<Spanned<T>> {
        let mut guard = ParseGuard {
            stream: self.tokens,
            diagnostics: self.diagnostics,
            committed: 0,
            index: &mut 0,
        };

        guard.spanning(T::parse)
    }
}

pub struct ParseGuard<'d, 's, 'i> {
    diagnostics: &'d mut Diagnostics,
    index: &'i mut usize,
    committed: usize,
    stream: &'s [Spanned<Token>],
}

impl<'d, 's, 'i> ParseGuard<'d, 's, 'i> {
    pub fn commit(&mut self) {
        self.committed = *self.index;
    }

    pub fn rollback(&mut self) {
        *self.index = self.committed;
    }

    #[expect(clippy::should_implement_trait)]
    pub fn next(&mut self) -> ParseResult<Spanned<Token>> {
        match self.stream.get(*self.index) {
            Some(v) => {
                *self.index += 1;
                Ok(*v)
            }
            None => Err(ParseError::Eof),
        }
    }

    pub fn peek(&self) -> ParseResult<Spanned<Token>> {
        self.stream.get(*self.index).copied().ok_or(ParseError::Eof)
    }

    pub fn next_require(&mut self, kind: TokenKind) -> ParseResult<Spanned<Token>> {
        match self.stream.get(*self.index) {
            Some(v) if v.item.kind == kind => {
                *self.index += 1;
                Ok(*v)
            }
            Some(v) => Err(ParseError::TokenMismatch(vec![v.item.kind], v.span)),
            None => Err(ParseError::Eof),
        }
    }

    pub fn peek_require(&self, kind: TokenKind) -> ParseResult<Spanned<Token>> {
        match self.stream.get(*self.index) {
            Some(v) if v.item.kind == kind => Ok(*v),
            Some(v) => Err(ParseError::TokenMismatch(vec![v.item.kind], v.span)),
            None => Err(ParseError::Eof),
        }
    }

    pub fn peek_n(&self, n: usize) -> ParseResult<Spanned<Token>> {
        match self.stream.get(*self.index + n) {
            Some(v) => Ok(*v),
            None => Err(ParseError::Eof),
        }
    }

    pub fn spanning<F, T>(&mut self, f: F) -> ParseResult<Spanned<T>>
    where
        for<'d2, 's2, 'i2> F: FnOnce(ParseGuard<'d2, 's2, 'i2>) -> ParseResult<T>,
    {
        let mut index = *self.index;
        let guard = ParseGuard {
            diagnostics: self.diagnostics,
            committed: index,
            index: &mut index,
            stream: self.stream,
        };

        let result = f(guard)?;

        let span = self.stream[*self.index..index]
            .iter()
            .fold(self.stream[*self.index].span, |pre, t| pre.extend(t.span));
        *self.index = index;

        Ok(Spanned::new(span, result))
    }

    pub fn with<F, T>(&mut self, f: F) -> ParseResult<T>
    where
        for<'d2, 's2, 'i2> F: FnOnce(ParseGuard<'d2, 's2, 'i2>) -> ParseResult<T>,
    {
        let mut index = *self.index;
        let guard = ParseGuard {
            diagnostics: self.diagnostics,
            committed: index,
            index: &mut index,
            stream: self.stream,
        };

        let result = f(guard)?;
        *self.index = index;
        Ok(result)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Path {
    segments: Vec<Spanned<Intern<str>>>,
    is_fully_qualified: bool,
}

impl Path {
    pub fn single(val: Spanned<Intern<str>>) -> Self {
        Self {
            segments: vec![val],
            is_fully_qualified: false,
        }
    }
}

impl Parse for Path {
    fn is_ok(&self) -> bool {
        true
    }

    fn parse<'diag, 'source, 'index>(
        mut guard: ParseGuard<'diag, 'source, 'index>,
    ) -> ParseResult<Self> {
        // using spanning so that it's atomic
        let is_fully_qualified = guard.spanning(consume_double_colon).is_ok();
        let mut segments = vec![guard.next_require(TokenKind::Ident)?.map(|x| x.symbol)];

        loop {
            if guard.spanning(consume_double_colon).is_err() {
                break;
            }

            segments.push(guard.next_require(TokenKind::Ident)?.map(|x| x.symbol));
        }

        Ok(Self {
            segments,
            is_fully_qualified,
        })
    }
}

fn consume_double_colon(mut guard: ParseGuard) -> ParseResult<Spanned<()>> {
    guard
        .next_require(TokenKind::Colon)
        .and_then(|_| guard.next_require(TokenKind::Colon))
        .map(|x| x.map(|_| ()))
}

pub trait Parse: Sized {
    fn parse<'diag, 'source, 'index>(
        guard: ParseGuard<'diag, 'source, 'index>,
    ) -> ParseResult<Self>;

    /// Specify which state of this can be interpreted as a successfully parsed element.
    fn is_ok(&self) -> bool;
}

#[cfg(test)]
#[track_caller]
fn assert_eq<T>(input: impl Into<String>, other: Spanned<T>)
where
    T: Parse + PartialEq + std::fmt::Debug,
{
    use lexer::Lexer;
    use span::source::{SourceFile, SourceMap};

    let mut sources = SourceMap::new();
    let source_file = SourceFile::from_memory(input.into());
    let source_idx = sources.insert(source_file);
    let mut diagnostics = Diagnostics::new(source_idx);

    let lexer = Lexer::new(&sources, source_idx, &mut diagnostics);
    let tokens = match lexer.lex() {
        Ok(v) => v,
        Err(_) => {
            eprintln!("Input failed to lex. Diagnostics: ");
            diagnostics.write_stderr(&sources).unwrap();
            panic!();
        }
    };

    dbg!(&tokens);

    let mut parser = Parser::new(&tokens, &mut diagnostics);
    let parsed = match parser.parse::<T>() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Failed to parse tokens. Error: {e:#?}. Diagnostics: ");
            diagnostics.write_stderr(&sources).unwrap();
            panic!();
        }
    };

    assert_eq!(other, parsed)
}

#[cfg(test)]
mod tests {
    use lexer::Intern;
    use span::{Span, Spanned};

    use crate::{Path, assert_eq};

    #[test]
    fn parse_fq_path() {
        assert_eq(
            "::std::io",
            Spanned::new(
                Span::new(0, 9),
                Path {
                    is_fully_qualified: true,
                    segments: vec![
                        Spanned::new(Span::new(2, 5), Intern::from("std")),
                        Spanned::new(Span::new(7, 9), Intern::from("io")),
                    ],
                },
            ),
        );
    }

    #[test]
    fn parse_path() {
        assert_eq(
            "std::io",
            Spanned::new(
                Span::new(0, 7),
                Path {
                    is_fully_qualified: false,
                    segments: vec![
                        Spanned::new(Span::new(0, 3), Intern::from("std")),
                        Spanned::new(Span::new(5, 7), Intern::from("io")),
                    ],
                },
            ),
        )
    }

    #[test]
    fn parse_single_path() {
        assert_eq(
            "tmp",
            Spanned::new(
                Span::new(0, 3),
                Path::single(Spanned::new(Span::new(0, 3), Intern::from("tmp"))),
            ),
        );
    }
}
