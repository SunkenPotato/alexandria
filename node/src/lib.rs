//! Types for representing AST nodes.
//!
//! See [`Node`] for more information.

use std::sync::atomic::AtomicU32;

use derive_more::{Deref, DerefMut};
use span::{Span, Spanned};

/// A unique node ID.
///
/// There is a guarantee that every node ID is unique. Cloned IDs are not unique.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct NodeId(u32);

impl Default for NodeId {
    fn default() -> Self {
        Self::new()
    }
}

impl NodeId {
    /// Create a new node ID.
    ///
    /// It is guaranteed that any created ID will be unique (until cloned or copied).
    pub fn new() -> Self {
        static NEXT_NODE_ID: AtomicU32 = AtomicU32::new(0);

        NodeId(NEXT_NODE_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
    }
}

/// An AST node.
///
/// The unique node ID may be accessed through [`Node::id`].
#[derive(Clone, Copy, Debug, Deref, DerefMut)]
pub struct Node<T> {
    /// The item.
    #[deref]
    #[deref_mut]
    pub spanned: Spanned<T>,
    id: NodeId,
}

impl<T> From<Spanned<T>> for Node<T> {
    fn from(value: Spanned<T>) -> Self {
        Node::new(value.span, value.item)
    }
}

impl<T> Node<T> {
    /// Create a new node with the given span.
    pub fn new(span: Span, item: T) -> Self {
        Self {
            spanned: Spanned::new(span, item),
            id: NodeId::new(),
        }
    }

    /// Obtain the [`NodeId`] of this Node.
    pub const fn id(&self) -> NodeId {
        self.id
    }

    /// Convert this `Node<T>` into a `Node<U>`.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Node<U> {
        Node {
            spanned: self.spanned.map(f),
            id: self.id,
        }
    }

    /// Convert a `Node<T>` into a `Node<&T>`.
    pub fn as_ref(&self) -> Node<&T> {
        Node {
            spanned: self.spanned.as_ref(),
            id: self.id,
        }
    }
}

impl<T> PartialEq for Node<T>
where
    T: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        // not the node id!
        self.item == other.item && self.span == other.span
    }
}
