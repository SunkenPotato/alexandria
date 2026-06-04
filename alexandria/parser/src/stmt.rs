use diagnostic::Diagnostic;
use lexer::{Intern, TokenKind};
use span::{Span, Spanned};

use crate::{DECL, Parse, ParseError, expr::Expr};

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Binding(Binding),
    ExprSemi(Expr),
}

impl Parse for Stmt {
    fn is_ok(&self) -> bool {
        match self {
            Self::Binding(v) => v.is_ok(),
            Self::ExprSemi(v) => v.is_ok(),
        }
    }

    fn parse<'diag, 'source, 'index>(
        mut guard: crate::ParseGuard<'diag, 'source, 'index>,
    ) -> crate::ParseResult<Self> {
        guard.with(Binding::parse).map(Self::Binding).or_else(|_| {
            guard
                .with(|mut g| {
                    let expr = g.with(Expr::parse);
                    g.next_require(TokenKind::Semicolon)?;
                    Ok(expr)
                })?
                .map(Self::ExprSemi)
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    pub is_mutable: Option<Span>,
    pub ident: Spanned<Intern<str>>,
    pub value: Spanned<Expr>,
}

impl Parse for Binding {
    fn is_ok(&self) -> bool {
        true
    }

    fn parse<'diag, 'source, 'index>(
        mut guard: crate::ParseGuard<'diag, 'source, 'index>,
    ) -> crate::ParseResult<Self> {
        let decl = guard.next_require(TokenKind::Ident)?;
        if decl.item.symbol != *DECL {
            return Err(ParseError::ExpectedKw(*DECL, decl.span));
        }

        let is_mutable = match guard.next_require(TokenKind::Tilde) {
            Ok(v) => Some(v.span),
            Err(ParseError::TokenMismatch { .. }) => {
                let this = guard.peek().unwrap();
                if this.item.kind != TokenKind::Ident {
                    guard.diagnostics.push(Diagnostic::error(
                        this.span,
                        format!("expected `~` or Ident, got {:?}", this.item.kind),
                        None,
                    ));
                }

                None
            }
            Err(e) => return Err(e),
        };

        let ident = guard.next_require(TokenKind::Ident)?.map(|x| x.symbol);

        guard.next_require(TokenKind::Equal)?;

        let value = guard.spanning(Expr::parse)?;
        guard.next_require(TokenKind::Semicolon)?;

        Ok(Self {
            is_mutable,
            ident,
            value,
        })
    }
}

#[cfg(test)]
mod tests {
    use lexer::Intern;
    use span::{Span, Spanned};

    use crate::{
        assert_eq,
        expr::{
            BaseExpr, Expr,
            literal::{IntegerLiteral, Literal},
        },
        stmt::{Binding, Stmt},
    };

    #[test]
    fn parse_binding() {
        assert_eq(
            "decl x = 5;",
            Spanned::new(
                Span::new(0, 11),
                Stmt::Binding(Binding {
                    is_mutable: None,
                    ident: Spanned::new(Span::new(5, 6), Intern::from("x")),
                    value: Spanned::new(
                        Span::new(9, 10),
                        Expr::Base(BaseExpr::Literal(Literal::Int(IntegerLiteral::Ok(5)))),
                    ),
                }),
            ),
        );
    }

    #[test]
    fn parse_mut_binding() {
        assert_eq(
            "decl ~x = 5;",
            Spanned::new(
                Span::new(0, 12),
                Stmt::Binding(Binding {
                    is_mutable: Some(Span::new(5, 6)),
                    ident: Spanned::new(Span::new(6, 7), Intern::from("x")),
                    value: Spanned::new(
                        Span::new(10, 11),
                        Expr::Base(BaseExpr::Literal(Literal::Int(IntegerLiteral::Ok(5)))),
                    ),
                }),
            ),
        )
    }
}
