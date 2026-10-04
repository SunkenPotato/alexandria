use std::iter::once;

use diagnostic::Diagnostics;
use lexer::Intern;
use node::Node;
use parser::{
    Parser,
    ast_table::AstTable,
    crate_table::CrateTable,
    expr::{BaseExpr, Block, Expr},
    item::{Item, ProductDef, SumDef, Type},
    path::Path,
    stmt::Stmt,
};
use source::{SourceFile, SourceMap};
use span::Span;

use crate::{
    NameResId, ScopeId,
    resolver::{NameResOutput, Res, ResolutionError, Resolver},
    sym_info::{SymbolInfo, SymbolKind},
};

/// The result of parsing and resolving a single in-memory file.
struct Resolved {
    src: String,
    ast: AstTable,
    sources: SourceMap,
    root: source::SourceIdx,
    diagnostics: Diagnostics,
    out: Result<NameResOutput, ResolutionError>,
}

#[track_caller]
fn resolve(input: &str) -> Resolved {
    let diagnostics = Diagnostics::default();
    let sources = SourceMap::new();
    let root = sources.insert(SourceFile::from_memory(input.to_owned()));
    let crate_table = CrateTable::default();
    let crate_id = crate_table.insert("test".into(), root).unwrap();
    let ast = AstTable::default();

    let parsed = Parser::new(
        sources.clone(),
        crate_id,
        diagnostics.clone(),
        ast.clone(),
        crate_table.clone(),
    )
    .parse();
    assert!(parsed, "test input failed to parse");

    let out = Resolver::new(&ast, crate_table, crate_id, diagnostics.clone()).fill();

    Resolved {
        src: input.to_owned(),
        ast,
        sources,
        root,
        diagnostics,
        out,
    }
}

impl Resolved {
    /// The resolver output, panicking with the diagnostics if resolution failed.
    #[track_caller]
    fn ok(&self) -> &NameResOutput {
        match &self.out {
            Ok(out) => out,
            Err(e) => panic!("resolution failed ({e:?}):\n{}", self.errors()),
        }
    }

    /// The rendered diagnostics.
    fn errors(&self) -> String {
        let mut buf = vec![];
        self.diagnostics.write(&self.sources, &mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    /// `text@offset` of the given span.
    fn show(&self, span: Span) -> String {
        let text = &self.src[span.start() as usize..span.stop() as usize];
        format!("{text}@{}", span.start())
    }

    /// Every path in the file, rendered as `use -> definition`, in source order.
    #[track_caller]
    fn uses(&self) -> Vec<String> {
        let out = self.ok();
        let mut paths = vec![];
        let module = self.ast.by_src(self.root);
        collect_items(&module.items, &mut paths);

        paths
            .into_iter()
            .map(|path| {
                let target = match out.resolutions.get(path.id()) {
                    Some(Res::Def(id)) => self.show(out.nrt.get(id).span()),
                    Some(Res::PrimTy(prim)) => format!("prim {prim:?}"),
                    None => "unresolved".to_owned(),
                };
                format!("{} -> {target}", self.show(path.span))
            })
            .collect()
    }
}

/// Look up a `::`-separated path starting from the crate root, using only the scope arena.
fn lookup<'a>(out: &'a NameResOutput, path: &str) -> Option<&'a SymbolInfo> {
    let mut scope = ScopeId::from_usize(0);
    let mut found: Option<NameResId> = None;

    for segment in path.split("::") {
        if let Some(id) = found {
            scope = out.nrt.get(id).projectable()?;
        }
        found = Some(out.arena.get_any(scope, Intern::from(segment))?);
    }

    found.map(|id| out.nrt.get(id))
}

/////////////////////////////////////////////////////
// PATH COLLECTION                                 //
/////////////////////////////////////////////////////

// mirrors the traversal of the resolver's second pass

fn collect_items<'a>(items: &'a [Node<Item>], out: &mut Vec<&'a Node<Path>>) {
    for item in items {
        collect_item(item, out);
    }
}

fn collect_item<'a>(item: &'a Node<Item>, out: &mut Vec<&'a Node<Path>>) {
    match &item.item {
        Item::ConstDef(def) | Item::StaticDef(def) => {
            collect_ty(&def.ty, out);
            collect_expr(&def.value, out);
        }
        Item::FnDef(def) => {
            for arg in &def.args {
                collect_ty(&arg.ty, out);
            }
            if let Some(ret) = &def.ret_ty {
                collect_ty(ret, out);
            }
            collect_block(&def.block, out);
        }
        Item::Module(module) => collect_items(&module.module.items, out),
        Item::ProductDef(ProductDef { fields, .. }) | Item::SumDef(SumDef { fields, .. }) => {
            for field in fields {
                collect_ty(&field.ty, out);
            }
        }
        // imports are resolved, but their path node isn't recorded in the resolution table
        // (the import item is), and included files live outside of this AST
        Item::Import(_) | Item::Include(_) | Item::IncludeCrate(_) => (),
    }
}

fn collect_ty<'a>(ty: &'a Type, out: &mut Vec<&'a Node<Path>>) {
    out.push(&ty.path);
    for generic in ty.generics.iter().flat_map(|g| &g.item) {
        collect_ty(generic, out);
    }
}

fn collect_block<'a>(block: &'a Block, out: &mut Vec<&'a Node<Path>>) {
    for stmt in &block.stmts {
        match &stmt.item {
            Stmt::Binding(bind) => {
                if let Some(ty) = &bind.ty {
                    collect_ty(ty, out);
                }
                collect_expr(&bind.value, out);
            }
            Stmt::ExprSemi(expr) => collect_expr(expr, out),
            Stmt::Item(item) => collect_item(item, out),
        }
    }

    if let Some(tail) = &block.tail {
        collect_expr(tail, out);
    }
}

fn collect_expr<'a>(expr: &'a Expr, out: &mut Vec<&'a Node<Path>>) {
    match expr {
        Expr::Base(base) => collect_base_expr(base, out),
        Expr::Binary(bin) => {
            collect_expr(&bin.lhs, out);
            collect_expr(&bin.rhs, out);
        }
        Expr::Assignment(assignment) => {
            collect_expr(&assignment.object, out);
            collect_expr(&assignment.value, out);
        }
    }
}

fn collect_base_expr<'a>(expr: &'a BaseExpr, out: &mut Vec<&'a Node<Path>>) {
    match expr {
        BaseExpr::Path(path) => out.push(path),
        BaseExpr::Block(block) | BaseExpr::Loop(block) => collect_block(block, out),
        BaseExpr::Break(Some(expr)) | BaseExpr::Return(Some(expr)) => collect_expr(expr, out),
        BaseExpr::Conditional(cond) => {
            for branch in once(&cond.main).chain(&cond.alternatives) {
                collect_expr(&branch.condition, out);
                collect_block(&branch.block, out);
            }
            if let Some(fallback) = &cond.fallback {
                collect_block(fallback, out);
            }
        }
        BaseExpr::FnCall(fcall) => {
            collect_base_expr(&fcall.object, out);
            for arg in &fcall.args {
                collect_expr(arg, out);
            }
        }
        BaseExpr::Parenthesized(paren) => collect_expr(paren, out),
        BaseExpr::Literal(_)
        | BaseExpr::Continue
        | BaseExpr::Break(None)
        | BaseExpr::Return(None) => (),
    }
}

/////////////////////////////////////////////////////
// HOISTING                                        //
/////////////////////////////////////////////////////

#[test]
fn registers_items() {
    let r = resolve(
        "product P[T] {}
         func f { a: i32, b: i32 } {}
         static S: i32 = 1;
         module m { pub product Q {} }",
    );
    let out = r.ok();

    assert!(matches!(
        lookup(out, "P").unwrap().kind(),
        SymbolKind::Type {
            generics: 1,
            subscope: Some(_)
        }
    ));
    assert_eq!(lookup(out, "f").unwrap().kind(), &SymbolKind::Function(2));
    assert_eq!(
        lookup(out, "S").unwrap().kind(),
        &SymbolKind::Variable(true)
    );
    assert!(matches!(
        lookup(out, "m").unwrap().kind(),
        SymbolKind::Module(_)
    ));
    assert!(lookup(out, "m::Q").is_some());
    assert!(lookup(out, "Q").is_none());
}

#[test]
fn imports_land_in_scope() {
    let r = resolve(
        "import a::X;
         module a { import super::b::X; }
         module b { pub product X {} }",
    );
    let out = r.ok();

    let x = lookup(out, "b::X").unwrap().span();
    assert_eq!(lookup(out, "X").unwrap().span(), x);
    assert_eq!(lookup(out, "a::X").unwrap().span(), x);
}

/////////////////////////////////////////////////////
// RESOLUTION                                      //
/////////////////////////////////////////////////////

#[test]
fn primitives() {
    let r = resolve("static S: i32 = 1;");
    assert_eq!(r.uses(), ["i32@10 -> prim I32"]);
}

#[test]
fn shadowing() {
    let r = resolve("func f {} { decl x = 1; decl x = x; x }");
    assert_eq!(r.uses(), ["x@33 -> x@17", "x@36 -> x@29"]);
}

#[test]
fn block_scope_ends() {
    let r = resolve("func f {} { decl x = 1; { decl x = 2; x }; x }");
    assert_eq!(r.uses(), ["x@38 -> x@31", "x@43 -> x@17"]);
}

#[test]
fn args_and_generics() {
    let r = resolve("func id[T] { a: T }: T { a }");
    assert_eq!(r.uses(), ["T@16 -> T@8", "T@21 -> T@8", "a@25 -> a@13"]);
}

#[test]
fn hoisted_fn() {
    let r = resolve("func main {} { helper() } func helper {} {}");
    assert_eq!(r.uses(), ["helper@15 -> helper@31"]);
}

#[test]
fn super_and_crate() {
    let r = resolve(
        "product i {}
module m {
    pub product A { a: super::i }
    static S: crate::i = 1;
}
static T: m::A = 1;",
    );
    assert_eq!(
        r.uses(),
        [
            "super::i@47 -> i@8",
            "crate::i@72 -> i@8",
            "m::A@98 -> A@40",
        ]
    );
}
