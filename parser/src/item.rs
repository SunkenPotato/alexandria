//! Top-level items.

use diagnostic::Diagnostic;
use lexer::{Intern, TokenKind};
use node::{Node, NodeId};
use source::{SOURCE_EXTENSION, SourceFileError};
use span::Spanned;

use crate::{
    CONST, FUNC, IMPORT, INCLUDE, MODULE, PRODUCT, PUBLIC, Parse, ParseError, ParseGuard,
    ParseResult, STATIC, SUM,
    expr::{Block, Expr},
    path::Path,
};

type Generics<T> = Option<Spanned<Vec<Spanned<T>>>>;

fn parse_def_generic(mut guard: ParseGuard) -> ParseResult<Intern<str>> {
    guard.next_require(TokenKind::Ident).map(|x| x.symbol)
}

/// An item.
#[derive(PartialEq, Clone, Debug)]
pub enum Item {
    /// A function definition.
    FnDef(FnDef),
    /// A product definition.
    ProductDef(ProductDef),
    /// A sum definition.
    SumDef(SumDef),
    /// An import.
    Import(Import),
    /// A static variable definition.
    StaticDef(GlobalDef),
    /// A constant variable definition.
    ConstDef(GlobalDef),
    /// A module.
    Module(Module),
    /// An include definition.
    Include(IncludeDef),
}

impl Item {
    pub(crate) fn parse<'diag, 'source, 'index, 'a>(
        mut guard: ParseGuard<'diag, 'source, 'index, 'a>,
    ) -> ParseResult<Node<Self>> {
        guard
            .spanning(FnDef::parse)
            .map(|x| x.map(Self::FnDef).into())
            .or_else(|_| {
                guard
                    .spanning(Module::parse)
                    .map(|x| x.map(Self::Module).into())
            })
            .or_else(|_| {
                guard
                    .spanning(GlobalDef::parser(GlobalDefKind::Const))
                    .map(|x| x.map(Self::ConstDef).into())
            })
            .or_else(|_| {
                guard
                    .spanning(GlobalDef::parser(GlobalDefKind::Static))
                    .map(|x| x.map(Self::StaticDef).into())
            })
            .or_else(|_| {
                guard
                    .spanning(ProductDef::parse)
                    .map(|x| x.map(Self::ProductDef).into())
            })
            .or_else(|_| {
                guard
                    .spanning(SumDef::parse)
                    .map(|x| x.map(Self::SumDef).into())
            })
            .or_else(|_| {
                guard
                    .spanning(Import::parse)
                    .map(|x| x.map(Self::Import).into())
            })
            .or_else(|_| guard.with(Node::parse).map(|x| x.map(Self::Include)))
    }
}

/// A global variable definition.
#[derive(PartialEq, Clone, Debug)]
pub struct GlobalDef {
    /// The visibility.
    pub vis: Spanned<Visibility>,
    /// The variable definition kind (`static` or `const`).
    pub kind: GlobalDefKind,
    /// The identifier.
    pub ident: Spanned<Intern<str>>,
    /// The type.
    pub ty: Spanned<Type>,
    /// The value.
    pub value: Spanned<Expr>,
}

/// A global variable definition kind.
#[derive(PartialEq, Clone, Copy, Debug)]
pub enum GlobalDefKind {
    /// `static`.
    Static,
    /// `const`.
    Const,
}

impl GlobalDef {
    /// Return a parser to parse [`Self`] based on the given global definition kind.
    pub fn parser(kind: GlobalDefKind) -> impl Fn(ParseGuard) -> ParseResult<GlobalDef> {
        let def_kw = match kind {
            GlobalDefKind::Const => *CONST,
            GlobalDefKind::Static => *STATIC,
        };

        move |mut guard| {
            let vis = guard.spanning(Visibility::parse)?;
            let kw = guard.next_require(TokenKind::Ident)?;

            if kw.item.symbol != def_kw {
                return Err(ParseError::ExpectedKw(def_kw, kw.span));
            }

            let ident = guard.next_require(TokenKind::Ident)?.map(|x| x.symbol);
            guard.next_require(TokenKind::Colon)?;

            let ty = guard.spanning(Type::parse)?;
            guard.next_require(TokenKind::Equal)?;

            let value = guard.spanning(Expr::parse)?;
            guard.next_require(TokenKind::Semicolon)?;

            Ok(GlobalDef {
                vis,
                kind,
                ident,
                ty,
                value,
            })
        }
    }
}

impl Parse for GlobalDef {
    fn is_ok(&self) -> bool {
        self.vis.item.is_ok() && self.ty.item.is_ok() && self.value.item.is_ok()
    }

    fn parse<'diag, 'source, 'index, 'a>(
        _guard: ParseGuard<'diag, 'source, 'index, 'a>,
    ) -> ParseResult<Self> {
        unimplemented!()
    }
}

/// An import.
#[derive(PartialEq, Clone, Debug)]
pub enum Import {
    /// Full success.
    Ok(Path),
    /// The parser only found the path.
    MissingSemi(Path),
}

impl Import {
    /// Retrieve the path that this import references.
    pub const fn path(&self) -> &Path {
        match self {
            Self::Ok(p) | Self::MissingSemi(p) => p,
        }
    }
}

impl Parse for Import {
    fn is_ok(&self) -> bool {
        matches!(self, Self::Ok { .. })
    }

    fn parse<'diag, 'source, 'index, 'a>(
        mut guard: ParseGuard<'diag, 'source, 'index, 'a>,
    ) -> ParseResult<Self> {
        let import_token = guard.next_require(TokenKind::Ident)?;

        if import_token.item.symbol != *IMPORT {
            return Err(ParseError::ExpectedKw(*IMPORT, import_token.span));
        }

        let path = guard.with(Path::parse)?;
        if let Err(e) = guard.next_require(TokenKind::Semicolon) {
            e.display(guard.source_idx, guard.diagnostics);
            Ok(Self::MissingSemi(path))
        } else {
            Ok(Self::Ok(path))
        }
    }
}

/// The visibility of an item.
#[derive(PartialEq, Clone, Debug)]
pub enum Visibility {
    /// Public.
    Public,
    /// Private.
    Private,
}

impl Parse for Visibility {
    fn is_ok(&self) -> bool {
        true
    }

    fn parse<'diag, 'source, 'index, 'a>(
        mut guard: ParseGuard<'diag, 'source, 'index, 'a>,
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

/// A product definition.
#[derive(PartialEq, Clone, Debug)]
pub struct ProductDef {
    /// The visibility.
    pub vis: Spanned<Visibility>,
    /// The identifier.
    pub ident: Spanned<Intern<str>>,
    /// The generics.
    pub generics: Generics<Intern<str>>,
    /// The fields.
    pub fields: Vec<Field>,
}

impl Parse for ProductDef {
    fn is_ok(&self) -> bool {
        self.vis.item.is_ok() && self.fields.iter().all(|x| x.is_ok())
    }

    fn parse<'diag, 'source, 'index, 'a>(
        mut guard: ParseGuard<'diag, 'source, 'index, 'a>,
    ) -> ParseResult<Self> {
        let vis = guard.spanning(Visibility::parse)?;
        let kw = guard.next_require(TokenKind::Ident)?;

        if kw.item.symbol != *PRODUCT {
            return Err(ParseError::ExpectedKw(*PRODUCT, kw.span));
        }

        let ident = guard.next_require(TokenKind::Ident)?.map(|x| x.symbol);
        let generics = if guard.peek_require(TokenKind::LBracket).is_ok() {
            Some(guard.spanning(parse_generics(parse_def_generic))?)
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

/// A field.
#[derive(PartialEq, Debug, Clone)]
pub struct Field {
    /// The identifier.
    pub ident: Spanned<Intern<str>>,
    /// The type.
    pub ty: Spanned<Type>,
}

impl Parse for Field {
    fn is_ok(&self) -> bool {
        self.ty.item.is_ok()
    }

    fn parse<'diag, 'source, 'index, 'a>(
        mut guard: ParseGuard<'diag, 'source, 'index, 'a>,
    ) -> ParseResult<Self> {
        let ident = guard.next_require(TokenKind::Ident)?.map(|x| x.symbol);
        guard.next_require(TokenKind::Colon)?;
        let ty = guard.spanning(Type::parse)?;

        Ok(Self { ident, ty })
    }
}

/// A sum definition.
#[derive(PartialEq, Clone, Debug)]
pub struct SumDef {
    /// The visiblity.
    pub vis: Spanned<Visibility>,
    /// The identifier.
    pub ident: Spanned<Intern<str>>,
    /// The generics.
    pub generics: Generics<Intern<str>>,
    /// The fields.
    pub fields: Vec<Field>,
}

impl Parse for SumDef {
    fn is_ok(&self) -> bool {
        self.vis.item.is_ok() && self.fields.iter().all(|x| x.is_ok())
    }

    fn parse<'diag, 'source, 'index, 'a>(
        mut guard: ParseGuard<'diag, 'source, 'index, 'a>,
    ) -> ParseResult<Self> {
        let vis = guard.spanning(Visibility::parse)?;
        let kw = guard.next_require(TokenKind::Ident)?;

        if kw.item.symbol != *SUM {
            return Err(ParseError::ExpectedKw(*SUM, kw.span));
        }

        let ident = guard.next_require(TokenKind::Ident)?.map(|x| x.symbol);
        let generics = if guard.peek_require(TokenKind::LBracket).is_ok() {
            Some(guard.spanning(parse_generics(parse_def_generic))?)
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

/// A type usage.
#[derive(PartialEq, Debug, Clone)]
pub struct Type {
    /// The path of the type.
    pub path: Node<Path>,
    /// The generics of the type.
    pub generics: Generics<Type>,
}

impl Parse for Type {
    fn is_ok(&self) -> bool {
        self.path.item.is_ok()
    }

    fn parse<'diag, 'source, 'index, 'a>(
        mut guard: crate::ParseGuard<'diag, 'source, 'index, 'a>,
    ) -> crate::ParseResult<Self> {
        let path = guard.spanning(Path::parse)?;

        let generics = if guard.peek_require(TokenKind::LBracket).is_ok() {
            Some(guard.spanning(parse_generics(Self::parse))?)
        } else {
            None
        };

        Ok(Self {
            path: path.into(),
            generics,
        })
    }
}

fn parse_generics<T>(
    subparser: impl FnMut(ParseGuard) -> ParseResult<T> + Copy,
) -> impl FnOnce(ParseGuard) -> ParseResult<Vec<Spanned<T>>> {
    move |mut guard| {
        guard.next_require(TokenKind::LBracket)?;

        let mut generics = vec![];
        while let Ok(item) = guard.spanning(subparser) {
            generics.push(item);
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
}

/// A function definition.
#[derive(PartialEq, Clone, Debug)]
pub struct FnDef {
    /// The visibility.
    pub vis: Spanned<Visibility>,
    /// The identifier.
    pub ident: Spanned<Intern<str>>,
    /// The generics.
    pub generics: Generics<Intern<str>>,
    /// The arguments.
    pub args: Vec<Field>,
    /// The return type.
    pub ret_ty: Option<Spanned<Type>>,
    /// The block.
    pub block: Node<Block>,
}

impl Parse for FnDef {
    fn is_ok(&self) -> bool {
        self.vis.item.is_ok()
            && self.ret_ty.as_ref().is_none_or(|x| x.item.is_ok())
            && self.args.iter().all(Parse::is_ok)
            && self.block.item.is_ok()
    }

    fn parse<'diag, 'source, 'index, 'a>(
        mut guard: ParseGuard<'diag, 'source, 'index, 'a>,
    ) -> ParseResult<Self> {
        let vis = guard.spanning(Visibility::parse)?;
        let func_kw = guard.next_require(TokenKind::Ident)?;

        if func_kw.item.symbol != *FUNC {
            return Err(ParseError::ExpectedKw(*FUNC, func_kw.span));
        }

        let ident = guard.next_require(TokenKind::Ident)?.map(|x| x.symbol);
        let generics = if guard.peek_require(TokenKind::LBracket).is_ok() {
            Some(guard.spanning(parse_generics(parse_def_generic))?)
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

        let block = guard.spanning(Block::parse)?.into();

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

/// An inline module.
#[derive(Debug, Clone, PartialEq)]
pub struct InlineModule {
    /// The items contained within the module.
    pub items: Vec<Node<Item>>,
}

impl Parse for InlineModule {
    fn is_ok(&self) -> bool {
        true
    }

    fn parse<'diag, 'source, 'index, 'a>(
        mut guard: crate::ParseGuard<'diag, 'source, 'index, 'a>,
    ) -> crate::ParseResult<Self> {
        let mut items = vec![];

        while let Ok(item) = guard.with(Item::parse) {
            items.push(item);
        }

        Ok(InlineModule { items })
    }
}

/// A module.
#[derive(Debug, Clone, PartialEq)]
pub struct Module {
    /// Its visibility.
    pub vis: Spanned<Visibility>,
    /// The name.
    pub ident: Spanned<Intern<str>>,
    /// The items contained within.
    pub module: InlineModule,
}

impl Parse for Module {
    fn is_ok(&self) -> bool {
        self.vis.item.is_ok() && self.module.is_ok()
    }

    fn parse<'diag, 'source, 'index, 'a>(
        mut guard: crate::ParseGuard<'diag, 'source, 'index, 'a>,
    ) -> crate::ParseResult<Self> {
        let vis = guard.spanning(Visibility::parse)?;
        let kw = guard.next_require(TokenKind::Ident)?;
        if kw.item.symbol != *MODULE {
            return Err(ParseError::ExpectedKw(*MODULE, kw.span));
        }

        let ident = guard.next_require(TokenKind::Ident)?.map(|x| x.symbol);
        guard.next_require(TokenKind::LCurly)?;
        let mut items = vec![];
        guard.commit_diag();

        loop {
            let res = guard.with(Item::parse);

            match res {
                Ok(res) => items.push(res),
                Err(e) => {
                    if guard.next_require(TokenKind::RCurly).is_ok() {
                        guard.rollback_diag();
                        break;
                    } else {
                        return Err(e);
                    }
                }
            }

            guard.commit_diag();
        }

        Ok(Self {
            vis,
            ident,
            module: InlineModule { items },
        })
    }
}

/// An `include` definition.
#[derive(PartialEq, Clone, Debug)]
pub struct IncludeDef {
    /// The visibility.
    pub vis: Spanned<Visibility>,
    /// The identifier.
    pub ident: Spanned<Intern<str>>,
    /// Whether the parser was able to associate a semicolon with this.
    pub missing_semi: bool,
}

impl Parse for Node<IncludeDef> {
    fn is_ok(&self) -> bool {
        self.missing_semi
    }

    fn parse<'diag, 'source, 'index, 'a>(
        mut guard: ParseGuard<'diag, 'source, 'index, 'a>,
    ) -> ParseResult<Self> {
        let parsed: Node<_> = guard
            .spanning(|mut guard| {
                let vis = guard.spanning(Visibility::parse)?;
                let kw = guard.next_require(TokenKind::Ident)?;
                if kw.symbol != *INCLUDE {
                    return Err(ParseError::ExpectedKw(*INCLUDE, kw.span));
                }

                let ident = guard.next_require(TokenKind::Ident)?.map(|x| x.symbol);

                let semi = guard.next_require(TokenKind::Semicolon);

                if let Err(e) = &semi {
                    e.display(guard.source_idx, guard.diagnostics);
                }

                Ok(IncludeDef {
                    vis,
                    ident,
                    missing_semi: semi.is_err(),
                })
            })?
            .into();

        #[cfg(not(test))]
        IncludeDef::parse_file(guard, parsed.ident, parsed.id());

        Ok(parsed)
    }
}

impl IncludeDef {
    #[allow(unused)]
    fn parse_file(mut guard: ParseGuard, ident: Spanned<Intern<str>>, node_id: NodeId) {
        let (file_id, is_nested) =
            match guard.sources.load_from_mod(&guard.module_tree, &ident.item) {
                Ok(r) => r,
                Err(SourceFileError::Io(_) | SourceFileError::TooLarge(_)) => {
                    return guard.diagnostics.push(Diagnostic::error(
                        ident.span,
                        format!("failed to read source for module `{}`", ident.item),
                        None,
                        guard.source_idx,
                    ));
                }
                Err(SourceFileError::Utf8(_)) => {
                    return guard.diagnostics.push(Diagnostic::error(
                        ident.span,
                        format!("the module `{}` does not contain valid UTF-8", ident.item),
                        None,
                        guard.source_idx,
                    ));
                }
                Err(SourceFileError::NoMatches) => {
                    return guard.diagnostics.push(Diagnostic::error(
                        ident.span,
                        format!(
                            "failed to find file `{name}.{ext}` or `{name}/mod.{ext}`",
                            name = ident.item,
                            ext = SOURCE_EXTENSION,
                        ),
                        None,
                        guard.source_idx,
                    ));
                }
            };

        if let Err(e) = guard.parse_module(file_id, node_id, ident.item, is_nested) {
            e.display(guard.source_idx, guard.diagnostics);
        }
    }
}

#[cfg(test)]
mod tests {
    use span::Span;

    use crate::{
        assert_eq, assert_eq_custom_parser,
        expr::{
            BaseExpr, BinaryExpr, BinaryOp, Expr, FnCall,
            literal::{IntegerLiteral, Literal, StringLiteral},
        },
        path::Segment,
        stmt::Stmt,
    };

    use super::*;

    #[test]
    fn parse_type() {
        assert_eq(
            "vector[int,]",
            Spanned::new(
                Span::new(0, 12),
                Type {
                    path: Path::single(Spanned::new(Span::new(0, 6), Intern::from("vector")))
                        .into(),
                    generics: Some(Spanned::new(
                        Span::new(6, 12),
                        vec![Spanned::new(
                            Span::new(7, 10),
                            Type {
                                path: Path::single(Spanned::new(
                                    Span::new(7, 10),
                                    Intern::from("int"),
                                ))
                                .into(),
                                generics: None,
                            },
                        )],
                    )),
                },
            ),
        );
    }

    #[test]
    fn parse_product() {
        assert_eq_custom_parser(
            Item::parse,
            "pub product Vec[T] { ptr: Ptr[T], len: usize }",
            Node::from(Spanned::new(
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
                                    ))
                                    .into(),
                                    generics: Some(Spanned::new(
                                        Span::new(29, 32),
                                        vec![Spanned::new(
                                            Span::new(30, 31),
                                            Type {
                                                path: Path::single(Spanned::new(
                                                    Span::new(30, 31),
                                                    Intern::from("T"),
                                                ))
                                                .into(),
                                                generics: None,
                                            },
                                        )],
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
                                    ))
                                    .into(),
                                    generics: None,
                                },
                            ),
                        },
                    ],
                }),
            )),
            None,
        );
    }

    #[test]
    fn parse_sum_def() {
        assert_eq_custom_parser(
            Item::parse,
            "sum Result[T, E] { Ok: T, Err: E }",
            Node::from(Spanned::new(
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
                                    ))
                                    .into(),
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
                                    ))
                                    .into(),
                                    generics: None,
                                },
                            ),
                        },
                    ],
                }),
            )),
            None,
        )
    }

    #[test]
    fn parse_fn_def() {
        assert_eq_custom_parser(
            Item::parse,
            "pub func add[T] { rhs: T, lhs: T }: T { rhs + lhs }",
            Node::from(Spanned::new(
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
                                    ))
                                    .into(),
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
                                    ))
                                    .into(),
                                    generics: None,
                                },
                            ),
                        },
                    ],
                    ret_ty: Some(Spanned::new(
                        Span::new(36, 37),
                        Type {
                            path: Path::single(Spanned::new(Span::new(36, 37), Intern::from("T")))
                                .into(),
                            generics: None,
                        },
                    )),
                    block: Node::new(
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
                                            .into(),
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
                                            .into(),
                                        )),
                                    )),
                                }),
                            ))),
                        },
                    ),
                }),
            )),
            None,
        );
    }

    #[test]
    fn parse_import() {
        assert_eq_custom_parser(
            Item::parse,
            "import std::print;",
            Node::from(Spanned::new(
                Span::new(0, 18),
                Item::Import(Import::Ok(Path {
                    segments: vec![
                        Spanned::new(Span::new(7, 10), Segment::new("std", false)),
                        Spanned::new(Span::new(12, 17), Segment::new("print", false)),
                    ],
                    is_fully_qualified: false,
                })),
            )),
            None,
        )
    }

    #[test]
    fn parse_import_missing_semi() {
        assert_eq_custom_parser(
            Item::parse,
            "import print",
            Node::from(Spanned::new(
                Span::new(0, 12),
                Item::Import(Import::MissingSemi(
                    Path::single(Spanned::new(Span::new(7, 12), Intern::from("print"))).item,
                )),
            )),
            None,
        )
    }

    #[test]
    fn parse_global() {
        assert_eq_custom_parser(
            Item::parse,
            "const X: u32 = 5;",
            Node::from(Spanned::new(
                Span::new(0, 17),
                Item::ConstDef(GlobalDef {
                    vis: Spanned::new(Span::new(0, 0), Visibility::Private),
                    kind: GlobalDefKind::Const,
                    ident: Spanned::new(Span::new(6, 7), Intern::from("X")),
                    ty: Spanned::new(
                        Span::new(9, 12),
                        Type {
                            path: Path::single(Spanned::new(Span::new(9, 12), Intern::from("u32")))
                                .into(),
                            generics: None,
                        },
                    ),
                    value: Spanned::new(
                        Span::new(15, 16),
                        Expr::Base(BaseExpr::Literal(Literal::Int(IntegerLiteral::Ok(5)))),
                    ),
                }),
            )),
            None,
        )
    }

    #[test]
    fn parse_fn_def_main() {
        assert_eq_custom_parser(
            Item::parse,
            "func main {} { println(\"H\"); }",
            Node::from(Spanned::new(
                Span::new(0, 30),
                Item::FnDef(FnDef {
                    vis: Spanned::new(Span::new(0, 0), Visibility::Private),
                    ident: Spanned::new(Span::new(5, 9), Intern::from("main")),
                    generics: None,
                    args: vec![],
                    ret_ty: None,
                    block: Node::new(
                        Span::new(13, 30),
                        Block {
                            stmts: vec![Spanned::new(
                                Span::new(15, 28),
                                Stmt::ExprSemi(Expr::Base(BaseExpr::FnCall(FnCall {
                                    object: Box::new(Spanned::new(
                                        Span::new(15, 22),
                                        BaseExpr::Path(
                                            Path::single(Spanned::new(
                                                Span::new(15, 22),
                                                Intern::from("println"),
                                            ))
                                            .into(),
                                        ),
                                    )),
                                    args: vec![Spanned::new(
                                        Span::new(23, 26),
                                        Expr::Base(BaseExpr::Literal(Literal::Str(
                                            StringLiteral::Ok(Intern::from("H")),
                                        ))),
                                    )],
                                }))),
                            )],
                            tail: None,
                        },
                    ),
                }),
            )),
            None,
        )
    }
}
