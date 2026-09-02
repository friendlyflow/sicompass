//! The on-disk store: a directory that mirrors the tree one-to-one.
//!
//! ```text
//! notes/
//! |-- .listmeta        {"sha256":"6f2c...","children":[{"n":1,"id":7,"sha256":"a3f1..."}]}
//! |-- 0001             "Groceries"                     a branch's own file
//! |-- 0001.d/                                          its children
//! |   |-- .listmeta    {"visibility":"private","sha256":"a3f1...","children":[...]}
//! |   |-- 0001         "milk"                          a leaf
//! |   |-- 0002         "Weekend"
//! |   `-- 0002.d/
//! |       |-- .listmeta
//! |       `-- 0001     "bread"
//! `-- 0002             "Ideas"
//! ```
//!
//! A file's contents are the element's text, verbatim, with no trailing
//! newline. The `NNNN` prefix carries order and nothing else: it is renumbered
//! densely on every insert, delete and reorder so that `ls` order is list
//! order. Identity lives in `.listmeta`, not in the name, which is why renaming
//! a note does not have to cascade through its ancestors.
//!
//! `.d` is what keeps the layout legal: POSIX will not hold a file and a
//! directory of the same name in one parent, so a branch's children go in a
//! sibling folder rather than one named after its own file.

use crate::tree::{Node, NodeId, Tree, Visibility};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The sidecar. Named with a leading dot so it sorts away from the numbered
/// entries and is skipped by the `NNNN` filter on load.
pub const LISTMETA: &str = ".listmeta";

/// Suffix for a branch's children folder.
pub const CHILD_DIR_SUFFIX: &str = ".d";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChildMeta {
    /// 1-based position, matching the file name.
    pub n: usize,
    pub id: NodeId,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListMeta {
    /// The hash of the node that owns this list, or the root hash at the top.
    pub sha256: String,
    /// Present only on a top-level note's own folder.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub visibility: Option<String>,
    /// Per-child position, id and hash, so a peer can diff a subtree without
    /// reading a single file's contents.
    #[serde(default)]
    pub children: Vec<ChildMeta>,
}

fn entry_name(n: usize) -> String {
    format!("{n:04}")
}

fn child_dir_name(n: usize) -> String {
    format!("{:04}{CHILD_DIR_SUFFIX}", n)
}

/// A `NNNN` entry name back to its position, or `None` for anything else
/// (`.listmeta`, a `.d` folder, a stray file a user dropped in).
fn parse_entry_name(name: &str) -> Option<usize> {
    if name.len() == 4 && name.bytes().all(|b| b.is_ascii_digit()) {
        name.parse().ok()
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Load
// ---------------------------------------------------------------------------

/// Read the tree at `root`.
///
/// `None` means "could not read", which is **not** the same as "empty". The
/// whole store is rewritten on save, so treating an unreadable directory as an
/// empty tree would delete the user's notes on the next keystroke. A caller
/// that gets `None` must leave the disk alone and say so.
///
/// A missing directory is a different case, and does mean empty: there is
/// nothing to lose yet.
pub fn load_tree(root: &Path) -> Option<Tree> {
    if !root.exists() {
        let mut t = Tree::new();
        t.reseat_counter();
        return Some(t);
    }
    let mut tree = Tree::new();
    tree.notes = load_list(root)?;
    tree.reseat_counter();
    assign_missing_ids(&mut tree);
    load_visibility(root, &mut tree);
    Some(tree)
}

fn read_listmeta(dir: &Path) -> Option<ListMeta> {
    let raw = std::fs::read_to_string(dir.join(LISTMETA)).ok()?;
    serde_json::from_str(&raw).ok()
}

fn load_list(dir: &Path) -> Option<Vec<Node>> {
    // Only the ids are read here. Visibility describes the node that *owns* a
    // list, and a list cannot see its owner, so `load_visibility` stitches it
    // on afterwards from the top down.
    let meta = read_listmeta(dir);

    let mut positions: Vec<usize> = Vec::new();
    for entry in std::fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(n) = parse_entry_name(&name) {
            positions.push(n);
        }
    }
    positions.sort_unstable();

    let mut out = Vec::with_capacity(positions.len());
    for n in positions {
        let file = dir.join(entry_name(n));
        let text = std::fs::read_to_string(&file).ok()?;
        let child_dir = dir.join(child_dir_name(n));
        let is_branch = child_dir.is_dir();
        let children = if is_branch {
            load_list(&child_dir)?
        } else {
            Vec::new()
        };
        // Id 0 marks "not yet known"; `assign_missing_ids` mints a real one
        // once the whole tree is loaded and the counter is above every id on
        // disk. Minting here would risk colliding with an id further down.
        let id = meta
            .as_ref()
            .and_then(|m| m.children.iter().find(|c| c.n == n))
            .map(|c| c.id)
            .unwrap_or(0);
        out.push(Node {
            id,
            text,
            is_branch,
            children,
            visibility: None,
        });
    }

    Some(out)
}

/// Give an id to every node that came off disk without one — a store written by
/// an older version, or one a user edited by hand.
fn assign_missing_ids(tree: &mut Tree) {
    fn walk(nodes: &mut [Node], next: &mut NodeId) {
        for n in nodes.iter_mut() {
            if n.id == 0 {
                n.id = *next;
                *next += 1;
            }
            walk(&mut n.children, next);
        }
    }
    let mut next = Node::max_id(&tree.notes) + 1;
    walk(&mut tree.notes, &mut next);
    tree.reseat_counter();
}

/// Read each top-level note's visibility from its own folder's `.listmeta`.
///
/// Separate from `load_list` because visibility describes the *owner* of a
/// list, and a list does not know who owns it.
pub fn load_visibility(root: &Path, tree: &mut Tree) {
    for (i, note) in tree.notes.iter_mut().enumerate() {
        let dir = root.join(child_dir_name(i + 1));
        let v = read_listmeta(&dir)
            .and_then(|m| m.visibility)
            .map(|s| Visibility::parse(&s));
        note.visibility = Some(v.unwrap_or(Visibility::Private));
    }
}

// ---------------------------------------------------------------------------
// Save
// ---------------------------------------------------------------------------

/// Write the tree to `root`, reconciling rather than rewriting.
///
/// For each directory: write what changed, create what is missing, remove what
/// is no longer in the tree. An unchanged subtree costs one `.listmeta` read,
/// because the stored hash is enough to know its contents already match.
pub fn save_tree(root: &Path, tree: &Tree) -> std::io::Result<()> {
    std::fs::create_dir_all(root)?;
    save_list(root, &tree.notes, None, &tree.root_hash_hex())?;
    Ok(())
}

fn save_list(
    dir: &Path,
    nodes: &[Node],
    visibility: Option<Visibility>,
    own_hash: &str,
) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;

    // No "the hash matches, skip this subtree" shortcut here, tempting as it is.
    // The hash covers content only — visibility is deliberately outside it — so a
    // note switched from private to public leaves every hash in the tree
    // identical, and a skip at the root would mean the change never reached the
    // disk at all. `write_if_changed` below already gives the thing the shortcut
    // was for: an untouched note keeps its bytes and its mtime, so a backup tool
    // does not see the whole tree change on every keystroke.
    let want_visibility = visibility.map(|v| v.as_str().to_owned());

    let mut keep: Vec<String> = vec![LISTMETA.to_owned()];
    let mut children_meta = Vec::with_capacity(nodes.len());

    for (i, node) in nodes.iter().enumerate() {
        let n = i + 1;
        let file = dir.join(entry_name(n));
        keep.push(entry_name(n));
        write_if_changed(&file, &node.text)?;

        let child_dir = dir.join(child_dir_name(n));
        if node.is_branch {
            keep.push(child_dir_name(n));
            save_list(
                &child_dir,
                &node.children,
                node.visibility,
                &node.hash_hex(),
            )?;
        } else if child_dir.exists() {
            // The node used to be a branch and is not any more.
            std::fs::remove_dir_all(&child_dir)?;
        }

        children_meta.push(ChildMeta {
            n,
            id: node.id,
            sha256: node.hash_hex(),
        });
    }

    // Anything not in `keep` belonged to a node that is gone, or to a position
    // the list has shrunk past.
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if keep.contains(&name) {
            continue;
        }
        // Leave anything we do not recognise alone. A user who drops a README
        // into their notes directory should still have it afterwards.
        let is_ours = parse_entry_name(&name).is_some()
            || name
                .strip_suffix(CHILD_DIR_SUFFIX)
                .and_then(parse_entry_name)
                .is_some();
        if !is_ours {
            continue;
        }
        if entry.path().is_dir() {
            std::fs::remove_dir_all(entry.path())?;
        } else {
            std::fs::remove_file(entry.path())?;
        }
    }

    let meta = ListMeta {
        sha256: own_hash.to_owned(),
        visibility: want_visibility,
        children: children_meta,
    };
    let json = serde_json::to_string_pretty(&meta).unwrap_or_default();
    write_if_changed(&dir.join(LISTMETA), &json)
}

/// Write only when the bytes differ, so an untouched note keeps its mtime and a
/// backup tool does not see the whole tree change on every keystroke.
fn write_if_changed(path: &Path, contents: &str) -> std::io::Result<()> {
    if std::fs::read_to_string(path).is_ok_and(|current| current == contents) {
        return Ok(());
    }
    std::fs::write(path, contents)
}

/// The folder holding the children of the note at 1-based position `n`.
pub fn note_child_dir(root: &Path, n: usize) -> PathBuf {
    root.join(child_dir_name(n))
}
