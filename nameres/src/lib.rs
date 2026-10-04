//! The name resolver.

pub mod resolver;
pub mod sym_info;

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use index_vec::{self, IndexVec, define_index_type};
use lexer::Intern;
use source::SourceIdx;

use crate::sym_info::SymbolInfo;

/// A symbol table. This maps symbols to name resolution IDs.
pub type SymbolTable = HashMap<Intern<str>, NameResId>;

define_index_type! {
    /// The ID of a resolved symbol. Points to associated metadata ([`SymbolInfo`]).
    pub struct NameResId = u64;
}

/// A name resolution table. This associates name resolution IDs with symbol information.
#[derive(Default, Debug)]
pub struct NameResTable {
    table: IndexVec<NameResId, SymbolInfo>,
}

impl NameResTable {
    /// Retrieve the information about a symbol.
    pub fn get(&self, id: NameResId) -> &SymbolInfo {
        &self.table[id]
    }
}

define_index_type! {
    /// A pointer to a scope within an arena.
    pub struct ScopeId = u32;
}

/// The kind of a scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeKind {
    /// The root of a crate.
    CrateRoot,
    /// A module (inline or included from a file).
    Module,
    /// The scope of an item, e.g., the generics of a type or the arguments of a function.
    Item,
    /// A block.
    Block,
}

impl ScopeKind {
    /// Whether this scope is a module boundary, i.e., a module or a crate root.
    pub const fn is_module(self) -> bool {
        matches!(self, Self::CrateRoot | Self::Module)
    }
}

/// The namespace of a symbol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Namespace {
    /// Types and modules.
    Type,
    /// Functions and values.
    Value,
}

/// A scope arena.
#[derive(Default, Debug)]
pub struct ScopeArena {
    scopes: IndexVec<ScopeId, Scope>,
}

impl ScopeArena {
    /// Create a scope arena.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a scope with the given parent and kind. `source` must be set if this scope is the
    /// root of a file.
    pub fn create_scope(
        &mut self,
        parent: Option<ScopeId>,
        kind: ScopeKind,
        source: Option<SourceIdx>,
    ) -> ScopeId {
        self.scopes.push(Scope {
            values: HashMap::new(),
            types: HashMap::new(),
            parent,
            kind,
            source,
        })
    }

    /// Retrieve a scope.
    pub fn get(&self, scope: ScopeId) -> &Scope {
        &self.scopes[scope]
    }

    pub(crate) fn table_mut(&mut self, scope: ScopeId, ns: Namespace) -> &mut SymbolTable {
        let scope = &mut self.scopes[scope];
        match ns {
            Namespace::Type => &mut scope.types,
            Namespace::Value => &mut scope.values,
        }
    }

    /// Lookup a symbol in exactly the given scope, preferring types over values.
    pub fn get_any(&self, scope: ScopeId, symbol: Intern<str>) -> Option<NameResId> {
        let scope = &self.scopes[scope];
        scope
            .types
            .get(&symbol)
            .or_else(|| scope.values.get(&symbol))
            .copied()
    }

    /// Lookup a symbol lexically, preferring types over values within each scope.
    ///
    /// This traverses scopes upwards, up to and including the closest module. Items of outer
    /// modules are not visible; they must be imported or referenced via `crate::`/`super::`.
    pub fn lookup_any(&self, start: ScopeId, symbol: Intern<str>) -> Option<NameResId> {
        let mut current = start;
        loop {
            if let Some(id) = self.get_any(current, symbol) {
                return Some(id);
            }

            let scope = &self.scopes[current];
            match scope.parent {
                Some(parent) if !scope.kind.is_module() => current = parent,
                _ => return None,
            }
        }
    }

    /// Retrieve the closest module (or crate root) containing this scope, including itself.
    pub fn nearest_module(&self, mut scope: ScopeId) -> ScopeId {
        while !self.scopes[scope].kind.is_module() {
            scope = self.scopes[scope]
                .parent
                .expect("logic violation: scope outside of a module");
        }

        scope
    }

    /// Retrieve the parent module of the given module. Returns [`None`] for crate roots.
    pub fn parent_module(&self, module: ScopeId) -> Option<ScopeId> {
        self.scopes[module]
            .parent
            .map(|parent| self.nearest_module(parent))
    }

    /// Lookup the root of the crate containing this scope.
    pub fn crate_root(&self, mut scope: ScopeId) -> ScopeId {
        while let Some(parent) = self.scopes[scope].parent {
            scope = parent;
        }

        scope
    }

    /// Lookup the file of this scope.
    pub fn lookup_root(&self, mut scope: ScopeId) -> SourceIdx {
        loop {
            let data = &self.scopes[scope];
            if let Some(source) = data.source {
                return source;
            }

            scope = data
                .parent
                .expect("logic violation: scope without parent or source file reference");
        }
    }
}

/// Represents a lexical scope.
#[derive(Debug)]
pub struct Scope {
    /// Lookup table for functions (since they are called as expressions) and regular values.
    values: SymbolTable,
    /// Lookup table for types.
    types: SymbolTable,
    /// The parent.
    parent: Option<ScopeId>,
    /// The kind of scope.
    kind: ScopeKind,
    /// The file this scope is the root of, if any.
    source: Option<SourceIdx>,
}

impl Scope {
    /// Retrieve the ID of parent scope.
    pub fn parent(&self) -> Option<ScopeId> {
        self.parent
    }

    /// Retrieve the kind of this scope.
    pub fn kind(&self) -> ScopeKind {
        self.kind
    }
}
