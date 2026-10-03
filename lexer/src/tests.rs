use diagnostic::Diagnostics;
use internment::Intern;
use source::{SourceFile, SourceMap};
use span::{Span, Spanned};

use crate::{Lexer, Token, TokenKind};

#[track_caller]
fn assert(input: &str, expect: &[Spanned<Token>]) {
    let map = SourceMap::new();
    let source_file = SourceFile::from_memory(input.to_owned());
    let idx = map.insert(source_file);
    let diagnostics = Diagnostics::default();
    let lexer = Lexer::new(&map, idx, diagnostics.clone());
    match lexer.lex() {
        Ok(v) => assert_eq!(v.tokens(), expect),
        Err(_) => {
            eprintln!("Failed to lex, diagnostics following: ");
            diagnostics.write_stdout(&map).unwrap();
            panic!()
        }
    }
}

macro_rules! lex_atom {
    ($($atom:literal = $expect:expr),*) => {
        $(
            paste::paste! {
                #[test]
                #[allow(non_snake_case)]
                fn [<lex_ $expect:lower>]() {
                    assert($atom, &[
                        Spanned::new(Span::new(0, 1), Token::new(TokenKind::$expect, Intern::from($atom)))
                    ]);
                }
            }
        )*
    }
}

lex_atom![
    "!" = Bang,
    "^" = Caret,
    "&" = Ampersand,
    "*" = Asterisk,
    "(" = LParen,
    ")" = RParen,
    "+" = Plus,
    "=" = Equal,
    "-" = Minus,
    "/" = Slash,
    "<" = LessThan,
    ">" = GreaterThan,
    ":" = Colon,
    ";" = Semicolon,
    "," = Comma,
    "." = Dot,
    "?" = Question,
    "~" = Tilde,
    "[" = LBracket,
    "]" = RBracket,
    "{" = LCurly,
    "}" = RCurly,
    "%" = Percent,
    "|" = Pipe
];

#[test]
fn lex_int() {
    assert(
        "  0001_234543",
        &[Spanned::new(
            Span::new(2, 13),
            Token::new(TokenKind::Integer, "0001_234543"),
        )],
    );
}

#[test]
fn lex_str() {
    assert(
        r#""abcd\"f""#,
        &[Spanned::new(
            Span::new(0, 9),
            Token::new(TokenKind::StringLit, r#""abcd\"f""#),
        )],
    )
}

#[test]
fn lex_ident() {
    assert(
        "let",
        &[Spanned::new(
            Span::new(0, 3),
            Token::new(TokenKind::Ident, "let"),
        )],
    )
}

#[test]
fn lex_non_ascii_uses_byte_offsets() {
    assert(
        "\"é\" x",
        &[
            Spanned::new(Span::new(0, 4), Token::new(TokenKind::StringLit, "\"é\"")),
            Spanned::new(Span::new(5, 6), Token::new(TokenKind::Ident, "x")),
        ],
    )
}

#[test]
fn lex_non_ascii_whitespace() {
    assert(
        "a\u{a0}b",
        &[
            Spanned::new(Span::new(0, 1), Token::new(TokenKind::Ident, "a")),
            Spanned::new(Span::new(3, 4), Token::new(TokenKind::Ident, "b")),
        ],
    )
}

#[test]
fn lex_unknown_char_is_error() {
    let map = SourceMap::new();
    let idx = map.insert(SourceFile::from_memory("a # b".to_owned()));
    let diagnostics = Diagnostics::default();
    assert!(Lexer::new(&map, idx, diagnostics.clone()).lex().is_err());
    assert_eq!(diagnostics.error_count(), 1);
}

#[test]
fn lex_line_comment() {
    assert("//", &[]);
}

#[test]
fn lex_block_comment() {
    assert("/**/", &[]);
}

#[test]
fn lex_nested_block_comment() {
    assert(
        "a/* hello */b",
        &[
            Spanned::new(Span::new(0, 1), Token::new(TokenKind::Ident, "a")),
            Spanned::new(Span::new(12, 13), Token::new(TokenKind::Ident, "b")),
        ],
    );
}
