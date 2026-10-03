//! Information associated with a symbol.

use source::SourceIdx;
use span::Span;

use crate::{Namespace, ScopeId};

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

impl SymbolKind {
    /// The namespace symbols of this kind live in.
    pub const fn namespace(&self) -> Namespace {
        match self {
            Self::Variable(_) | Self::Function(_) => Namespace::Value,
            Self::Type { .. } | Self::Module(_) => Namespace::Type,
        }
    }
}

/// Symbol info.
#[derive(Clone, Copy, Debug)]
pub struct SymbolInfo {
    kind: SymbolKind,
    used: bool,
    span: Span,
    source: SourceIdx,
}

impl SymbolInfo {
    /// Create a new [`SymbolInfo`] for a symbol defined at `span` in `source`.
    pub const fn new(kind: SymbolKind, span: Span, source: SourceIdx) -> Self {
        Self {
            kind,
            used: false,
            span,
            source,
        }
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

    /// The span of the symbol's definition.
    pub fn span(&self) -> Span {
        self.span
    }

    /// The file the symbol is defined in.
    pub fn source(&self) -> SourceIdx {
        self.source
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
