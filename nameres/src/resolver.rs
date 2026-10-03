//! The resolver machinery.

use std::{
    collections::{HashMap, HashSet},
    iter::once,
};

use diagnostic::{Diagnostic, Diagnostics};
use lexer::Intern;
use node::{Node, NodeId};
use parser::{
    CRATE, SUPER,
    ast_table::AstTable,
    crate_table::{CrateId, CrateTable},
    expr::{BaseExpr, Block, Expr},
    item::{
        FnDef, GlobalDef, GlobalDefKind, IncludeCrate, IncludeDef, InlineModule, Item, Module,
        ProductDef, SumDef, Type,
    },
    path::Path,
    stmt::Stmt,
};
use span::Spanned;

use crate::{
    NameResId, NameResTable, Namespace, ScopeArena, ScopeId, ScopeKind,
    sym_info::{SymbolInfo, SymbolKind},
};

/// A table mapping nodes to subscopes.
#[derive(Default, Debug)]
pub struct SubscopeTable {
    table: HashMap<NodeId, ScopeId>,
}

impl SubscopeTable {
    /// Retrieve the scope defined by a node.
    pub fn get(&self, node: NodeId) -> Option<ScopeId> {
        self.table.get(&node).copied()
    }
}

/// A table mapping nodes to name resolution IDs.
#[derive(Default, Debug)]
pub struct ResolutionTable {
    table: HashMap<NodeId, NameResId>,
}

impl ResolutionTable {
    /// Retrieve the symbol a node resolves to.
    pub fn get(&self, node: NodeId) -> Option<NameResId> {
        self.table.get(&node).copied()
    }
}

/// The tables produced by name resolution.
#[derive(Default, Debug)]
pub struct NameResOutput {
    /// The scopes.
    pub arena: ScopeArena,
    /// Information about every symbol.
    pub nrt: NameResTable,
    /// The scopes defined by nodes.
    pub subscopes: SubscopeTable,
    /// The symbols paths resolve to.
    pub resolutions: ResolutionTable,
}

/// An import that has not been resolved yet.
struct PendingImport {
    path: Path,
    scope: ScopeId,
    node: NodeId,
}

/// The name resolver.
pub struct Resolver<'ast> {
    out: NameResOutput,
    diagnostics: Diagnostics,
    ast_table: &'ast AstTable,
    crate_table: CrateTable,
    entry: CrateId,
    /// The root scope of every registered crate.
    crate_scopes: HashMap<CrateId, ScopeId>,
    /// The crates whose items have been resolved.
    resolved_crates: HashSet<CrateId>,
    imports: Vec<PendingImport>,
}

/// A resolution error.
#[derive(Debug)]
pub enum ResolutionError {
    /// An error occurred while registering symbols.
    RegError,
    /// An error occurred while resolving symbols.
    ResError,
}

impl<'ast> Resolver<'ast> {
    /// Create a new resolver for the crate `entry`.
    pub fn new(
        ast_table: &'ast AstTable,
        crate_table: CrateTable,
        entry: CrateId,
        diagnostics: Diagnostics,
    ) -> Self {
        Self {
            out: NameResOutput::default(),
            diagnostics,
            ast_table,
            crate_table,
            entry,
            crate_scopes: HashMap::new(),
            resolved_crates: HashSet::new(),
            imports: vec![],
        }
    }

    /// Resolve the given inputs. Errors are reported as diagnostics.
    pub fn fill(mut self) -> Result<NameResOutput, ResolutionError> {
        let errors = self.diagnostics.error_count();
        self.register_crate(self.entry);
        self.register_imports();
        if errors != self.diagnostics.error_count() {
            return Err(ResolutionError::RegError);
        }

        self.resolve_crate(self.entry);
        if errors != self.diagnostics.error_count() {
            return Err(ResolutionError::ResError);
        }

        Ok(self.out)
    }

    fn subscope(&self, node: NodeId) -> ScopeId {
        self.out
            .subscopes
            .get(node)
            .expect("logic violation: node without a registered subscope")
    }

    /// Insert `name` into the given scope, reporting a diagnostic if it is already defined.
    fn insert_name(
        &mut self,
        scope: ScopeId,
        ns: Namespace,
        name: Spanned<Intern<str>>,
        id: NameResId,
    ) {
        let Some(prev) = self.out.arena.table_mut(scope, ns).insert(name.item, id) else {
            return;
        };

        let prev = self.out.nrt.table[prev];
        let source = self.out.arena.lookup_root(scope);
        self.diagnostics.push(
            Diagnostic::error(
                name.span,
                format!("`{}` is defined multiple times", name.item),
                None,
                source,
            )
            .with_secondary_in(prev.source(), prev.span()),
        );
    }

    /// Define a new symbol in the given scope.
    fn define(
        &mut self,
        scope: ScopeId,
        name: Spanned<Intern<str>>,
        kind: SymbolKind,
    ) -> NameResId {
        let source = self.out.arena.lookup_root(scope);
        let id = self
            .out
            .nrt
            .table
            .push(SymbolInfo::new(kind, name.span, source));

        self.insert_name(scope, kind.namespace(), name, id);
        id
    }

    /// Define a local variable. Unlike [`Self::define`], this allows shadowing.
    fn bind(&mut self, scope: ScopeId, name: Spanned<Intern<str>>, mutable: bool) {
        let source = self.out.arena.lookup_root(scope);
        let id = self.out.nrt.table.push(SymbolInfo::new(
            SymbolKind::Variable(mutable),
            name.span,
            source,
        ));

        self.out
            .arena
            .table_mut(scope, Namespace::Value)
            .insert(name.item, id);
    }
}

/////////////////////////////////////////////////////
// FIRST PASS // HOISTING                          //
/////////////////////////////////////////////////////

impl Resolver<'_> {
    /// Register the items of a crate, if not done yet, and return its root scope.
    fn register_crate(&mut self, id: CrateId) -> ScopeId {
        if let Some(&scope) = self.crate_scopes.get(&id) {
            return scope;
        }

        let source = self.crate_table.get(id).root;
        let root = self
            .out
            .arena
            .create_scope(None, ScopeKind::CrateRoot, Some(source));
        // insert before registering the items, so that cyclic crate includes terminate
        self.crate_scopes.insert(id, root);

        let ast = self.ast_table.by_src(source);
        self.register_module(&ast.items, root);

        root
    }

    fn register_module(&mut self, items: &[Node<Item>], scope: ScopeId) {
        for item in items {
            self.register_item(item.as_ref(), scope);
        }
    }

    fn register_item(&mut self, item: Node<&Item>, scope: ScopeId) {
        match item.item {
            Item::ConstDef(def) | Item::StaticDef(def) => self.register_glob_def(def, scope),
            Item::FnDef(fndef) => self.register_fn_def(item.map(|_| fndef), scope),
            // we don't know whether the thing we're importing goes into the type or value
            // namespace yet. imports are registered once all items are known.
            Item::Import(imp) => self.imports.push(PendingImport {
                path: imp.path().clone(),
                scope,
                node: item.id(),
            }),
            Item::Module(module) => self.register_inline_module(item.map(|_| module), scope),
            Item::ProductDef(prod) => {
                self.register_type_def(item.id(), prod.ident, prod.generics.as_ref(), scope)
            }
            Item::SumDef(sum) => {
                self.register_type_def(item.id(), sum.ident, sum.generics.as_ref(), scope)
            }
            Item::Include(include) => self.register_include(item.map(|_| include), scope),
            Item::IncludeCrate(include) => {
                self.register_include_crate(item.map(|_| include), scope)
            }
        }
    }

    fn register_glob_def(&mut self, def: &GlobalDef, scope: ScopeId) {
        self.define(
            scope,
            def.ident,
            SymbolKind::Variable(def.kind == GlobalDefKind::Static),
        );

        self.register_expr(&def.value.item, scope);
    }

    fn register_generics(
        &mut self,
        generics: Option<&Spanned<Vec<Spanned<Intern<str>>>>>,
        scope: ScopeId,
    ) {
        for generic in generics.into_iter().flat_map(|x| &x.item) {
            self.define(
                scope,
                *generic,
                SymbolKind::Type {
                    generics: 0,
                    subscope: None,
                },
            );
        }
    }

    fn register_fn_def(&mut self, fndef: Node<&FnDef>, scope: ScopeId) {
        self.define(scope, fndef.ident, SymbolKind::Function(fndef.args.len()));

        let subscope = self
            .out
            .arena
            .create_scope(Some(scope), ScopeKind::Item, None);
        self.out.subscopes.table.insert(fndef.id(), subscope);

        self.register_generics(fndef.generics.as_ref(), subscope);

        // TODO: change when function argument mutability is introduced
        for arg in &fndef.args {
            self.define(subscope, arg.ident, SymbolKind::Variable(true));
        }

        self.register_block(fndef.item.block.as_ref(), scope, Some(subscope));
    }

    fn register_inline_module(&mut self, module: Node<&Module>, scope: ScopeId) {
        let subscope = self
            .out
            .arena
            .create_scope(Some(scope), ScopeKind::Module, None);
        self.define(scope, module.ident, SymbolKind::Module(subscope));
        self.out.subscopes.table.insert(module.id(), subscope);

        self.register_module(&module.module.items, subscope);
    }

    /// Register a product or sum definition.
    fn register_type_def(
        &mut self,
        node: NodeId,
        ident: Spanned<Intern<str>>,
        generics: Option<&Spanned<Vec<Spanned<Intern<str>>>>>,
        scope: ScopeId,
    ) {
        let subscope = self
            .out
            .arena
            .create_scope(Some(scope), ScopeKind::Item, None);
        self.define(
            scope,
            ident,
            SymbolKind::Type {
                generics: generics.map(|x| x.len()).unwrap_or_default(),
                subscope: Some(subscope),
            },
        );
        self.out.subscopes.table.insert(node, subscope);

        self.register_generics(generics, subscope);
    }

    fn register_block(&mut self, block: Node<&Block>, scope: ScopeId, subscope: Option<ScopeId>) {
        let subscope = subscope.unwrap_or_else(|| {
            self.out
                .arena
                .create_scope(Some(scope), ScopeKind::Block, None)
        });
        self.out.subscopes.table.insert(block.id(), subscope);

        for stmt in &block.stmts {
            self.register_stmt(&stmt.item, subscope);
        }

        if let Some(tail) = &block.tail {
            self.register_expr(&tail.item, subscope);
        }
    }

    fn register_stmt(&mut self, stmt: &Stmt, scope: ScopeId) {
        match stmt {
            // the binding itself is registered while resolving, so that it cannot be referenced
            // before its definition. its value may contain blocks, though.
            Stmt::Binding(bind) => self.register_expr(&bind.value.item, scope),
            Stmt::ExprSemi(ex) => self.register_expr(ex, scope),
            Stmt::Item(item) => self.register_item(item.as_ref(), scope),
        }
    }

    fn register_expr(&mut self, expr: &Expr, scope: ScopeId) {
        match expr {
            Expr::Base(base) => self.register_base_expr(base, scope),
            Expr::Binary(binary) => {
                self.register_expr(&binary.lhs, scope);
                self.register_expr(&binary.rhs, scope);
            }
        }
    }

    fn register_base_expr(&mut self, expr: &BaseExpr, scope: ScopeId) {
        match expr {
            BaseExpr::Block(block) | BaseExpr::Loop(block) => {
                self.register_block(block.as_ref(), scope, None)
            }
            BaseExpr::Break(Some(ex)) | BaseExpr::Return(Some(ex)) => {
                self.register_expr(&ex.item, scope)
            }
            BaseExpr::Conditional(cond) => {
                for branch in once(&cond.main).chain(&cond.alternatives) {
                    self.register_expr(&branch.condition, scope);
                    self.register_block(branch.block.as_ref(), scope, None);
                }

                if let Some(fallback) = &cond.fallback {
                    self.register_block(fallback.as_ref(), scope, None);
                }
            }
            BaseExpr::FnCall(fcall) => {
                self.register_base_expr(&fcall.object.item, scope);
                for arg in &fcall.args {
                    self.register_expr(&arg.item, scope);
                }
            }
            BaseExpr::Parenthesized(paren) => self.register_expr(paren, scope),
            BaseExpr::Literal(_)
            | BaseExpr::Path(_)
            | BaseExpr::Continue
            | BaseExpr::Break(None)
            | BaseExpr::Return(None) => (),
        }
    }

    fn register_include(&mut self, include: Node<&IncludeDef>, scope: ScopeId) {
        let source_id = self.ast_table.source_idx(include.id());
        let subscope = self
            .out
            .arena
            .create_scope(Some(scope), ScopeKind::Module, Some(source_id));

        self.define(scope, include.ident, SymbolKind::Module(subscope));
        self.out.subscopes.table.insert(include.id(), subscope);

        let ast = self.ast_table.by_node_id(include.id());
        self.register_module(&ast.items, subscope);
    }

    /// Register an `include crate ...;` definition and the underlying crate.
    fn register_include_crate(&mut self, include: Node<&IncludeCrate>, scope: ScopeId) {
        // unknown crates have already been reported by the parser
        let Some(crate_id) = include.crate_id else {
            return;
        };

        let root = self.register_crate(crate_id);
        self.define(scope, include.ident, SymbolKind::Module(root));
        self.out.subscopes.table.insert(include.id(), root);
    }

    /// Resolve and register all imports.
    ///
    /// Imports may refer to other imports, so this is repeated until no more progress is made.
    fn register_imports(&mut self) {
        let mut pending = std::mem::take(&mut self.imports);

        loop {
            let before = pending.len();
            let mut failed = vec![];
            let mut errors = vec![];

            for import in pending {
                match self.resolve_path(&import.path, import.scope) {
                    Ok(id) => {
                        let ns = self.out.nrt.table[id].kind().namespace();
                        let last = import.path.segments.last().unwrap();
                        let name = Spanned::new(last.span, last.as_intern_str());

                        self.insert_name(import.scope, ns, name, id);
                        self.out.resolutions.table.insert(import.node, id);
                    }
                    Err(e) => {
                        failed.push(import);
                        errors.push(e);
                    }
                }
            }

            if failed.is_empty() {
                break;
            }

            if failed.len() == before {
                for error in errors {
                    self.diagnostics.push(error);
                }
                break;
            }

            pending = failed;
        }
    }
}

/////////////////////////////////////////////////////
// SECOND PASS // RESOLUTION                       //
/////////////////////////////////////////////////////

impl Resolver<'_> {
    fn resolve_path(&self, path: &Path, scope: ScopeId) -> Result<NameResId, Diagnostic> {
        let arena = &self.out.arena;
        let source_file = arena.lookup_root(scope);
        let error = |span, msg: String| Diagnostic::error(span, msg, None, source_file);

        let first = &path.segments[0];
        let mut prev = first;

        // the last resolved symbol, or the scope `crate`/`super` refers to
        let mut last = if first.item == *CRATE {
            Err(arena.crate_root(scope))
        } else if first.item == *SUPER {
            match arena.parent_module(arena.nearest_module(scope)) {
                Some(parent) => Err(parent),
                None => {
                    return Err(error(
                        first.span,
                        "`super` cannot be used at the root of a crate".to_owned(),
                    ));
                }
            }
        } else {
            match arena.lookup_any(scope, first.as_intern_str()) {
                Some(id) => Ok(id),
                None => {
                    return Err(error(
                        first.span,
                        format!("could not find `{}` in scope", &*first.item),
                    ));
                }
            }
        };

        for segment in &path.segments[1..] {
            let container = match last {
                Err(scope) => scope,
                Ok(id) => self.out.nrt.table[id].projectable().ok_or_else(|| {
                    error(
                        prev.span,
                        format!("`{}` is not a module or a type", &*prev.item),
                    )
                })?,
            };

            let id = arena
                .get_any(container, segment.as_intern_str())
                .ok_or_else(|| {
                    error(
                        segment.span,
                        format!("`{}` does not exist in `{}`", &*segment.item, &*prev.item),
                    )
                })?;

            last = Ok(id);
            prev = segment;
        }

        last.map_err(|_| {
            error(
                prev.span,
                format!("`{}` cannot be used on its own", &*prev.item),
            )
        })
    }

    fn resolve_crate(&mut self, id: CrateId) {
        if !self.resolved_crates.insert(id) {
            return;
        }

        let scope = self.crate_scopes[&id];
        let ast = self.ast_table.by_src(self.crate_table.get(id).root);
        self.resolve_module(&ast, scope);
    }

    fn resolve_module(&mut self, items: &InlineModule, scope: ScopeId) {
        for item in &items.items {
            self.resolve_item(item.as_ref(), scope)
        }
    }

    fn resolve_item(&mut self, item: Node<&Item>, scope: ScopeId) {
        match item.item {
            Item::ConstDef(def) | Item::StaticDef(def) => {
                self.resolve_ty(&def.ty.item, scope);
                self.resolve_expr(&def.value.item, scope);
            }
            Item::FnDef(def) => self.resolve_fn_def(item.map(|_| def)),
            // imports have already been resolved while registering them
            Item::Import(_) => (),
            Item::Module(module) => {
                let subscope = self.subscope(item.id());
                self.resolve_module(&module.module, subscope);
            }
            Item::ProductDef(ProductDef { fields, .. }) | Item::SumDef(SumDef { fields, .. }) => {
                let subscope = self.subscope(item.id());
                for field in fields {
                    self.resolve_ty(&field.ty, subscope);
                }
            }
            Item::Include(_) => {
                let subscope = self.subscope(item.id());
                let items = self.ast_table.by_node_id(item.id());
                self.resolve_module(&items, subscope);
            }
            Item::IncludeCrate(include) => {
                if let Some(id) = include.crate_id {
                    self.resolve_crate(id);
                }
            }
        }
    }

    fn resolve_fn_def(&mut self, fn_def: Node<&FnDef>) {
        let subscope = self.subscope(fn_def.id());

        for field in &fn_def.args {
            self.resolve_ty(&field.ty, subscope);
        }

        if let Some(ret) = &fn_def.ret_ty {
            self.resolve_ty(ret, subscope);
        }

        self.resolve_block(fn_def.block.as_ref());
    }

    fn resolve_ty(&mut self, ty: &Type, scope: ScopeId) {
        match self.resolve_path(&ty.path, scope) {
            Ok(r) => _ = self.out.resolutions.table.insert(ty.path.id(), r),
            Err(e) => self.diagnostics.push(e),
        }

        if let Some(generics) = &ty.generics {
            for generic in &generics.item {
                self.resolve_ty(generic, scope);
            }
        }
    }

    fn resolve_expr(&mut self, expr: &Expr, scope: ScopeId) {
        match expr {
            Expr::Base(base) => self.resolve_base_expr(base, scope),
            Expr::Binary(bin) => {
                self.resolve_expr(&bin.lhs, scope);
                self.resolve_expr(&bin.rhs, scope);
            }
        }
    }

    fn resolve_base_expr(&mut self, expr: &BaseExpr, scope: ScopeId) {
        match expr {
            BaseExpr::Block(block) | BaseExpr::Loop(block) => self.resolve_block(block.as_ref()),
            BaseExpr::Break(Some(expr)) | BaseExpr::Return(Some(expr)) => {
                self.resolve_expr(expr, scope)
            }
            BaseExpr::Conditional(cond) => {
                for branch in once(&cond.main).chain(&cond.alternatives) {
                    self.resolve_expr(&branch.condition, scope);
                    self.resolve_block(branch.block.as_ref());
                }

                if let Some(fallback) = &cond.fallback {
                    self.resolve_block(fallback.as_ref());
                }
            }
            BaseExpr::FnCall(fcall) => {
                self.resolve_base_expr(&fcall.object, scope);
                for arg in &fcall.args {
                    self.resolve_expr(arg, scope);
                }
            }
            BaseExpr::Parenthesized(paren) => self.resolve_expr(paren, scope),
            BaseExpr::Path(path) => match self.resolve_path(path, scope) {
                Ok(id) => _ = self.out.resolutions.table.insert(path.id(), id),
                Err(e) => self.diagnostics.push(e),
            },
            BaseExpr::Literal(_)
            | BaseExpr::Continue
            | BaseExpr::Break(None)
            | BaseExpr::Return(None) => (),
        }
    }

    fn resolve_block(&mut self, block: Node<&Block>) {
        let subscope = self.subscope(block.id());

        for stmt in &block.stmts {
            match &stmt.item {
                Stmt::Binding(bind) => {
                    if let Some(ty) = &bind.ty {
                        self.resolve_ty(ty, subscope);
                    }

                    // resolve the value first, so that it cannot refer to the binding itself
                    self.resolve_expr(&bind.value, subscope);
                    self.bind(subscope, bind.ident, bind.is_mutable.is_some());
                }
                Stmt::ExprSemi(expr) => self.resolve_expr(expr, subscope),
                Stmt::Item(item) => self.resolve_item(item.as_ref(), subscope),
            }
        }

        if let Some(tail) = &block.tail {
            self.resolve_expr(tail, subscope);
        }
    }
}
