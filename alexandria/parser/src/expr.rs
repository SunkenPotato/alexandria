use diagnostic::Diagnostic;
use lexer::TokenKind;
use span::{Span, Spanned};

use crate::{
    ELSE, IF, Parse, ParseError, ParseGuard, ParseResult, Path,
    expr::literal::Literal,
    stmt::{Binding, Stmt},
};

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
                    smallvec::smallvec![
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
    Block(Block),
    FnCall(FnCall),
    Parenthesized(Box<Expr>),
    Conditional(ConditionalExpr),
}

impl Parse for BaseExpr {
    fn is_ok(&self) -> bool {
        match self {
            Self::Literal(v) => v.is_ok(),
            Self::Path(v) => v.is_ok(),
            Self::Block(v) => v.is_ok(),
            Self::FnCall(v) => v.object.item.is_ok() && v.args.iter().all(|x| x.item.is_ok()),
            Self::Parenthesized(v) => v.is_ok(),
            Self::Conditional(v) => v.is_ok(),
        }
    }

    fn parse<'diag, 'source, 'index>(
        mut guard: crate::ParseGuard<'diag, 'source, 'index>,
    ) -> crate::ParseResult<Self> {
        if guard.next_require(TokenKind::LParen).is_ok() {
            let expr = Box::new(guard.with(Expr::parse)?);
            guard.next_require(TokenKind::RParen)?;
            Ok(Self::Parenthesized(expr))
        } else {
            let object = Box::new(
                guard
                    .spanning(Literal::parse)
                    .map(|x| x.map(Self::Literal))
                    .or_else(|_| {
                        dbg!(
                            guard
                                .spanning(ConditionalExpr::parse)
                                .map(|x| x.map(Self::Conditional))
                        )
                    })
                    .or_else(|_| {
                        guard
                            .spanning(|mut g| match g.with(Path::parse) {
                                Ok(v) if v.is_ok() => Ok(v),
                                Ok(_) => Err(ParseError::InternalParseError),
                                Err(e) => Err(e),
                            })
                            .map(|x| x.map(Self::Path))
                    })
                    .or_else(|_| guard.spanning(Block::parse).map(|x| x.map(Self::Block)))?,
            );

            if guard.next_require(TokenKind::LParen).is_ok() {
                let mut args = vec![];

                while let Ok(arg) = guard.spanning(Expr::parse) {
                    args.push(arg);
                    if guard.next_require(TokenKind::Comma).is_ok() {
                        if guard.next_require(TokenKind::RParen).is_ok() {
                            break;
                        } else {
                            continue;
                        }
                    } else {
                        guard.next_require(TokenKind::RParen)?;
                    }
                }

                Ok(Self::FnCall(FnCall { object, args }))
            } else {
                Ok(object.item)
            }
        }
    }
}

pub mod literal {
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
                    smallvec::smallvec![TokenKind::Integer, TokenKind::StringLit],
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

#[derive(Debug, PartialEq, Clone)]
pub struct Block {
    pub stmts: Vec<Spanned<Stmt>>,
    pub tail: Option<Box<Spanned<Expr>>>,
}

impl Parse for Block {
    fn is_ok(&self) -> bool {
        self.stmts.iter().all(|x| x.item.is_ok())
            && self.tail.as_ref().is_some_and(|x| x.item.is_ok())
    }

    fn parse<'diag, 'source, 'index>(
        mut guard: ParseGuard<'diag, 'source, 'index>,
    ) -> ParseResult<Self> {
        let opening = guard.next_require(TokenKind::LCurly)?;
        let mut stmts = vec![];
        loop {
            if let Ok(b) = guard.spanning(Binding::parse) {
                stmts.push(b.map(Stmt::Binding));
                continue;
            }

            let mut expr = guard.spanning(Expr::parse)?;
            if let Ok(v) = guard.next_require(TokenKind::Semicolon) {
                expr.span = expr.span.extend(v.span);
                stmts.push(expr.map(Stmt::ExprSemi));
                continue;
            }

            if let Err(e) = guard.next_require(TokenKind::RCurly) {
                guard.diagnostics.push(
                    Diagnostic::error(
                        Span::new(expr.span.stop() - 1, expr.span.stop()),
                        "expected closing delimiter ('}') here",
                        Some("add a '}' here".to_owned()),
                    )
                    .with_secondary(opening.span),
                );
                return Err(e);
            }

            return Ok(Self {
                stmts,
                tail: Some(Box::new(expr)),
            });
        }
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct FnCall {
    pub object: Box<Spanned<BaseExpr>>,
    pub args: Vec<Spanned<Expr>>,
}

#[derive(Clone, PartialEq, Debug)]
pub struct ConditionalExpr {
    pub main: Spanned<ConditionalBlock>,
    pub alternatives: Vec<Spanned<ConditionalBlock>>,
    pub fallback: Option<Spanned<Block>>,
}

impl Parse for ConditionalExpr {
    fn is_ok(&self) -> bool {
        self.main.item.is_ok()
            && self.alternatives.iter().all(|x| x.item.is_ok())
            && self.fallback.as_ref().is_none_or(|x| x.item.is_ok())
    }

    fn parse<'diag, 'source, 'index>(
        mut guard: ParseGuard<'diag, 'source, 'index>,
    ) -> ParseResult<Self> {
        let main = guard.spanning(ConditionalBlock::parse)?;
        let mut alternatives = vec![];

        while let Ok(block) = guard.spanning(ConditionalBlock::parse_else_if) {
            alternatives.push(block);
        }

        let fallback = 'a: {
            let Ok(else_kw) = guard.next_require(TokenKind::Ident) else {
                break 'a None;
            };

            if else_kw.item.symbol != *ELSE {
                break 'a None;
            }

            guard.spanning(Block::parse).ok()
        };

        Ok(Self {
            main,
            alternatives,
            fallback,
        })
    }
}

#[derive(Clone, PartialEq, Debug)]
pub struct ConditionalBlock {
    pub condition: Box<Spanned<Expr>>,
    pub block: Spanned<Block>,
}

impl ConditionalBlock {
    fn parse_else_if(mut guard: ParseGuard) -> ParseResult<Self> {
        let else_kw = guard.next_require(TokenKind::Ident)?;
        if else_kw.item.symbol != *ELSE {
            return Err(ParseError::ExpectedKw(else_kw.item.symbol, else_kw.span));
        }

        let block = guard.with(Self::parse)?;

        Ok(block)
    }
}

impl Parse for ConditionalBlock {
    fn is_ok(&self) -> bool {
        self.condition.item.is_ok() && self.block.item.is_ok()
    }

    fn parse<'diag, 'source, 'index>(
        mut guard: ParseGuard<'diag, 'source, 'index>,
    ) -> ParseResult<Self> {
        let kw = guard.next_require(TokenKind::Ident)?;
        if kw.item.symbol != *IF {
            return Err(ParseError::ExpectedKw(kw.item.symbol, kw.span));
        }

        guard.next_require(TokenKind::LParen)?;
        let condition = Box::new(guard.spanning(Expr::parse)?);
        guard.next_require(TokenKind::RParen)?;

        let block = guard.spanning(Block::parse)?;

        Ok(Self { condition, block })
    }
}

// --- tests ---
#[cfg(test)]
mod tests {
    use lexer::Intern;
    use span::{Span, Spanned};

    use crate::{
        Path, assert_eq,
        expr::{
            BaseExpr, BinaryExpr, BinaryOp, Block, ConditionalBlock, ConditionalExpr, Expr, FnCall,
            literal::{IntegerLiteral, Literal, StringLiteral},
        },
        stmt::{Binding, Stmt},
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

    #[test]
    fn parse_block() {
        assert_eq(
            "{decl x = 5; x + y}",
            Spanned::new(
                Span::new(0, 19),
                Expr::Base(BaseExpr::Block(Block {
                    stmts: vec![Spanned::new(
                        Span::new(1, 12),
                        Stmt::Binding(Binding {
                            is_mutable: None,
                            ident: Spanned::new(Span::new(6, 7), Intern::from("x")),
                            value: Spanned::new(
                                Span::new(10, 11),
                                Expr::Base(BaseExpr::Literal(Literal::Int(IntegerLiteral::Ok(5)))),
                            ),
                        }),
                    )],
                    tail: Some(Box::new(Spanned::new(
                        Span::new(13, 18),
                        Expr::Binary(BinaryExpr {
                            lhs: Box::new(Spanned::new(
                                Span::new(13, 14),
                                Expr::Base(BaseExpr::Path(
                                    Path::single(Spanned::new(
                                        Span::new(13, 14),
                                        Intern::from("x"),
                                    ))
                                    .item,
                                )),
                            )),
                            op: Spanned::new(Span::new(15, 16), BinaryOp::Add),
                            rhs: Box::new(Spanned::new(
                                Span::new(17, 18),
                                Expr::Base(BaseExpr::Path(
                                    Path::single(Spanned::new(
                                        Span::new(17, 18),
                                        Intern::from("y"),
                                    ))
                                    .item,
                                )),
                            )),
                        }),
                    ))),
                })),
            ),
        );
    }

    #[test]
    fn parse_parenthesized() {
        assert_eq(
            "(5)",
            Spanned::new(
                Span::new(0, 3),
                Expr::Base(BaseExpr::Parenthesized(Box::new(Expr::Base(
                    BaseExpr::Literal(Literal::Int(IntegerLiteral::Ok(5))),
                )))),
            ),
        );
    }

    #[test]
    fn parse_fn_call() {
        assert_eq(
            "add(2, 2)",
            Spanned::new(
                Span::new(0, 9),
                Expr::Base(BaseExpr::FnCall(FnCall {
                    object: Box::new(Spanned::new(
                        Span::new(0, 3),
                        BaseExpr::Path(
                            Path::single(Spanned::new(Span::new(0, 3), Intern::from("add"))).item,
                        ),
                    )),
                    args: vec![
                        Spanned::new(
                            Span::new(4, 5),
                            Expr::Base(BaseExpr::Literal(Literal::Int(IntegerLiteral::Ok(2)))),
                        ),
                        Spanned::new(
                            Span::new(7, 8),
                            Expr::Base(BaseExpr::Literal(Literal::Int(IntegerLiteral::Ok(2)))),
                        ),
                    ],
                })),
            ),
        )
    }

    #[test]
    fn parse_cond_expr() {
        assert_eq(
            "if (1) { 2 }",
            Spanned::new(
                Span::new(0, 12),
                Expr::Base(BaseExpr::Conditional(ConditionalExpr {
                    main: Spanned::new(
                        Span::new(0, 12),
                        ConditionalBlock {
                            condition: Box::new(Spanned::new(
                                Span::new(4, 5),
                                Expr::Base(BaseExpr::Literal(Literal::Int(IntegerLiteral::Ok(1)))),
                            )),
                            block: Spanned::new(
                                Span::new(7, 12),
                                Block {
                                    stmts: vec![],
                                    tail: Some(Box::new(Spanned::new(
                                        Span::new(9, 10),
                                        Expr::Base(BaseExpr::Literal(Literal::Int(
                                            IntegerLiteral::Ok(2),
                                        ))),
                                    ))),
                                },
                            ),
                        },
                    ),
                    alternatives: vec![],
                    fallback: None,
                })),
            ),
        );
    }
}
