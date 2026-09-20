//! The on-disk store: a directory that mirrors the board one-to-one.
//!
//! ```text
//! projectmanagement/
//! |-- .listmeta        {"children":[{"n":1,"id":1},{"n":2,"id":4,"archive":true}]}
//! |-- 0001             "To do"                    a column's own file
//! |-- 0001.d/                                     its cards
//! |   |-- .listmeta    {"children":[{"n":1,"id":2},{"n":2,"id":3}]}
//! |   |-- 0001         "fix login"
//! |   `-- 0002         "write docs"
//! |-- 0002             "Doing"
//! `-- 0002.d/
//!     |-- .listmeta
//!     `-- 0001         "kanban ui"
//! ```
//!
//! Deliberately the same layout as `lib_notes`, so a board is as inspectable,
//! greppable and mergeable as a note tree, minus that provider's Merkle chain: a
//! board has no peer-sync story to justify hashing every level.
//!
//! A file's contents are the card's or column's text, verbatim, with no trailing
//! newline. The `NNNN` prefix carries order and nothing else: it is renumbered
//! densely on every insert, delete and reorder so that `ls` order is board order.
//! Identity lives in `.listmeta`, not in the name, which is why renaming a column
//! does not have to touch a single card.
//!
//! `.d` is what keeps the layout legal: POSIX will not hold a file and a
//! directory of the same name in one parent, so a column's cards go in a sibling
//! folder rather than one named after the column's own file.

use crate::board::{Board, Card, Column, Id};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The sidecar. Named with a leading dot so it sorts away from the numbered
/// entries and is skipped by the `NNNN` filter on load.
pub const LISTMETA: &str = ".listmeta";

/// Suffix for a column's card folder.
pub const CHILD_DIR_SUFFIX: &str = ".d";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChildMeta {
    /// 1-based position, matching the file name.
    pub n: usize,
    pub id: Id,
    /// True for the one column that is the archive. Only ever set in the root
    /// `.listmeta`, because only a column can be the archive.
    ///
    /// Skipped when false rather than written as `false` everywhere. Without
    /// that, the first save by this version would rewrite every sidecar in the
    /// store, including one per card directory, to record a flag nobody set, and
    /// `write_if_changed` exists precisely so an untouched board keeps its bytes.
    #[serde(default, skip_serializing_if = "is_not_archive")]
    pub archive: bool,
}

fn is_not_archive(flag: &bool) -> bool {
    !*flag
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ListMeta {
    /// Per-child position and id, so identity survives a rename or a reorder.
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

/// Read the board at `root`.
///
/// `None` means "could not read", which is **not** the same as "empty". The
/// whole store is reconciled on save, so treating an unreadable directory as an
/// empty board would delete the user's work on the next keystroke. A caller that
/// gets `None` must leave the disk alone and say so.
///
/// A missing directory is a different case, and does mean empty: there is
/// nothing to lose yet.
pub fn load_board(root: &Path) -> Option<Board> {
    if !root.exists() {
        let mut b = Board::new();
        b.reseat_counter();
        return Some(b);
    }
    let mut board = Board::new();
    let (columns, archive_at) = load_columns(root)?;
    board.columns = columns;
    // Ids first. The archive column may have come off disk without one -- an
    // older store, or a hand edit -- and `assign_missing_ids` is what fills those
    // in, so its id is not knowable until this has run. Hence a *position* on the
    // way out of `load_columns` and an id only here.
    board.assign_missing_ids();
    if let Some(i) = archive_at
        && let Some(id) = board.columns.get(i).map(|c| c.id)
    {
        board.set_archive(id);
    }
    Some(board)
}

fn read_listmeta(dir: &Path) -> Option<ListMeta> {
    let raw = std::fs::read_to_string(dir.join(LISTMETA)).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Positions of the `NNNN` entries in `dir`, in order.
fn positions(dir: &Path) -> Option<Vec<usize>> {
    let mut out: Vec<usize> = Vec::new();
    for entry in std::fs::read_dir(dir).ok()? {
        let entry = entry.ok()?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(n) = parse_entry_name(&name) {
            out.push(n);
        }
    }
    out.sort_unstable();
    Some(out)
}

/// Id recorded for position `n`, or 0 for "not yet known".
///
/// Zero rather than a freshly minted id, because minting here would risk
/// colliding with an id further down that has not been read yet.
/// `Board::assign_missing_ids` fills these in once the whole board is loaded.
fn id_at(meta: &Option<ListMeta>, n: usize) -> Id {
    meta.as_ref()
        .and_then(|m| m.children.iter().find(|c| c.n == n))
        .map(|c| c.id)
        .unwrap_or(0)
}

/// The columns, and the position of the one flagged as the archive.
///
/// A position rather than an id, because an id read here can still be 0 --
/// see [`load_board`]. A second flagged child (only a hand edit can produce one)
/// is ignored: the first wins and the rest stay ordinary columns.
fn load_columns(dir: &Path) -> Option<(Vec<Column>, Option<usize>)> {
    let meta = read_listmeta(dir);
    let mut out = Vec::new();
    let mut archive_at = None;
    for n in positions(dir)? {
        let title = std::fs::read_to_string(dir.join(entry_name(n))).ok()?;
        let card_dir = dir.join(child_dir_name(n));
        let cards = if card_dir.is_dir() {
            load_cards(&card_dir)?
        } else {
            Vec::new()
        };
        if archive_at.is_none() && is_archive_at(&meta, n) {
            archive_at = Some(out.len());
        }
        out.push(Column {
            id: id_at(&meta, n),
            title,
            cards,
        });
    }
    Some((out, archive_at))
}

/// Whether the child at position `n` carries the archive flag.
fn is_archive_at(meta: &Option<ListMeta>, n: usize) -> bool {
    meta.as_ref()
        .and_then(|m| m.children.iter().find(|c| c.n == n))
        .is_some_and(|c| c.archive)
}

/// Cards are leaves, so a `NNNN.d` folder inside a card directory is not read.
/// That is the depth cap holding on disk as well as in memory: a third level
/// hand-created by a user is ignored rather than silently loaded and then
/// deleted by the next save.
fn load_cards(dir: &Path) -> Option<Vec<Card>> {
    let meta = read_listmeta(dir);
    let mut out = Vec::new();
    for n in positions(dir)? {
        let text = std::fs::read_to_string(dir.join(entry_name(n))).ok()?;
        out.push(Card {
            id: id_at(&meta, n),
            text,
        });
    }
    Some(out)
}

// ---------------------------------------------------------------------------
// Save
// ---------------------------------------------------------------------------

/// Write the board to `root`, reconciling rather than rewriting: write what
/// changed, create what is missing, remove what is no longer on the board.
pub fn save_board(root: &Path, board: &Board) -> std::io::Result<()> {
    std::fs::create_dir_all(root)?;

    let mut keep: Vec<String> = vec![LISTMETA.to_owned()];
    let mut children = Vec::with_capacity(board.columns.len());

    for (i, col) in board.columns.iter().enumerate() {
        let n = i + 1;
        keep.push(entry_name(n));
        write_if_changed(&root.join(entry_name(n)), &col.title)?;

        let card_dir = root.join(child_dir_name(n));
        keep.push(child_dir_name(n));
        save_cards(&card_dir, &col.cards)?;

        children.push(ChildMeta {
            n,
            id: col.id,
            archive: board.is_archive(col.id),
        });
    }

    prune(root, &keep)?;
    write_meta(root, children)
}

fn save_cards(dir: &Path, cards: &[Card]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let mut keep: Vec<String> = vec![LISTMETA.to_owned()];
    let mut children = Vec::with_capacity(cards.len());
    for (i, card) in cards.iter().enumerate() {
        let n = i + 1;
        keep.push(entry_name(n));
        write_if_changed(&dir.join(entry_name(n)), &card.text)?;
        children.push(ChildMeta {
            n,
            id: card.id,
            // Only a column can be the archive, so a card's meta never carries
            // the flag and `skip_serializing_if` keeps it out of the bytes.
            archive: false,
        });
    }
    prune(dir, &keep)?;
    write_meta(dir, children)
}

/// Remove what belonged to an entry that is gone, or to a position the list has
/// shrunk past.
///
/// Anything unrecognised is left alone. A user who drops a README into their
/// board directory should still have it afterwards.
fn prune(dir: &Path, keep: &[String]) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if keep.iter().any(|k| k == &name) {
            continue;
        }
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
    Ok(())
}

fn write_meta(dir: &Path, children: Vec<ChildMeta>) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(&ListMeta { children }).unwrap_or_default();
    write_if_changed(&dir.join(LISTMETA), &json)
}

/// Write only when the bytes differ, so an untouched card keeps its mtime and a
/// backup tool does not see the whole board change on every keystroke.
fn write_if_changed(path: &Path, contents: &str) -> std::io::Result<()> {
    if std::fs::read_to_string(path).is_ok_and(|current| current == contents) {
        return Ok(());
    }
    std::fs::write(path, contents)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample() -> Board {
        let mut b = Board::new();
        let mut todo = Column::new(1, "To do");
        todo.cards.push(Card::new(2, "fix login"));
        todo.cards.push(Card::new(3, "write docs"));
        let mut doing = Column::new(4, "Doing");
        doing.cards.push(Card::new(5, "kanban ui"));
        b.columns.push(todo);
        b.columns.push(doing);
        b.reseat_counter();
        b
    }

    #[test]
    fn a_board_survives_a_round_trip_with_its_ids() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("board");
        let board = sample();
        save_board(&root, &board).unwrap();
        let back = load_board(&root).expect("a board just written must read back");
        assert_eq!(back.columns, board.columns);
    }

    #[test]
    fn a_missing_directory_is_an_empty_board_not_a_failure() {
        let dir = TempDir::new().unwrap();
        let board = load_board(&dir.path().join("never-written")).expect("missing means empty");
        assert!(board.columns.is_empty());
    }

    #[test]
    fn an_unreadable_entry_is_not_reported_as_an_empty_board() {
        // The failure this guards is data loss, not a flaky read: `save_board`
        // reconciles, so an unreadable store reported as empty would delete
        // every column on the next keystroke.
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("board");
        save_board(&root, &sample()).unwrap();
        // Non-UTF-8 bytes in a column file: readable as bytes, not as a string.
        std::fs::write(root.join("0001"), [0xff, 0xfe, 0x00]).unwrap();
        assert!(load_board(&root).is_none());
    }

    #[test]
    fn a_file_the_provider_does_not_recognise_survives_a_save() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("board");
        save_board(&root, &sample()).unwrap();
        std::fs::write(root.join("README"), "mine").unwrap();
        save_board(&root, &sample()).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("README")).unwrap(),
            "mine"
        );
    }

    #[test]
    fn a_deleted_column_takes_its_card_folder_with_it() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("board");
        save_board(&root, &sample()).unwrap();
        assert!(root.join("0002.d").is_dir());

        let mut smaller = sample();
        smaller.columns.pop();
        save_board(&root, &smaller).unwrap();
        assert!(!root.join("0002").exists());
        assert!(!root.join("0002.d").exists());
    }

    #[test]
    fn an_untouched_card_keeps_its_bytes_across_a_save() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("board");
        let board = sample();
        save_board(&root, &board).unwrap();
        let card = root.join("0001.d").join("0001");
        let before = std::fs::metadata(&card).unwrap().modified().unwrap();
        save_board(&root, &board).unwrap();
        let after = std::fs::metadata(&card).unwrap().modified().unwrap();
        assert_eq!(before, after, "an unchanged card must not be rewritten");
    }

    #[test]
    fn a_third_level_on_disk_is_ignored_rather_than_loaded() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("board");
        save_board(&root, &sample()).unwrap();
        // A user hand-creating children under a card. Cards are leaves.
        let deeper = root.join("0001.d").join("0001.d");
        std::fs::create_dir_all(&deeper).unwrap();
        std::fs::write(deeper.join("0001"), "subtask").unwrap();
        let back = load_board(&root).unwrap();
        assert_eq!(back.columns[0].cards.len(), 2);
        assert_eq!(back.columns[0].cards[0].text, "fix login");
    }

    // ---- The archive flag -----------------------------------------------

    fn with_archive() -> Board {
        let mut b = sample();
        b.columns.push(Column::new(9, "Archive"));
        b.columns[2].cards.push(Card::new(10, "shipped"));
        b.set_archive(9);
        b.reseat_counter();
        b
    }

    #[test]
    fn the_archive_flag_survives_a_round_trip() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("board");
        save_board(&root, &with_archive()).unwrap();
        let back = load_board(&root).expect("a board just written must read back");
        assert_eq!(back.archive_id(), Some(9));
        assert_eq!(back.visible_len(), 2);
        assert_eq!(back.columns.last().unwrap().cards[0].text, "shipped");
    }

    #[test]
    fn a_store_written_before_the_archive_existed_loads_with_none() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("board");
        save_board(&root, &sample()).unwrap();
        let back = load_board(&root).unwrap();
        assert_eq!(back.archive_id(), None);
        assert_eq!(back.visible_len(), back.columns.len());
    }

    #[test]
    fn a_board_with_no_archive_writes_no_archive_key_anywhere() {
        // `skip_serializing_if` is not cosmetic: without it the first save by
        // this version rewrites every sidecar in the store, one per card folder
        // included, to record a flag nobody set.
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("board");
        save_board(&root, &sample()).unwrap();
        for dir in [root.clone(), root.join("0001.d"), root.join("0002.d")] {
            let raw = std::fs::read_to_string(dir.join(LISTMETA)).unwrap();
            assert!(!raw.contains("archive"), "{dir:?} carries a flag: {raw}");
        }
    }

    #[test]
    fn only_the_archive_column_carries_the_flag() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("board");
        save_board(&root, &with_archive()).unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join(LISTMETA))
                .unwrap()
                .matches("\"archive\"")
                .count(),
            1
        );
        // A card is never the archive, so no card's sidecar mentions it.
        let cards = std::fs::read_to_string(root.join("0003.d").join(LISTMETA)).unwrap();
        assert!(!cards.contains("archive"), "{cards}");
    }

    #[test]
    fn an_archive_column_with_no_id_on_disk_still_becomes_the_archive() {
        // The ordering trap: ids are healed by `assign_missing_ids` *after* the
        // columns are read, so the flag has to travel as a position and be
        // resolved to an id afterwards.
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("board");
        save_board(&root, &with_archive()).unwrap();
        // As an older version, or a hand edit, would leave it: the flag is there
        // but the ids are not.
        std::fs::write(
            root.join(LISTMETA),
            r#"{"children":[{"n":1,"id":0},{"n":2,"id":0},{"n":3,"id":0,"archive":true}]}"#,
        )
        .unwrap();
        let back = load_board(&root).unwrap();
        let id = back
            .archive_id()
            .expect("the flag must survive a missing id");
        assert_eq!(back.column(id).unwrap().title, "Archive");
        assert_ne!(id, 0, "the archive must have been given a real id");
    }

    #[test]
    fn an_archive_flagged_in_the_middle_is_pinned_last_on_load() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("board");
        save_board(&root, &sample()).unwrap();
        std::fs::write(
            root.join(LISTMETA),
            r#"{"children":[{"n":1,"id":1,"archive":true},{"n":2,"id":4}]}"#,
        )
        .unwrap();
        let back = load_board(&root).unwrap();
        assert_eq!(back.archive_id(), Some(1));
        assert_eq!(back.columns.last().unwrap().id, 1);
        assert_eq!(back.columns[0].id, 4, "the real column moved up");
    }

    #[test]
    fn reordering_renumbers_densely_so_ls_order_is_board_order() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().join("board");
        let mut board = sample();
        board.columns.swap(0, 1);
        save_board(&root, &board).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("0001")).unwrap(), "Doing");
        assert_eq!(std::fs::read_to_string(root.join("0002")).unwrap(), "To do");
        let back = load_board(&root).unwrap();
        assert_eq!(
            back.columns[0].id, 4,
            "identity follows the id, not the name"
        );
    }
}
