//! The resolver machinery.

use std::{collections::HashMap, iter::once};

use diagnostic::{Diagnostic, Diagnostics};
use node::{Node, NodeId};
use parser::{
    AstTable, CRATE, SUPER,
    expr::{BaseExpr, Block, Expr},
    item::{
        FnDef, GlobalDef, GlobalDefKind, IncludeDef, InlineModule, Item, ProductDef, SumDef, Type,
    },
    path::Path,
    stmt::Stmt,
};
use source::SourceIdx;

use crate::{
    NameResId, NameResTable, ScopeArena, ScopeId,
    sym_info::{SymbolInfo, SymbolKind},
};

/// A table mapping nodes to subscopes.
#[derive(Default, Debug)]
pub struct SubscopeTable {
    table: HashMap<NodeId, ScopeId>,
}

/// A table mapping nodes to name resolution IDs.
#[derive(Default)]
pub struct ResolutionTable {
    table: HashMap<NodeId, NameResId>,
}

/// The name resolver.
pub struct Resolver<'arena, 'nrt, 'sscopes, 'diag, 'res, 'ast> {
    arena: &'arena mut ScopeArena,
    nrt: &'nrt mut NameResTable,
    subscopes: &'sscopes mut SubscopeTable,
    diagnostics: &'diag mut Diagnostics,
    resolutions: &'res mut ResolutionTable,
    entrypoint: SourceIdx,
    ast_table: &'ast AstTable,
    imports: Vec<(*const Path, ScopeId)>,
    root: ScopeId,
}

/// A resolution error.
#[derive(Debug)]
pub enum ResolutionError {
    /// An error occurred while registering symbols.
    RegError,
    /// An error occurred while resolving symbols.
    ResError,
}

impl<'arena, 'nrt, 'sscopes, 'diag, 'res, 'ast>
    Resolver<'arena, 'nrt, 'sscopes, 'diag, 'res, 'ast>
{
    /// Create a new resolver.
    pub fn new(
        arena: &'arena mut ScopeArena,
        nrt: &'nrt mut NameResTable,
        subscopes: &'sscopes mut SubscopeTable,
        diagnostics: &'diag mut Diagnostics,
        resolutions: &'res mut ResolutionTable,
        ast_table: &'ast AstTable,
        entrypoint: SourceIdx,
    ) -> Self {
        let root = arena.create_scope(None, Some(entrypoint));

        Self {
            arena,
            nrt,
            subscopes,
            diagnostics,
            resolutions,
            entrypoint,
            ast_table,
            root,
            imports: vec![],
        }
    }

    /// Resolve the given inputs.
    pub fn fill(mut self) -> Result<(), ResolutionError> {
        self.register();

        let mut diags = self.diagnostics.len();
        self.register_imports();
        if diags != self.diagnostics.len() {
            return Err(ResolutionError::RegError);
        }

        diags = self.diagnostics.len();
        self.resolve();
        if diags != self.diagnostics.len() {
            return Err(ResolutionError::ResError);
        }

        Ok(())
    }
}

/////////////////////////////////////////////////////
// FIRST PASS // HOISTING                          //
/////////////////////////////////////////////////////

impl<'arena, 'nrt, 'sscopes, 'diag, 'res, 'ast>
    Resolver<'arena, 'nrt, 'sscopes, 'diag, 'res, 'ast>
{
    fn register(&mut self) {
        let entrypoint_items = self.ast_table.by_src(self.entrypoint);

        self.register_module(&entrypoint_items.items, self.root);
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
            // ... so we don't really know whether the thing we're importing goes into the type
            // or value namespace. we'll handle this in the second pass. for now, we'll just make record of it.
            Item::Import(imp) => self.imports.push((imp.path(), scope)),
            Item::Module(module) => {
                let subscope = self.arena.create_scope(Some(scope), None);
                let info = SymbolInfo::new(SymbolKind::Module(subscope));
                self.arena.scopes[scope]
                    .types
                    .insert(module.ident.item, self.nrt.table.push(info));
                self.register_module(&module.module.items, subscope);
            }
            Item::ProductDef(prod) => self.register_prod_def(item.map(|_| prod), scope),
            Item::SumDef(sum) => self.register_sum_def(item.map(|_| sum), scope),
            Item::Include(include) => self.register_include(item.map(|_| include), scope),
        }
    }

    fn register_glob_def(&mut self, def: &parser::item::GlobalDef, scope: ScopeId) {
        let info = SymbolInfo::new(SymbolKind::Variable(def.kind == GlobalDefKind::Static));

        self.arena.scopes[scope]
            .values
            .insert(def.ident.item, self.nrt.table.push(info));

        self.register_expr(&def.value.item, scope);
    }

    fn register_fn_def(&mut self, fndef: Node<&FnDef>, scope: ScopeId) {
        let info = SymbolInfo::new(SymbolKind::Function(fndef.args.len()));
        self.arena.scopes[scope]
            .values
            .insert(fndef.item.ident.item, self.nrt.table.push(info));

        let subscope = self.arena.create_scope(Some(scope), None);
        self.subscopes.table.insert(fndef.id(), subscope);

        if let Some(generics) = &fndef.item.generics {
            let ty_info = SymbolInfo::new(SymbolKind::Type {
                generics: 0,
                subscope: None,
            });

            for generic in &generics.item {
                self.arena.scopes[subscope]
                    .types
                    .insert(generic.item, self.nrt.table.push(ty_info));
            }
        }

        // TODO: change when function argument mutability is introduced
        let arg_info = SymbolInfo::new(SymbolKind::Variable(true));
        for arg in &fndef.args {
            self.arena.scopes[subscope]
                .values
                .insert(arg.ident.item, self.nrt.table.push(arg_info));
        }

        self.register_block(fndef.item.block.as_ref(), scope, Some(subscope));
    }

    fn register_prod_def(&mut self, prod_def: Node<&ProductDef>, scope: ScopeId) {
        let n_generics = prod_def
            .generics
            .as_ref()
            .map(|x| x.len())
            .unwrap_or_default();

        let subscope = self.arena.create_scope(Some(scope), None);

        self.arena.scopes[scope].types.insert(
            prod_def.ident.item,
            self.nrt.table.push(SymbolInfo::new(SymbolKind::Type {
                generics: n_generics,
                subscope: Some(subscope),
            })),
        );

        self.subscopes.table.insert(prod_def.id(), subscope);

        let info = SymbolInfo::new(SymbolKind::Type {
            generics: 0,
            subscope: None,
        });

        for generic in prod_def
            .generics
            .as_ref()
            .map(|x| &x.item)
            .into_iter()
            .flatten()
        {
            self.arena.scopes[subscope]
                .types
                .insert(generic.item, self.nrt.table.push(info));
        }
    }

    fn register_sum_def(&mut self, sum_def: Node<&SumDef>, scope: ScopeId) {
        let n_generics = sum_def
            .generics
            .as_ref()
            .map(|x| x.len())
            .unwrap_or_default();

        let subscope = self.arena.create_scope(Some(scope), None);
        self.arena.scopes[scope].types.insert(
            sum_def.ident.item,
            self.nrt.table.push(SymbolInfo::new(SymbolKind::Type {
                generics: n_generics,
                subscope: Some(subscope),
            })),
        );

        self.subscopes.table.insert(sum_def.id(), subscope);

        let info = SymbolInfo::new(SymbolKind::Type {
            generics: 0,
            subscope: None,
        });

        for generic in sum_def
            .generics
            .as_ref()
            .map(|x| &x.item)
            .into_iter()
            .flatten()
        {
            self.arena.scopes[subscope]
                .types
                .insert(generic.item, self.nrt.table.push(info));
        }
    }

    fn register_block(&mut self, block: Node<&Block>, scope: ScopeId, subscope: Option<ScopeId>) {
        let subscope = subscope.unwrap_or_else(|| self.arena.create_scope(Some(scope), None));
        self.subscopes.table.insert(block.id(), subscope);

        for stmt in &block.stmts {
            self.register_stmt(&stmt.item, subscope);
        }
    }

    fn register_stmt(&mut self, stmt: &Stmt, scope: ScopeId) {
        match stmt {
            Stmt::Binding(_bind) => {
                // we actually don't want to really register bindings, because this would let us
                // reference variables later that are defined later.
                // _ = self.arena.scopes[scope].values.insert(
                //     bind.ident.item,
                //     self.nrt.table.push(SymbolInfo::new(SymbolKind::Variable(
                //         bind.is_mutable.is_some(),
                //     ))),
                // );
            }
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
                for branch in cond.alternatives.iter().chain(once(&cond.main)) {
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
            _ => (),
        }
    }

    fn register_include(&mut self, include: Node<&IncludeDef>, scope: ScopeId) {
        let source_id = self.ast_table.source_idx(include.id());
        let subscope = self.arena.create_scope(Some(scope), Some(source_id));

        self.arena.scopes[scope].types.insert(
            include.ident.item,
            self.nrt
                .table
                .push(SymbolInfo::new(SymbolKind::Module(subscope))),
        );

        self.register_module(&self.ast_table.by_node_id(include.id()).items, subscope);
    }
}

/////////////////////////////////////////////////////
// SECOND PASS // IMPORT REG. + RESOLUTION         //
/////////////////////////////////////////////////////

impl<'arena, 'nrt, 'sscopes, 'diag, 'res, 'ast>
    Resolver<'arena, 'nrt, 'sscopes, 'res, 'diag, 'ast>
{
    fn resolve_path(&self, path: &Path, scope: ScopeId) -> Result<NameResId, Diagnostic> {
        let source_file = self.arena.lookup_root(scope);
        let first = &path.segments[0];

        let (mut nr_id, mut scope) = if first.item == *CRATE {
            let scope = self.arena.lookup_crate_scope(scope);

            (None, scope)
        } else if first.item == *SUPER {
            match self.arena.scopes[scope].parent() {
                Some(parent) => (None, parent),
                None => {
                    return Err(Diagnostic::error(
                        first.span,
                        "the current scope has no parent module".to_owned(),
                        None,
                        source_file,
                    ));
                }
            }
        } else {
            match self
                .arena
                .lookup_any(scope, path.segments[0].item.as_intern_str())
            {
                Some(r) => (Some(r.0), r.1),
                None => {
                    return Err(Diagnostic::error(
                        path.segments[0].span,
                        format!("could not find `{}` in scope", &*path.segments[0].item),
                        None,
                        source_file,
                    ));
                }
            }
        };

        let mut prev_seg = &path.segments[0];

        for segment in &path.segments[1..] {
            let subscope = match nr_id {
                Some(r) => match self.nrt.table[r].projectable() {
                    Some(r) => r,
                    None => {
                        return Err(Diagnostic::error(
                            prev_seg.span,
                            format!("`{}` is not projectable", &*prev_seg.item),
                            None,
                            source_file,
                        ));
                    }
                },
                None => scope,
            };

            let Some(next_nr_id) = self.arena.scopes[scope]
                .types
                .get(&segment.item.as_intern_str())
                .or_else(|| {
                    self.arena.scopes[scope]
                        .values
                        .get(&segment.item.as_intern_str())
                })
            else {
                return Err(Diagnostic::error(
                    segment.span,
                    format!(
                        "`{}` does not exist in `{}`",
                        &*segment.item, &*prev_seg.item
                    ),
                    None,
                    source_file,
                ));
            };

            scope = subscope;
            nr_id = Some(*next_nr_id);
            prev_seg = segment;
        }

        let last_seg = path.segments.last().unwrap();
        let Some((nr_id, _)) = self.arena.lookup_any(scope, last_seg.item.as_intern_str()) else {
            return Err(Diagnostic::error(
                last_seg.span,
                format!(
                    "`{}` does not exist in `{}`",
                    &*last_seg.item, &*prev_seg.item
                ),
                None,
                source_file,
            ));
        };

        Ok(nr_id)
    }

    fn register_imports(&mut self) {
        for import in &self.imports {
            let path = unsafe { &*import.0 };

            let nr_id = match self.resolve_path(path, import.1) {
                Ok(r) => r,
                Err(d) => {
                    self.diagnostics.push(d);
                    continue;
                }
            };
            let info = self.nrt.table[nr_id];
            let target = match info.kind() {
                SymbolKind::Function { .. } | SymbolKind::Variable { .. } => {
                    &mut self.arena.scopes[import.1].values
                }
                _ => &mut self.arena.scopes[import.1].types,
            };

            target.insert(path.segments.last().unwrap().item.as_intern_str(), nr_id);
        }
    }

    fn resolve(&mut self) {
        let inline_module = self.ast_table.by_src(self.entrypoint);

        self.resolve_module(&inline_module, self.root);
    }

    fn resolve_module(&mut self, items: &InlineModule, scope: ScopeId) {
        for item in &items.items {
            self.resolve_item(item.as_ref(), scope)
        }
    }

    fn resolve_item(&mut self, item: Node<&Item>, scope: ScopeId) {
        match item.item {
            Item::ConstDef(def) | Item::StaticDef(def) => {
                self.resolve_glob_def(item.map(|_| def), scope)
            }
            Item::FnDef(def) => self.resolve_fn_def(item.map(|_| def), scope),
            Item::Import(imp) => {
                let nrid = match self.resolve_path(imp.path(), scope) {
                    Ok(r) => r,
                    Err(e) => {
                        self.diagnostics.push(e);
                        return;
                    }
                };

                self.resolutions.table.insert(item.id(), nrid);
            }
            Item::Module(module) => {
                let subscope = self.subscopes.table[&item.id()];
                self.resolve_module(&module.module, subscope);
            }
            Item::ProductDef(prod_def) => self.resolve_prod_def(item.map(|_| prod_def), scope),
            Item::SumDef(sum_def) => self.resolve_sum_def(item.map(|_| sum_def), scope),
            Item::Include(include) => self.resolve_include(item.map(|_| include)),
        }
    }

    fn resolve_glob_def(&mut self, glob_def: Node<&GlobalDef>, scope: ScopeId) {
        self.resolve_ty(&glob_def.ty.item, scope);
        self.resolve_expr(&glob_def.value.item, scope);
    }

    fn resolve_fn_def(&mut self, fn_def: Node<&FnDef>, scope: ScopeId) {
        for field in &fn_def.args {
            self.resolve_ty(&field.ty, scope);
        }

        if let Some(ret) = &fn_def.ret_ty {
            self.resolve_ty(ret, scope);
        }

        self.resolve_block(fn_def.block.as_ref(), scope);
    }

    fn resolve_prod_def(&mut self, prod_def: Node<&ProductDef>, _scope: ScopeId) {
        let subscope = self.subscopes.table[&prod_def.id()];

        for field in &prod_def.fields {
            self.resolve_ty(&field.ty, subscope);
        }
    }

    fn resolve_sum_def(&mut self, sum_def: Node<&SumDef>, _scope: ScopeId) {
        let subscope = self.subscopes.table[&sum_def.id()];

        for field in &sum_def.fields {
            self.resolve_ty(&field.ty, subscope);
        }
    }

    fn resolve_ty(&mut self, ty: &Type, scope: ScopeId) {
        match self.resolve_path(&ty.path, scope) {
            Ok(r) => _ = self.resolutions.table.insert(ty.path.id(), r),
            Err(e) => self.diagnostics.push(e),
        }

        if let Some(generics) = &ty.generics {
            for generic in &generics.item {
                self.resolve_ty(generic, scope);
            }
        }
    }

    fn resolve_include(&mut self, include: Node<&IncludeDef>) {
        let items = self.ast_table.by_node_id(include.id());
        let subscope = self.subscopes.table[&include.id()];

        self.resolve_module(&items, subscope);
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
            BaseExpr::Block(block) | BaseExpr::Loop(block) => {
                self.resolve_block(block.as_ref(), scope)
            }
            BaseExpr::Break(Some(expr)) | BaseExpr::Return(Some(expr)) => {
                self.resolve_expr(expr, scope)
            }
            BaseExpr::Conditional(cond) => {
                for branch in cond.alternatives.iter().chain(once(&cond.main)) {
                    self.resolve_expr(&branch.condition, scope);
                    self.resolve_block(branch.block.as_ref(), scope);
                }

                if let Some(fallback) = &cond.fallback {
                    self.resolve_block(fallback.as_ref(), scope);
                }
            }
            BaseExpr::FnCall(fcall) => {
                self.resolve_base_expr(&fcall.object, scope);
                for arg in &fcall.args {
                    self.resolve_expr(arg, scope);
                }
            }
            BaseExpr::Parenthesized(paren) => self.resolve_expr(paren, scope),
            BaseExpr::Path(path) => {
                let nrid = match self.resolve_path(path, scope) {
                    Ok(r) => r,
                    Err(e) => {
                        self.diagnostics.push(e);
                        return;
                    }
                };

                self.resolutions.table.insert(path.id(), nrid);
            }
            _ => (),
        }
    }

    fn resolve_block(&mut self, block: Node<&Block>, _scope: ScopeId) {
        let subscope = self.subscopes.table[&block.id()];

        for stmt in &block.stmts {
            match &stmt.item {
                Stmt::Binding(bind) => {
                    _ = self.arena.scopes[subscope].values.insert(
                        bind.ident.item,
                        self.nrt.table.push(SymbolInfo::new(SymbolKind::Variable(
                            bind.is_mutable.is_some(),
                        ))),
                    );

                    if let Some(ty) = &bind.ty {
                        self.resolve_ty(ty, subscope);
                    }

                    self.resolve_expr(&bind.value, subscope);
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
