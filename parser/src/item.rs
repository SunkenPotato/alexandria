//! Top-level items.

use diagnostic::Diagnostic;
use lexer::{Intern, TokenKind};
use node::Node;
use span::Spanned;

use crate::{
    CONST, CRATE, FUNC, IMPORT, INCLUDE, MODULE, PRODUCT, PUBLIC, Parse, ParseError, ParseGuard,
    ParseResult, STATIC, SUM,
    crate_table::CrateId,
    expr::{Block, Expr},
    path::Path,
};

type Generics<T> = Option<Spanned<Vec<Spanned<T>>>>;

/// Parse the generics of a definition (e.g., `[T, U]`), if present.
fn parse_def_generics(guard: &mut ParseGuard) -> ParseResult<Generics<Intern<str>>> {
    if !guard.peek_kind(TokenKind::LBracket) {
        return Ok(None);
    }

    guard
        .spanning(|mut g| {
            g.parse_delimited(TokenKind::LBracket, TokenKind::RBracket, |mut g| {
                g.expect_ident().map(|x| x.item)
            })
        })
        .map(Some)
}

/// Parse a list of fields delimited by curly braces (e.g., `{ a: T, b: U }`).
fn parse_fields(guard: &mut ParseGuard) -> ParseResult<Vec<Field>> {
    Ok(guard
        .parse_delimited(TokenKind::LCurly, TokenKind::RCurly, Field::parse)?
        .into_iter()
        .map(|x| x.item)
        .collect())
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
    /// An include crate definition.
    IncludeCrate(IncludeCrate),
}

impl Item {
    /// Check whether the next tokens start an item.
    pub(crate) fn starts_item(guard: &ParseGuard) -> bool {
        let offset = usize::from(guard.peek_kw(*PUBLIC));
        [
            *FUNC, *MODULE, *CONST, *STATIC, *PRODUCT, *SUM, *IMPORT, *INCLUDE,
        ]
        .into_iter()
        .any(|kw| guard.peek_n_kw(offset, kw))
    }

    pub(crate) fn parse<'source, 'index>(
        mut guard: ParseGuard<'source, 'index>,
    ) -> ParseResult<Node<Self>> {
        let node: Node<Self> = guard.spanning(Self::parse_kind)?.into();

        if let Self::Include(include) = &node.item {
            guard.load_module(include.ident, node.id());
        }

        Ok(node)
    }

    /// Dispatch to the parser of the item introduced by the leading keyword.
    fn parse_kind(mut guard: ParseGuard) -> ParseResult<Self> {
        let offset = usize::from(guard.peek_kw(*PUBLIC));
        let token = guard.peek_n(offset)?;
        let is = |kw: Intern<str>| guard.peek_n_kw(offset, kw);

        if is(*FUNC) {
            guard.with(FnDef::parse).map(Self::FnDef)
        } else if is(*MODULE) {
            guard.with(Module::parse).map(Self::Module)
        } else if is(*CONST) {
            guard
                .with(GlobalDef::parser(GlobalDefKind::Const))
                .map(Self::ConstDef)
        } else if is(*STATIC) {
            guard
                .with(GlobalDef::parser(GlobalDefKind::Static))
                .map(Self::StaticDef)
        } else if is(*PRODUCT) {
            guard.with(ProductDef::parse).map(Self::ProductDef)
        } else if is(*SUM) {
            guard.with(SumDef::parse).map(Self::SumDef)
        } else if is(*IMPORT) {
            guard.with(Import::parse).map(Self::Import)
        } else if is(*INCLUDE) && guard.peek_n_kw(offset + 1, *CRATE) {
            guard.with(IncludeCrate::parse).map(Self::IncludeCrate)
        } else if is(*INCLUDE) {
            guard.with(IncludeDef::parse).map(Self::Include)
        } else {
            Err(ParseError::ExpectedItem(token.span))
        }
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
            guard.expect_kw(def_kw)?;

            let ident = guard.expect_ident()?;
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
    fn parse<'source, 'index>(mut guard: ParseGuard<'source, 'index>) -> ParseResult<Self> {
        guard.expect_kw(*IMPORT)?;

        let path = guard.with(Path::parse)?;
        if let Err(e) = guard.next_require(TokenKind::Semicolon) {
            e.display(guard.source_idx, &guard.ctx.diagnostics);
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
    fn parse<'source, 'index>(mut guard: ParseGuard<'source, 'index>) -> ParseResult<Self> {
        if guard.peek_kw(*PUBLIC) {
            guard.next()?;
            Ok(Self::Public)
        } else {
            Ok(Self::Private)
        }
    }
}

/// The parts shared by product and sum definitions.
struct TypeDef {
    vis: Spanned<Visibility>,
    ident: Spanned<Intern<str>>,
    generics: Generics<Intern<str>>,
    fields: Vec<Field>,
}

/// Parse a product or sum definition, introduced by `kw`.
fn parse_type_def(guard: &mut ParseGuard, kw: Intern<str>) -> ParseResult<TypeDef> {
    let vis = guard.spanning(Visibility::parse)?;
    guard.expect_kw(kw)?;

    let ident = guard.expect_ident()?;
    let generics = parse_def_generics(guard)?;
    let fields = parse_fields(guard)?;

    Ok(TypeDef {
        vis,
        ident,
        generics,
        fields,
    })
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
    fn parse<'source, 'index>(mut guard: ParseGuard<'source, 'index>) -> ParseResult<Self> {
        let TypeDef {
            vis,
            ident,
            generics,
            fields,
        } = parse_type_def(&mut guard, *PRODUCT)?;

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
    fn parse<'source, 'index>(mut guard: ParseGuard<'source, 'index>) -> ParseResult<Self> {
        let ident = guard.expect_ident()?;
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
    fn parse<'source, 'index>(mut guard: ParseGuard<'source, 'index>) -> ParseResult<Self> {
        let TypeDef {
            vis,
            ident,
            generics,
            fields,
        } = parse_type_def(&mut guard, *SUM)?;

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
    fn parse<'source, 'index>(mut guard: ParseGuard<'source, 'index>) -> ParseResult<Self> {
        let path = guard.spanning(Path::parse)?;

        let generics = if guard.peek_kind(TokenKind::LBracket) {
            Some(guard.spanning(|mut g| {
                g.parse_delimited(TokenKind::LBracket, TokenKind::RBracket, Self::parse)
            })?)
        } else {
            None
        };

        Ok(Self {
            path: path.into(),
            generics,
        })
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
    fn parse<'source, 'index>(mut guard: ParseGuard<'source, 'index>) -> ParseResult<Self> {
        let vis = guard.spanning(Visibility::parse)?;
        guard.expect_kw(*FUNC)?;

        let ident = guard.expect_ident()?;
        let generics = parse_def_generics(&mut guard)?;
        let args = parse_fields(&mut guard)?;

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

/// An inline module, i.e., a list of items.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct InlineModule {
    /// The items contained within the module.
    pub items: Vec<Node<Item>>,
}

impl Parse for InlineModule {
    /// Parse items until the end of the input or a `}`, which is not consumed.
    fn parse<'source, 'index>(mut guard: ParseGuard<'source, 'index>) -> ParseResult<Self> {
        let mut items = vec![];

        while guard.peek().is_ok_and(|x| x.item.kind != TokenKind::RCurly) {
            items.push(guard.with(Item::parse)?);
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
    fn parse<'source, 'index>(mut guard: ParseGuard<'source, 'index>) -> ParseResult<Self> {
        let vis = guard.spanning(Visibility::parse)?;
        guard.expect_kw(*MODULE)?;

        let ident = guard.expect_ident()?;
        guard.next_require(TokenKind::LCurly)?;
        let module = guard.with(InlineModule::parse)?;
        guard.next_require(TokenKind::RCurly)?;

        Ok(Self { vis, ident, module })
    }
}

/// An `include` definition.
#[derive(PartialEq, Clone, Debug)]
pub struct IncludeDef {
    /// The visibility.
    pub vis: Spanned<Visibility>,
    /// The identifier.
    pub ident: Spanned<Intern<str>>,
    /// Whether the parser was unable to find a semicolon after this.
    pub missing_semi: bool,
}

impl Parse for IncludeDef {
    /// Parse the definition. This does not load the module; see [`Item::parse`].
    fn parse<'source, 'index>(mut guard: ParseGuard<'source, 'index>) -> ParseResult<Self> {
        let vis = guard.spanning(Visibility::parse)?;
        guard.expect_kw(*INCLUDE)?;

        let ident = guard.expect_ident()?;
        let semi = guard.next_require(TokenKind::Semicolon);

        if let Err(e) = &semi {
            e.display(guard.source_idx, &guard.ctx.diagnostics);
        }

        Ok(IncludeDef {
            vis,
            ident,
            missing_semi: semi.is_err(),
        })
    }
}

/// A statement to include a crate.
#[derive(Clone, Debug, PartialEq)]
pub struct IncludeCrate {
    /// The name of this crate
    pub ident: Spanned<Intern<str>>,
    /// The ID of the crate, if a crate with this name exists.
    pub crate_id: Option<CrateId>,
}

impl Parse for IncludeCrate {
    fn parse<'source, 'index>(mut guard: ParseGuard<'source, 'index>) -> ParseResult<Self> {
        guard.expect_kw(*INCLUDE)?;
        guard.expect_kw(*CRATE)?;

        let ident = guard.expect_ident()?;
        guard.next_require(TokenKind::Semicolon)?;

        let crate_id = guard.ctx.crate_table.id_by_name(ident.item);
        match crate_id {
            Some(id) => guard.ctx.crate_table.request(id),
            None => guard.emit(Diagnostic::error(
                ident.span,
                format!("reference to unknown crate `{}`", ident.item),
                Some(format!(
                    "pass the crate with `--crates {}=<path>`",
                    ident.item
                )),
                guard.source_idx,
            )),
        }

        Ok(Self { ident, crate_id })
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
                        Expr::Base(BaseExpr::Literal(Literal::Int(IntegerLiteral::Ok(5, None)))),
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
