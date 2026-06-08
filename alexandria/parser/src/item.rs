use lexer::{Intern, TokenKind};
use span::Spanned;

use crate::{
    FUNC, IMPORT, PRODUCT, PUBLIC, Parse, ParseError, ParseGuard, ParseResult, Path, SUM,
    expr::Block,
};

type Generics = Option<Spanned<Vec<Spanned<Intern<str>>>>>;

#[derive(PartialEq, Clone, Debug)]
pub enum Item {
    FnDef(FnDef),
    ProductDef(ProductDef),
    SumDef(SumDef),
    Import(Path),
}

impl Parse for Item {
    fn is_ok(&self) -> bool {
        match self {
            Self::FnDef(v) => v.is_ok(),
            Self::Import(v) => v.is_ok(),
            Self::ProductDef(v) => v.is_ok(),
            Self::SumDef(v) => v.is_ok(),
        }
    }

    fn parse<'diag, 'source, 'index>(
        mut guard: ParseGuard<'diag, 'source, 'index>,
    ) -> ParseResult<Self> {
        guard
            .with(FnDef::parse)
            .map(Self::FnDef)
            .or_else(|_| guard.with(ProductDef::parse).map(Self::ProductDef))
            .or_else(|_| guard.with(SumDef::parse).map(Self::SumDef))
            .or_else(|_| guard.with(parse_import).map(Self::Import))
    }
}

fn parse_import(mut guard: ParseGuard) -> ParseResult<Path> {
    let import_token = guard.next_require(TokenKind::Ident)?;

    if import_token.item.symbol != *IMPORT {
        return Err(ParseError::ExpectedKw(*IMPORT, import_token.span));
    }

    guard.with(Path::parse)
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
    pub fields: Vec<Field>,
}

impl Parse for ProductDef {
    fn is_ok(&self) -> bool {
        self.vis.item.is_ok() && self.fields.iter().all(|x| x.is_ok())
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

        while let Ok(field) = guard.with(Field::parse) {
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
pub struct Field {
    pub ident: Spanned<Intern<str>>,
    pub ty: Spanned<Type>,
}

impl Parse for Field {
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

#[derive(PartialEq, Clone, Debug)]
pub struct SumDef {
    pub vis: Spanned<Visibility>,
    pub ident: Spanned<Intern<str>>,
    pub generics: Generics,
    pub fields: Vec<Field>,
}

impl Parse for SumDef {
    fn is_ok(&self) -> bool {
        self.vis.item.is_ok() && self.fields.iter().all(|x| x.is_ok())
    }

    fn parse<'diag, 'source, 'index>(
        mut guard: ParseGuard<'diag, 'source, 'index>,
    ) -> ParseResult<Self> {
        let vis = guard.spanning(Visibility::parse)?;
        let kw = guard.next_require(TokenKind::Ident)?;

        if kw.item.symbol != *SUM {
            return Err(ParseError::ExpectedKw(*SUM, kw.span));
        }

        let ident = guard.next_require(TokenKind::Ident)?.map(|x| x.symbol);
        let generics = if guard.peek_require(TokenKind::LBracket).is_ok() {
            Some(guard.spanning(parse_generics)?)
        } else {
            None
        };

        guard.next_require(TokenKind::LCurly)?;

        let mut fields = vec![];

        while let Ok(field) = guard.with(Field::parse) {
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
    pub generics: Generics,
    pub args: Vec<Field>,
    pub ret_ty: Option<Spanned<Type>>,
    pub block: Spanned<Block>,
}

impl Parse for FnDef {
    fn is_ok(&self) -> bool {
        self.vis.item.is_ok()
            && self.ret_ty.as_ref().is_none_or(|x| x.item.is_ok())
            && self.args.iter().all(Parse::is_ok)
            && self.block.item.is_ok()
    }

    fn parse<'diag, 'source, 'index>(
        mut guard: ParseGuard<'diag, 'source, 'index>,
    ) -> ParseResult<Self> {
        let vis = guard.spanning(Visibility::parse)?;
        let func_kw = guard.next_require(TokenKind::Ident)?;

        if func_kw.item.symbol != *FUNC {
            return Err(ParseError::ExpectedKw(*FUNC, func_kw.span));
        }

        let ident = guard.next_require(TokenKind::Ident)?.map(|x| x.symbol);
        let generics = if guard.peek_require(TokenKind::LBracket).is_ok() {
            Some(guard.spanning(parse_generics)?)
        } else {
            None
        };

        guard.next_require(TokenKind::LCurly)?;

        let mut args = vec![];

        while let Ok(arg) = guard.with(Field::parse) {
            args.push(arg);
            if guard.next_require(TokenKind::Comma).is_ok() {
                if guard.peek_require(TokenKind::RCurly).is_ok() {
                    break;
                }
            } else {
                break;
            }
        }

        guard.next_require(TokenKind::RCurly)?;

        let ret_ty = if guard.next_require(TokenKind::Colon).is_ok() {
            Some(guard.spanning(Type::parse)?)
        } else {
            None
        };

        let block = guard.spanning(Block::parse)?;

        Ok(FnDef {
            vis,
            ident,
            generics,
            args,
            ret_ty,
            block,
        })
    }
}

#[cfg(test)]
mod tests {
    use span::Span;

    use crate::{
        Segment, assert_eq,
        expr::{BaseExpr, BinaryExpr, BinaryOp, Expr},
    };

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
                Item::ProductDef(ProductDef {
                    vis: Spanned::new(Span::new(0, 3), Visibility::Public),
                    ident: Spanned::new(Span::new(12, 15), Intern::from("Vec")),
                    generics: Some(Spanned::new(
                        Span::new(15, 18),
                        vec![Spanned::new(Span::new(16, 17), Intern::from("T"))],
                    )),
                    fields: vec![
                        Field {
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
                        Field {
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
                }),
            ),
        );
    }

    #[test]
    fn parse_sum_def() {
        assert_eq(
            "sum Result[T, E] { Ok: T, Err: E }",
            Spanned::new(
                Span::new(0, 34),
                Item::SumDef(SumDef {
                    vis: Spanned::new(Span::new(0, 0), Visibility::Private),
                    ident: Spanned::new(Span::new(4, 10), Intern::from("Result")),
                    generics: Some(Spanned::new(
                        Span::new(10, 16),
                        vec![
                            Spanned::new(Span::new(11, 12), Intern::from("T")),
                            Spanned::new(Span::new(14, 15), Intern::from("E")),
                        ],
                    )),
                    fields: vec![
                        Field {
                            ident: Spanned::new(Span::new(19, 21), Intern::from("Ok")),
                            ty: Spanned::new(
                                Span::new(23, 24),
                                Type {
                                    path: Path::single(Spanned::new(
                                        Span::new(23, 24),
                                        Intern::from("T"),
                                    )),
                                    generics: None,
                                },
                            ),
                        },
                        Field {
                            ident: Spanned::new(Span::new(26, 29), Intern::from("Err")),
                            ty: Spanned::new(
                                Span::new(31, 32),
                                Type {
                                    path: Path::single(Spanned::new(
                                        Span::new(31, 32),
                                        Intern::from("E"),
                                    )),
                                    generics: None,
                                },
                            ),
                        },
                    ],
                }),
            ),
        )
    }

    #[test]
    fn parse_fn_def() {
        assert_eq(
            "pub func add[T] { rhs: T, lhs: T }: T { rhs + lhs }",
            Spanned::new(
                Span::new(0, 51),
                Item::FnDef(FnDef {
                    vis: Spanned::new(Span::new(0, 3), Visibility::Public),
                    ident: Spanned::new(Span::new(9, 12), Intern::from("add")),
                    generics: Some(Spanned::new(
                        Span::new(12, 15),
                        vec![Spanned::new(Span::new(13, 14), Intern::from("T"))],
                    )),
                    args: vec![
                        Field {
                            ident: Spanned::new(Span::new(18, 21), Intern::from("rhs")),
                            ty: Spanned::new(
                                Span::new(23, 24),
                                Type {
                                    path: Path::single(Spanned::new(
                                        Span::new(23, 24),
                                        Intern::from("T"),
                                    )),
                                    generics: None,
                                },
                            ),
                        },
                        Field {
                            ident: Spanned::new(Span::new(26, 29), Intern::from("lhs")),
                            ty: Spanned::new(
                                Span::new(31, 32),
                                Type {
                                    path: Path::single(Spanned::new(
                                        Span::new(31, 32),
                                        Intern::from("T"),
                                    )),
                                    generics: None,
                                },
                            ),
                        },
                    ],
                    ret_ty: Some(Spanned::new(
                        Span::new(36, 37),
                        Type {
                            path: Path::single(Spanned::new(Span::new(36, 37), Intern::from("T"))),
                            generics: None,
                        },
                    )),
                    block: Spanned::new(
                        Span::new(38, 51),
                        Block {
                            stmts: vec![],
                            tail: Some(Box::new(Spanned::new(
                                Span::new(40, 49),
                                Expr::Binary(BinaryExpr {
                                    lhs: Box::new(Spanned::new(
                                        Span::new(40, 43),
                                        Expr::Base(BaseExpr::Path(
                                            Path::single(Spanned::new(
                                                Span::new(40, 43),
                                                Intern::from("rhs"),
                                            ))
                                            .item,
                                        )),
                                    )),
                                    op: Spanned::new(Span::new(44, 45), BinaryOp::Add),
                                    rhs: Box::new(Spanned::new(
                                        Span::new(46, 49),
                                        Expr::Base(BaseExpr::Path(
                                            Path::single(Spanned::new(
                                                Span::new(46, 49),
                                                Intern::from("lhs"),
                                            ))
                                            .item,
                                        )),
                                    )),
                                }),
                            ))),
                        },
                    ),
                }),
            ),
        );
    }

    #[test]
    fn parse_import() {
        assert_eq(
            "import std::print",
            Spanned::new(
                Span::new(0, 17),
                Item::Import(Path {
                    segments: vec![
                        Spanned::new(
                            Span::new(7, 10),
                            Segment {
                                is_kw: false,
                                segment: Intern::from("std"),
                            },
                        ),
                        Spanned::new(
                            Span::new(12, 17),
                            Segment {
                                is_kw: false,
                                segment: Intern::from("print"),
                            },
                        ),
                    ],
                    is_fully_qualified: false,
                }),
            ),
        )
    }
}
