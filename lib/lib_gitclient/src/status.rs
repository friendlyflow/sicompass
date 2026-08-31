//! Parsing `git status --porcelain=v2 -z`.
//!
//! Version 2 of the porcelain format is used rather than v1 because it is the
//! only one that reports the staged and worktree states of a file separately in
//! a documented, stable way, and the only one that reports rename sources,
//! submodule sub-states and the branch's ahead/behind counts at all.
//!
//! `-z` is used because a filename may contain a newline. Without it git
//! quotes and escapes such names, and the escaping is not reversible without
//! reimplementing C string literals.

use crate::git::{Git, GitError};

/// Which record produced an entry. The four record types have different field
/// layouts, and an unmerged file in particular has no meaningful X/Y pair.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// `1` — a tracked file with a changed index and/or worktree state.
    Ordinary,
    /// `2` — renamed or copied. `orig_path` is set.
    Renamed,
    /// `u` — unmerged: a conflict the user has to resolve.
    Unmerged,
    /// `?` — untracked.
    Untracked,
    /// `!` — ignored. Only present when explicitly asked for.
    Ignored,
}

/// One changed path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusEntry {
    /// The path as git reports it, relative to the worktree root, raw bytes.
    ///
    /// Bytes rather than `String`: a filename need not be valid UTF-8, and a
    /// lossy conversion produces a path that no longer names the file, so
    /// staging it would silently do nothing.
    pub path: Vec<u8>,
    /// Where a rename or copy came from.
    pub orig_path: Option<Vec<u8>>,
    /// The index (staged) state: `.` for unchanged, else `M`, `A`, `D`, `R`,
    /// `C`, `T`, `U`.
    pub index: u8,
    /// The worktree (unstaged) state, same alphabet.
    pub worktree: u8,
    pub kind: EntryKind,
    /// True when the path is a submodule, whose "diff" is a commit id rather
    /// than file content.
    pub submodule: bool,
}

impl StatusEntry {
    /// True when something is staged for this path.
    pub fn is_staged(&self) -> bool {
        matches!(self.kind, EntryKind::Ordinary | EntryKind::Renamed) && self.index != b'.'
    }

    /// True when something is not staged for this path.
    ///
    /// Untracked files count: they are what the unstaged group is for, and
    /// `stage` on one is the ordinary way to start tracking it.
    pub fn is_unstaged(&self) -> bool {
        match self.kind {
            EntryKind::Ordinary | EntryKind::Renamed => self.worktree != b'.',
            EntryKind::Untracked => true,
            EntryKind::Ignored | EntryKind::Unmerged => false,
        }
    }

    /// A path with no lossy surprises for display. Never hand this back to git.
    pub fn display_path(&self) -> String {
        String::from_utf8_lossy(&self.path).into_owned()
    }
}

/// The `# branch.*` headers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BranchInfo {
    /// The commit HEAD points at. `None` on an unborn HEAD, which is a fresh
    /// repository with no commits: `git log`, `rev-parse HEAD` and
    /// `commit --amend` all fail there, so the whole graph section has to
    /// behave differently.
    pub oid: Option<String>,
    /// The current branch. `None` when HEAD is detached.
    pub head: Option<String>,
    /// The upstream branch, when one is configured.
    pub upstream: Option<String>,
    pub ahead: i64,
    pub behind: i64,
}

impl BranchInfo {
    pub fn detached(&self) -> bool {
        self.head.is_none()
    }

    /// True for a repository with no commits yet.
    pub fn unborn(&self) -> bool {
        self.oid.is_none()
    }
}

/// The whole answer from one `git status`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    pub branch: BranchInfo,
    pub entries: Vec<StatusEntry>,
}

impl Status {
    pub fn staged(&self) -> impl Iterator<Item = &StatusEntry> {
        self.entries.iter().filter(|e| e.is_staged())
    }

    pub fn unstaged(&self) -> impl Iterator<Item = &StatusEntry> {
        self.entries.iter().filter(|e| e.is_unstaged())
    }

    pub fn conflicts(&self) -> impl Iterator<Item = &StatusEntry> {
        self.entries
            .iter()
            .filter(|e| e.kind == EntryKind::Unmerged)
    }

    pub fn has_conflicts(&self) -> bool {
        self.entries.iter().any(|e| e.kind == EntryKind::Unmerged)
    }
}

/// Run `git status` and parse it.
pub fn read(git: &Git) -> Result<Status, GitError> {
    let out = git.run([
        "status",
        "--porcelain=v2",
        "--branch",
        "-z",
        // Every untracked file, not just the directory containing them. A
        // collapsed `build/` row cannot be staged selectively and reads as one
        // item when it is hundreds.
        "--untracked-files=all",
        // Report a submodule whose checkout moved. Without this a bumped
        // submodule is invisible in the tree but very much present in the
        // commit.
        "--ignore-submodules=none",
    ])?;
    Ok(parse(&out))
}

/// Parse the raw `-z` byte stream.
///
/// Records are NUL-terminated, but a `2` (rename/copy) record spans **two**
/// fields: the new path, then the original path. A parser that splits the whole
/// stream on NUL and treats every field as a record desynchronises at the first
/// rename and mislabels every file after it, which is the classic bug with this
/// format.
pub fn parse(bytes: &[u8]) -> Status {
    let mut status = Status::default();
    let fields: Vec<&[u8]> = bytes.split(|b| *b == 0).collect();
    let mut i = 0;
    while i < fields.len() {
        let field = fields[i];
        i += 1;
        if field.is_empty() {
            continue;
        }
        match field[0] {
            b'#' => parse_header(field, &mut status.branch),
            b'1' => {
                if let Some(e) = parse_ordinary(field) {
                    status.entries.push(e);
                }
            }
            b'2' => {
                // Consume the original path from the following field before
                // anything else can.
                let orig = fields.get(i).map(|f| f.to_vec());
                i += 1;
                if let Some(mut e) = parse_ordinary(field) {
                    e.kind = EntryKind::Renamed;
                    e.orig_path = orig;
                    status.entries.push(e);
                }
            }
            b'u' => {
                if let Some(e) = parse_unmerged(field) {
                    status.entries.push(e);
                }
            }
            b'?' | b'!' => {
                let kind = if field[0] == b'?' {
                    EntryKind::Untracked
                } else {
                    EntryKind::Ignored
                };
                // `? <path>` — one space, then the path verbatim.
                if field.len() > 2 {
                    status.entries.push(StatusEntry {
                        path: field[2..].to_vec(),
                        orig_path: None,
                        index: b'.',
                        worktree: b'?',
                        kind,
                        submodule: false,
                    });
                }
            }
            _ => {}
        }
    }
    status
}

fn parse_header(field: &[u8], branch: &mut BranchInfo) {
    let text = String::from_utf8_lossy(field);
    let rest = match text.strip_prefix("# ") {
        Some(r) => r,
        None => return,
    };
    let (key, value) = match rest.split_once(' ') {
        Some(kv) => kv,
        None => return,
    };
    match key {
        "branch.oid" => {
            // "(initial)" is git's way of saying there are no commits yet.
            branch.oid = (value != "(initial)").then(|| value.to_owned());
        }
        "branch.head" => {
            branch.head = (value != "(detached)").then(|| value.to_owned());
        }
        "branch.upstream" => branch.upstream = Some(value.to_owned()),
        "branch.ab" => {
            // "+2 -1"
            for token in value.split_whitespace() {
                let (sign, digits) = token.split_at(1);
                let n: i64 = digits.parse().unwrap_or(0);
                match sign {
                    "+" => branch.ahead = n,
                    "-" => branch.behind = n,
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

/// `1 <XY> <sub> <mH> <mI> <mW> <hH> <hI> <path>` and the same prefix for `2`,
/// which inserts an `<X><score>` field before the path.
fn parse_ordinary(field: &[u8]) -> Option<StatusEntry> {
    let renamed = field[0] == b'2';
    // Fields before the path: the record type plus 7 (ordinary) or 8 (rename).
    let leading = if renamed { 9 } else { 8 };
    let (head, path) = split_leading_fields(field, leading)?;
    let xy = head.get(1)?;
    Some(StatusEntry {
        path: path.to_vec(),
        orig_path: None,
        index: *xy.first()?,
        worktree: *xy.get(1)?,
        kind: if renamed {
            EntryKind::Renamed
        } else {
            EntryKind::Ordinary
        },
        // The sub-state field is `N...` for a plain file and `S<c><m><u>` for a
        // submodule. A submodule's "diff" is a commit id, not file content, so
        // it must not be expanded as one.
        submodule: head.get(2).is_some_and(|s| s.first() == Some(&b'S')),
    })
}

/// `u <XY> <sub> <m1> <m2> <m3> <mW> <h1> <h2> <h3> <path>` — a different
/// layout from `1`/`2`, with three stage modes and three object names, so ten
/// fields before the path rather than eight.
fn parse_unmerged(field: &[u8]) -> Option<StatusEntry> {
    let (head, path) = split_leading_fields(field, 10)?;
    let xy = head.get(1)?;
    Some(StatusEntry {
        path: path.to_vec(),
        orig_path: None,
        index: *xy.first()?,
        worktree: *xy.get(1)?,
        kind: EntryKind::Unmerged,
        submodule: head.get(2).is_some_and(|s| s.first() == Some(&b'S')),
    })
}

/// Split off `count` space-separated leading fields, returning them and the
/// remainder verbatim.
///
/// The remainder is taken as raw bytes rather than split further, because it is
/// the path and a path may contain spaces.
fn split_leading_fields(field: &[u8], count: usize) -> Option<(Vec<&[u8]>, &[u8])> {
    let mut head: Vec<&[u8]> = Vec::with_capacity(count);
    let mut start = 0;
    for _ in 0..count {
        let rel = field[start..].iter().position(|b| *b == b' ')?;
        head.push(&field[start..start + rel]);
        start += rel + 1;
    }
    if start > field.len() {
        return None;
    }
    Some((head, &field[start..]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::fixture::Fixture;

    /// Build a `-z` stream from records, so the fixtures read like the format
    /// documentation rather than like escaped noise.
    fn z(records: &[&str]) -> Vec<u8> {
        let mut out = Vec::new();
        for r in records {
            out.extend_from_slice(r.as_bytes());
            out.push(0);
        }
        out
    }

    // --- headers ---

    #[test]
    fn a_normal_branch_header_is_parsed() {
        let s = parse(&z(&[
            "# branch.oid abc1234def",
            "# branch.head main",
            "# branch.upstream origin/main",
            "# branch.ab +2 -1",
        ]));
        assert_eq!(s.branch.oid.as_deref(), Some("abc1234def"));
        assert_eq!(s.branch.head.as_deref(), Some("main"));
        assert_eq!(s.branch.upstream.as_deref(), Some("origin/main"));
        assert_eq!(s.branch.ahead, 2);
        assert_eq!(s.branch.behind, 1);
        assert!(!s.branch.detached());
        assert!(!s.branch.unborn());
    }

    #[test]
    fn an_unborn_head_is_reported_as_having_no_commit() {
        // Everything commit-shaped fails in this state, so it has to be
        // detected rather than discovered by a failing `git log`.
        let s = parse(&z(&["# branch.oid (initial)", "# branch.head main"]));
        assert!(s.branch.unborn());
        assert_eq!(s.branch.head.as_deref(), Some("main"));
    }

    #[test]
    fn a_detached_head_has_no_branch_name() {
        let s = parse(&z(&["# branch.oid abc1234", "# branch.head (detached)"]));
        assert!(s.branch.detached());
        assert!(!s.branch.unborn());
    }

    #[test]
    fn a_branch_with_no_upstream_has_no_counts() {
        let s = parse(&z(&["# branch.oid abc1234", "# branch.head main"]));
        assert_eq!(s.branch.upstream, None);
        assert_eq!((s.branch.ahead, s.branch.behind), (0, 0));
    }

    // --- ordinary records ---

    #[test]
    fn an_ordinary_record_splits_index_and_worktree_state() {
        let s = parse(&z(&[
            "1 M. N... 100644 100644 100644 aaa bbb src/lib.rs",
            "1 .M N... 100644 100644 100644 aaa bbb README.md",
        ]));
        assert_eq!(s.entries.len(), 2);
        assert_eq!(s.entries[0].display_path(), "src/lib.rs");
        assert!(s.entries[0].is_staged());
        assert!(!s.entries[0].is_unstaged());
        assert!(!s.entries[1].is_staged());
        assert!(s.entries[1].is_unstaged());
    }

    #[test]
    fn a_partially_staged_file_is_in_both_groups() {
        // The same path appears twice in the tree, which is why a label alone
        // cannot identify an entry.
        let s = parse(&z(&["1 MM N... 100644 100644 100644 aaa bbb src/lib.rs"]));
        assert!(s.entries[0].is_staged());
        assert!(s.entries[0].is_unstaged());
    }

    #[test]
    fn a_path_containing_spaces_survives() {
        let s = parse(&z(&[
            "1 M. N... 100644 100644 100644 aaa bbb my notes/a b.txt",
        ]));
        assert_eq!(s.entries[0].display_path(), "my notes/a b.txt");
    }

    // --- the rename record, and the desync it causes when mishandled ---

    #[test]
    fn a_rename_record_consumes_its_original_path_field() {
        let s = parse(&z(&[
            "2 R. N... 100644 100644 100644 aaa bbb R100 new.rs",
            "old.rs",
            "1 .M N... 100644 100644 100644 aaa bbb after.rs",
        ]));
        assert_eq!(s.entries.len(), 2, "the orig path must not become a record");
        assert_eq!(s.entries[0].kind, EntryKind::Renamed);
        assert_eq!(s.entries[0].display_path(), "new.rs");
        assert_eq!(
            s.entries[0].orig_path.as_deref(),
            Some(b"old.rs".as_slice())
        );
        // The desync this guards: without consuming the extra field, this entry
        // would be parsed from "old.rs" and every later record shifted by one.
        assert_eq!(s.entries[1].display_path(), "after.rs");
        assert_eq!(s.entries[1].kind, EntryKind::Ordinary);
    }

    #[test]
    fn two_renames_in_a_row_stay_aligned() {
        let s = parse(&z(&[
            "2 R. N... 100644 100644 100644 aaa bbb R100 one.rs",
            "one-old.rs",
            "2 .R N... 100644 100644 100644 aaa bbb R087 two.rs",
            "two-old.rs",
            "? untracked.rs",
        ]));
        assert_eq!(s.entries.len(), 3);
        assert_eq!(s.entries[2].kind, EntryKind::Untracked);
        assert_eq!(s.entries[2].display_path(), "untracked.rs");
    }

    // --- other record types ---

    #[test]
    fn an_unmerged_record_is_a_conflict_in_neither_group() {
        // `u` has a different field count from `1`, so parsing it with the
        // ordinary layout would take part of the path as a field.
        let s = parse(&z(&[
            "u UU N... 100644 100644 100644 100644 aaa bbb ccc src/conflict.rs",
        ]));
        assert_eq!(s.entries.len(), 1);
        assert_eq!(s.entries[0].kind, EntryKind::Unmerged);
        assert_eq!(s.entries[0].display_path(), "src/conflict.rs");
        assert!(!s.entries[0].is_staged());
        assert!(!s.entries[0].is_unstaged());
        assert!(s.has_conflicts());
        assert_eq!(s.conflicts().count(), 1);
    }

    #[test]
    fn untracked_and_ignored_are_distinguished() {
        let s = parse(&z(&["? new.rs", "! target/debug/app"]));
        assert_eq!(s.entries[0].kind, EntryKind::Untracked);
        assert!(s.entries[0].is_unstaged());
        assert_eq!(s.entries[1].kind, EntryKind::Ignored);
        assert!(!s.entries[1].is_unstaged(), "ignored files are not changes");
    }

    #[test]
    fn a_submodule_is_flagged_so_it_is_not_expanded_as_a_file() {
        let s = parse(&z(&[
            "1 .M SC.. 160000 160000 160000 aaa bbb vendor/lib",
            "1 .M N... 100644 100644 100644 aaa bbb plain.rs",
        ]));
        assert!(s.entries[0].submodule);
        assert!(!s.entries[1].submodule);
    }

    #[test]
    fn a_non_utf8_path_is_kept_as_bytes() {
        let mut stream = b"1 .M N... 100644 100644 100644 aaa bbb caf\xE9.txt".to_vec();
        stream.push(0);
        let s = parse(&stream);
        assert_eq!(s.entries[0].path, b"caf\xE9.txt");
        // Display is lossy, but the bytes handed back to git are not.
        assert!(s.entries[0].display_path().contains('\u{FFFD}'));
    }

    #[test]
    fn empty_output_is_a_clean_tree() {
        let s = parse(b"");
        assert!(s.entries.is_empty());
        assert!(!s.has_conflicts());
    }

    #[test]
    fn a_truncated_record_is_skipped_rather_than_panicking() {
        let s = parse(&z(&["1 M.", "? ok.rs"]));
        assert_eq!(s.entries.len(), 1);
        assert_eq!(s.entries[0].display_path(), "ok.rs");
    }

    // --- against a real repository ---

    #[test]
    fn a_real_repository_reports_staged_and_unstaged_separately() {
        let f = Fixture::new();
        f.write("kept.txt", "one");
        f.commit("first");
        f.write("kept.txt", "two");
        f.write("added.txt", "new");
        f.run(["add", "added.txt"]);
        f.write("untracked.txt", "loose");

        let s = read(&f.git()).unwrap();
        let staged: Vec<String> = s.staged().map(|e| e.display_path()).collect();
        let unstaged: Vec<String> = s.unstaged().map(|e| e.display_path()).collect();
        assert_eq!(staged, vec!["added.txt"]);
        assert!(unstaged.contains(&"kept.txt".to_owned()), "{unstaged:?}");
        assert!(
            unstaged.contains(&"untracked.txt".to_owned()),
            "{unstaged:?}"
        );
        assert_eq!(s.branch.head.as_deref(), Some("main"));
    }

    #[test]
    fn a_real_repository_with_no_commits_reports_an_unborn_head() {
        let f = Fixture::new();
        f.write("a.txt", "x");
        let s = read(&f.git()).unwrap();
        assert!(s.branch.unborn(), "a fresh repo has no commits");
        assert_eq!(s.unstaged().count(), 1);
    }

    #[test]
    fn a_real_rename_round_trips() {
        let f = Fixture::new();
        f.write("old.txt", "some content worth detecting a rename over\n");
        f.commit("first");
        f.run(["mv", "old.txt", "new.txt"]);

        let s = read(&f.git()).unwrap();
        let e = s
            .entries
            .iter()
            .find(|e| e.kind == EntryKind::Renamed)
            .expect("git should detect the rename");
        assert_eq!(e.display_path(), "new.txt");
        assert_eq!(e.orig_path.as_deref(), Some(b"old.txt".as_slice()));
    }

    #[test]
    fn a_real_merge_conflict_produces_an_unmerged_entry() {
        let f = Fixture::new();
        f.write("c.txt", "base\n");
        f.commit("base");
        f.run(["checkout", "-q", "-b", "side"]);
        f.write("c.txt", "side\n");
        f.commit("side");
        f.run(["checkout", "-q", "main"]);
        f.write("c.txt", "main\n");
        f.commit("main");
        // The merge is expected to fail, so it does not go through `run`.
        let _ = f.git().try_run(["merge", "side"]);

        let s = read(&f.git()).unwrap();
        assert!(s.has_conflicts(), "entries: {:?}", s.entries);
        assert_eq!(s.conflicts().next().unwrap().display_path(), "c.txt");
    }
}
