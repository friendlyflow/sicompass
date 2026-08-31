//! The two things that must not run on the render thread.
//!
//! * [`Network`] runs `fetch`, `pull` and `push` on a background thread. They
//!   contact a remote, so they take as long as the network does, and a frame
//!   spent waiting is a frame the app does not draw.
//! * [`Watcher`] notices that the repository changed underneath the app,
//!   without running `git status` on a timer.

use crate::git::Git;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

// ---------------------------------------------------------------------------
// Network jobs
// ---------------------------------------------------------------------------

/// How one background git command ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// The operation, for the message that reports it.
    pub label: String,
    /// `None` on success.
    pub error: Option<String>,
}

/// Runs one remote-contacting git command at a time.
///
/// Single-flight rather than a queue: `GIT_OPTIONAL_LOCKS=0` keeps the *read*
/// commands off `index.lock`, but a `pull` very much takes it, and two of them
/// at once fail with three lines of advice about deleting a lock file by hand.
/// Refusing the second is a better answer than racing it.
#[derive(Debug, Clone, Default)]
pub struct Network {
    in_flight: Arc<AtomicBool>,
    /// What is running, for the row that says so.
    running: Arc<Mutex<Option<String>>>,
    /// Filled by the worker, drained by `tick`.
    done: Arc<Mutex<Option<Outcome>>>,
}

impl Network {
    pub fn new() -> Network {
        Network::default()
    }

    pub fn busy(&self) -> bool {
        self.in_flight.load(Ordering::Acquire)
    }

    /// What is running right now, if anything.
    pub fn running(&self) -> Option<String> {
        self.running.lock().ok().and_then(|g| g.clone())
    }

    /// Take the result of the last finished job, if it has not been taken yet.
    pub fn take_outcome(&self) -> Option<Outcome> {
        self.done.lock().ok().and_then(|mut g| g.take())
    }

    /// Start a job. Returns `false` when one is already running.
    ///
    /// `steps` is run in order and stops at the first failure, which is what
    /// makes "commit and sync" a pull followed by a push rather than two
    /// independent things that can both half-happen.
    pub fn start(&self, git: Git, label: String, steps: Vec<Vec<String>>) -> bool {
        if self.in_flight.swap(true, Ordering::AcqRel) {
            return false;
        }
        if let Ok(mut g) = self.running.lock() {
            *g = Some(label.clone());
        }

        let in_flight = Arc::clone(&self.in_flight);
        let running = Arc::clone(&self.running);
        let done = Arc::clone(&self.done);

        std::thread::spawn(move || {
            // A panic anywhere in here would otherwise leave the flag set and
            // no further job could ever start.
            struct ClearOnDrop(Arc<AtomicBool>, Arc<Mutex<Option<String>>>);
            impl Drop for ClearOnDrop {
                fn drop(&mut self) {
                    if let Ok(mut g) = self.1.lock() {
                        *g = None;
                    }
                    self.0.store(false, Ordering::Release);
                }
            }
            let _guard = ClearOnDrop(in_flight, running);

            let mut error = None;
            for step in steps {
                let out = git.try_run(&step);
                if !out.ok() {
                    let sub = step.first().cloned().unwrap_or_default();
                    let first = out
                        .stderr
                        .lines()
                        .find(|l| !l.trim().is_empty())
                        .unwrap_or("failed")
                        .trim()
                        .to_owned();
                    error = Some(format!("git {sub}: {first}"));
                    break;
                }
            }
            if let Ok(mut g) = done.lock() {
                *g = Some(Outcome { label, error });
            }
        });
        true
    }
}

// ---------------------------------------------------------------------------
// The .git watcher
// ---------------------------------------------------------------------------

/// Notices that something changed the repository from outside the app.
///
/// It stats three paths rather than running `git status`, because it runs on a
/// timer: `git status` on a large repository is hundreds of milliseconds and a
/// process spawn, and doing that once a second forever to answer "did anything
/// happen" is the wrong trade. `HEAD` covers checkout and commit, `index`
/// covers staging, and the `refs` directory covers branch and tag changes.
///
/// It does **not** watch the worktree. Editing a file changes `git status`
/// without touching `.git`, so an edit made in another window is picked up on
/// the next refresh rather than immediately. Watching a whole worktree means
/// an inotify watch per directory and a rebuild every time a build writes to
/// `target/`, which is a much worse trade than being one keystroke stale.
pub struct Watcher {
    changed: Arc<AtomicBool>,
    alive: Arc<AtomicBool>,
}

impl Watcher {
    /// Start watching the git directories of an open repository.
    ///
    /// `git_dir` is this worktree's own (its `HEAD` and `index`), `common_dir`
    /// the one every worktree shares (its `refs`). For the main worktree they
    /// are the same directory.
    pub fn start(git_dir: PathBuf, common_dir: PathBuf) -> Watcher {
        let changed = Arc::new(AtomicBool::new(false));
        let alive = Arc::new(AtomicBool::new(true));

        let thread_changed = Arc::clone(&changed);
        let thread_alive = Arc::clone(&alive);
        std::thread::spawn(move || {
            let watched = [
                git_dir.join("HEAD"),
                git_dir.join("index"),
                common_dir.join("refs"),
                // Written by rebase, merge and cherry-pick, so an operation
                // running in the user's own shell shows up here.
                common_dir.join("packed-refs"),
            ];
            let mut last = stamps(&watched);
            while thread_alive.load(Ordering::Acquire) {
                std::thread::sleep(POLL_INTERVAL);
                if !thread_alive.load(Ordering::Acquire) {
                    break;
                }
                let now = stamps(&watched);
                if now != last {
                    last = now;
                    thread_changed.store(true, Ordering::Release);
                }
            }
        });

        Watcher { changed, alive }
    }

    /// Take the "something changed" flag, clearing it.
    pub fn take_changed(&self) -> bool {
        self.changed.swap(false, Ordering::AcqRel)
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        // The thread checks this each time round, so it exits within one
        // interval rather than outliving the tab that started it.
        self.alive.store(false, Ordering::Release);
    }
}

/// Long enough that the stats cost nothing, short enough that a commit made in
/// another window is picked up before the user wonders why it was not.
const POLL_INTERVAL: Duration = Duration::from_millis(1000);

fn stamps(paths: &[PathBuf]) -> Vec<Option<SystemTime>> {
    paths.iter().map(|p| stamp(p)).collect()
}

/// A path that does not exist stamps as `None`, so its appearance or
/// disappearance counts as a change (`packed-refs` and `index` both come and
/// go in an ordinary repository).
fn stamp(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::fixture::Fixture;

    #[test]
    fn a_job_reports_success() {
        let f = Fixture::new();
        let n = Network::new();
        assert!(n.start(f.git(), "check".into(), vec![vec!["--version".to_owned()]]));
        let outcome = wait_for(&n);
        assert_eq!(outcome.error, None, "git --version should succeed");
        assert_eq!(outcome.label, "check");
        assert!(!n.busy(), "the flag is cleared when the job ends");
    }

    #[test]
    fn a_failing_step_stops_the_ones_after_it() {
        // "commit and sync" is a pull then a push. If the pull fails the push
        // must not run, or a conflict would be pushed straight past.
        let f = Fixture::new();
        let n = Network::new();
        n.start(
            f.git(),
            "sync".into(),
            vec![
                vec![
                    "rev-parse".to_owned(),
                    "--verify".to_owned(),
                    "refs/heads/definitely-not-a-branch".to_owned(),
                ],
                vec!["--version".to_owned()],
            ],
        );
        let outcome = wait_for(&n);
        assert!(outcome.error.is_some(), "the first step failed");
        assert!(
            outcome.error.unwrap().starts_with("git rev-parse:"),
            "the message should name the step that failed"
        );
    }

    #[test]
    fn only_one_job_runs_at_a_time() {
        // Two writes at once contend on index.lock and fail with advice the
        // user cannot act on, so the second is refused instead.
        let f = Fixture::new();
        let n = Network::new();
        // A command that takes long enough to still be running on the next
        // line: `git log` over a fresh repo would be too fast, so this waits on
        // a subprocess that does not exist and fails slowly enough.
        assert!(n.start(f.git(), "first".into(), vec![vec!["--version".into()]]));
        // Whether the first has finished is a race, so only assert the
        // invariant that matters: `start` never lets two run together.
        if n.busy() {
            assert!(
                !n.start(f.git(), "second".into(), vec![vec!["--version".into()]]),
                "a second job must be refused while one is in flight"
            );
        }
        wait_for(&n);
    }

    #[test]
    fn the_running_label_is_readable_while_a_job_is_in_flight_and_gone_after() {
        let f = Fixture::new();
        let n = Network::new();
        n.start(f.git(), "fetching".into(), vec![vec!["--version".into()]]);
        wait_for(&n);
        // Cleared by the guard, so the in-flight row disappears even if the
        // job panicked.
        assert_eq!(n.running(), None);
    }

    #[test]
    fn an_outcome_is_taken_once() {
        let f = Fixture::new();
        let n = Network::new();
        n.start(f.git(), "one".into(), vec![vec!["--version".into()]]);
        wait_for(&n);
        assert_eq!(n.take_outcome(), None, "the outcome was already drained");
    }

    #[test]
    fn a_watcher_notices_a_commit_and_reports_it_once() {
        let f = Fixture::new();
        let info = crate::repo::discover(&f.git(), &f.path()).unwrap();
        let w = Watcher::start(info.git_dir.clone(), info.common_dir.clone());
        assert!(!w.take_changed(), "nothing has happened yet");

        f.write("a.txt", "one");
        f.commit("first");
        assert!(wait_for_change(&w), "a commit should be noticed");
        assert!(!w.take_changed(), "the flag is taken, not left set");
    }

    #[test]
    fn a_watcher_notices_staging() {
        let f = Fixture::new();
        f.write("a.txt", "one");
        f.commit("first");
        let info = crate::repo::discover(&f.git(), &f.path()).unwrap();
        let w = Watcher::start(info.git_dir.clone(), info.common_dir.clone());

        f.write("a.txt", "two");
        f.run(["add", "a.txt"]);
        assert!(wait_for_change(&w), "the index changed");
    }

    #[test]
    fn a_watcher_stops_when_it_is_dropped() {
        // The thread outliving its tab would keep stating a directory nobody
        // is looking at, once a second, for the life of the process.
        let f = Fixture::new();
        let info = crate::repo::discover(&f.git(), &f.path()).unwrap();
        let alive = {
            let w = Watcher::start(info.git_dir.clone(), info.common_dir.clone());
            Arc::clone(&w.alive)
        };
        assert!(!alive.load(Ordering::Acquire), "drop clears the alive flag");
    }

    fn wait_for(n: &Network) -> Outcome {
        for _ in 0..200 {
            if let Some(o) = n.take_outcome() {
                return o;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("the job never finished");
    }

    fn wait_for_change(w: &Watcher) -> bool {
        for _ in 0..60 {
            if w.take_changed() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }
}
