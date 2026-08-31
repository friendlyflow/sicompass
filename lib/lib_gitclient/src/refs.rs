//! Branches, stashes and remotes.

use crate::git::{Git, GitError};

const FIELD: char = '\u{1f}';

/// One local or remote-tracking branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    /// Short name: `main`, or `origin/main` for a remote-tracking one.
    pub name: String,
    /// True for a remote-tracking branch.
    ///
    /// Read off the full refname rather than guessed from the short one: a
    /// local branch may perfectly well be called `feature/thing`, and the
    /// slash says nothing about which side it lives on.
    pub remote: bool,
    /// The upstream this branch tracks, if any.
    pub upstream: Option<String>,
    /// Whether this is the branch HEAD is on.
    pub current: bool,
    pub short_oid: String,
    pub subject: String,
    pub ahead: u64,
    pub behind: u64,
    /// Set when the upstream has been deleted on the remote.
    pub upstream_gone: bool,
    /// The worktree that has this branch checked out, when it is not this one.
    ///
    /// git refuses to check out a branch that is already checked out in
    /// another worktree, so offering it would produce an error the user cannot
    /// act on.
    pub checked_out_in: Option<String>,
}

/// One stash entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stash {
    /// Position in `git stash list`, which is what `stash@{n}` means and what
    /// every stash command takes.
    pub index: usize,
    pub oid: String,
    /// `WIP on main: abc1234 subject`.
    pub message: String,
}

/// One configured remote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Remote {
    pub name: String,
    pub fetch_url: String,
    pub push_url: String,
}

/// Read every branch, local and remote-tracking.
///
/// `for-each-ref` rather than `branch -vv`: the latter's output is a
/// human-readable table with no stable machine format, and its `[ahead 2]`
/// column is produced by the same code path that draws the asterisk.
pub fn branches(git: &Git) -> Result<Vec<Branch>, GitError> {
    let format = format!(
        "--format=%(refname){FIELD}%(refname:short){FIELD}%(upstream:short){FIELD}%(HEAD){FIELD}\
         %(objectname:short){FIELD}%(upstream:track){FIELD}%(worktreepath){FIELD}%(contents:subject)"
    );
    let out = git.run(["for-each-ref", &format, "refs/heads", "refs/remotes"])?;
    let current_worktree = git
        .run_text(["rev-parse", "--show-toplevel"])
        .unwrap_or_default();
    Ok(parse_branches(
        &String::from_utf8_lossy(&out),
        &current_worktree,
    ))
}

pub fn parse_branches(text: &str, current_worktree: &str) -> Vec<Branch> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| {
            let mut f = line.split(FIELD);
            let refname = f.next()?;
            let name = f.next()?.to_owned();
            let upstream = f.next()?;
            let head = f.next()?;
            let short_oid = f.next()?.to_owned();
            let track = f.next()?;
            let worktree = f.next()?;
            let subject = f.next().unwrap_or("").to_owned();

            // `origin/HEAD` is a symbolic ref pointing at the remote's default
            // branch, not a branch of its own. Listing it puts a duplicate of
            // `origin/main` in the tree under a name nothing can act on.
            if refname.ends_with("/HEAD") {
                return None;
            }

            let (ahead, behind, gone) = parse_track(track);
            Some(Branch {
                remote: refname.starts_with("refs/remotes/"),
                name,
                upstream: (!upstream.is_empty()).then(|| upstream.to_owned()),
                current: head == "*",
                short_oid,
                subject,
                ahead,
                behind,
                upstream_gone: gone,
                checked_out_in: (!worktree.is_empty() && worktree != current_worktree)
                    .then(|| worktree.to_owned()),
            })
        })
        .collect()
}

/// `%(upstream:track)` is `[ahead 2, behind 1]`, `[gone]`, or empty.
fn parse_track(track: &str) -> (u64, u64, bool) {
    if track.contains("gone") {
        return (0, 0, true);
    }
    let inner = track.trim_start_matches('[').trim_end_matches(']');
    let mut ahead = 0;
    let mut behind = 0;
    for part in inner.split(", ") {
        if let Some(n) = part.strip_prefix("ahead ") {
            ahead = n.trim().parse().unwrap_or(0);
        } else if let Some(n) = part.strip_prefix("behind ") {
            behind = n.trim().parse().unwrap_or(0);
        }
    }
    (ahead, behind, false)
}

pub fn stashes(git: &Git) -> Result<Vec<Stash>, GitError> {
    // A stash message can contain anything, including a newline, so the
    // entries are NUL-separated rather than line-separated.
    let out = git.run([
        "stash",
        "list",
        "-z",
        &format!("--pretty=format:%H{FIELD}%gs"),
    ])?;
    Ok(parse_stashes(&String::from_utf8_lossy(&out)))
}

pub fn parse_stashes(text: &str) -> Vec<Stash> {
    text.split('\0')
        .filter(|r| !r.trim().is_empty())
        .enumerate()
        .filter_map(|(index, record)| {
            let mut f = record.trim_start_matches('\n').split(FIELD);
            Some(Stash {
                index,
                oid: f.next()?.to_owned(),
                message: f.next().unwrap_or("").to_owned(),
            })
        })
        .collect()
}

pub fn remotes(git: &Git) -> Result<Vec<Remote>, GitError> {
    let out = git.run_text(["remote", "-v"])?;
    Ok(parse_remotes(&out))
}

/// `git remote -v` prints two lines per remote, `<name>\t<url> (fetch)` and
/// `<name>\t<url> (push)`, and the two urls can differ.
pub fn parse_remotes(text: &str) -> Vec<Remote> {
    let mut out: Vec<Remote> = Vec::new();
    for line in text.lines() {
        let Some((name, rest)) = line.split_once('\t') else {
            continue;
        };
        let (url, kind) = match rest.rsplit_once(' ') {
            Some((u, k)) => (u, k),
            None => (rest, ""),
        };
        let entry = match out.iter_mut().find(|r| r.name == name) {
            Some(e) => e,
            None => {
                out.push(Remote {
                    name: name.to_owned(),
                    fetch_url: String::new(),
                    push_url: String::new(),
                });
                out.last_mut().unwrap()
            }
        };
        if kind == "(push)" {
            entry.push_url = url.to_owned();
        } else {
            entry.fetch_url = url.to_owned();
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::fixture::Fixture;

    fn line(fields: &[&str]) -> String {
        fields.join(&FIELD.to_string())
    }

    // --- branches ---

    #[test]
    fn a_current_branch_with_an_upstream_is_parsed() {
        let text = line(&[
            "refs/heads/main",
            "main",
            "origin/main",
            "*",
            "abc1234",
            "[ahead 2, behind 1]",
            "/home/nico/repo",
            "fix the parser",
        ]);
        let b = &parse_branches(&text, "/home/nico/repo")[0];
        assert_eq!(b.name, "main");
        assert_eq!(b.upstream.as_deref(), Some("origin/main"));
        assert!(b.current);
        assert_eq!((b.ahead, b.behind), (2, 1));
        assert!(!b.upstream_gone);
        assert_eq!(
            b.checked_out_in, None,
            "the worktree it is checked out in is this one"
        );
    }

    #[test]
    fn a_branch_with_no_upstream_has_none() {
        let text = line(&[
            "refs/heads/side",
            "side",
            "",
            "",
            "abc1234",
            "",
            "",
            "work in progress",
        ]);
        let b = &parse_branches(&text, "/r")[0];
        assert_eq!(b.upstream, None);
        assert!(!b.current);
        assert_eq!((b.ahead, b.behind), (0, 0));
    }

    #[test]
    fn a_deleted_upstream_is_reported_as_gone() {
        let text = line(&[
            "refs/heads/old",
            "old",
            "origin/old",
            "",
            "abc1234",
            "[gone]",
            "",
            "subject",
        ]);
        let b = &parse_branches(&text, "/r")[0];
        assert!(b.upstream_gone);
    }

    #[test]
    fn ahead_only_and_behind_only_both_parse() {
        let ahead = line(&["refs/heads/a", "a", "o/a", "", "1", "[ahead 3]", "", "s"]);
        let behind = line(&["refs/heads/b", "b", "o/b", "", "1", "[behind 4]", "", "s"]);
        assert_eq!(parse_branches(&ahead, "/r")[0].ahead, 3);
        assert_eq!(parse_branches(&behind, "/r")[0].behind, 4);
    }

    #[test]
    fn a_branch_checked_out_in_another_worktree_is_flagged() {
        // git refuses to check this one out, so the command must not offer it.
        let text = line(&[
            "refs/heads/side",
            "side",
            "",
            "",
            "abc1234",
            "",
            "/home/nico/other",
            "s",
        ]);
        let b = &parse_branches(&text, "/home/nico/repo")[0];
        assert_eq!(b.checked_out_in.as_deref(), Some("/home/nico/other"));
    }

    #[test]
    fn local_and_remote_branches_are_told_apart_by_their_full_refname() {
        // Not by the slash in the short name: `feature/thing` is local.
        let text = format!(
            "{}\n{}",
            line(&[
                "refs/heads/feature/thing",
                "feature/thing",
                "",
                "",
                "1",
                "",
                "",
                "s"
            ]),
            line(&[
                "refs/remotes/origin/main",
                "origin/main",
                "",
                "",
                "1",
                "",
                "",
                "s"
            ]),
        );
        let bs = parse_branches(&text, "/r");
        assert_eq!(bs.len(), 2);
        assert!(!bs[0].remote, "feature/thing is a local branch");
        assert!(bs[1].remote);
    }

    #[test]
    fn the_remote_head_symref_is_not_listed_as_a_branch() {
        // It points at origin/main, so listing it puts the same branch in the
        // tree twice under a name no command can act on.
        let text = format!(
            "{}\n{}",
            line(&[
                "refs/remotes/origin/HEAD",
                "origin/HEAD",
                "",
                "",
                "1",
                "",
                "",
                "s"
            ]),
            line(&[
                "refs/remotes/origin/main",
                "origin/main",
                "",
                "",
                "1",
                "",
                "",
                "s"
            ]),
        );
        let bs = parse_branches(&text, "/r");
        assert_eq!(bs.len(), 1);
        assert_eq!(bs[0].name, "origin/main");
    }

    #[test]
    fn a_subject_containing_a_tab_survives() {
        let text = line(&[
            "refs/heads/main",
            "main",
            "",
            "*",
            "abc",
            "",
            "",
            "fix\tthe\tparser",
        ]);
        assert_eq!(parse_branches(&text, "/r")[0].subject, "fix\tthe\tparser");
    }

    // --- stashes ---

    #[test]
    fn stashes_are_numbered_by_position() {
        // The index is what `stash@{n}` means, and every stash command takes
        // it, so it has to come from the order rather than from the message.
        let text = format!("aaa{FIELD}WIP on main: 111 first\0bbb{FIELD}On side: manual message\0");
        let s = parse_stashes(&text);
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].index, 0);
        assert_eq!(s[0].oid, "aaa");
        assert_eq!(s[1].index, 1);
        assert_eq!(s[1].message, "On side: manual message");
    }

    #[test]
    fn no_stashes_is_an_empty_list() {
        assert!(parse_stashes("").is_empty());
    }

    // --- remotes ---

    #[test]
    fn a_remote_with_one_url_fills_both_directions() {
        let text = "origin\tgit@example.invalid:a/b.git (fetch)\n\
                    origin\tgit@example.invalid:a/b.git (push)";
        let r = parse_remotes(text);
        assert_eq!(r.len(), 1, "two lines are one remote");
        assert_eq!(r[0].name, "origin");
        assert_eq!(r[0].fetch_url, "git@example.invalid:a/b.git");
        assert_eq!(r[0].push_url, "git@example.invalid:a/b.git");
    }

    #[test]
    fn a_remote_pushing_somewhere_else_keeps_both_urls() {
        let text = "origin\thttps://example.invalid/a.git (fetch)\n\
                    origin\tgit@example.invalid:a.git (push)";
        let r = parse_remotes(text);
        assert_eq!(r[0].fetch_url, "https://example.invalid/a.git");
        assert_eq!(r[0].push_url, "git@example.invalid:a.git");
    }

    #[test]
    fn several_remotes_stay_separate() {
        let text = "origin\tA (fetch)\norigin\tA (push)\nfork\tB (fetch)\nfork\tB (push)";
        let r = parse_remotes(text);
        assert_eq!(r.len(), 2);
        assert_eq!(r[1].name, "fork");
    }

    // --- against a real repository ---

    #[test]
    fn a_real_repository_lists_its_branches_with_the_current_one_marked() {
        let f = Fixture::new();
        f.write("a.txt", "one");
        f.commit("first");
        f.run(["branch", "side"]);

        let bs = branches(&f.git()).unwrap();
        let names: Vec<&str> = bs.iter().map(|b| b.name.as_str()).collect();
        assert!(names.contains(&"main"), "{names:?}");
        assert!(names.contains(&"side"), "{names:?}");
        assert_eq!(bs.iter().filter(|b| b.current).count(), 1);
        assert!(bs.iter().find(|b| b.current).unwrap().name == "main");
    }

    #[test]
    fn a_real_stash_round_trips() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit("first");
        f.write("a.txt", "two\n");
        f.run(["stash", "push", "-q", "-m", "my stash"]);

        let s = stashes(&f.git()).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].index, 0);
        assert!(s[0].message.contains("my stash"), "{}", s[0].message);
    }

    #[test]
    fn a_real_remote_round_trips() {
        // `git remote add` writes .git/config and touches no network, which is
        // why it must not be refused by the no-network test guard.
        let f = Fixture::new();
        f.run(["remote", "add", "origin", "https://example.invalid/a.git"]);
        let r = remotes(&f.git()).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].name, "origin");
        assert_eq!(r[0].fetch_url, "https://example.invalid/a.git");
    }

    #[test]
    fn a_repository_with_no_remotes_lists_none() {
        let f = Fixture::new();
        assert!(remotes(&f.git()).unwrap().is_empty());
    }
}
