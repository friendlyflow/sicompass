//! Reading commit history.
//!
//! `--graph` is deliberately not used. Its ASCII art is a picture of the
//! branch topology, and a picture read out one character at a time
//! ("asterisk pipe pipe backslash") is noise in front of every subject line.
//! The part of it that carries information is the ref decoration, which is
//! asked for directly instead.

use crate::git::{Git, GitError};

/// Separates fields inside one record. A commit subject can contain anything
/// printable, so the separators are control characters that cannot appear in
/// one: unit separator between fields, record separator between commits.
const FIELD: char = '\u{1f}';
const RECORD: char = '\u{1e}';

/// One commit, as the graph list needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commit {
    pub oid: String,
    pub short: String,
    pub subject: String,
    /// The message after the subject line, verbatim and possibly empty.
    pub body: String,
    pub author: String,
    pub date: String,
    /// `HEAD -> main, origin/main`, or empty.
    pub refs: String,
}

/// One file touched by a commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitFile {
    /// `M`, `A`, `D`, `R`, `C`, `T`.
    pub status: u8,
    pub path: Vec<u8>,
    /// Where a rename came from.
    pub orig_path: Option<Vec<u8>>,
    /// `None` for a binary file, which git reports as `-`.
    pub insertions: Option<u64>,
    pub deletions: Option<u64>,
}

impl CommitFile {
    pub fn display_path(&self) -> String {
        String::from_utf8_lossy(&self.path).into_owned()
    }
}

/// The totals across a commit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    pub files: usize,
    pub insertions: u64,
    pub deletions: u64,
}

/// Read a page of history.
///
/// `all` widens the walk from the current branch to every ref, which is what
/// makes commits on other branches reachable at all.
pub fn read(git: &Git, skip: usize, count: usize, all: bool) -> Result<Vec<Commit>, GitError> {
    let format = format!(
        "--pretty=format:%H{FIELD}%h{FIELD}%an{FIELD}%ad{FIELD}%D{FIELD}%s{FIELD}%b{RECORD}"
    );
    let mut args = vec![
        "log".to_owned(),
        format,
        // A fixed format rather than the user's `log.date`, so the row reads
        // the same everywhere and sorts the way it looks.
        "--date=format:%Y-%m-%d %H:%M".to_owned(),
        format!("--skip={skip}"),
        format!("--max-count={count}"),
    ];
    if all {
        args.push("--all".to_owned());
    }
    let out = git.run(args)?;
    Ok(parse_log(&String::from_utf8_lossy(&out)))
}

pub fn parse_log(text: &str) -> Vec<Commit> {
    text.split(RECORD)
        .map(|r| r.trim_start_matches('\n'))
        .filter(|r| !r.is_empty())
        .filter_map(|record| {
            let mut f = record.split(FIELD);
            Some(Commit {
                oid: f.next()?.to_owned(),
                short: f.next()?.to_owned(),
                author: f.next()?.to_owned(),
                date: f.next()?.to_owned(),
                refs: f.next()?.to_owned(),
                subject: f.next()?.to_owned(),
                // The body is the last field and keeps its own newlines.
                body: f.next().unwrap_or("").to_owned(),
            })
        })
        .collect()
}

/// The files one commit touched, with per-file line counts.
///
/// Two calls rather than one: `--numstat` has the line counts but not what
/// happened to the file, and `--name-status` has the reverse. Neither can be
/// derived from the other (an added file and a file whose every line changed
/// both show up as additions only).
pub fn files(git: &Git, oid: &str) -> Result<Vec<CommitFile>, GitError> {
    // `-m` so a merge commit reports against its first parent instead of
    // reporting nothing at all, which is what `git show` does for merges by
    // default and would make every merge look empty.
    let names = git.run([
        "show",
        "--format=",
        "--name-status",
        "-z",
        "-m",
        "--first-parent",
        oid,
    ])?;
    let numbers = git.run([
        "show",
        "--format=",
        "--numstat",
        "-z",
        "-m",
        "--first-parent",
        oid,
    ])?;
    Ok(merge_file_lists(
        parse_name_status(&names),
        &parse_numstat(&numbers),
    ))
}

/// `<status>NUL<path>NUL`, and for a rename or copy
/// `R<score>NUL<old>NUL<new>NUL` — three fields, not two.
pub fn parse_name_status(bytes: &[u8]) -> Vec<CommitFile> {
    let fields: Vec<&[u8]> = bytes.split(|b| *b == 0).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < fields.len() {
        let status_field = fields[i];
        i += 1;
        if status_field.is_empty() {
            continue;
        }
        let status = status_field[0];
        if status == b'R' || status == b'C' {
            let Some(orig) = fields.get(i) else { break };
            let Some(path) = fields.get(i + 1) else { break };
            i += 2;
            out.push(CommitFile {
                status,
                path: path.to_vec(),
                orig_path: Some(orig.to_vec()),
                insertions: None,
                deletions: None,
            });
        } else {
            let Some(path) = fields.get(i) else { break };
            i += 1;
            if path.is_empty() {
                continue;
            }
            out.push(CommitFile {
                status,
                path: path.to_vec(),
                orig_path: None,
                insertions: None,
                deletions: None,
            });
        }
    }
    out
}

/// `<adds>\t<dels>\t<path>NUL`, and for a rename
/// `<adds>\t<dels>\tNUL<old>NUL<new>NUL` — the path is empty in the tab-joined
/// part and arrives as the two following fields.
///
/// `-` in place of a number means a binary file, which has no line counts.
pub fn parse_numstat(bytes: &[u8]) -> Vec<(Vec<u8>, Option<u64>, Option<u64>)> {
    let fields: Vec<&[u8]> = bytes.split(|b| *b == 0).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < fields.len() {
        let field = fields[i];
        i += 1;
        if field.is_empty() {
            continue;
        }
        let mut parts = field.splitn(3, |b| *b == b'\t');
        let adds = parts.next().map(count).unwrap_or(None);
        let dels = parts.next().map(count).unwrap_or(None);
        let Some(rest) = parts.next() else { continue };
        let path = if rest.is_empty() {
            // A rename: skip the source and take the destination.
            let dest = fields.get(i + 1).map(|f| f.to_vec());
            i += 2;
            match dest {
                Some(d) => d,
                None => break,
            }
        } else {
            rest.to_vec()
        };
        out.push((path, adds, dels));
    }
    out
}

fn count(field: &[u8]) -> Option<u64> {
    if field == b"-" {
        return None;
    }
    std::str::from_utf8(field).ok()?.parse().ok()
}

fn merge_file_lists(
    mut names: Vec<CommitFile>,
    numbers: &[(Vec<u8>, Option<u64>, Option<u64>)],
) -> Vec<CommitFile> {
    for f in &mut names {
        if let Some((_, adds, dels)) = numbers.iter().find(|(p, _, _)| *p == f.path) {
            f.insertions = *adds;
            f.deletions = *dels;
        }
    }
    names
}

/// Totals for the row that says how big a commit is.
pub fn stats(files: &[CommitFile]) -> Stats {
    Stats {
        files: files.len(),
        insertions: files.iter().filter_map(|f| f.insertions).sum(),
        deletions: files.iter().filter_map(|f| f.deletions).sum(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::fixture::Fixture;

    fn record(fields: &[&str]) -> String {
        format!("{}{RECORD}", fields.join(&FIELD.to_string()))
    }

    #[test]
    fn a_commit_record_is_split_into_its_fields() {
        let text = record(&[
            "abc123full",
            "abc123",
            "Nico",
            "2026-08-28 14:03",
            "HEAD -> main, origin/main",
            "fix the parser",
            "A longer body.\nOver two lines.",
        ]);
        let commits = parse_log(&text);
        assert_eq!(commits.len(), 1);
        let c = &commits[0];
        assert_eq!(c.oid, "abc123full");
        assert_eq!(c.short, "abc123");
        assert_eq!(c.author, "Nico");
        assert_eq!(c.date, "2026-08-28 14:03");
        assert_eq!(c.refs, "HEAD -> main, origin/main");
        assert_eq!(c.subject, "fix the parser");
        assert_eq!(c.body, "A longer body.\nOver two lines.");
    }

    #[test]
    fn a_subject_containing_separators_would_not_survive_but_control_chars_cannot_occur() {
        // The point of using unit/record separators: a subject may contain
        // tabs, pipes and colons, none of which can be the delimiter.
        let text = record(&["o", "s", "a", "d", "", "feat: add a|b\tc thing", ""]);
        let c = &parse_log(&text)[0];
        assert_eq!(c.subject, "feat: add a|b\tc thing");
    }

    #[test]
    fn an_empty_log_is_no_commits_rather_than_one_blank_one() {
        assert!(parse_log("").is_empty());
        assert!(parse_log("\n").is_empty());
    }

    #[test]
    fn a_commit_with_no_body_and_no_refs_parses() {
        let c = &parse_log(&record(&["o", "s", "a", "d", "", "subject", ""]))[0];
        assert_eq!(c.body, "");
        assert_eq!(c.refs, "");
    }

    // --- name-status ---

    #[test]
    fn name_status_pairs_a_letter_with_a_path() {
        let files = parse_name_status(b"M\0src/lib.rs\0A\0docs/new.md\0");
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].status, b'M');
        assert_eq!(files[0].display_path(), "src/lib.rs");
        assert_eq!(files[1].status, b'A');
    }

    #[test]
    fn a_rename_in_name_status_consumes_two_paths() {
        // Same desync as the status parser: a rename is three fields, and
        // reading it as two shifts everything after it.
        let files = parse_name_status(b"R100\0old.rs\0new.rs\0M\0after.rs\0");
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].display_path(), "new.rs");
        assert_eq!(files[0].orig_path.as_deref(), Some(b"old.rs".as_slice()));
        assert_eq!(files[1].display_path(), "after.rs", "alignment was lost");
    }

    // --- numstat ---

    #[test]
    fn numstat_reads_line_counts() {
        let n = parse_numstat(b"3\t1\tsrc/lib.rs\0" as &[u8]);
        assert_eq!(n.len(), 1);
        assert_eq!(n[0].0, b"src/lib.rs");
        assert_eq!((n[0].1, n[0].2), (Some(3), Some(1)));
    }

    #[test]
    fn a_binary_file_has_no_line_counts() {
        let n = parse_numstat(b"-\t-\tlogo.png\0" as &[u8]);
        assert_eq!((n[0].1, n[0].2), (None, None));
    }

    #[test]
    fn a_rename_in_numstat_takes_the_destination_path() {
        let n = parse_numstat(b"1\t1\t\0old.rs\0new.rs\0-\t-\tafter.png\0" as &[u8]);
        assert_eq!(n.len(), 2);
        assert_eq!(n[0].0, b"new.rs");
        assert_eq!(n[1].0, b"after.png", "alignment was lost");
    }

    #[test]
    fn totals_ignore_binary_files_but_still_count_them() {
        let files = merge_file_lists(
            parse_name_status(b"M\0a.rs\0M\0logo.png\0"),
            &parse_numstat(b"3\t1\ta.rs\0-\t-\tlogo.png\0" as &[u8]),
        );
        let s = stats(&files);
        assert_eq!(s.files, 2);
        assert_eq!((s.insertions, s.deletions), (3, 1));
    }

    // --- against a real repository ---

    #[test]
    fn a_real_log_reads_newest_first() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit("first");
        f.write("b.txt", "two\n");
        f.commit("second");

        let commits = read(&f.git(), 0, 10, false).unwrap();
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].subject, "second");
        assert_eq!(commits[1].subject, "first");
        assert!(
            commits[0].refs.contains("HEAD"),
            "refs: {}",
            commits[0].refs
        );
        assert_eq!(commits[0].author, "Test");
    }

    #[test]
    fn paging_skips_and_limits() {
        let f = Fixture::new();
        for i in 0..5 {
            f.write("a.txt", &format!("{i}\n"));
            f.commit(&format!("c{i}"));
        }
        let page = read(&f.git(), 2, 2, false).unwrap();
        assert_eq!(
            page.iter().map(|c| c.subject.as_str()).collect::<Vec<_>>(),
            vec!["c2", "c1"]
        );
    }

    #[test]
    fn a_repository_with_no_commits_errors_rather_than_returning_junk() {
        // `git log` exits 128 on an unborn HEAD, so the graph section has to
        // check for that before asking.
        let f = Fixture::new();
        assert!(read(&f.git(), 0, 10, false).is_err());
    }

    #[test]
    fn the_root_commit_reports_its_files() {
        // `git show` on a root commit is the case that catches a diff computed
        // against a parent that does not exist.
        let f = Fixture::new();
        f.write("a.txt", "one\ntwo\n");
        f.commit("root");
        let oid = f.run(["rev-parse", "HEAD"]);

        let files = files(&f.git(), &oid).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].display_path(), "a.txt");
        assert_eq!(files[0].status, b'A');
        assert_eq!(stats(&files).insertions, 2);
    }

    #[test]
    fn a_merge_commit_reports_files_instead_of_nothing() {
        let f = Fixture::new();
        f.write("base.txt", "base\n");
        f.commit("base");
        f.run(["checkout", "-q", "-b", "side"]);
        f.write("side.txt", "side\n");
        f.commit("side");
        f.run(["checkout", "-q", "main"]);
        f.write("main.txt", "main\n");
        f.commit("main");
        f.run(["merge", "-q", "--no-ff", "-m", "merge side", "side"]);
        let oid = f.run(["rev-parse", "HEAD"]);

        let files = files(&f.git(), &oid).unwrap();
        assert!(
            files.iter().any(|f| f.display_path() == "side.txt"),
            "a merge showed no files: {files:?}"
        );
    }
}
