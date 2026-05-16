//! Shared OS-trash snapshot/restore helpers.
//!
//! Filesystem providers (`lib_filebrowser`, `lib_texteditor`) move deleted
//! items to the OS trash. To make a delete undoable even after the trash is
//! emptied, they snapshot the target's content into an [`FsSideEffect`] before
//! deleting, and replay that snapshot on undo. The logic is identical across
//! providers, so it lives here next to the [`TrashedTree`] / [`FsSideEffect`]
//! types it produces.

use crate::timeline::{FsSideEffect, TrashedTree};
use std::path::Path;

/// Skip building a `TrashedTree` snapshot above this size — the OS trash
/// becomes the source of truth for restoration. If the trash no longer has
/// the file at undo time, the undo reports an error.
pub const TRASH_SNAPSHOT_LIMIT_BYTES: u64 = 4 * 1024 * 1024;

/// Recursively snapshot a directory's contents, bailing out (returning `None`)
/// once the cumulative byte count exceeds `budget`.
pub fn snapshot_dir_capped(root: &Path, budget: &mut u64) -> Option<TrashedTree> {
    let mut children: Vec<(String, TrashedTree)> = Vec::new();
    let entries = std::fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = entry.path();
        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(_) => return None,
        };
        if meta.is_dir() {
            let sub = snapshot_dir_capped(&path, budget)?;
            children.push((name, sub));
        } else {
            let size = meta.len();
            if size > *budget {
                return None;
            }
            *budget -= size;
            let bytes = std::fs::read(&path).ok()?;
            children.push((name, TrashedTree::File(bytes)));
        }
    }
    Some(TrashedTree::Dir(children))
}

/// Write a [`TrashedTree`] snapshot back to disk rooted at `root`.
pub fn restore_trashed_tree(root: &Path, tree: &TrashedTree) -> std::io::Result<()> {
    match tree {
        TrashedTree::File(bytes) => {
            if let Some(parent) = root.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(root, bytes)
        }
        TrashedTree::Dir(children) => {
            std::fs::create_dir_all(root)?;
            for (name, child) in children {
                restore_trashed_tree(&root.join(name), child)?;
            }
            Ok(())
        }
    }
}

/// Best-effort restore of `original` from the OS trash. Used by undo of an
/// oversized (`RenameOnly`) delete, which has no in-app content snapshot to
/// write back. Picks the most recently trashed item whose original location
/// matches `original`. `Err` carries a human-readable reason for the caller
/// to surface alongside the manual-restore hint.
#[cfg(any(
    target_os = "windows",
    all(unix, not(target_os = "macos"), not(target_os = "ios"), not(target_os = "android"))
))]
pub fn restore_from_os_trash(original: &Path) -> Result<(), String> {
    if original.exists() {
        return Err("the original path is already occupied".to_owned());
    }
    let items = trash::os_limited::list().map_err(|e| e.to_string())?;
    // A path may have been deleted more than once; restore the newest.
    let item = items
        .into_iter()
        .filter(|it| it.original_path() == original)
        .max_by_key(|it| it.time_deleted)
        .ok_or_else(|| "no matching item found in the OS trash".to_owned())?;
    trash::os_limited::restore_all([item]).map_err(|e| e.to_string())
}

/// Platforms without `trash::os_limited` (macOS) cannot restore programmatically.
#[cfg(not(any(
    target_os = "windows",
    all(unix, not(target_os = "macos"), not(target_os = "ios"), not(target_os = "android"))
)))]
pub fn restore_from_os_trash(_original: &Path) -> Result<(), String> {
    Err("automatic OS-trash restore is unsupported on this platform".to_owned())
}

/// Snapshot `full` before it is moved to the OS trash, capturing enough to
/// undo the delete. Directories larger than [`TRASH_SNAPSHOT_LIMIT_BYTES`] —
/// and files just as large — fall back to `RenameOnly`, whose undo relies on
/// the OS trash. A non-existent path yields `None`.
pub fn snapshot_for_delete(full: &Path) -> FsSideEffect {
    let meta = match std::fs::metadata(full) {
        Ok(m) => m,
        Err(_) => return FsSideEffect::None,
    };
    if meta.is_dir() {
        let mut budget = TRASH_SNAPSHOT_LIMIT_BYTES;
        match snapshot_dir_capped(full, &mut budget) {
            Some(tree) => FsSideEffect::TrashedDir {
                original_path: full.to_path_buf(),
                content_tree: tree,
            },
            None => FsSideEffect::RenameOnly {
                from: full.to_path_buf(),
                to: full.to_path_buf(),
            },
        }
    } else {
        match std::fs::read(full) {
            Ok(bytes) if (bytes.len() as u64) <= TRASH_SNAPSHOT_LIMIT_BYTES => {
                FsSideEffect::TrashedFile {
                    original_path: full.to_path_buf(),
                    content_snapshot: bytes,
                }
            }
            _ => FsSideEffect::RenameOnly {
                from: full.to_path_buf(),
                to: full.to_path_buf(),
            },
        }
    }
}

/// Reverse a delete by replaying its [`FsSideEffect`] snapshot. Writes a
/// human-readable reason into `error` if the restore fails.
pub fn restore_side_effect(side_effect: &FsSideEffect, error: &mut String) {
    match side_effect {
        FsSideEffect::TrashedFile { original_path, content_snapshot } => {
            if let Some(parent) = original_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = std::fs::write(original_path, content_snapshot) {
                *error = format!("undo delete: write failed: {e}");
            }
        }
        FsSideEffect::TrashedDir { original_path, content_tree } => {
            if let Err(e) = restore_trashed_tree(original_path, content_tree) {
                *error = format!("undo delete: dir restore failed: {e}");
            }
        }
        FsSideEffect::RenameOnly { from, .. } => {
            // Snapshot was oversized — there is no in-app copy to write back.
            // Best-effort: ask the OS trash to restore the item. If that is
            // unavailable or fails, point the user at the manual restore path.
            if let Err(reason) = restore_from_os_trash(from) {
                *error = format!(
                    "undo delete: could not auto-restore {} ({reason}); \
                     please restore it from the OS trash",
                    from.display()
                );
            }
        }
        FsSideEffect::None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn snapshot_for_delete_file_captures_bytes() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("a.txt");
        std::fs::write(&file, b"hello").unwrap();
        match snapshot_for_delete(&file) {
            FsSideEffect::TrashedFile { content_snapshot, .. } => {
                assert_eq!(content_snapshot, b"hello");
            }
            other => panic!("expected TrashedFile, got {other:?}"),
        }
    }

    #[test]
    fn snapshot_for_delete_dir_captures_tree() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("d");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("inner.txt"), b"nested").unwrap();
        match snapshot_for_delete(&dir) {
            FsSideEffect::TrashedDir { content_tree: TrashedTree::Dir(children), .. } => {
                assert_eq!(children.len(), 1);
                assert_eq!(children[0].0, "inner.txt");
            }
            other => panic!("expected TrashedDir, got {other:?}"),
        }
    }

    #[test]
    fn snapshot_for_delete_oversized_file_is_rename_only() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("huge.bin");
        std::fs::write(&file, vec![0u8; (TRASH_SNAPSHOT_LIMIT_BYTES + 1) as usize]).unwrap();
        assert!(matches!(
            snapshot_for_delete(&file),
            FsSideEffect::RenameOnly { .. }
        ));
    }

    #[test]
    fn snapshot_for_delete_missing_path_is_none() {
        let tmp = TempDir::new().unwrap();
        assert!(matches!(
            snapshot_for_delete(&tmp.path().join("nope")),
            FsSideEffect::None
        ));
    }

    #[test]
    fn restore_trashed_tree_roundtrip() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join("d");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("inner.txt"), b"nested").unwrap();
        let sub = dir.join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::fs::write(sub.join("deep.txt"), b"deeper").unwrap();

        let mut budget = TRASH_SNAPSHOT_LIMIT_BYTES;
        let tree = snapshot_dir_capped(&dir, &mut budget).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();

        restore_trashed_tree(&dir, &tree).unwrap();
        assert_eq!(std::fs::read(dir.join("inner.txt")).unwrap(), b"nested");
        assert_eq!(std::fs::read(sub.join("deep.txt")).unwrap(), b"deeper");
    }

    #[test]
    fn restore_side_effect_writes_file() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("a.txt");
        let se = FsSideEffect::TrashedFile {
            original_path: file.clone(),
            content_snapshot: b"restored".to_vec(),
        };
        let mut err = String::new();
        restore_side_effect(&se, &mut err);
        assert!(err.is_empty(), "unexpected error: {err}");
        assert_eq!(std::fs::read(&file).unwrap(), b"restored");
    }
}
