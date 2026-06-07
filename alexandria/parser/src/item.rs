use lexer::{Intern, TokenKind};
use span::Spanned;

use crate::{PRODUCT, PUBLIC, Parse, ParseError, ParseGuard, ParseResult, Path};

type Generics = Option<Spanned<Vec<Spanned<Intern<str>>>>>;

#[derive(PartialEq, Clone, Debug)]
pub enum Item {
    FnDef(FnDef),
    ProductDef(ProductDef),
    // SumDef(SumDef),
    Import(Path),
}

#[derive(PartialEq, Clone, Debug)]
pub enum Visibility {
    Public,
    Private,
}

impl Parse for Visibility {
    fn is_ok(&self) -> bool {
        true
    }

    fn parse<'diag, 'source, 'index>(
        mut guard: ParseGuard<'diag, 'source, 'index>,
    ) -> ParseResult<Self> {
        let Ok(ident) = guard.peek_require(TokenKind::Ident) else {
            return Ok(Self::Private);
        };

        if ident.item.symbol == *PUBLIC {
            _ = guard.next();
            Ok(Self::Public)
        } else {
            Ok(Self::Private)
        }
    }
}

#[derive(PartialEq, Clone, Debug)]
pub struct ProductDef {
    pub vis: Spanned<Visibility>,
    pub ident: Spanned<Intern<str>>,
    pub generics: Generics,
    pub fields: Vec<ProductField>,
}

impl Parse for ProductDef {
    fn is_ok(&self) -> bool {
        todo!()
    }

    fn parse<'diag, 'source, 'index>(
        mut guard: ParseGuard<'diag, 'source, 'index>,
    ) -> ParseResult<Self> {
        let vis = guard.spanning(Visibility::parse)?;
        let kw = guard.next_require(TokenKind::Ident)?;

        if kw.item.symbol != *PRODUCT {
            return Err(ParseError::ExpectedKw(*PRODUCT, kw.span));
        }

        let ident = guard.next_require(TokenKind::Ident)?.map(|x| x.symbol);
        let generics = if guard.peek_require(TokenKind::LBracket).is_ok() {
            Some(guard.spanning(parse_generics)?)
        } else {
            None
        };

        guard.next_require(TokenKind::LCurly)?;

        let mut fields = vec![];

        while let Ok(field) = guard.with(ProductField::parse) {
            fields.push(field);
            if guard.next_require(TokenKind::Comma).is_ok() {
                if guard.peek_require(TokenKind::RCurly).is_ok() {
                    break;
                }
            } else {
                break;
            }
        }

        guard.next_require(TokenKind::RCurly)?;

        Ok(Self {
            vis,
            ident,
            generics,
            fields,
        })
    }
}

#[derive(PartialEq, Debug, Clone)]
pub struct ProductField {
    pub ident: Spanned<Intern<str>>,
    pub ty: Spanned<Type>,
}

impl Parse for ProductField {
    fn is_ok(&self) -> bool {
        self.ty.item.is_ok()
    }

    fn parse<'diag, 'source, 'index>(
        mut guard: ParseGuard<'diag, 'source, 'index>,
    ) -> ParseResult<Self> {
        let ident = guard.next_require(TokenKind::Ident)?.map(|x| x.symbol);
        guard.next_require(TokenKind::Colon)?;
        let ty = guard.spanning(Type::parse)?;

        Ok(Self { ident, ty })
    }
}

#[derive(PartialEq, Debug, Clone)]
pub struct Type {
    pub path: Spanned<Path>,
    pub generics: Generics,
}

impl Parse for Type {
    fn is_ok(&self) -> bool {
        self.path.item.is_ok()
    }

    fn parse<'diag, 'source, 'index>(
        mut guard: crate::ParseGuard<'diag, 'source, 'index>,
    ) -> crate::ParseResult<Self> {
        let path = guard.spanning(Path::parse)?;

        let generics = if guard.peek_require(TokenKind::LBracket).is_ok() {
            Some(guard.spanning(parse_generics)?)
        } else {
            None
        };

        Ok(Self { path, generics })
    }
}

fn parse_generics(mut guard: ParseGuard) -> ParseResult<Vec<Spanned<Intern<str>>>> {
    guard.next_require(TokenKind::LBracket)?;

    let mut generics = vec![];
    while let Ok(ident) = guard.next_require(TokenKind::Ident) {
        generics.push(ident.map(|x| x.symbol));
        if guard.next_require(TokenKind::Comma).is_ok() {
            if guard.peek_require(TokenKind::RBracket).is_ok() {
                break;
            }
        } else {
            break;
        }
    }

    guard.next_require(TokenKind::RBracket)?;

    Ok(generics)
}

#[derive(PartialEq, Clone, Debug)]
pub struct FnDef {
    pub vis: Spanned<Visibility>,
    pub ident: Spanned<Intern<str>>,
}

#[cfg(test)]
mod tests {
    use span::Span;

    use crate::assert_eq;

    use super::*;

    #[test]
    fn parse_type() {
        assert_eq(
            "vector[int,]",
            Spanned::new(
                Span::new(0, 12),
                Type {
                    path: Path::single(Spanned::new(Span::new(0, 6), Intern::from("vector"))),
                    generics: Some(Spanned::new(
                        Span::new(6, 12),
                        vec![Spanned::new(Span::new(7, 10), Intern::from("int"))],
                    )),
                },
            ),
        );
    }

    #[test]
    fn parse_product() {
        assert_eq(
            "pub product Vec[T] { ptr: Ptr[T], len: usize }",
            Spanned::new(
                Span::new(0, 46),
                ProductDef {
                    vis: Spanned::new(Span::new(0, 3), Visibility::Public),
                    ident: Spanned::new(Span::new(12, 15), Intern::from("Vec")),
                    generics: Some(Spanned::new(
                        Span::new(15, 18),
                        vec![Spanned::new(Span::new(16, 17), Intern::from("T"))],
                    )),
                    fields: vec![
                        ProductField {
                            ident: Spanned::new(Span::new(21, 24), Intern::from("ptr")),
                            ty: Spanned::new(
                                Span::new(26, 32),
                                Type {
                                    path: Path::single(Spanned::new(
                                        Span::new(26, 29),
                                        Intern::from("Ptr"),
                                    )),
                                    generics: Some(Spanned::new(
                                        Span::new(29, 32),
                                        vec![Spanned::new(Span::new(30, 31), Intern::from("T"))],
                                    )),
                                },
                            ),
                        },
                        ProductField {
                            ident: Spanned::new(Span::new(34, 37), Intern::from("len")),
                            ty: Spanned::new(
                                Span::new(39, 44),
                                Type {
                                    path: Path::single(Spanned::new(
                                        Span::new(39, 44),
                                        Intern::from("usize"),
                                    )),
                                    generics: None,
                                },
                            ),
                        },
                    ],
                },
            ),
        );
    }
}
