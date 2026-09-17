//! Expression parsers.

use lexer::{Intern, TokenKind};
use node::Node;
use span::{Span, Spanned};

use crate::{
    BREAK, CONTINUE, ELSE, IF, LOOP, Parse, ParseError, ParseGuard, ParseResult, RETURN,
    expr::literal::Literal,
    path::Path,
    stmt::{Binding, Stmt},
};

/// An expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// A binary expression (e.g., a + b)
    Binary(BinaryExpr),
    /// A base expression.
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

    fn parse<'diag, 'source, 'index, 'a>(
        guard: ParseGuard<'diag, 'source, 'index, 'a>,
    ) -> ParseResult<Self> {
        Self::parse_1(guard, 0)
    }
}

impl Expr {
    fn parse_1(mut guard: ParseGuard, precedence: u8) -> ParseResult<Self> {
        let mut base: Spanned<_> = guard.spanning(BaseExpr::parse)?.map(Self::Base);

        while let Ok(op) = guard.spanning(|guard| match BinaryOp::parse(guard) {
            Ok(v) if v.precedence() > precedence => Ok(v),
            Ok(_) | Err(_) => Err(()),
        }) {
            let rhs: Spanned<_> = guard.spanning(|g| Self::parse_1(g, op.item.precedence()))?;

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

/// A binary expression.
#[derive(Debug, Clone, PartialEq)]
pub struct BinaryExpr {
    /// The left-hand side of the expression
    pub lhs: Box<Spanned<Expr>>,
    /// The operation to perform.
    pub op: Spanned<BinaryOp>,
    /// The right-hand side of the expression.
    pub rhs: Box<Spanned<Expr>>,
}

/// A binary operation.
#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOp {
    /// `+`.
    Add,
    /// `-`.
    Sub,
    /// `*`.
    Mul,
    /// `/`.
    Div,
    /// `%`.
    Rem,
    /// `=`.
    Eq,
    /// `!=`.
    NotEq,
    /// `<`.
    Lt,
    /// `>`.
    Gt,
    /// `<=`.
    Le,
    /// `>=`.
    Ge,
    /// `>>`.
    Shr,
    /// `<<`.
    Shl,
    /// `&&`.
    And,
    /// `||`.
    Or,
    /// `&`.
    BitAnd,
    /// `|`.
    BitOr,
    /// `^`.
    BitXor,
}

impl Parse for BinaryOp {
    fn is_ok(&self) -> bool {
        true
    }

    fn parse<'diag, 'source, 'index, 'a>(
        mut guard: crate::ParseGuard<'diag, 'source, 'index, 'a>,
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
    /// The precedence of this operation. The order is as follows: \
    /// 1. `*,/,%` \
    /// 2. `+,-` \
    /// 3. `<<,>>` \
    /// 4. `<,<=,>,>=` \
    /// 5. `==,!=,&&,&,||,|,^`
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

/// A base expression.
#[derive(Debug, Clone, PartialEq)]
pub enum BaseExpr {
    /// A literal.
    Literal(Literal),
    /// A path.
    Path(Node<Path>),
    // can't avoid the duplicate span
    /// A block.
    Block(Node<Block>),
    /// A function call.
    FnCall(FnCall),
    /// A parenthesized expression.
    Parenthesized(Box<Expr>),
    /// A conditional expression (if, else if, else).
    Conditional(ConditionalExpr),
    /// A loop expression.
    Loop(Node<Block>),
    /// `continue`.
    Continue,
    /// `break`. This may contain an expression.
    Break(Option<Box<Spanned<Expr>>>),
    /// A return expression.
    Return(Option<Box<Spanned<Expr>>>),
}

impl Parse for BaseExpr {
    fn is_ok(&self) -> bool {
        match self {
            Self::Literal(v) => v.is_ok(),
            Self::Path(v) => v.is_ok(),
            Self::Block(v) | Self::Loop(v) => v.is_ok(),
            Self::FnCall(v) => v.object.item.is_ok() && v.args.iter().all(|x| x.item.is_ok()),
            Self::Parenthesized(v) => v.is_ok(),
            Self::Conditional(v) => v.is_ok(),
            Self::Continue => true,
            Self::Break(v) | Self::Return(v) => v.as_ref().is_none_or(|x| x.item.is_ok()),
        }
    }

    fn parse<'diag, 'source, 'index, 'a>(
        mut guard: crate::ParseGuard<'diag, 'source, 'index, 'a>,
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
                        guard
                            .spanning(ConditionalExpr::parse)
                            .map(|x| x.map(Self::Conditional))
                    })
                    .or_else(|_| guard.spanning(parse_kw_with_expr(*BREAK, BaseExpr::Break)))
                    .or_else(|_| guard.spanning(parse_kw_with_expr(*RETURN, BaseExpr::Return)))
                    .or_else(|_| guard.spanning(parse_continue))
                    .or_else(|_| guard.spanning(parse_loop))
                    .or_else(|_| {
                        guard
                            .spanning(|mut g| match g.with(Path::parse) {
                                Ok(v) if v.is_ok() => Ok(v),
                                Ok(_) => Err(ParseError::InternalParseError),
                                Err(e) => Err(e),
                            })
                            .map(|sp| Spanned {
                                item: BaseExpr::Path(Node::new(sp.span, sp.item)),
                                span: sp.span,
                            })
                    })
                    .or_else(|_| {
                        guard
                            .spanning(Block::parse)
                            .map(|x| Spanned::new(x.span, Self::Block(Node::from(x))))
                    })?,
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

fn parse_loop(mut guard: ParseGuard) -> ParseResult<BaseExpr> {
    let kw = guard.next_require(TokenKind::Ident)?;

    if kw.item.symbol != *LOOP {
        return Err(ParseError::ExpectedKw(*LOOP, kw.span));
    }

    let block = guard.spanning(Block::parse)?;

    Ok(BaseExpr::Loop(Node::from(block)))
}

fn parse_continue(mut guard: ParseGuard) -> ParseResult<BaseExpr> {
    let kw = guard.next_require(TokenKind::Ident)?;

    if kw.item.symbol != *CONTINUE {
        return Err(ParseError::ExpectedKw(*LOOP, kw.span));
    }

    Ok(BaseExpr::Continue)
}

fn parse_kw_with_expr(
    kw: Intern<str>,
    mapper: impl Fn(Option<Box<Spanned<Expr>>>) -> BaseExpr,
) -> impl Fn(ParseGuard) -> ParseResult<BaseExpr> {
    move |mut guard| {
        let next = guard.next_require(TokenKind::Ident)?;

        if next.item.symbol != kw {
            return Err(ParseError::ExpectedKw(kw, next.span));
        }

        let expr = guard.spanning(Expr::parse).ok().map(Box::new);

        Ok(mapper(expr))
    }
}

/// Literal expressions.
pub mod literal {
    use diagnostic::Diagnostic;
    use lexer::{Intern, TokenKind};
    use span::Span;

    use crate::{Parse, ParseError};

    /// Literals.
    #[derive(Debug, Clone, PartialEq)]
    pub enum Literal {
        /// An integer.
        Int(IntegerLiteral),
        /// A string.
        Str(StringLiteral),
    }

    impl Parse for Literal {
        fn is_ok(&self) -> bool {
            match self {
                Self::Int(v) => v.is_ok(),
                Self::Str(v) => v.is_ok(),
            }
        }

        fn parse<'diag, 'source, 'index, 'a>(
            guard: crate::ParseGuard<'diag, 'source, 'index, 'a>,
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

    /// An integer.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum IntegerLiteral {
        /// A successfull integer parse.
        Ok(u128),
        /// The integer would have overflowed.
        Overflow,
    }

    impl Parse for IntegerLiteral {
        fn is_ok(&self) -> bool {
            matches!(self, Self::Ok(..))
        }

        fn parse<'diag, 'source, 'index, 'a>(
            mut guard: crate::ParseGuard<'diag, 'source, 'index, 'a>,
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
                    guard.source_idx,
                ));

                return Ok(Self::Overflow);
            };

            Ok(Self::Ok(int))
        }
    }

    /// A string literal.
    #[derive(Debug, Clone, PartialEq)]
    pub enum StringLiteral {
        /// The string literal.
        Ok(Intern<str>),
        /// The literal contained an invalid escape.
        InvalidEsc,
    }

    impl Parse for StringLiteral {
        fn is_ok(&self) -> bool {
            matches!(self, Self::Ok(..))
        }

        fn parse<'diag, 'source, 'index, 'a>(
            mut guard: crate::ParseGuard<'diag, 'source, 'index, 'a>,
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
                                    guard.source_idx,
                                ));
                                is_fail = true;
                                continue;
                            }
                            other => {
                                guard.diagnostics.push(Diagnostic::error(
                                    token.span,
                                    format!("'{other}' is not an escape character"),
                                    None,
                                    guard.source_idx,
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

/// A block expression.
#[derive(Debug, PartialEq, Clone)]
pub struct Block {
    /// The statements.
    pub stmts: Vec<Spanned<Stmt>>,
    /// The tail expression.
    pub tail: Option<Box<Spanned<Expr>>>,
}

impl Parse for Block {
    fn is_ok(&self) -> bool {
        self.stmts.iter().all(|x| x.item.is_ok())
            && self.tail.as_ref().is_none_or(|x| x.item.is_ok())
    }

    fn parse<'diag, 'source, 'index, 'a>(
        mut guard: ParseGuard<'diag, 'source, 'index, 'a>,
    ) -> ParseResult<Self> {
        guard.next_require(TokenKind::LCurly)?;
        let mut stmts = vec![];
        loop {
            if let Ok(b) = guard.spanning(Binding::parse) {
                stmts.push(b.map(Stmt::Binding));
                continue;
            }

            let tail = if let Ok(mut expr) = guard.spanning(Expr::parse) {
                if let Ok(v) = guard.next_require(TokenKind::Semicolon) {
                    expr.span = expr.span.extend(v.span);
                    stmts.push(expr.map(Stmt::ExprSemi));
                    continue;
                } else {
                    Some(Box::new(expr))
                }
            } else {
                None
            };

            guard.next_require(TokenKind::RCurly)?;

            return Ok(Self { stmts, tail });
        }
    }
}

/// A function call.
#[derive(Clone, PartialEq, Debug)]
pub struct FnCall {
    /// The object. This is usually a path.
    pub object: Box<Spanned<BaseExpr>>,
    /// The arguments.
    pub args: Vec<Spanned<Expr>>,
}

/// A conditional expression (if, else if, else).
#[derive(Clone, PartialEq, Debug)]
pub struct ConditionalExpr {
    /// The main expression (if).
    pub main: Spanned<ConditionalBlock>,
    /// Alternative expression (else if).
    pub alternatives: Vec<Spanned<ConditionalBlock>>,
    /// The fallback expression (else).
    pub fallback: Option<Node<Block>>,
}

impl Parse for ConditionalExpr {
    fn is_ok(&self) -> bool {
        self.main.item.is_ok()
            && self.alternatives.iter().all(|x| x.item.is_ok())
            && self.fallback.as_ref().is_none_or(|x| x.item.is_ok())
    }

    fn parse<'diag, 'source, 'index, 'a>(
        mut guard: ParseGuard<'diag, 'source, 'index, 'a>,
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

            guard.spanning(Block::parse).ok().map(Into::into)
        };

        Ok(Self {
            main,
            alternatives,
            fallback,
        })
    }
}

/// A conditional block.
#[derive(Clone, PartialEq, Debug)]
pub struct ConditionalBlock {
    /// The condition.
    pub condition: Box<Spanned<Expr>>,
    /// The expression to execute, should the condition be true.
    pub block: Node<Block>,
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

    fn parse<'diag, 'source, 'index, 'a>(
        mut guard: ParseGuard<'diag, 'source, 'index, 'a>,
    ) -> ParseResult<Self> {
        let kw = guard.next_require(TokenKind::Ident)?;
        if kw.item.symbol != *IF {
            return Err(ParseError::ExpectedKw(kw.item.symbol, kw.span));
        }

        guard.next_require(TokenKind::LParen)?;
        let condition = Box::new(guard.spanning(Expr::parse)?);
        guard.next_require(TokenKind::RParen)?;

        let block = guard.spanning(Block::parse)?.into();

        Ok(Self { condition, block })
    }
}

// --- tests ---
#[cfg(test)]
mod tests {
    use lexer::Intern;
    use node::Node;
    use span::{Span, Spanned};

    use crate::{
        assert_eq,
        expr::{
            BaseExpr, BinaryExpr, BinaryOp, Block, ConditionalBlock, ConditionalExpr, Expr, FnCall,
            literal::{IntegerLiteral, Literal, StringLiteral},
        },
        item::Type,
        path::Path,
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
            "{decl x: int = 5; x + y}",
            Spanned::new(
                Span::new(0, 24),
                Expr::Base(BaseExpr::Block(Node::new(
                    Span::new(0, 24),
                    Block {
                        stmts: vec![Spanned::new(
                            Span::new(1, 17),
                            Stmt::Binding(Binding {
                                is_mutable: None,
                                ident: Spanned::new(Span::new(6, 7), Intern::from("x")),
                                ty: Some(Spanned::new(
                                    Span::new(9, 12),
                                    Type {
                                        path: Path::single(Spanned::new(
                                            Span::new(9, 12),
                                            Intern::from("int"),
                                        ))
                                        .into(),
                                        generics: None,
                                    },
                                )),
                                value: Spanned::new(
                                    Span::new(15, 16),
                                    Expr::Base(BaseExpr::Literal(Literal::Int(
                                        IntegerLiteral::Ok(5),
                                    ))),
                                ),
                            }),
                        )],
                        tail: Some(Box::new(Spanned::new(
                            Span::new(18, 23),
                            Expr::Binary(BinaryExpr {
                                lhs: Box::new(Spanned::new(
                                    Span::new(18, 19),
                                    Expr::Base(BaseExpr::Path(
                                        Path::single(Spanned::new(
                                            Span::new(18, 19),
                                            Intern::from("x"),
                                        ))
                                        .into(),
                                    )),
                                )),
                                op: Spanned::new(Span::new(20, 21), BinaryOp::Add),
                                rhs: Box::new(Spanned::new(
                                    Span::new(22, 23),
                                    Expr::Base(BaseExpr::Path(
                                        Path::single(Spanned::new(
                                            Span::new(22, 23),
                                            Intern::from("y"),
                                        ))
                                        .into(),
                                    )),
                                )),
                            }),
                        ))),
                    },
                ))),
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
                            Path::single(Spanned::new(Span::new(0, 3), Intern::from("add"))).into(),
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
                            block: Node::new(
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

    #[test]
    fn parse_loop() {
        assert_eq(
            "loop { print(\"x\") }",
            Spanned::new(
                Span::new(0, 19),
                Expr::Base(BaseExpr::Loop(Node::new(
                    Span::new(5, 19),
                    Block {
                        stmts: vec![],
                        tail: Some(Box::new(Spanned::new(
                            Span::new(7, 17),
                            Expr::Base(BaseExpr::FnCall(FnCall {
                                object: Box::new(Spanned::new(
                                    Span::new(7, 12),
                                    BaseExpr::Path(
                                        Path::single(Spanned::new(
                                            Span::new(7, 12),
                                            Intern::from("print"),
                                        ))
                                        .into(),
                                    ),
                                )),
                                args: vec![Spanned::new(
                                    Span::new(13, 16),
                                    Expr::Base(BaseExpr::Literal(Literal::Str(StringLiteral::Ok(
                                        Intern::from("x"),
                                    )))),
                                )],
                            })),
                        ))),
                    },
                ))),
            ),
        )
    }

    #[test]
    fn parse_continue() {
        assert_eq(
            "continue",
            Spanned::new(Span::new(0, 8), Expr::Base(BaseExpr::Continue)),
        );
    }

    #[test]
    fn parse_break() {
        assert_eq(
            "break",
            Spanned::new(Span::new(0, 5), Expr::Base(BaseExpr::Break(None))),
        )
    }

    #[test]
    fn parse_break_with_expr() {
        assert_eq(
            "break 5",
            Spanned::new(
                Span::new(0, 7),
                Expr::Base(BaseExpr::Break(Some(Box::new(Spanned::new(
                    Span::new(6, 7),
                    Expr::Base(BaseExpr::Literal(Literal::Int(IntegerLiteral::Ok(5)))),
                ))))),
            ),
        )
    }

    #[test]
    fn parse_empty_block() {
        assert_eq(
            "{}",
            Spanned::new(
                Span::new(0, 2),
                Expr::Base(BaseExpr::Block(Node::new(
                    Span::new(0, 2),
                    Block {
                        stmts: vec![],
                        tail: None,
                    },
                ))),
            ),
        )
    }
}
