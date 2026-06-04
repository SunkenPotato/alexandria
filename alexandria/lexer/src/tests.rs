use diagnostic::Diagnostics;
use internment::Intern;
use span::{
    Span, Spanned,
    source::{SourceFile, SourceMap},
};

use crate::{Lexer, Token, TokenKind};

#[track_caller]
fn assert(input: &str, expect: &[Spanned<Token>]) {
    let mut map = SourceMap::new();
    let source_file = SourceFile::from_memory(input.to_owned());
    let idx = map.insert(source_file);
    let mut diagnostics = Diagnostics::new(idx);
    let lexer = Lexer::new(&map, idx, &mut diagnostics);
    match lexer.lex() {
        Ok(v) => assert_eq!(v.tokens, expect),
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
