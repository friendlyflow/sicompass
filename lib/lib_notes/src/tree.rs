//! The in-memory note tree, its stable ids, and its Merkle hashes.
//!
//! # The hash definition is a wire format
//!
//! These digests are the whole point of the tree: a server can compare one root
//! hash instead of diffing documents. That only works if both sides compute the
//! same bytes, so the definition below is a contract, not an implementation
//! detail. Changing it invalidates every stored `.listmeta` and every
//! comparison a peer has already made.
//!
//! ```text
//! hash(leaf)   = sha256( b"s\0" || text )
//! hash(branch) = sha256( b"o\0" || key || b"\0" || d0 || d1 || ... )
//! root         = sha256( b"r\0" || d0 || d1 || ... )
//! ```
//!
//! where `d0..dn` are the children's **raw 32-byte digests** in list order, not
//! their hex forms.
//!
//! The `s` / `o` / `r` prefixes are domain separation. Without them a leaf
//! whose text is `x` and a childless branch whose key is `x` would hash
//! identically, and a tree could be restructured without changing its root.
//!
//! Two things are deliberately **not** hashed:
//!
//! * **Ids**, because they are local bookkeeping. Two machines that built the
//!   same tree independently must agree on its root hash.
//! * **Visibility**, because marking a note public is a change of audience, not
//!   of content. Hashing it would make a peer see a flipped switch as a rewrite
//!   and re-send the whole note.

use sha2::{Digest, Sha256};

/// A node's local identity. Minted once, never reused, never hashed.
///
/// Position cannot serve as identity: inserting a row above a note renumbers
/// it, and anything holding a position (a navigation path, a pending rename)
/// would silently start pointing at a different note.
pub type NodeId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Private,
    Public,
}

impl Visibility {
    /// The stored, language-neutral form. Never localized: this string is
    /// written to disk and read by a server.
    pub fn as_str(self) -> &'static str {
        match self {
            Visibility::Private => "private",
            Visibility::Public => "public",
        }
    }

    /// Unknown values read back as `Private`. A note whose visibility we cannot
    /// understand must not be treated as published.
    pub fn parse(s: &str) -> Self {
        match s {
            "public" => Visibility::Public,
            _ => Visibility::Private,
        }
    }
}

/// One node: a line of text, plus children if it has any.
///
/// A leaf and a childless branch are different things — a branch is somewhere
/// the user can descend into and add to, so `is_branch` is stored rather than
/// derived from `children.is_empty()`.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub id: NodeId,
    pub text: String,
    pub is_branch: bool,
    pub children: Vec<Node>,
    /// Set only on a top-level note, which is the unit of sharing. Deeper nodes
    /// inherit their note's visibility and store `None`.
    pub visibility: Option<Visibility>,
}

impl Node {
    pub fn leaf(id: NodeId, text: impl Into<String>) -> Self {
        Node {
            id,
            text: text.into(),
            is_branch: false,
            children: Vec::new(),
            visibility: None,
        }
    }

    pub fn branch(id: NodeId, text: impl Into<String>) -> Self {
        Node {
            id,
            text: text.into(),
            is_branch: true,
            children: Vec::new(),
            visibility: None,
        }
    }

    pub fn hash(&self) -> [u8; 32] {
        if self.is_branch {
            let mut h = Sha256::new();
            h.update(b"o\0");
            h.update(self.text.as_bytes());
            h.update(b"\0");
            for c in &self.children {
                h.update(c.hash());
            }
            h.finalize().into()
        } else {
            let mut h = Sha256::new();
            h.update(b"s\0");
            h.update(self.text.as_bytes());
            h.finalize().into()
        }
    }

    pub fn hash_hex(&self) -> String {
        hex(&self.hash())
    }

    /// Depth-first search for a node by id, returning a mutable borrow.
    pub fn find_mut(nodes: &mut [Node], id: NodeId) -> Option<&mut Node> {
        for n in nodes.iter_mut() {
            if n.id == id {
                return Some(n);
            }
            if let Some(found) = Node::find_mut(&mut n.children, id) {
                return Some(found);
            }
        }
        None
    }

    pub fn find(nodes: &[Node], id: NodeId) -> Option<&Node> {
        for n in nodes {
            if n.id == id {
                return Some(n);
            }
            if let Some(found) = Node::find(&n.children, id) {
                return Some(found);
            }
        }
        None
    }

    /// The chain of ids from the root down to the list that *contains* `id`.
    ///
    /// `Some(vec![])` means the node is a top-level note. `None` means no node
    /// with that id exists.
    pub fn path_to_parent_of(nodes: &[Node], id: NodeId) -> Option<Vec<NodeId>> {
        for n in nodes {
            if n.id == id {
                return Some(Vec::new());
            }
            if let Some(mut rest) = Node::path_to_parent_of(&n.children, id) {
                let mut path = vec![n.id];
                path.append(&mut rest);
                return Some(path);
            }
        }
        None
    }

    pub fn max_id(nodes: &[Node]) -> NodeId {
        nodes
            .iter()
            .map(|n| n.id.max(Node::max_id(&n.children)))
            .max()
            .unwrap_or(0)
    }
}

/// The whole tree: the top-level notes, plus the id counter.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tree {
    pub notes: Vec<Node>,
    next_id: NodeId,
}

impl Tree {
    pub fn new() -> Self {
        Tree {
            notes: Vec::new(),
            next_id: 1,
        }
    }

    /// Rebuild the counter from what is actually in the tree. Called after a
    /// load, so an id read off disk can never be handed out a second time.
    pub fn reseat_counter(&mut self) {
        self.next_id = Node::max_id(&self.notes) + 1;
    }

    pub fn mint_id(&mut self) -> NodeId {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        id
    }

    /// The root hash: the tree's identity, and what a peer compares first.
    pub fn root_hash(&self) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(b"r\0");
        for n in &self.notes {
            h.update(n.hash());
        }
        h.finalize().into()
    }

    pub fn root_hash_hex(&self) -> String {
        hex(&self.root_hash())
    }

    /// The list at `path` (a chain of node ids from the root), or the top-level
    /// notes for an empty path. `None` when any id along the way is gone.
    pub fn list_at(&self, path: &[NodeId]) -> Option<&Vec<Node>> {
        let mut cur = &self.notes;
        for id in path {
            let node = cur.iter().find(|n| n.id == *id)?;
            cur = &node.children;
        }
        Some(cur)
    }

    pub fn list_at_mut(&mut self, path: &[NodeId]) -> Option<&mut Vec<Node>> {
        let mut cur = &mut self.notes;
        for id in path {
            let idx = cur.iter().position(|n| n.id == *id)?;
            cur = &mut cur[idx].children;
        }
        Some(cur)
    }
}

/// Lowercase hex. Hand-rolled because `hex` is not a workspace dependency and
/// the SDK boundary puts `lib_updater`'s copy out of reach.
pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}
