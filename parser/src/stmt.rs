//! Statements.
//!
//! Statements are executable pieces of code that do not evaluate to a value.

use lexer::{Intern, TokenKind};
use node::Node;
use span::{Span, Spanned};

use crate::{
    DECL, Parse,
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
    fn parse<'source, 'index>(
        mut guard: crate::ParseGuard<'source, 'index>,
    ) -> crate::ParseResult<Self> {
        if Item::starts_item(&guard) {
            guard.with(Item::parse).map(Self::Item)
        } else if guard.peek_kw(*DECL) {
            guard.with(Binding::parse).map(Self::Binding)
        } else {
            let expr = guard.with(Expr::parse)?;
            guard.next_require(TokenKind::Semicolon)?;
            Ok(Self::ExprSemi(expr))
        }
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
    fn parse<'source, 'index>(
        mut guard: crate::ParseGuard<'source, 'index>,
    ) -> crate::ParseResult<Self> {
        guard.expect_kw(*DECL)?;

        let is_mutable = guard.next_require(TokenKind::Tilde).ok().map(|x| x.span);
        let ident = guard.expect_ident()?;

        let ty = if guard.next_require(TokenKind::Colon).is_ok() {
            Some(guard.spanning(Type::parse)?)
        } else {
            None
        };

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
                        Expr::Base(BaseExpr::Literal(Literal::Int(IntegerLiteral::Ok(5, None)))),
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
                        Expr::Base(BaseExpr::Literal(Literal::Int(IntegerLiteral::Ok(5, None)))),
                    ),
                }),
            ),
        )
    }
}
