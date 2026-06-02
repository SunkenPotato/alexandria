pub mod expr;

use diagnostic::Diagnostics;
use lexer::{Token, TokenKind, stream::TokenStream};
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
