//! The name resolver.

pub mod resolver;
pub mod sym_info;

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

define_index_type! {
    /// A pointer to a scope within an arena.
    pub struct ScopeId = u32;
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

    /// Create a scope with the given parent and whether this is the root of a file.
    pub fn create_scope(&mut self, parent: Option<ScopeId>, root: Option<SourceIdx>) -> ScopeId {
        self.scopes.push(Scope {
            values: HashMap::new(),
            types: HashMap::new(),
            root,
            parent,
        })
    }

    /// Lookup a symbol. This traverses scopes upwards.
    pub fn lookup(&self, start: ScopeId, symbol: Intern<str>) -> Option<(NameResId, ScopeId)> {
        let mut current = Some(start);
        while let Some(id) = current {
            if let Some(nr_id) = self.scopes[id].values.get(&symbol) {
                return Some((*nr_id, id));
            }
            current = self.scopes[id].parent;
        }
        None
    }

    /// Lookup a type. This traverses scopes upwards.
    pub fn lookup_ty(&self, start: ScopeId, symbol: Intern<str>) -> Option<(NameResId, ScopeId)> {
        let mut current = Some(start);
        while let Some(id) = current {
            if let Some(nr_id) = self.scopes[id].types.get(&symbol) {
                return Some((*nr_id, id));
            }

            current = self.scopes[id].parent;
        }
        None
    }

    /// Lookup any kind of symbol. This traverses scopes upwards.
    pub fn lookup_any(&self, start: ScopeId, symbol: Intern<str>) -> Option<(NameResId, ScopeId)> {
        self.lookup_ty(start, symbol)
            .or_else(|| self.lookup(start, symbol))
    }

    /// Lookup the root of the crate containing this scope.
    pub fn lookup_crate_scope(&self, scope_id: ScopeId) -> ScopeId {
        let scope = &self.scopes[scope_id];

        if let Some(parent) = scope.parent {
            self.lookup_crate_scope(parent)
        } else {
            scope_id
        }
    }

    /// Lookup the file of this scope.
    pub fn lookup_root(&self, scope: ScopeId) -> SourceIdx {
        let scope = &self.scopes[scope];

        if let Some(root) = scope.root {
            root
        } else if let Some(parent) = scope.parent {
            self.lookup_root(parent)
        } else {
            panic!("logic violation: scope without parent or source file reference")
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
    /// Whether this is the root of a file.
    root: Option<SourceIdx>,
}

impl Scope {
    /// Retrieve the ID of parent scope.
    pub fn parent(&self) -> Option<ScopeId> {
        self.parent
    }
}
