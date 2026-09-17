//! Information associated with a symbol.

use crate::ScopeId;

/// A symbol kind.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum SymbolKind {
    /// Variable + mutability.
    Variable(bool),
    /// Function + arity.
    Function(usize),
    /// Type, the scope it defines (if it has one, generics don't), and the # of generics.
    Type {
        /// The # of generics this type carries.
        generics: usize,
        /// Whether it defines a subscope.
        subscope: Option<ScopeId>,
    },
    /// Module and the scope it defines.
    Module(ScopeId),
}

/// Symbol info.
#[derive(Clone, Copy, Debug)]
pub struct SymbolInfo {
    kind: SymbolKind,
    used: bool,
}

impl SymbolInfo {
    /// Create a new [`SymbolInfo`].
    pub const fn new(kind: SymbolKind) -> Self {
        Self { kind, used: false }
    }

    /// Mark this symbol as used.
    pub const fn set_used(&mut self) {
        self.used = true;
    }

    /// Retrieve the kind of this symbol.
    pub fn kind(&self) -> &SymbolKind {
        &self.kind
    }

    /// Check whether this symbol has been marked as used.
    pub fn used(&self) -> bool {
        self.used
    }

    /// Check whether this symbol is projectable, and if so, return the scope ID.
    pub const fn projectable(&self) -> Option<ScopeId> {
        match self.kind {
            SymbolKind::Module(id) => Some(id),
            SymbolKind::Type { subscope, .. } => subscope,
            _ => None,
        }
    }
}
