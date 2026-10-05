//! Expression parsers.

use lexer::{Intern, TokenKind};
use node::Node;
use span::{Span, Spanned};

use crate::{
    BREAK, CONTINUE, DECL, ELSE, IF, LOOP, Parse, ParseError, ParseGuard, ParseResult, RETURN,
    expr::literal::Literal, item::Item, path::Path, stmt::Stmt,
};

/// An expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    /// A binary expression (e.g., a + b)
    Binary(BinaryExpr),
    /// A base expression.
    Base(BaseExpr),
    /// An assignment expression.
    Assignment(Assignment),
}

impl Parse for Expr {
    fn parse<'source, 'index>(mut guard: ParseGuard<'source, 'index>) -> ParseResult<Self> {
        let base = guard.spanning(|guard| Self::parse_1(guard, 0))?;

        if guard.peek_kind(TokenKind::Equal)
            && guard
                .peek_n(1)
                .is_ok_and(|x| x.item.kind != TokenKind::Equal)
        {
            guard.next()?;
            let value = Box::new(guard.spanning(Expr::parse)?);

            Ok(Self::Assignment(Assignment {
                object: Box::new(base),
                value,
            }))
        } else {
            Ok(base.item)
        }
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

/// Check whether a token of the given kind may start an expression.
fn starts_expr(kind: TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::LParen
            | TokenKind::Integer
            | TokenKind::StringLit
            | TokenKind::LCurly
            | TokenKind::Ident
    )
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
    /// `==`.
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
    /// Parse an operator. Operators made up of two tokens (e.g., `<=`) must not contain
    /// whitespace.
    fn parse<'source, 'index>(mut guard: ParseGuard<'source, 'index>) -> ParseResult<Self> {
        let next = guard.next()?;
        let mut adjacent = |kind| guard.next_adjacent(next.span, kind).is_some();

        let op = match next.item.kind {
            TokenKind::Plus => Self::Add,
            TokenKind::Minus => Self::Sub,
            TokenKind::Asterisk => Self::Mul,
            TokenKind::Slash => Self::Div,
            TokenKind::Percent => Self::Rem,
            TokenKind::Equal if adjacent(TokenKind::Equal) => Self::Eq,
            TokenKind::Bang if adjacent(TokenKind::Equal) => Self::NotEq,
            TokenKind::LessThan if adjacent(TokenKind::LessThan) => Self::Shl,
            TokenKind::LessThan if adjacent(TokenKind::Equal) => Self::Le,
            TokenKind::LessThan => Self::Lt,
            TokenKind::GreaterThan if adjacent(TokenKind::GreaterThan) => Self::Shr,
            TokenKind::GreaterThan if adjacent(TokenKind::Equal) => Self::Ge,
            TokenKind::GreaterThan => Self::Gt,
            TokenKind::Ampersand if adjacent(TokenKind::Ampersand) => Self::And,
            TokenKind::Ampersand => Self::BitAnd,
            TokenKind::Pipe if adjacent(TokenKind::Pipe) => Self::Or,
            TokenKind::Pipe => Self::BitOr,
            TokenKind::Caret => Self::BitXor,
            TokenKind::Equal | TokenKind::Bang => {
                return Err(ParseError::TokenMismatch(
                    smallvec::smallvec![TokenKind::Equal],
                    Span::new(next.span.stop(), next.span.stop()),
                ));
            }
            _ => {
                return Err(ParseError::TokenMismatch(
                    smallvec::smallvec![
                        TokenKind::Plus,
                        TokenKind::Minus,
                        TokenKind::Asterisk,
                        TokenKind::Slash,
                        TokenKind::Percent,
                        TokenKind::LessThan,
                    ],
                    next.span,
                ));
            }
        };

        Ok(op)
    }
}

impl BinaryOp {
    /// The precedence of this operation. From the tightest to the loosest binding: \
    /// 1. `*`, `/`, `%` \
    /// 2. `+`, `-` \
    /// 3. `<<`, `>>` \
    /// 4. `<`, `<=`, `>`, `>=` \
    /// 5. `==`, `!=` \
    /// 6. `&` \
    /// 7. `^` \
    /// 8. `|` \
    /// 9. `&&` \
    /// 10. `||`
    ///
    /// All operators are left-associative.
    pub const fn precedence(&self) -> u8 {
        match self {
            BinaryOp::Or => 10,
            BinaryOp::And => 20,
            BinaryOp::BitOr => 30,
            BinaryOp::BitXor => 40,
            BinaryOp::BitAnd => 50,
            BinaryOp::Eq | BinaryOp::NotEq => 60,
            BinaryOp::Lt | BinaryOp::Le | BinaryOp::Ge | BinaryOp::Gt => 70,
            BinaryOp::Shr | BinaryOp::Shl => 80,
            BinaryOp::Add | BinaryOp::Sub => 90,
            BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem => 100,
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
    fn parse<'source, 'index>(mut guard: ParseGuard<'source, 'index>) -> ParseResult<Self> {
        if guard.next_require(TokenKind::LParen).is_ok() {
            let expr = Box::new(guard.with(Expr::parse)?);
            guard.next_require(TokenKind::RParen)?;
            return Ok(Self::Parenthesized(expr));
        }

        let object = guard.spanning(Self::parse_object)?;

        if guard.peek_kind(TokenKind::LParen) {
            let args = guard.parse_delimited(TokenKind::LParen, TokenKind::RParen, Expr::parse)?;

            Ok(Self::FnCall(FnCall {
                object: Box::new(object),
                args,
            }))
        } else {
            Ok(object.item)
        }
    }
}

impl BaseExpr {
    /// Parse a base expression, excluding calls and parenthesized expressions.
    fn parse_object(mut guard: ParseGuard) -> ParseResult<Self> {
        let token = guard.peek()?;

        match token.item.kind {
            TokenKind::Integer | TokenKind::StringLit => {
                guard.with(Literal::parse).map(Self::Literal)
            }
            TokenKind::LCurly => guard
                .spanning(Block::parse)
                .map(|x| Self::Block(Node::from(x))),
            TokenKind::Ident => {
                let symbol = token.item.symbol;

                if symbol == *IF {
                    guard.with(ConditionalExpr::parse).map(Self::Conditional)
                } else if symbol == *LOOP {
                    guard.with(parse_loop)
                } else if symbol == *CONTINUE {
                    guard.next()?;
                    Ok(Self::Continue)
                } else if symbol == *BREAK {
                    guard.with(parse_kw_with_expr(*BREAK, Self::Break))
                } else if symbol == *RETURN {
                    guard.with(parse_kw_with_expr(*RETURN, Self::Return))
                } else {
                    guard
                        .spanning(Path::parse)
                        .map(|x| Self::Path(Node::from(x)))
                }
            }
            _ => Err(ParseError::TokenMismatch(
                smallvec::smallvec![
                    TokenKind::LParen,
                    TokenKind::Integer,
                    TokenKind::StringLit,
                    TokenKind::LCurly,
                    TokenKind::Ident,
                ],
                token.span,
            )),
        }
    }
}

fn parse_loop(mut guard: ParseGuard) -> ParseResult<BaseExpr> {
    guard.expect_kw(*LOOP)?;
    let block = guard.spanning(Block::parse)?;

    Ok(BaseExpr::Loop(Node::from(block)))
}

fn parse_kw_with_expr(
    kw: Intern<str>,
    mapper: impl Fn(Option<Box<Spanned<Expr>>>) -> BaseExpr,
) -> impl Fn(ParseGuard) -> ParseResult<BaseExpr> {
    move |mut guard| {
        guard.expect_kw(kw)?;

        let expr = if guard.peek().is_ok_and(|x| starts_expr(x.item.kind)) {
            Some(Box::new(guard.spanning(Expr::parse)?))
        } else {
            None
        };

        Ok(mapper(expr))
    }
}

/// Literal expressions.
pub mod literal {
    use diagnostic::Diagnostic;
    use lexer::{Intern, TokenKind};
    use source::SourceIdx;
    use span::{Span, Spanned};

    use crate::{Parse, ParseError};

    /// Literals.
    #[derive(Debug, Clone, PartialEq)]
    pub enum Literal {
        /// An integer.
        Int(IntegerLiteral),
        /// A float.
        Float(FloatLiteral),
        /// A string.
        Str(StringLiteral),
    }

    impl Parse for Literal {
        fn parse<'source, 'index>(
            guard: crate::ParseGuard<'source, 'index>,
        ) -> crate::ParseResult<Self> {
            let next = guard.peek()?;
            match next.item.kind {
                TokenKind::Float => Ok(Self::Float(FloatLiteral::parse(guard)?)),
                TokenKind::Integer => Ok(Self::Int(IntegerLiteral::parse(guard)?)),
                TokenKind::StringLit => Ok(Self::Str(StringLiteral::parse(guard)?)),
                _ => Err(ParseError::TokenMismatch(
                    smallvec::smallvec![TokenKind::Integer, TokenKind::StringLit],
                    next.span,
                )),
            }
        }
    }

    fn parse_integer(
        input: Spanned<&str>,
        source_idx: SourceIdx,
    ) -> Result<Spanned<u128>, Diagnostic> {
        let value = input
            .chars()
            .filter(|x| *x != '_')
            .try_fold(0u128, |acc, digit| {
                acc.checked_mul(10)?
                    .checked_add(u128::from(digit.to_digit(10)?))
            });

        let Some(value) = value else {
            return Err(Diagnostic::error(
                input.span,
                "integer literal overflow: integer literals have a maximum value of 2^128 - 1",
                None,
                source_idx,
            ));
        };

        Ok(input.map(|_| value))
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
        fn parse<'source, 'index>(
            mut guard: crate::ParseGuard<'source, 'index>,
        ) -> crate::ParseResult<Self> {
            let token = guard.next_require(TokenKind::Integer)?;
            let int = match parse_integer(token.map(|_| &token.symbol as &str), guard.source_idx) {
                Ok(v) => v,
                Err(e) => {
                    guard.emit(e);
                    return Ok(Self::Overflow);
                }
            };

            Ok(Self::Ok(int.item))
        }
    }

    /// A float literal.
    #[derive(Debug, Clone, PartialEq)]
    pub enum FloatLiteral {
        /// A successful float parse.
        Ok {
            /// The integer part of this float (e.g., `123` in `123.456`).
            integer: Spanned<u128>,
            /// The fractional part of this float (e.g., `456` in `123.456`).
            fractional: Spanned<u128>,
        },
        /// The integer part of this float would have overflowed it's internal representation capacity.
        IntOverflow,
        /// The fraction part of this float would have overflowed it's internal representation capacity.
        FracOverflow,
    }

    impl Parse for FloatLiteral {
        fn parse<'source, 'index>(
            mut guard: crate::ParseGuard<'source, 'index>,
        ) -> crate::ParseResult<Self> {
            let token = guard.next_require(TokenKind::Float)?;
            let (int, frac) = token.symbol.split_once('.').unwrap();
            let parsed_int = match parse_integer(
                Spanned::new(
                    Span::new(token.span.start(), token.span.start() + int.len() as u32),
                    int,
                ),
                guard.source_idx,
            ) {
                Ok(r) => r,
                Err(e) => {
                    guard.emit(e);
                    return Ok(Self::IntOverflow);
                }
            };
            let parsed_frac = match parse_integer(
                Spanned::new(
                    Span::new(
                        parsed_int.span.stop() + 1,
                        parsed_int.span.stop() + 1 + frac.len() as u32,
                    ),
                    frac,
                ),
                guard.source_idx,
            ) {
                Ok(r) => r,
                Err(e) => {
                    guard.emit(e);
                    return Ok(Self::FracOverflow);
                }
            };

            Ok(Self::Ok {
                integer: parsed_int,
                fractional: parsed_frac,
            })
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
        fn parse<'source, 'index>(
            mut guard: crate::ParseGuard<'source, 'index>,
        ) -> crate::ParseResult<Self> {
            let token = guard.next_require(TokenKind::StringLit)?;
            let symbol = token.item.symbol;
            let mut buf = String::with_capacity(symbol.len());

            // skip the opening quote
            let mut iter = symbol.char_indices().skip(1);
            let mut is_fail = false;
            while let Some((i, strch)) = iter.next() {
                let to_append = match strch {
                    '\\' => {
                        // an unterminated literal; the lexer has already reported it
                        let Some((j, esc)) = iter.next() else {
                            break;
                        };

                        match esc {
                            't' => '\t',
                            'n' => '\n',
                            '0' => '\0',
                            '"' => '"',
                            '\\' => '\\',
                            other => {
                                let span = Span::new(
                                    token.span.start() + i as u32,
                                    token.span.start() + (j + other.len_utf8()) as u32,
                                );
                                let message = if other == '{' {
                                    "unicode escapes are not yet supported".to_owned()
                                } else {
                                    format!("`\\{other}` is not a valid escape sequence")
                                };

                                guard.emit(Diagnostic::error(
                                    span,
                                    message,
                                    None,
                                    guard.source_idx,
                                ));
                                is_fail = true;
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
    fn parse<'source, 'index>(mut guard: ParseGuard<'source, 'index>) -> ParseResult<Self> {
        guard.next_require(TokenKind::LCurly)?;
        let mut stmts = vec![];
        let mut tail = None;

        loop {
            match guard.next_require(TokenKind::RCurly) {
                Ok(_) => break,
                Err(e @ ParseError::Eof(..)) => return Err(e),
                Err(_) => (),
            }

            if Item::starts_item(&guard) || guard.peek_kw(*DECL) {
                stmts.push(guard.spanning(Stmt::parse)?);
                continue;
            }

            let expr = guard.spanning(Expr::parse)?;
            if let Ok(semi) = guard.next_require(TokenKind::Semicolon) {
                stmts.push(expr.extend(semi.span).map(Stmt::ExprSemi));
            } else {
                tail = Some(Box::new(expr));
                guard.next_require(TokenKind::RCurly)?;
                break;
            }
        }

        Ok(Self { stmts, tail })
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
    fn parse<'source, 'index>(mut guard: ParseGuard<'source, 'index>) -> ParseResult<Self> {
        let main = guard.spanning(ConditionalBlock::parse)?;
        let mut alternatives = vec![];
        let mut fallback = None;

        while guard.peek_kw(*ELSE) {
            guard.next()?;

            if guard.peek_kw(*IF) {
                alternatives.push(guard.spanning(ConditionalBlock::parse)?);
            } else {
                fallback = Some(guard.spanning(Block::parse)?.into());
                break;
            }
        }

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

impl Parse for ConditionalBlock {
    fn parse<'source, 'index>(mut guard: ParseGuard<'source, 'index>) -> ParseResult<Self> {
        guard.expect_kw(*IF)?;

        guard.next_require(TokenKind::LParen)?;
        let condition = Box::new(guard.spanning(Expr::parse)?);
        guard.next_require(TokenKind::RParen)?;

        let block = guard.spanning(Block::parse)?.into();

        Ok(Self { condition, block })
    }
}

/// An assignment of the form `$ident = $expr`.
#[derive(Clone, PartialEq, Debug)]
pub struct Assignment {
    /// The identifier.
    pub object: Box<Spanned<Expr>>,
    /// The expression. Note: `x = y = 2` does not mean that `x` and `y` are both equal to `2`. It means that `x` will be equal
    /// to the value of an assignment, which is always `nil` (`y` will be equal to `2`).
    pub value: Box<Spanned<Expr>>,
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
            Assignment, BaseExpr, BinaryExpr, BinaryOp, Block, ConditionalBlock, ConditionalExpr,
            Expr, FnCall,
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
    fn parse_float() {
        assert_eq(
            "3.141",
            Spanned::new(
                Span::new(0, 5),
                Literal::Float(super::literal::FloatLiteral::Ok {
                    integer: Spanned::new(Span::new(0, 1), 3),
                    fractional: Spanned::new(Span::new(2, 5), 141),
                }),
            ),
        )
    }

    #[test]
    fn parse_float_frac_overflow() {
        assert_eq(
            "1.340282366920938463463374607431768211456",
            Spanned::new(
                Span::new(0, 41),
                Literal::Float(super::literal::FloatLiteral::FracOverflow),
            ),
        )
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

    #[test]
    fn parse_assignment() {
        assert_eq(
            "x = 5",
            Spanned::new(
                Span::new(0, 5),
                Expr::Assignment(Assignment {
                    object: Box::new(Spanned::new(
                        Span::new(0, 1),
                        Expr::Base(BaseExpr::Path(
                            Path::single(Spanned::new(Span::new(0, 1), Intern::from("x"))).into(),
                        )),
                    )),
                    value: Box::new(Spanned::new(
                        Span::new(4, 5),
                        Expr::Base(BaseExpr::Literal(Literal::Int(IntegerLiteral::Ok(5)))),
                    )),
                }),
            ),
        )
    }

    // make sure that this doesn't trigger the assignment parser
    #[test]
    fn equality_is_not_assignment() {
        assert_eq(
            "x == 5",
            Spanned::new(
                Span::new(0, 6),
                Expr::Binary(BinaryExpr {
                    lhs: Box::new(Spanned::new(
                        Span::new(0, 1),
                        Expr::Base(BaseExpr::Path(
                            Path::single(Spanned::new(Span::new(0, 1), Intern::from("x"))).into(),
                        )),
                    )),
                    op: Spanned::new(Span::new(2, 4), BinaryOp::Eq),
                    rhs: Box::new(Spanned::new(
                        Span::new(5, 6),
                        Expr::Base(BaseExpr::Literal(Literal::Int(IntegerLiteral::Ok(5)))),
                    )),
                }),
            ),
        )
    }

    // ensure this doesn't parse as as a + (b = 5)
    #[test]
    fn binary_expr_with_assignment() {
        assert_eq(
            "a + b = 5",
            Spanned::new(
                Span::new(0, 9),
                Expr::Assignment(Assignment {
                    object: Box::new(Spanned::new(
                        Span::new(0, 5),
                        Expr::Binary(BinaryExpr {
                            lhs: Box::new(Spanned::new(
                                Span::new(0, 1),
                                Expr::Base(BaseExpr::Path(
                                    Path::single(Spanned::new(Span::new(0, 1), Intern::from("a")))
                                        .into(),
                                )),
                            )),
                            op: Spanned::new(Span::new(2, 3), BinaryOp::Add),
                            rhs: Box::new(Spanned::new(
                                Span::new(4, 5),
                                Expr::Base(BaseExpr::Path(
                                    Path::single(Spanned::new(Span::new(4, 5), Intern::from("b")))
                                        .into(),
                                )),
                            )),
                        }),
                    )),
                    value: Box::new(Spanned::new(
                        Span::new(8, 9),
                        Expr::Base(BaseExpr::Literal(Literal::Int(IntegerLiteral::Ok(5)))),
                    )),
                }),
            ),
        )
    }
}

#[cfg(test)]
mod regression_tests {
    use lexer::Intern;
    use span::{Span, Spanned};

    use crate::{
        Parse, ParseError, assert_eq_custom_parser,
        expr::{
            BaseExpr, Block, Expr, FnCall,
            literal::{IntegerLiteral, Literal, StringLiteral},
        },
        parse_err,
        path::Path,
        stmt::{Binding, Stmt},
    };

    fn int(span: Span, v: u128) -> Box<Spanned<Expr>> {
        Box::new(Spanned::new(
            span,
            Expr::Base(BaseExpr::Literal(Literal::Int(IntegerLiteral::Ok(v)))),
        ))
    }

    fn path(span: Span, name: &str) -> BaseExpr {
        BaseExpr::Path(Path::single(Spanned::new(span, Intern::from(name))).into())
    }

    /// Render the structure of an expression, e.g. `((1 == 2) && (3 == 4))`.
    fn shape(expr: &Expr) -> String {
        match expr {
            Expr::Binary(b) => format!("({} {:?} {})", shape(&b.lhs), b.op.item, shape(&b.rhs)),
            Expr::Base(BaseExpr::Literal(Literal::Int(IntegerLiteral::Ok(v)))) => v.to_string(),
            Expr::Base(other) => format!("{other:?}"),
            Expr::Assignment(assignment) => format!("{assignment:?}"),
        }
    }

    fn parse_shape(input: &str) -> String {
        let mut out = None;
        assert_eq_custom_parser(
            |g| {
                let e = Expr::parse(g)?;
                out = Some(shape(&e));
                Ok::<_, ParseError>(())
            },
            input,
            (),
            None,
        );
        out.unwrap()
    }

    #[test]
    fn zero_arg_call() {
        assert_eq_custom_parser(
            Expr::parse,
            "f()",
            Expr::Base(BaseExpr::FnCall(FnCall {
                object: Box::new(Spanned::new(Span::new(0, 1), path(Span::new(0, 1), "f"))),
                args: vec![],
            })),
            Some(Span::new(0, 3)),
        );
    }

    #[test]
    fn call_with_trailing_comma() {
        assert_eq_custom_parser(
            Expr::parse,
            "f(1,)",
            Expr::Base(BaseExpr::FnCall(FnCall {
                object: Box::new(Spanned::new(Span::new(0, 1), path(Span::new(0, 1), "f"))),
                args: vec![*int(Span::new(2, 3), 1)],
            })),
            Some(Span::new(0, 5)),
        );
    }

    #[test]
    fn precedence_levels() {
        assert_eq!(parse_shape("1 == 2 && 3 == 4"), "((1 Eq 2) And (3 Eq 4))");
        assert_eq!(parse_shape("1 || 2 && 3"), "(1 Or (2 And 3))");
        assert_eq!(
            parse_shape("1 | 2 ^ 3 & 4"),
            "(1 BitOr (2 BitXor (3 BitAnd 4)))"
        );
        assert_eq!(parse_shape("1 & 2 == 3"), "(1 BitAnd (2 Eq 3))");
        assert_eq!(parse_shape("1 - 2 - 3"), "((1 Sub 2) Sub 3)");
    }

    #[test]
    fn two_token_operators_must_be_adjacent() {
        assert_eq!(parse_shape("1 <= 2"), "(1 Le 2)");
        let (err, _) = parse_err(Expr::parse, "1 < < 2");
        assert!(matches!(err, ParseError::TokenMismatch(..)), "{err:?}");
    }

    #[test]
    fn integer_with_separators() {
        assert_eq_custom_parser(
            Literal::parse,
            "1_000",
            Literal::Int(IntegerLiteral::Ok(1000)),
            None,
        );
    }

    #[test]
    fn string_with_trailing_backslash_does_not_panic() {
        // the lexer reports the unclosed literal; the parser must not panic on it
        assert_eq_custom_parser(
            Literal::parse,
            r#""ab\"#,
            Literal::Str(StringLiteral::Ok(Intern::from("ab"))),
            None,
        );
    }

    #[test]
    fn string_with_unknown_escape_is_invalid() {
        assert_eq_custom_parser(
            Literal::parse,
            r#""\q""#,
            Literal::Str(StringLiteral::InvalidEsc),
            None,
        );
    }

    #[test]
    fn non_ascii_string() {
        assert_eq_custom_parser(
            Literal::parse,
            "\"é\\n\"",
            Literal::Str(StringLiteral::Ok(Intern::from("é\n"))),
            Some(Span::new(0, 6)),
        );
    }

    #[test]
    fn conditional_does_not_consume_following_ident() {
        // previously `x` was silently swallowed while looking for `else`
        let (err, _) = parse_err(Block::parse, "{ if (1) { 2 } x }");
        assert!(
            matches!(err, ParseError::TokenMismatch(_, span) if span == Span::new(15, 16)),
            "{err:?}"
        );
    }

    #[test]
    fn else_if_else_chain() {
        let mut out = None;
        assert_eq_custom_parser(
            |g| {
                let e = Expr::parse(g)?;
                out = Some(e);
                Ok::<_, ParseError>(())
            },
            "if (1) { 1 } else if (2) { 2 } else { 3 }",
            (),
            None,
        );

        let Some(Expr::Base(BaseExpr::Conditional(cond))) = out else {
            panic!("expected a conditional");
        };
        assert_eq!(cond.alternatives.len(), 1);
        assert!(cond.fallback.is_some());
    }

    #[test]
    fn binding_without_type() {
        assert_eq_custom_parser(
            Stmt::parse,
            "decl x = 5;",
            Stmt::Binding(Binding {
                is_mutable: None,
                ident: Spanned::new(Span::new(5, 6), Intern::from("x")),
                ty: None,
                value: Spanned::new(
                    Span::new(9, 10),
                    Expr::Base(BaseExpr::Literal(Literal::Int(IntegerLiteral::Ok(5)))),
                ),
            }),
            Some(Span::new(0, 11)),
        );
    }

    #[test]
    fn block_with_item() {
        let mut out = None;
        assert_eq_custom_parser(
            |g| {
                let b = Block::parse(g)?;
                out = Some(b);
                Ok::<_, ParseError>(())
            },
            "{ func inner {} { } inner() }",
            (),
            None,
        );

        let block = out.unwrap();
        assert!(matches!(block.stmts[0].item, Stmt::Item(_)));
        assert!(block.tail.is_some());
    }

    #[test]
    fn return_without_value_before_brace() {
        assert_eq_custom_parser(
            Block::parse,
            "{ return }",
            Block {
                stmts: vec![],
                tail: Some(Box::new(Spanned::new(
                    Span::new(2, 8),
                    Expr::Base(BaseExpr::Return(None)),
                ))),
            },
            None,
        );
    }

    #[test]
    fn syntax_error_points_at_offending_token() {
        let (err, rendered) = parse_err(Block::parse, "{ 1 + ; }");
        assert!(matches!(err, ParseError::TokenMismatch(_, span) if span == Span::new(6, 7)));
        assert!(
            rendered[0].starts_with("error: expected one of"),
            "{rendered:?}"
        );
    }

    #[test]
    fn keyword_as_binding_name() {
        let (err, _) = parse_err(Stmt::parse, "decl loop = 1;");
        assert!(matches!(err, ParseError::KwAsIdent(..)), "{err:?}");
    }

    #[test]
    fn wrong_keyword_is_reported_as_expected() {
        let (err, _) = parse_err(Expr::parse, "if 1");
        assert!(matches!(err, ParseError::TokenMismatch(..)), "{err:?}");
    }
}
