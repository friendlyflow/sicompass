//! Running the `git` binary.
//!
//! Every git call in this crate goes through here, because every git call needs
//! the same hygiene and getting any of it wrong is silent rather than loud.
//!
//! Shelling out rather than linking `git2` or `gix` is deliberate. `git2` pulls
//! `libgit2-sys`, which needs a C toolchain on all four release runners and in
//! the Nix derivation, and `libssh2`/`openssl` on top for push and pull; `gix`
//! still leans on a git subprocess for the credential helpers those need
//! anyway. A subprocess adds no dependency, no entry in
//! `THIRD-PARTY-LICENSES.html`, and reuses the user's own credential helpers,
//! ssh agent and config exactly as their shell would.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

// ---------------------------------------------------------------------------
// Test stub: never reach the network from a test.
//
// `push`, `pull` and `fetch` contact a real remote. A test that drives them
// would hit whatever remote the fixture repo happens to point at, hang on a
// credential prompt, or fail differently depending on whether the machine
// running it is online.
//
// Two audiences, hence both a compile-time default and a runtime setter, the
// same shape as `sicompass_terminal::_set_test_no_history`:
//
// * This crate's own unit tests get it from `cfg!(test)`. Remembering to set a
//   per-instance override is not a defence: forgetting one does not fail the
//   test, it goes to the network.
// * The app's integration tests are a different binary, where this crate is an
//   ordinary dependency compiled *without* `cfg(test)` and reached as a
//   `Box<dyn Provider>`. They call `_set_test_no_network(true)` once per binary.
// ---------------------------------------------------------------------------

static TEST_NO_NETWORK: AtomicBool = AtomicBool::new(cfg!(test));

#[doc(hidden)]
pub fn _set_test_no_network(enabled: bool) {
    TEST_NO_NETWORK.store(enabled, Ordering::Release);
}

#[inline]
fn test_no_network() -> bool {
    TEST_NO_NETWORK.load(Ordering::Acquire)
}

/// Subcommands that contact a remote, and so are refused under
/// [`_set_test_no_network`].
const NETWORK_SUBCOMMANDS: &[&str] = &["push", "pull", "fetch", "clone", "ls-remote", "remote"];

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// A git invocation that did not succeed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitError {
    /// The subcommand, for a message the user can act on ("git status failed").
    pub subcommand: String,
    /// Exit status, or `None` when the binary could not be started at all.
    pub code: Option<i32>,
    /// git's own explanation, trimmed. Normally stderr, falling back to the
    /// first line of stdout: some refusals ("nothing to commit, working tree
    /// clean") are printed there and leave stderr empty, and reporting
    /// "failed" for those tells the user nothing.
    pub stderr: String,
    /// Why the process could not be spawned, if that is what happened.
    pub os_error: Option<String>,
}

impl GitError {
    /// A single line suitable for the app's error row.
    pub fn message(&self) -> String {
        if let Some(os) = &self.os_error {
            return format!("git {}: {}", self.subcommand, os);
        }
        let first = self
            .stderr
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("failed");
        format!("git {}: {}", self.subcommand, first.trim())
    }

    /// True when git refused because another process holds `index.lock`.
    ///
    /// Worth distinguishing: the raw stderr for this is three lines of advice
    /// about removing the file by hand, which is the wrong thing to tell
    /// someone who simply has a rebase running in another window.
    pub fn is_index_locked(&self) -> bool {
        self.stderr.contains("index.lock")
    }
}

/// What one invocation produced. Used where a non-zero exit is an answer rather
/// than a failure (`rev-parse --is-bare-repository` on a non-repo, say).
#[derive(Debug, Clone)]
pub struct Output {
    pub code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: String,
}

impl Output {
    pub fn ok(&self) -> bool {
        self.code == Some(0)
    }

    /// stdout as text, with the trailing newline git adds to most output
    /// removed. Lossy: display only, never for an argument handed back to git.
    pub fn text(&self) -> String {
        let s = String::from_utf8_lossy(&self.stdout);
        s.trim_end_matches(['\n', '\r']).to_owned()
    }
}

// ---------------------------------------------------------------------------
// Building the command
// ---------------------------------------------------------------------------

/// A `git` invocation with this crate's hygiene already applied.
///
/// `cwd` is where git is pointed; for repository work that is the worktree
/// root, for discovery it is whatever folder is being probed.
fn base_command(binary: &str, cwd: &Path) -> Command {
    let mut cmd = Command::new(binary);

    // `-C` before the subcommand, and `--no-pager` before it too: `git log
    // --no-pager` is not the same thing and is silently accepted as a pathspec
    // error. stdout is not a tty here so no pager would start anyway, but the
    // user's `core.pager` can be set to something that does not check.
    cmd.arg("-C").arg(cwd);
    cmd.arg("--no-pager");
    // Raw UTF-8 in output that is not NUL-separated, instead of git's
    // octal-escaped, double-quoted form for anything non-ASCII.
    cmd.arg("-c").arg("core.quotePath=false");

    // If sicompass itself was launched from inside a repository — which, being
    // a development tool, it usually is — any of these inherited variables
    // silently overrides `-C` on every single call, and the provider would
    // report on sicompass's own repo no matter which one the user opened.
    for var in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_COMMON_DIR",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CEILING_DIRECTORIES",
        "GIT_PREFIX",
    ] {
        cmd.env_remove(var);
    }

    // Nothing here has a terminal to prompt on, and a blocked prompt would hang
    // the thread that is driving the render loop. `GIT_TERMINAL_PROMPT` covers
    // the terminal case; without the two askpass variables a graphical
    // ssh-askpass can still appear over the app.
    cmd.env("GIT_TERMINAL_PROMPT", "0");
    cmd.env("GIT_ASKPASS", "");
    cmd.env("SSH_ASKPASS", "");
    // Stable, parseable output regardless of the user's locale.
    cmd.env("LC_ALL", "C");
    // Read-only commands otherwise take the index lock to refresh stat info,
    // which fights with a rebase or a commit running in the user's own shell.
    // (It does nothing for the write commands, which is why those are
    // serialised through one worker instead.)
    cmd.env("GIT_OPTIONAL_LOCKS", "0");

    #[cfg(windows)]
    {
        // Without this every git call flashes a console window over the app.
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    cmd
}

/// Wrap a path as an explicit-literal pathspec.
///
/// `--` separates paths from revisions but does **not** turn off glob magic, so
/// a file genuinely named `foo[1].txt`, `a*b` or `#tag.md` is read as a pattern
/// and matches the wrong set (usually nothing, so the operation silently does
/// nothing at all). `:(literal)` is what turns that off.
pub fn literal_pathspec(path: &[u8]) -> OsString {
    let mut out = OsString::from(":(literal)");
    out.push(os_string_from_bytes(path));
    out
}

/// A path straight from git's `-z` output, as an OS string.
///
/// Deliberately not `String::from_utf8_lossy`: a filename that is not valid
/// UTF-8 comes back from git as raw bytes, and lossy conversion replaces them
/// with U+FFFD. Handing *that* back to `git add` addresses a file that does not
/// exist, and the stage silently does nothing.
#[cfg(unix)]
pub fn os_string_from_bytes(bytes: &[u8]) -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(bytes.to_vec())
}

/// Windows has no byte-oriented path API, and git on Windows emits UTF-8
/// regardless of the filesystem encoding, so lossy conversion is exact here.
#[cfg(not(unix))]
pub fn os_string_from_bytes(bytes: &[u8]) -> OsString {
    OsString::from(String::from_utf8_lossy(bytes).into_owned())
}

// ---------------------------------------------------------------------------
// Git
// ---------------------------------------------------------------------------

/// A `git` binary pointed at one directory.
#[derive(Debug, Clone)]
pub struct Git {
    binary: String,
    cwd: PathBuf,
}

impl Git {
    pub fn new(binary: impl Into<String>, cwd: impl Into<PathBuf>) -> Self {
        Git {
            binary: binary.into(),
            cwd: cwd.into(),
        }
    }

    /// Run git and hand back whatever happened, including a non-zero exit.
    ///
    /// Use this where failure is an answer (probing whether a directory is a
    /// repository) rather than something to report.
    pub fn try_run<I, S>(&self, args: I) -> Output
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let args: Vec<OsString> = args.into_iter().map(|a| a.as_ref().to_owned()).collect();

        if let Some(sub) = args.first().and_then(|a| a.to_str()) {
            // `remote` is only a network command in its `update` form; plain
            // `remote -v` reads the config file.
            let touches_network = NETWORK_SUBCOMMANDS.contains(&sub)
                && (sub != "remote" || args.iter().any(|a| a == "update"));
            if touches_network && test_no_network() {
                return Output {
                    code: Some(1),
                    stdout: Vec::new(),
                    stderr: format!("git {sub}: refused, network disabled for tests"),
                };
            }
        }

        let mut cmd = base_command(&self.binary, &self.cwd);
        cmd.args(&args);
        match cmd.output() {
            Ok(out) => Output {
                code: out.status.code(),
                stdout: out.stdout,
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
            },
            Err(e) => Output {
                code: None,
                stdout: Vec::new(),
                stderr: e.to_string(),
            },
        }
    }

    /// Run git, turning a non-zero exit into a [`GitError`].
    pub fn run<I, S>(&self, args: I) -> Result<Vec<u8>, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let args: Vec<OsString> = args.into_iter().map(|a| a.as_ref().to_owned()).collect();
        let subcommand = args
            .first()
            .and_then(|a| a.to_str())
            .unwrap_or("")
            .to_owned();
        let out = self.try_run(&args);
        if out.ok() {
            return Ok(out.stdout);
        }
        // stderr leads with the reason ("fatal: ..."), so its first line is
        // the one to keep. stdout leads with context and *ends* with the
        // conclusion ("On branch main" ... "nothing to commit"), so there the
        // last line is.
        let explanation = if out.stderr.is_empty() {
            out.text()
                .lines()
                .rfind(|l| !l.trim().is_empty())
                .unwrap_or("")
                .to_owned()
        } else {
            out.stderr.clone()
        };
        Err(GitError {
            subcommand,
            code: out.code,
            os_error: if out.code.is_none() {
                Some(out.stderr.clone())
            } else {
                None
            },
            stderr: explanation,
        })
    }

    /// [`Git::run`], decoded lossily and with the trailing newline removed.
    ///
    /// For output that is displayed or matched on, never for a path handed back
    /// to git.
    pub fn run_text<I, S>(&self, args: I) -> Result<String, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<std::ffi::OsStr>,
    {
        let bytes = self.run(args)?;
        let s = String::from_utf8_lossy(&bytes);
        Ok(s.trim_end_matches(['\n', '\r']).to_owned())
    }

    /// A copy pointed at a different directory, keeping the configured binary.
    pub fn at(&self, cwd: impl Into<PathBuf>) -> Git {
        Git {
            binary: self.binary.clone(),
            cwd: cwd.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Tests that flip the network flag have to be serialised, since cargo runs
    /// them in parallel and the flag is process-global. The guard puts the
    /// compile-time default back so an unrelated later test cannot inherit it.
    static NETWORK_FLAG: Mutex<()> = Mutex::new(());

    struct NetworkFlagGuard(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

    impl Drop for NetworkFlagGuard {
        fn drop(&mut self) {
            _set_test_no_network(cfg!(test));
        }
    }

    fn network_flag_guard(no_network: bool) -> NetworkFlagGuard {
        let guard = NETWORK_FLAG.lock().unwrap_or_else(|e| e.into_inner());
        _set_test_no_network(no_network);
        NetworkFlagGuard(guard)
    }

    fn git_here() -> Git {
        Git::new("git", std::env::temp_dir())
    }

    #[test]
    fn git_version_runs() {
        let out = git_here().try_run(["--version"]);
        assert!(out.ok(), "git is expected on PATH: {}", out.stderr);
        assert!(out.text().starts_with("git version"), "{}", out.text());
    }

    #[test]
    fn a_missing_binary_reports_the_os_error_not_a_panic() {
        let g = Git::new("git-that-does-not-exist", std::env::temp_dir());
        let err = g.run(["status"]).unwrap_err();
        assert_eq!(err.code, None);
        assert!(err.os_error.is_some());
        assert!(
            err.message().starts_with("git status: "),
            "{}",
            err.message()
        );
    }

    #[test]
    fn a_non_zero_exit_becomes_an_error_carrying_stderr() {
        // A directory that is not a repository.
        let dir = tempfile::tempdir().unwrap();
        let g = Git::new("git", dir.path());
        let err = g.run(["rev-parse", "--show-toplevel"]).unwrap_err();
        assert_eq!(err.subcommand, "rev-parse");
        assert!(err.code.is_some_and(|c| c != 0));
        assert!(!err.stderr.is_empty(), "stderr should explain the refusal");
    }

    #[test]
    fn try_run_reports_a_non_zero_exit_without_erroring() {
        let dir = tempfile::tempdir().unwrap();
        let out = Git::new("git", dir.path()).try_run(["rev-parse", "--show-toplevel"]);
        assert!(!out.ok(), "a plain directory is not a repository");
    }

    // --- the network guard ---

    #[test]
    fn no_unit_test_can_reach_the_network() {
        // The compile-time default is what protects a test that forgets to
        // think about this at all.
        let _guard = network_flag_guard(cfg!(test));
        let out = git_here().try_run(["push"]);
        assert!(!out.ok());
        assert!(
            out.stderr.contains("network disabled"),
            "push must be refused before it spawns: {}",
            out.stderr
        );
    }

    #[test]
    fn every_network_subcommand_is_refused() {
        let _guard = network_flag_guard(true);
        for sub in ["push", "pull", "fetch", "clone", "ls-remote"] {
            let out = git_here().try_run([sub]);
            assert!(
                out.stderr.contains("network disabled"),
                "{sub} was not refused"
            );
        }
    }

    #[test]
    fn reading_the_remote_list_is_not_a_network_command() {
        // `git remote -v` reads .git/config. Refusing it would make the
        // remotes section untestable for no safety gain.
        let _guard = network_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        let out = Git::new("git", dir.path()).try_run(["remote", "-v"]);
        assert!(
            !out.stderr.contains("network disabled"),
            "remote -v must not be treated as network access"
        );
    }

    #[test]
    fn remote_update_is_a_network_command() {
        let _guard = network_flag_guard(true);
        let out = git_here().try_run(["remote", "update"]);
        assert!(out.stderr.contains("network disabled"), "{}", out.stderr);
    }

    #[test]
    fn clearing_the_flag_lets_the_subcommand_through() {
        // Or every network test would be inert rather than passing.
        let _guard = network_flag_guard(false);
        let dir = tempfile::tempdir().unwrap();
        let out = Git::new("git", dir.path()).try_run(["fetch"]);
        assert!(
            !out.stderr.contains("network disabled"),
            "the flag should be off here"
        );
    }

    // --- hygiene ---

    #[test]
    fn an_inherited_git_dir_does_not_override_the_target() {
        // The failure this prevents is silent: with GIT_DIR set, every call
        // reports on *that* repository whatever `-C` says.
        let repo = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let g = Git::new("git", repo.path());
        g.try_run(["init", "-q"]);

        // SAFETY: single-threaded within this test; the variable is removed
        // again before it returns.
        unsafe { std::env::set_var("GIT_DIR", other.path().join(".git")) };
        let out = g.try_run(["rev-parse", "--is-inside-work-tree"]);
        unsafe { std::env::remove_var("GIT_DIR") };

        assert!(out.ok(), "{}", out.stderr);
        assert_eq!(out.text(), "true");
    }

    #[test]
    fn literal_pathspec_marks_a_glob_looking_name() {
        let spec = literal_pathspec(b"foo[1].txt");
        assert_eq!(spec.to_str().unwrap(), ":(literal)foo[1].txt");
    }

    #[cfg(unix)]
    #[test]
    fn a_non_utf8_path_survives_as_an_argument() {
        // Lossy conversion here would produce U+FFFD and address a file that
        // does not exist, so the operation would silently do nothing.
        let raw = b"caf\xE9.txt";
        let spec = literal_pathspec(raw);
        use std::os::unix::ffi::OsStrExt;
        assert!(spec.as_os_str().as_bytes().ends_with(raw));
        assert!(spec.to_str().is_none(), "the bytes are not valid UTF-8");
    }

    #[test]
    fn index_lock_failures_are_recognisable() {
        let err = GitError {
            subcommand: "add".into(),
            code: Some(128),
            stderr: "fatal: Unable to create '/r/.git/index.lock': File exists.".into(),
            os_error: None,
        };
        assert!(err.is_index_locked());
    }

    #[test]
    fn a_refusal_printed_on_stdout_is_still_explained() {
        // `git commit` with nothing to commit says so on stdout and leaves
        // stderr empty, and "git commit: failed" helps nobody.
        let f = crate::repo::fixture::Fixture::new();
        let err = f.git().run(["commit", "-m", "nothing"]).unwrap_err();
        assert!(
            err.message().contains("nothing to commit"),
            "message was {:?}",
            err.message()
        );
    }

    #[test]
    fn the_error_message_is_one_line() {
        let err = GitError {
            subcommand: "commit".into(),
            code: Some(1),
            stderr: "\nerror: nothing to commit\nhint: use git add\n".into(),
            os_error: None,
        };
        assert_eq!(err.message(), "git commit: error: nothing to commit");
    }
}
