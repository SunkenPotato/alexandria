pub mod expr;
pub mod item;
pub mod stmt;

use diagnostic::Diagnostics;
use lexer::{Intern, Token, TokenKind, stream::TokenStream};
use smallvec::SmallVec;
use span::{Span, Spanned, source::SourceIdx};

pub type ParseResult<T> = std::result::Result<T, ParseError>;

macro_rules! keywords {
    (
        $(
            $ident:ident = $value:expr
        ),*
    ) => {
        $(
            pub static $ident: ::std::sync::LazyLock<Intern<str>> =
            ::std::sync::LazyLock::new(|| Intern::from($value));
        )*

        pub static KEYWORDS: &[&::std::sync::LazyLock<Intern<str>>] = &[$(&$ident),*];
    };
}

keywords! {
    DECL = "decl",
    IF = "if",
    ELSE = "else",
    LOOP = "loop",
    CONTINUE = "continue",
    BREAK = "break",
    RETURN = "return",
    PRODUCT = "product",
    SUM = "sum",
    FUNC = "func",
    PUBLIC = "pub",
    IMPORT = "import"
}

#[derive(Clone, Debug)]
pub enum ParseError {
    TokenMismatch(SmallVec<[TokenKind; 6]>, Span),
    ExpectedKw(Intern<str>, Span),
    InternalParseError,
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
            diag_len: 0,
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
    diag_len: usize,
    stream: &'s [Spanned<Token>],
}

impl<'d, 's, 'i> ParseGuard<'d, 's, 'i> {
    pub fn commit_diag(&mut self) {
        self.diag_len = self.diagnostics.len();
    }

    pub fn rollback_diag(&mut self) {
        self.diagnostics.cull(self.diag_len);
    }

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
            None => {
                let span = self
                    .stream
                    .get(self.index.saturating_sub(1))
                    .map(|x| x.span)
                    .unwrap_or(Span::new(0, 0));

                Err(ParseError::TokenMismatch(smallvec::smallvec![], span))
            }
        }
    }

    pub fn peek(&self) -> ParseResult<Spanned<Token>> {
        self.stream.get(*self.index).copied().ok_or_else(|| {
            let span = self
                .stream
                .get(self.index.saturating_sub(1))
                .map(|x| x.span)
                .unwrap_or(Span::new(0, 0));

            ParseError::TokenMismatch(smallvec::smallvec![], span)
        })
    }

    pub fn next_require(&mut self, kind: TokenKind) -> ParseResult<Spanned<Token>> {
        match self.stream.get(*self.index) {
            Some(v) if v.item.kind == kind => {
                *self.index += 1;
                Ok(*v)
            }
            Some(v) => Err(ParseError::TokenMismatch(smallvec::smallvec![kind], v.span)),
            None => {
                let span = self
                    .stream
                    .get(self.index.saturating_sub(1))
                    .map(|x| x.span)
                    .unwrap_or(Span::new(0, 0));

                Err(ParseError::TokenMismatch(smallvec::smallvec![kind], span))
            }
        }
    }

    pub fn peek_require(&self, kind: TokenKind) -> ParseResult<Spanned<Token>> {
        match self.stream.get(*self.index) {
            Some(v) if v.item.kind == kind => Ok(*v),
            Some(v) => Err(ParseError::TokenMismatch(smallvec::smallvec![kind], v.span)),
            None => {
                let span = self
                    .stream
                    .get(self.index.saturating_sub(1))
                    .map(|x| x.span)
                    .unwrap_or(Span::new(0, 0));

                Err(ParseError::TokenMismatch(smallvec::smallvec![kind], span))
            }
        }
    }

    pub fn peek_n(&self, n: usize) -> ParseResult<Spanned<Token>> {
        match self.stream.get(*self.index + n) {
            Some(v) => Ok(*v),
            None => {
                let span = self
                    .stream
                    .get(self.index.saturating_sub(1))
                    .map(|x| x.span)
                    .unwrap_or(Span::new(0, 0));

                Err(ParseError::TokenMismatch(smallvec::smallvec![], span))
            }
        }
    }

    pub fn spanning<F, T, E>(&mut self, f: F) -> Result<Spanned<T>, E>
    where
        for<'d2, 's2, 'i2> F: FnOnce(ParseGuard<'d2, 's2, 'i2>) -> Result<T, E>,
    {
        let mut index = *self.index;
        let guard = ParseGuard {
            diag_len: self.diagnostics.len(),
            diagnostics: self.diagnostics,
            committed: index,
            index: &mut index,
            stream: self.stream,
        };

        let result = f(guard)?;

        let span = if *self.index == index {
            let next_token_span = self
                .stream
                .get(index)
                .map(|x| x.span)
                .unwrap_or(Span::new(0, 0));

            Span::new(next_token_span.start(), next_token_span.start())
        } else {
            self.stream[*self.index..index]
                .iter()
                .fold(self.stream[*self.index].span, |pre, t| pre.extend(t.span))
        };
        *self.index = index;

        Ok(Spanned::new(span, result))
    }

    pub fn with<F, T, E>(&mut self, f: F) -> Result<T, E>
    where
        for<'d2, 's2, 'i2> F: FnOnce(ParseGuard<'d2, 's2, 'i2>) -> Result<T, E>,
    {
        let mut index = *self.index;
        let guard = ParseGuard {
            diag_len: self.diagnostics.len(),
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
    pub segments: Vec<Spanned<Segment>>,
    pub is_fully_qualified: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub is_kw: bool,
    pub segment: Intern<str>,
}

impl Segment {
    #[cfg(test)]
    pub fn new(val: &str) -> Self {
        Self {
            is_kw: false,
            segment: Intern::from(val),
        }
    }
}

impl Path {
    #[cfg(test)]
    pub fn single(val: Spanned<Intern<str>>) -> Spanned<Self> {
        Spanned::new(
            val.span,
            Self {
                segments: vec![val.map(|x| Segment {
                    is_kw: false,
                    segment: x,
                })],
                is_fully_qualified: false,
            },
        )
    }
}

impl Parse for Path {
    fn is_ok(&self) -> bool {
        self.segments.iter().all(|x| !x.item.is_kw)
    }

    fn parse<'diag, 'source, 'index>(
        mut guard: ParseGuard<'diag, 'source, 'index>,
    ) -> ParseResult<Self> {
        // using new guard so that it's atomic
        let is_fully_qualified = guard.spanning(consume_double_colon).is_ok();
        let first = guard.next_require(TokenKind::Ident)?.map(|x| x.symbol);
        let first_is_kw = KEYWORDS.iter().any(|x| ***x == first.item);
        let mut segments = vec![first.map(|x| Segment {
            is_kw: first_is_kw,
            segment: x,
        })];

        loop {
            if guard.spanning(consume_double_colon).is_err() {
                break;
            }

            let segment = guard.next_require(TokenKind::Ident)?.map(|x| x.symbol);
            let is_kw = KEYWORDS.iter().any(|x| ***x == segment.item);

            segments.push(segment.map(|x| Segment { is_kw, segment: x }));
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
            diagnostics.write_stdout(&sources).unwrap();
            panic!();
        }
    };

    let mut parser = Parser::new(&tokens, &mut diagnostics);
    let parsed = match parser.parse::<T>() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("Failed to parse tokens. Error: {e:#?}. Diagnostics: ");
            diagnostics.write_stdout(&sources).unwrap();
            panic!();
        }
    };

    pretty_assertions::assert_eq!(other, parsed)
}

#[cfg(test)]
mod tests {
    use lexer::Intern;
    use span::{Span, Spanned};

    use crate::{Path, Segment, assert_eq};

    #[test]
    fn parse_fq_path() {
        assert_eq(
            "::std::io",
            Spanned::new(
                Span::new(0, 9),
                Path {
                    is_fully_qualified: true,
                    segments: vec![
                        Spanned::new(Span::new(2, 5), Segment::new("std")),
                        Spanned::new(Span::new(7, 9), Segment::new("io")),
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
                        Spanned::new(Span::new(0, 3), Segment::new("std")),
                        Spanned::new(Span::new(5, 7), Segment::new("io")),
                    ],
                },
            ),
        )
    }

    #[test]
    fn parse_single_path() {
        assert_eq(
            "tmp",
            Path::single(Spanned::new(Span::new(0, 3), Intern::from("tmp"))),
        );
    }
}
