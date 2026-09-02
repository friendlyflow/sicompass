//! Finding a repository, and the handful of facts about it that decide what the
//! tree can show.

use crate::git::Git;
use std::path::{Path, PathBuf};

/// Where a repository lives, resolved once when it is opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoInfo {
    /// The worktree root. Empty for a bare repository, which has none.
    pub root: PathBuf,
    /// This worktree's own git directory. For a linked worktree that is
    /// `.git/worktrees/<name>`, not the repository's `.git`.
    pub git_dir: PathBuf,
    /// The git directory shared by every worktree. Branches, stashes, remotes
    /// and the reflog live here; for the main worktree it equals `git_dir`.
    ///
    /// Kept separately because the watcher has to stat `refs` and `HEAD` in the
    /// right one, and a linked worktree has its own `HEAD` but shares `refs`.
    pub common_dir: PathBuf,
    /// A bare repository has no worktree, so there is nothing to stage and no
    /// `changes` section to show.
    pub bare: bool,
    /// True when this is a linked worktree rather than the main one. Branches
    /// checked out elsewhere cannot be checked out here.
    pub linked_worktree: bool,
}

impl RepoInfo {
    /// The name shown for the repository: the worktree's own folder name.
    pub fn name(&self) -> String {
        self.root
            .file_name()
            .or_else(|| self.common_dir.parent().and_then(|p| p.file_name()))
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "repository".to_owned())
    }
}

/// Resolve the repository containing `dir`, if there is one.
///
/// Returns `None` for a plain directory. `git` is used rather than looking for
/// a `.git` entry by hand, because `.git` is a *file* in a linked worktree and
/// a submodule, absent in a bare repository, and inherited from a parent
/// directory in the ordinary case.
pub fn discover(git: &Git, dir: &Path) -> Option<RepoInfo> {
    let g = git.at(dir);

    // One call for all three, so a repository is probed once rather than three
    // times per folder the user arrows over.
    let out = g.try_run([
        "rev-parse",
        "--is-bare-repository",
        "--absolute-git-dir",
        "--path-format=absolute",
        "--git-common-dir",
    ]);
    if !out.ok() {
        return None;
    }
    let text = out.text();
    let mut lines = text.lines();
    let bare = lines.next()? == "true";
    let git_dir = PathBuf::from(lines.next()?);
    let common_dir = PathBuf::from(lines.next()?);

    // `--show-toplevel` fails outright in a bare repository ("this operation
    // must be run in a work tree"), so it is asked for separately and only when
    // there is a worktree to ask about.
    let root = if bare {
        PathBuf::new()
    } else {
        let top = g.try_run(["rev-parse", "--show-toplevel"]);
        if !top.ok() {
            return None;
        }
        PathBuf::from(top.text())
    };

    Some(RepoInfo {
        linked_worktree: git_dir != common_dir,
        root,
        git_dir,
        common_dir,
        bare,
    })
}

#[cfg(test)]
pub(crate) mod fixture {
    use crate::git::Git;
    use std::path::Path;
    use std::process::Command;
    use tempfile::TempDir;

    /// A repository built from nothing, isolated from the developer's own git.
    ///
    /// Every one of these matters. Without `GIT_CONFIG_*` the test picks up the
    /// user's `~/.gitconfig`, so their aliases, their `commit.gpgsign` and
    /// their `core.hooksPath` all apply, and a test that passes here fails on
    /// the next machine. Without an explicit identity `git commit` refuses on a
    /// machine that has never configured one, and *uses the developer's real
    /// name* on one that has.
    pub struct Fixture {
        pub dir: TempDir,
    }

    impl Fixture {
        pub fn new() -> Fixture {
            let dir = TempDir::new().unwrap();
            let f = Fixture { dir };
            f.run(["init", "-q"]);
            // Write the identity into the repository's *own* config, not just
            // into `run`'s `-c` flags.
            //
            // `run` covers git invoked by the tests. It does not cover git
            // invoked by the provider, which is the code under test: it shells
            // out on its own, with none of these flags and none of this
            // environment. On a developer's machine that silently picks up
            // their global `user.name`, so every commit test passes and quietly
            // records their real name. On a machine that has never configured
            // one it fails with "Author identity unknown", which is what every
            // commit test did the first time CI ever ran this crate.
            //
            // Local config belongs to the repository, so both callers get it.
            f.run(["config", "user.name", "Test"]);
            f.run(["config", "user.email", "test@example.invalid"]);
            f.run(["config", "commit.gpgsign", "false"]);
            f
        }

        /// The worktree root, canonicalised: `/tmp` is a symlink to
        /// `/private/tmp` on macOS and git reports the resolved path, so an
        /// un-canonicalised comparison fails there and nowhere else.
        pub fn path(&self) -> std::path::PathBuf {
            self.dir.path().canonicalize().unwrap()
        }

        pub fn git(&self) -> Git {
            Git::new("git", self.path())
        }

        /// Run git in the fixture with the isolating configuration applied.
        pub fn run<I, S>(&self, args: I) -> String
        where
            I: IntoIterator<Item = S>,
            S: AsRef<std::ffi::OsStr>,
        {
            let mut cmd = Command::new("git");
            cmd.arg("-C").arg(self.dir.path());
            for var in [
                "GIT_DIR",
                "GIT_WORK_TREE",
                "GIT_INDEX_FILE",
                "GIT_COMMON_DIR",
            ] {
                cmd.env_remove(var);
            }
            cmd.env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("HOME", self.dir.path())
                .env("GIT_TERMINAL_PROMPT", "0")
                .env("LC_ALL", "C");
            cmd.args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "init.defaultBranch=main",
                "-c",
                "core.hooksPath=",
            ]);
            cmd.args(args);
            let out = cmd.output().expect("git should be on PATH");
            assert!(
                out.status.success(),
                "git failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            String::from_utf8_lossy(&out.stdout).trim_end().to_owned()
        }

        pub fn write(&self, rel: &str, contents: &str) {
            let p = self.dir.path().join(rel);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::write(p, contents).unwrap();
        }

        pub fn commit(&self, message: &str) {
            self.run(["add", "-A"]);
            self.run(["commit", "-q", "-m", message]);
        }

        pub fn subdir(&self, rel: &str) -> std::path::PathBuf {
            let p = self.path().join(rel);
            std::fs::create_dir_all(&p).unwrap();
            p
        }
    }

    /// A directory that is deliberately not a repository.
    pub fn plain_dir() -> TempDir {
        TempDir::new().unwrap()
    }

    pub fn git_at(dir: &Path) -> Git {
        Git::new("git", dir)
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::*;
    use super::*;

    #[test]
    fn a_plain_directory_is_not_a_repository() {
        let dir = plain_dir();
        let git = git_at(dir.path());
        assert!(discover(&git, dir.path()).is_none());
    }

    #[test]
    fn a_fresh_repository_is_found_with_no_commits() {
        let f = Fixture::new();
        let info = discover(&f.git(), &f.path()).expect("should be a repository");
        assert_eq!(info.root, f.path());
        assert!(!info.bare);
        assert!(!info.linked_worktree);
        assert_eq!(info.git_dir, info.common_dir);
    }

    #[test]
    fn a_subdirectory_resolves_to_the_worktree_root() {
        // The user browses to `src/`, not to the repository root, and the
        // provider has to open the repository, not the folder.
        let f = Fixture::new();
        let sub = f.subdir("src/deep");
        let info = discover(&f.git(), &sub).expect("should be a repository");
        assert_eq!(info.root, f.path());
    }

    #[test]
    fn the_repository_name_is_its_folder_name() {
        let f = Fixture::new();
        let info = discover(&f.git(), &f.path()).unwrap();
        let expected = f.path().file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(info.name(), expected);
    }

    #[test]
    fn the_git_directory_itself_is_not_somewhere_to_open() {
        // `rev-parse --git-dir` succeeds in here, but `--show-toplevel` does
        // not, so anything that answers the cheaper question offers to open a
        // repository it then cannot open.
        let f = Fixture::new();
        let hooks = f.path().join(".git/hooks");
        assert!(hooks.is_dir(), "git creates this");
        assert!(discover(&f.git(), &hooks).is_none());
    }

    #[test]
    fn a_bare_repository_is_reported_as_bare_and_has_no_worktree() {
        // `rev-parse --show-toplevel` fails outright here, so probing for it
        // first would report "not a repository" and hide the whole thing.
        let dir = plain_dir();
        let git = git_at(dir.path());
        assert!(git.try_run(["init", "-q", "--bare"]).ok());
        let info = discover(&git, dir.path()).expect("a bare repo is still a repository");
        assert!(info.bare);
        assert_eq!(info.root, std::path::PathBuf::new());
    }

    #[test]
    fn a_linked_worktree_is_recognised_and_keeps_the_shared_git_dir() {
        let f = Fixture::new();
        f.write("a.txt", "one");
        f.commit("first");
        let linked = f.dir.path().join("wt");
        f.run([
            "worktree",
            "add",
            "-q",
            "-b",
            "side",
            linked.to_str().unwrap(),
        ]);

        let git = git_at(&linked);
        let info = discover(&git, &linked).expect("a linked worktree is a repository");
        assert!(info.linked_worktree, "git_dir and common_dir should differ");
        assert_ne!(info.git_dir, info.common_dir);
        // Branches and stashes live in the shared directory, so the watcher and
        // every ref read has to use that one.
        assert!(info.common_dir.ends_with(".git"), "{:?}", info.common_dir);
    }
}
