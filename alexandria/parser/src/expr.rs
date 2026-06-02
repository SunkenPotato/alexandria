use lexer::TokenKind;
use span::{Span, Spanned};

use crate::{Parse, ParseError, ParseGuard, ParseResult, Path, expr::literal::Literal};

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Binary(BinaryExpr),
    Base(BaseExpr),
}

impl Parse for Expr {
    fn is_ok(&self) -> bool {
        match self {
            Expr::Base(base) => base.is_ok(),
            Expr::Binary(binary) => {
                binary.lhs.item.is_ok() && binary.op.item.is_ok() && binary.rhs.item.is_ok()
            }
        }
    }

    fn parse<'diag, 'source, 'index>(
        guard: ParseGuard<'diag, 'source, 'index>,
    ) -> ParseResult<Self> {
        Self::parse_1(guard, 0)
    }
}
impl Expr {
    fn parse_1(mut guard: ParseGuard, precedence: u8) -> ParseResult<Self> {
        let mut base = guard.spanning(BaseExpr::parse)?.map(Self::Base);

        while let Ok(op) = guard.spanning(|guard| match BinaryOp::parse(guard) {
            Ok(v) if v.precedence() > precedence => Ok(v),
            Ok(_) | Err(_) => Err(()),
        }) {
            let rhs = guard.spanning(|g| Self::parse_1(g, op.item.precedence()))?;

            base = Spanned::new(
                Span::new(base.span.start(), rhs.span.stop()),
                Self::Binary(BinaryExpr {
                    lhs: Box::new(base),
                    op,
                    rhs: Box::new(rhs),
                }),
            );
        }

        Ok(base.item)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct BinaryExpr {
    pub lhs: Box<Spanned<Expr>>,
    pub op: Spanned<BinaryOp>,
    pub rhs: Box<Spanned<Expr>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    NotEq,
    Lt,
    Gt,
    Le,
    Ge,
    Shr,
    Shl,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
}

impl Parse for BinaryOp {
    fn is_ok(&self) -> bool {
        true
    }

    fn parse<'diag, 'source, 'index>(
        mut guard: crate::ParseGuard<'diag, 'source, 'index>,
    ) -> crate::ParseResult<Self> {
        let next = guard.next()?;
        let op = match next.item.kind {
            TokenKind::Plus => Self::Add,
            TokenKind::Minus => Self::Sub,
            TokenKind::Asterisk => Self::Mul,
            TokenKind::Slash => Self::Div,
            TokenKind::Percent => Self::Rem,
            TokenKind::Equal => {
                guard.next_require(TokenKind::Equal)?;
                Self::Eq
            }
            TokenKind::Bang => {
                guard.next_require(TokenKind::Equal)?;
                Self::NotEq
            }
            TokenKind::LessThan => {
                if guard.next_require(TokenKind::LessThan).is_ok() {
                    Self::Shl
                } else if guard.next_require(TokenKind::Equal).is_ok() {
                    Self::Le
                } else {
                    Self::Lt
                }
            }
            TokenKind::GreaterThan => {
                if guard.next_require(TokenKind::GreaterThan).is_ok() {
                    Self::Shr
                } else if guard.next_require(TokenKind::Equal).is_ok() {
                    Self::Ge
                } else {
                    Self::Gt
                }
            }
            TokenKind::Ampersand => {
                if guard.next_require(TokenKind::Ampersand).is_ok() {
                    Self::And
                } else {
                    Self::BitAnd
                }
            }
            TokenKind::Pipe => {
                if guard.next_require(TokenKind::Pipe).is_ok() {
                    Self::Or
                } else {
                    Self::BitOr
                }
            }
            TokenKind::Caret => Self::BitXor,
            _ => {
                return Err(ParseError::TokenMismatch(
                    vec![
                        TokenKind::Plus,
                        TokenKind::Minus,
                        TokenKind::Asterisk,
                        TokenKind::Slash,
                        TokenKind::Percent,
                        TokenKind::Equal,
                        TokenKind::Bang,
                        TokenKind::LessThan,
                        TokenKind::GreaterThan,
                        TokenKind::Ampersand,
                        TokenKind::Pipe,
                        TokenKind::Caret,
                    ],
                    next.span,
                ));
            }
        };

        Ok(op)
    }
}

impl BinaryOp {
    pub const fn precedence(&self) -> u8 {
        match self {
            BinaryOp::Eq
            | BinaryOp::NotEq
            | BinaryOp::And
            | BinaryOp::BitAnd
            | BinaryOp::Or
            | BinaryOp::BitOr
            | BinaryOp::BitXor => 10,
            BinaryOp::Lt | BinaryOp::Le | BinaryOp::Ge | BinaryOp::Gt => 20,
            BinaryOp::Shr | BinaryOp::Shl => 30,
            BinaryOp::Add | BinaryOp::Sub => 40,
            BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => 50,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BaseExpr {
    Literal(Literal),
    Path(Path),
}

impl Parse for BaseExpr {
    fn is_ok(&self) -> bool {
        match self {
            Self::Literal(v) => v.is_ok(),
            Self::Path(v) => v.is_ok(),
        }
    }

    fn parse<'diag, 'source, 'index>(
        mut guard: crate::ParseGuard<'diag, 'source, 'index>,
    ) -> crate::ParseResult<Self> {
        guard
            .with(Literal::parse)
            .map(Self::Literal)
            .or_else(|_| guard.with(Path::parse).map(Self::Path))
    }
}

mod literal {
    use diagnostic::Diagnostic;
    use lexer::{Intern, TokenKind};
    use span::Span;

    use crate::{Parse, ParseError};

    #[derive(Debug, Clone, PartialEq)]
    pub enum Literal {
        Int(IntegerLiteral),
        Str(StringLiteral),
    }

    impl Parse for Literal {
        fn is_ok(&self) -> bool {
            match self {
                Self::Int(v) => v.is_ok(),
                Self::Str(v) => v.is_ok(),
            }
        }

        fn parse<'diag, 'source, 'index>(
            guard: crate::ParseGuard<'diag, 'source, 'index>,
        ) -> crate::ParseResult<Self> {
            let next = guard.peek()?;
            match next.item.kind {
                TokenKind::Integer => Ok(Self::Int(IntegerLiteral::parse(guard)?)),
                TokenKind::StringLit => Ok(Self::Str(StringLiteral::parse(guard)?)),
                _ => Err(ParseError::TokenMismatch(
                    vec![TokenKind::Integer, TokenKind::StringLit],
                    next.span,
                )),
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum IntegerLiteral {
        Ok(u128),
        Overflow,
    }

    impl Parse for IntegerLiteral {
        fn is_ok(&self) -> bool {
            matches!(self, Self::Ok(..))
        }

        fn parse<'diag, 'source, 'index>(
            mut guard: crate::ParseGuard<'diag, 'source, 'index>,
        ) -> crate::ParseResult<Self> {
            let next = guard.next_require(TokenKind::Integer)?;
            let Some(int) = next.item.symbol.chars().try_fold(0u128, |c, next| {
                c.checked_mul(10)
                    .and_then(|c| c.checked_add((next as u32 - 0x30) as u128))
            }) else {
                guard.diagnostics.push(Diagnostic::error(
                    next.span,
                    "integer literal overflow: integer literals have a maximum capacity of 2^128",
                    None,
                ));

                return Ok(Self::Overflow);
            };

            Ok(Self::Ok(int))
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    pub enum StringLiteral {
        Ok(Intern<str>),
        InvalidEsc,
    }

    impl Parse for StringLiteral {
        fn is_ok(&self) -> bool {
            matches!(self, Self::Ok(..))
        }

        fn parse<'diag, 'source, 'index>(
            mut guard: crate::ParseGuard<'diag, 'source, 'index>,
        ) -> crate::ParseResult<Self> {
            let token = guard.next_require(TokenKind::StringLit)?;
            let mut buf = String::with_capacity(token.item.symbol.len());

            let mut iter = token.item.symbol.chars().enumerate().skip(1);
            let mut is_fail = false;
            while let Some((i, strch)) = iter.next() {
                let to_append = match strch {
                    '\\' => {
                        let esc_ch = iter.next().unwrap();
                        match esc_ch.1 {
                            't' => '\t',
                            'n' => '\n',
                            '0' => '\0',
                            '"' => '"',
                            '\\' => '\\',
                            '{' => {
                                guard.diagnostics.push(Diagnostic::error(
                                    Span::new(
                                        token.span.start() + i as u32,
                                        token.span.start() + esc_ch.0 as u32,
                                    ),
                                    "unicode escapes are not yet supported",
                                    None,
                                ));
                                is_fail = true;
                                continue;
                            }
                            other => {
                                guard.diagnostics.push(Diagnostic::error(
                                    token.span,
                                    format!("'{other}' is not an escape character"),
                                    None,
                                ));
                                continue;
                            }
                        }
                    }
                    '"' => break,
                    other => other,
                };

                buf.push(to_append);
            }

            if is_fail {
                return Ok(Self::InvalidEsc);
            }

            Ok(Self::Ok(buf.as_str().into()))
        }
    }
}

// --- tests ---
#[cfg(test)]
mod tests {
    use lexer::Intern;
    use span::{Span, Spanned};

    use crate::{
        assert_eq,
        expr::{
            BaseExpr, BinaryExpr, BinaryOp, Expr,
            literal::{IntegerLiteral, Literal, StringLiteral},
        },
    };

    #[test]
    fn parse_int() {
        assert_eq(
            "123456789",
            Spanned::new(Span::new(0, 9), Literal::Int(IntegerLiteral::Ok(123456789))),
        );
    }

    #[test]
    fn parse_int_overflow() {
        assert_eq(
            "340282366920938463463374607431768211456",
            Spanned::new(Span::new(0, 39), Literal::Int(IntegerLiteral::Overflow)),
        );
    }

    #[test]
    fn parse_str() {
        assert_eq(
            r#""gallia in tres partes divisa\nest""#,
            Spanned::new(
                Span::new(0, 35),
                Literal::Str(StringLiteral::Ok(Intern::from(
                    "gallia in tres partes divisa\nest",
                ))),
            ),
        )
    }

    #[test]
    fn parse_str_invalid_escape_codes() {
        assert_eq(
            r#""\x\{0001}""#,
            Spanned::new(Span::new(0, 11), Literal::Str(StringLiteral::InvalidEsc)),
        )
    }

    #[test]
    fn parse_simple_bin_expr() {
        assert_eq(
            "2 + 2",
            Spanned::new(
                Span::new(0, 5),
                Expr::Binary(BinaryExpr {
                    lhs: Box::new(Spanned::new(
                        Span::new(0, 1),
                        Expr::Base(BaseExpr::Literal(Literal::Int(IntegerLiteral::Ok(2)))),
                    )),
                    op: Spanned::new(Span::new(2, 3), BinaryOp::Add),
                    rhs: Box::new(Spanned::new(
                        Span::new(4, 5),
                        Expr::Base(BaseExpr::Literal(Literal::Int(IntegerLiteral::Ok(2)))),
                    )),
                }),
            ),
        );
    }

    #[test]
    fn parse_complex_bin_expr() {
        assert_eq(
            "4 - 11 % 7 == 16 >> 4",
            Spanned::new(
                Span::new(0, 21),
                Expr::Binary(BinaryExpr {
                    lhs: Box::new(Spanned::new(
                        Span::new(0, 10),
                        Expr::Binary(BinaryExpr {
                            lhs: Box::new(Spanned::new(
                                Span::new(0, 1),
                                Expr::Base(BaseExpr::Literal(Literal::Int(IntegerLiteral::Ok(4)))),
                            )),
                            op: Spanned::new(Span::new(2, 3), BinaryOp::Sub),
                            rhs: Box::new(Spanned::new(
                                Span::new(4, 10),
                                Expr::Binary(BinaryExpr {
                                    lhs: Box::new(Spanned::new(
                                        Span::new(4, 6),
                                        Expr::Base(BaseExpr::Literal(Literal::Int(
                                            IntegerLiteral::Ok(11),
                                        ))),
                                    )),
                                    op: Spanned::new(Span::new(7, 8), BinaryOp::Rem),
                                    rhs: Box::new(Spanned::new(
                                        Span::new(9, 10),
                                        Expr::Base(BaseExpr::Literal(Literal::Int(
                                            IntegerLiteral::Ok(7),
                                        ))),
                                    )),
                                }),
                            )),
                        }),
                    )),
                    op: Spanned::new(Span::new(11, 13), BinaryOp::Eq),
                    rhs: Box::new(Spanned::new(
                        Span::new(14, 21),
                        Expr::Binary(BinaryExpr {
                            lhs: Box::new(Spanned::new(
                                Span::new(14, 16),
                                Expr::Base(BaseExpr::Literal(Literal::Int(IntegerLiteral::Ok(16)))),
                            )),
                            op: Spanned::new(Span::new(17, 19), BinaryOp::Shr),
                            rhs: Box::new(Spanned::new(
                                Span::new(20, 21),
                                Expr::Base(BaseExpr::Literal(Literal::Int(IntegerLiteral::Ok(4)))),
                            )),
                        }),
                    )),
                }),
            ),
        )
    }
}
