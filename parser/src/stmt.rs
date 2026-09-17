//! Statements.
//!
//! Statements are executable pieces of code that do not evaluate to a value.

use diagnostic::Diagnostic;
use lexer::{Intern, TokenKind};
use node::Node;
use span::{Span, Spanned};

use crate::{
    DECL, Parse, ParseError,
    expr::Expr,
    item::{Item, Type},
};

/// A statement.
#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    /// A binding.
    Binding(Binding),
    /// An expression.
    ExprSemi(Expr),
    /// An item.
    Item(Node<Item>),
}

impl Parse for Stmt {
    fn is_ok(&self) -> bool {
        match self {
            Self::Binding(v) => v.is_ok(),
            Self::ExprSemi(v) => v.is_ok(),
            Self::Item(_) => true,
        }
    }

    fn parse<'diag, 'source, 'index, 'a>(
        mut guard: crate::ParseGuard<'diag, 'source, 'index, 'a>,
    ) -> crate::ParseResult<Self> {
        guard.with(Item::parse).map(Self::Item).or_else(|_| {
            guard.with(Binding::parse).map(Self::Binding).or_else(|_| {
                guard.with(|mut g| {
                    let expr = g.with(Expr::parse)?;
                    g.next_require(TokenKind::Semicolon)?;
                    Ok(Self::ExprSemi(expr))
                })
            })
        })
    }
}

/// A binding.
#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    /// Whether the binding is mutable or not.
    pub is_mutable: Option<Span>,
    /// The name of this binding.
    pub ident: Spanned<Intern<str>>,
    /// The type of the binding.
    pub ty: Option<Spanned<Type>>,
    /// The value of it.
    pub value: Spanned<Expr>,
}

impl Parse for Binding {
    fn is_ok(&self) -> bool {
        true
    }

    fn parse<'diag, 'source, 'index, 'a>(
        mut guard: crate::ParseGuard<'diag, 'source, 'index, 'a>,
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
                        guard.source_idx,
                    ));
                }

                None
            }
            Err(e) => return Err(e),
        };

        let ident = guard.next_require(TokenKind::Ident)?.map(|x| x.symbol);
        let ty = guard
            .next_require(TokenKind::Colon)
            .and_then(|_| guard.spanning(Type::parse))
            .map(Some)?;

        guard.next_require(TokenKind::Equal)?;

        let value = guard.spanning(Expr::parse)?;
        guard.next_require(TokenKind::Semicolon)?;

        Ok(Self {
            is_mutable,
            ident,
            ty,
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
        item::Type,
        path::Path,
        stmt::{Binding, Stmt},
    };

    #[test]
    fn parse_binding() {
        assert_eq(
            "decl x: int = 5;",
            Spanned::new(
                Span::new(0, 16),
                Stmt::Binding(Binding {
                    is_mutable: None,
                    ident: Spanned::new(Span::new(5, 6), Intern::from("x")),
                    ty: Some(Spanned::new(
                        Span::new(8, 11),
                        Type {
                            path: Path::single(Spanned::new(Span::new(8, 11), Intern::from("int")))
                                .into(),
                            generics: None,
                        },
                    )),
                    value: Spanned::new(
                        Span::new(14, 15),
                        Expr::Base(BaseExpr::Literal(Literal::Int(IntegerLiteral::Ok(5)))),
                    ),
                }),
            ),
        );
    }

    #[test]
    fn parse_mut_binding() {
        assert_eq(
            "decl ~x: int = 5;",
            Spanned::new(
                Span::new(0, 17),
                Stmt::Binding(Binding {
                    is_mutable: Some(Span::new(5, 6)),
                    ident: Spanned::new(Span::new(6, 7), Intern::from("x")),
                    ty: Some(Spanned::new(
                        Span::new(9, 12),
                        Type {
                            path: Path::single(Spanned::new(Span::new(9, 12), Intern::from("int")))
                                .into(),
                            generics: None,
                        },
                    )),
                    value: Spanned::new(
                        Span::new(15, 16),
                        Expr::Base(BaseExpr::Literal(Literal::Int(IntegerLiteral::Ok(5)))),
                    ),
                }),
            ),
        )
    }
}
