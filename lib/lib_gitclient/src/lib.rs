//! Git client provider: VS Code's Source Control view as a tree of lists.
//!
//! ## Shape
//!
//! Two views, the same arrangement the terminal and claude providers use:
//!
//! * [`View::Browse`] lists subdirectories. `:` on a folder inside a repository
//!   opens it, because that is the browse view's only command and the app fires
//!   a lone command rather than showing a one-item palette.
//! * [`View::Repo`] is the repository: `changes`, `graph`, `branches`,
//!   `stashes` and `remotes`. `:` here is the ordinary command palette.
//!
//! `changes` holds the commit message, the four commit buttons, and then every
//! changed file in the order the work moves through: conflicts first, because
//! nothing can be committed until they are gone, then what is staged, then what
//! is not. There is no separate conflicts section, so a `u` record cannot go
//! unnoticed.
//!
//! ## Two invariants that are easy to break and silent when broken
//!
//! 1. **Every git-derived string goes through [`escape::escape_markup`].** The
//!    app reads `<input>`, `<button>` and friends out of any string, so an
//!    unescaped diff of an HTML file grows live form controls. See `escape.rs`.
//!
//! 2. **An `Obj` key is an identifier, not a status line.** The key is the path
//!    segment *and* the key the cursor is restored by after a refresh
//!    (`refresh_visible_path` in the app). A key carrying a count or a ref
//!    decoration changes when the thing it counts changes, the restore misses,
//!    and the cursor silently lands on a different row. Counts and decorations
//!    are child rows.

mod escape;
mod git;
mod log;
mod refs;
mod repo;
mod status;
mod worker;

use escape::escape_markup;
use git::Git;
use repo::RepoInfo;
use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::localize;
use sicompass_sdk::{
    BuiltinManifest, ListItem, Provider, SettingDecl, register_builtin_manifest,
    register_provider_factory,
};
use status::Status;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

pub use git::_set_test_no_network;

/// Register this crate's Fluent bundles. Idempotent.
///
/// Called from `register()` *and* from every trait method that resolves a
/// string: a provider built through the factory can be reached before
/// `register()` has run on some paths, and an unresolved key renders as the key
/// itself.
pub fn register_translations() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let _ = localize::register_bundle("en-US", include_str!("../locales/en-US.ftl"));
        let _ = localize::register_bundle("nl-BE", include_str!("../locales/nl-BE.ftl"));
        let _ = localize::register_bundle("fr-BE", include_str!("../locales/fr-BE.ftl"));
        let _ = localize::register_bundle("de-BE", include_str!("../locales/de-BE.ftl"));
    });
}

// ---------------------------------------------------------------------------
// Command ids
//
// These are what the user reads: the app's palette renders the raw strings from
// `commands()` and never calls `command_label()` (src/sicompass/src/list.rs).
// `command_label` is implemented anyway, for a WASM host and for the day that
// changes, but nothing here may depend on it.
//
// None of them may equal a reserved id. `"delete"` hijacks Ctrl+D and Delete,
// and `"toggle bookmark"` hijacks `b`; both are matched by exact equality, so
// `"delete branch"` is safe. `"refresh"` is claimed deliberately: F5 dispatches
// it. `"browse"` is the app's view-swap sentinel and must be left alone.
// ---------------------------------------------------------------------------

/// The browse view's only command, so `:` fires it directly.
pub const CMD_OPEN: &str = "open repository";
/// Go back to the folder listing.
///
/// Handled but never listed in [`commands()`](Provider::commands): Escape at
/// the repository root is the way out, and a palette entry doing exactly that
/// would be a second name for one action. The app dispatches it by name when
/// Escape fires.
pub const CMD_CLOSE: &str = "close repository";
pub const CMD_REFRESH: &str = "refresh";
pub const CMD_FETCH: &str = "fetch";
pub const CMD_PULL: &str = "pull";
pub const CMD_PUSH: &str = "push";

const CMD_STAGE: &str = "stage";
const CMD_UNSTAGE: &str = "unstage";
const CMD_STAGE_ALL: &str = "stage all";
const CMD_UNSTAGE_ALL: &str = "unstage all";
const CMD_DISCARD: &str = "discard changes";
const CMD_IGNORE: &str = "ignore file";

const CMD_CHECKOUT_COMMIT: &str = "checkout commit";
const CMD_BRANCH_HERE: &str = "create branch here";
const CMD_REVERT: &str = "revert commit";
const CMD_CHERRY_PICK: &str = "cherry-pick commit";
const CMD_ALL_BRANCHES: &str = "show all branches";
const CMD_THIS_BRANCH: &str = "show only this branch";
const CMD_LOAD_MORE: &str = "load more commits";

const CMD_CHECKOUT_BRANCH: &str = "checkout branch";
const CMD_CREATE_BRANCH: &str = "create branch";
const CMD_DELETE_BRANCH: &str = "delete branch";
const CMD_MERGE: &str = "merge branch into current";
const CMD_REBASE: &str = "rebase current onto branch";

const CMD_STASH: &str = "stash changes";
const CMD_APPLY_STASH: &str = "apply stash";
const CMD_POP_STASH: &str = "pop stash";
const CMD_DROP_STASH: &str = "drop stash";

const CMD_ADD_REMOTE: &str = "add remote";
const CMD_REMOVE_REMOTE: &str = "remove remote";
const CMD_RENAME_REMOTE: &str = "rename remote";

/// What the app is waiting for the user to supply before a command can run.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Pending {
    /// A destructive command that has asked for confirmation.
    ///
    /// The app has no dialogs, so the confirmation is the second phase of the
    /// ordinary two-phase command: `command_list_items` offers "cancel" and
    /// "yes", and `execute_command` receives the answer.
    Confirm { command: String, target: Segment },
    /// A command that needs typed text, which arrives through `commit_edit`.
    Text(TextOp),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TextOp {
    /// A new branch at HEAD.
    CreateBranch,
    /// A new branch at a particular commit.
    BranchAt(String),
    /// `<name> <url>`.
    AddRemote,
    /// Rename this remote to what is typed.
    RenameRemote(String),
    /// An optional message for a new stash.
    StashMessage,
}

// ---------------------------------------------------------------------------
// Path model
// ---------------------------------------------------------------------------

/// One step down the repository tree.
///
/// Stable identifiers rather than display text. The displayed labels are
/// localized and carry git's own words, neither of which can be a path segment:
/// `current_path()` has to round-trip through `set_current_path` unchanged
/// (`refresh_visible_path` saves and restores it around every refresh) and has
/// to survive a language change between sessions.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Segment {
    Section(Section),
    /// A changed file under `changes`.
    ///
    /// The group is part of the identity, not decoration: a partially staged
    /// file is listed twice, once with its staged change and once with its
    /// unstaged one, and `stage` on the two rows means opposite things.
    File(Group, Vec<u8>),
    /// A file inside a commit or a stash, where there is only one of it.
    Path(Vec<u8>),
    /// Inside `graph`: a commit, by full object id.
    Commit(String),
    /// Inside `branches`: local or remote-tracking.
    Scope(Scope),
    Branch(String),
    /// Inside `stashes`: the stash's index in `stash list`.
    Stash(usize),
    Remote(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Section {
    Changes,
    Graph,
    Branches,
    Stashes,
    Remotes,
}

/// Which half of the index a change sits in.
///
/// Conflicts are a third state rather than a flavour of unstaged: they cannot
/// be committed, and `stage` on one means "mark resolved".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Group {
    Conflict,
    Staged,
    Unstaged,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Scope {
    Local,
    Remote,
}

impl Segment {
    /// The token used inside `current_path()`.
    ///
    /// A prefix letter keeps the alphabets apart, and `/` and `%` are
    /// percent-escaped so a path segment containing a slash (every file below
    /// the top level) cannot be mistaken for two segments.
    fn token(&self) -> String {
        match self {
            Segment::Section(s) => match s {
                Section::Changes => "changes".into(),
                Section::Graph => "graph".into(),
                Section::Branches => "branches".into(),
                Section::Stashes => "stashes".into(),
                Section::Remotes => "remotes".into(),
            },
            Segment::File(g, p) => format!(
                "{}:{}",
                match g {
                    Group::Conflict => "x",
                    Group::Staged => "g",
                    Group::Unstaged => "u",
                },
                escape_token(&String::from_utf8_lossy(p))
            ),
            Segment::Path(p) => format!("f:{}", escape_token(&String::from_utf8_lossy(p))),
            Segment::Commit(c) => format!("c:{c}"),
            Segment::Scope(s) => match s {
                Scope::Local => "local".into(),
                Scope::Remote => "remote".into(),
            },
            Segment::Branch(b) => format!("b:{}", escape_token(b)),
            Segment::Stash(i) => format!("s:{i}"),
            Segment::Remote(r) => format!("r:{}", escape_token(r)),
        }
    }

    fn from_token(token: &str) -> Option<Segment> {
        Some(match token {
            "changes" => Segment::Section(Section::Changes),
            "graph" => Segment::Section(Section::Graph),
            "branches" => Segment::Section(Section::Branches),
            "stashes" => Segment::Section(Section::Stashes),
            "remotes" => Segment::Section(Section::Remotes),
            "local" => Segment::Scope(Scope::Local),
            "remote" => Segment::Scope(Scope::Remote),
            other => {
                let (tag, rest) = other.split_once(':')?;
                match tag {
                    "x" => Segment::File(Group::Conflict, unescape_token(rest).into_bytes()),
                    "g" => Segment::File(Group::Staged, unescape_token(rest).into_bytes()),
                    "u" => Segment::File(Group::Unstaged, unescape_token(rest).into_bytes()),
                    "f" => Segment::Path(unescape_token(rest).into_bytes()),
                    "c" => Segment::Commit(rest.to_owned()),
                    "b" => Segment::Branch(unescape_token(rest)),
                    "s" => Segment::Stash(rest.parse().ok()?),
                    "r" => Segment::Remote(unescape_token(rest)),
                    _ => return None,
                }
            }
        })
    }
}

/// Parse the token half of a repository path.
///
/// An unparseable token is a path saved by an older layout: the repository root
/// is always a real place, half a guess is not.
fn parse_tokens(rest: &str) -> Vec<Segment> {
    let mut out = Vec::new();
    for token in rest.split('/').filter(|t| !t.is_empty()) {
        match Segment::from_token(token) {
            Some(s) => out.push(s),
            None => return Vec::new(),
        }
    }
    out
}

fn escape_token(s: &str) -> String {
    s.replace('%', "%25").replace('/', "%2F")
}

fn unescape_token(s: &str) -> String {
    s.replace("%2F", "/").replace("%25", "%")
}

/// Separates the repository's own directory from the tokens below it inside
/// `current_path()`.
///
/// A unit separator, because it has to be something no directory is ever
/// called: the app hands `current_path()` back on restart as a filesystem path,
/// and this is what tells the folder view "that was a repository, reopen it"
/// rather than "browse to that folder".
const REPO_MARKER: &str = "\u{1f}";

/// Split a repository path into the repository's directory and the tokens
/// below it. `None` for an ordinary filesystem path.
fn split_repo_path(path: &str) -> Option<(&str, &str)> {
    let needle = format!("/{REPO_MARKER}");
    let at = path.find(&needle)?;
    let rest = &path[at + needle.len()..];
    Some((&path[..at], rest.trim_start_matches('/')))
}

/// Which list `fetch()` is serving.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum View {
    /// Subdirectories, to pick a repository from. No git state is held.
    #[default]
    Browse,
    /// The repository itself.
    Repo,
}

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

pub struct GitClientProvider {
    view: View,
    /// The folder the browse view is listing.
    browse_path: PathBuf,
    /// The open repository. `None` in the browse view.
    repo: Option<RepoInfo>,
    /// Where the cursor is inside the repository.
    segments: Vec<Segment>,

    /// The configured `git` binary (the `gitBinary` setting).
    binary: String,

    /// The commit message being composed, as typed.
    message: String,

    /// Cached reads, all dropped together by
    /// [`GitClientProvider::invalidate`].
    ///
    /// A single refresh renders several levels, and the repository root alone
    /// needs `git status` for its header. Without a cache one keypress would
    /// run the same command three or four times.
    status: Option<Status>,
    commits: Option<Vec<log::Commit>>,
    branches: Option<Vec<refs::Branch>>,
    stashes: Option<Vec<refs::Stash>>,
    remotes: Option<Vec<refs::Remote>>,

    /// How many commits the graph is showing. Grows by a page each time the
    /// `load more` button is pressed.
    graph_limit: usize,
    /// Whether the graph walks every ref or only the current branch.
    graph_all: bool,

    /// What a two-phase command is waiting for.
    ///
    /// `command_list_items` takes `&self`, so a command that needs a second
    /// phase has to work out its target in `handle_command` and leave it here.
    pending: Option<Pending>,

    /// Runs the remote-contacting commands off the render thread.
    network: worker::Network,
    /// Notices changes made to the repository from outside the app.
    watcher: Option<worker::Watcher>,
    /// Set when something changed on disk while the cursor was too deep for a
    /// silent refresh to be safe. Renders as a row rather than moving anything.
    stale: bool,
    needs_refresh: bool,

    /// Undo entries produced since the app last drained them.
    timeline: Vec<sicompass_sdk::timeline::TimelineEntry>,

    /// How often to fetch on our own, in minutes. Zero is off, and off is the
    /// default: an app that contacts a remote unattended can sit failing on a
    /// credential prompt in the background forever, and nobody asked it to.
    autofetch_minutes: u64,
    /// When the last automatic fetch started, so the interval is measured from
    /// the fetch rather than from the first frame after it.
    last_autofetch: Option<std::time::Instant>,

    /// One-shot error for the app's status line.
    error: Option<String>,

    /// `current_path()` hands back a borrow, so the rendered form is kept in a
    /// field and refreshed by [`GitClientProvider::sync_rendered_path`] after
    /// anything that moves the cursor.
    rendered_path: String,

    /// Label to [`Segment`] per level, rebuilt on each `fetch()`.
    ///
    /// `push_path` receives the *displayed* label, which for a file is git's
    /// own word plus a possibly non-UTF-8 path, and for a section is localized
    /// text. Parsing an identifier back out of that is not possible, so the map
    /// that produced the labels answers instead. Keyed by the level, because
    /// the same label appears in two levels at once: a partially staged file is
    /// in both the staged and the unstaged group.
    labels: HashMap<Vec<Segment>, HashMap<String, Segment>>,
}

impl Default for GitClientProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl GitClientProvider {
    pub fn new() -> Self {
        register_translations();
        GitClientProvider {
            view: View::Browse,
            browse_path: PathBuf::from("/"),
            repo: None,
            segments: Vec::new(),
            binary: "git".to_owned(),
            message: String::new(),
            status: None,
            commits: None,
            branches: None,
            stashes: None,
            remotes: None,
            graph_limit: GRAPH_PAGE,
            graph_all: false,
            pending: None,
            network: worker::Network::new(),
            watcher: None,
            stale: false,
            needs_refresh: false,
            timeline: Vec::new(),
            autofetch_minutes: 0,
            last_autofetch: None,
            error: None,
            rendered_path: "/".to_owned(),
            labels: HashMap::new(),
        }
    }

    /// Recompute the string `current_path()` hands back.
    ///
    /// Called after anything that moves the cursor. In the browse view it is
    /// the folder being listed. In the repository it is the repository's own
    /// directory followed by the stable tokens, joined with `/`.
    ///
    /// Rooting it at a real directory rather than at a synthetic `"/"` is what
    /// makes a restart land somewhere useful. The app writes `current_path()`
    /// out when a tab closes and feeds it back as a filesystem path on restart,
    /// through `deep_rebuild_provider_tree`, which splits off the navigated
    /// segments with `PathBuf::pop` and re-fetches at what is left. A synthetic
    /// path made that prefix `"/"`, so the tab came back at the filesystem
    /// root; this way it comes back in the repository's own folder, one `:`
    /// from where it was. That is the same degradation the terminal has, which
    /// does not persist its shell either.
    fn sync_rendered_path(&mut self) {
        self.rendered_path = match self.view {
            View::Browse => self.browse_path.to_string_lossy().into_owned(),
            View::Repo => {
                let mut out = self
                    .repo_anchor()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "/".to_owned());
                out.push('/');
                out.push_str(REPO_MARKER);
                for seg in &self.segments {
                    out.push('/');
                    out.push_str(&seg.token());
                }
                out
            }
        };
    }

    /// A `git` pointed at the open repository's worktree, or at the browsed
    /// folder when none is open.
    fn git(&self) -> Git {
        Git::new(
            self.binary.clone(),
            self.repo_anchor()
                .unwrap_or_else(|| self.browse_path.clone()),
        )
    }

    /// The directory the open repository lives at: its worktree, or its git
    /// directory for a bare one, which has no worktree.
    ///
    /// This is what `current_path()` is rooted at, so it has to be a real
    /// directory: the app saves that string when a tab closes and hands it back
    /// as a filesystem path on restart.
    fn repo_anchor(&self) -> Option<PathBuf> {
        let info = self.repo.as_ref()?;
        Some(if info.root.as_os_str().is_empty() {
            info.common_dir.clone()
        } else {
            info.root.clone()
        })
    }

    /// Record the label a child was rendered with, so `push_path` can resolve
    /// it back to a [`Segment`].
    fn remember(&mut self, level: &[Segment], label: &str, segment: Segment) {
        self.labels
            .entry(level.to_vec())
            .or_default()
            .insert(label.to_owned(), segment);
    }

    /// Start a fresh label map for the level about to be rendered.
    fn forget_level(&mut self, level: &[Segment]) {
        self.labels.remove(level);
    }

    fn set_error(&mut self, message: String) {
        self.error = Some(message);
    }

    /// Drop every cached read. The next `fetch()` asks git again.
    fn invalidate(&mut self) {
        self.status = None;
        self.commits = None;
        self.branches = None;
        self.stashes = None;
        self.remotes = None;
    }

    /// `git status`, cached for the duration of one tree state.
    ///
    /// One `fetch()` of the repository root renders the header line and the
    /// group counts from it, and the `changes` level renders the file rows from
    /// it, so without a cache a single refresh runs `git status` several times.
    fn status(&mut self) -> Option<&Status> {
        if self.status.is_none() {
            match status::read(&self.git()) {
                Ok(s) => self.status = Some(s),
                Err(e) => {
                    self.set_error(e.message());
                    return None;
                }
            }
        }
        self.status.as_ref()
    }

    // ---- Browse view ----------------------------------------------------

    /// Subdirectories of `browse_path`, then one row saying whether this folder
    /// is a repository.
    ///
    /// Bare `Obj`s with no `<input>` tag, like the terminal's browse view:
    /// nothing here is renameable, and a `+i` row would look to the app like a
    /// live input slot.
    ///
    /// Files are left out. This view exists only to pick a repository, and
    /// omitting them keeps a large directory short enough to walk by ear.
    fn browse_children(&mut self) -> Vec<FfonElement> {
        // A restored path can name a folder that no longer exists: one deleted
        // since the tab was saved, or the repository path of a tab that was
        // closed inside a repository, whose trailing segments are tokens rather
        // than directories. Walking up to the nearest folder that does exist
        // lands the user near where they were instead of on an empty listing
        // they cannot navigate out of downwards.
        while !self.browse_path.is_dir() && self.browse_path.pop() {}
        if self.browse_path.as_os_str().is_empty() {
            self.browse_path = PathBuf::from("/");
        }
        self.sync_rendered_path();

        let mut names: Vec<String> = Vec::new();
        if let Ok(read_dir) = std::fs::read_dir(&self.browse_path) {
            for entry in read_dir.flatten() {
                // `metadata()` follows symlinks, so a symlink to a directory is
                // offered as one. Entries whose metadata cannot be read
                // (broken symlinks, races, permission holes) are skipped rather
                // than shown as dead ends.
                if entry.metadata().map(|m| m.is_dir()).unwrap_or(false) {
                    names.push(entry.file_name().to_string_lossy().into_owned());
                }
            }
        }
        names.sort_by(|a, b| natord::compare_ignore_case(a, b));

        // `.git` is listed like any other folder. It is the plainest possible
        // sign that a folder is a repository, it is searchable by name, and
        // walking into it is harmless: `is_repository` asks exactly what
        // `open repository` asks, so the answer there is simply no.
        //
        // Nothing here consults git. This runs on every arrow press through a
        // directory tree, and a `rev-parse` per row is not worth an indicator
        // the folder listing already gives away.
        let mut out: Vec<FfonElement> = names
            .into_iter()
            .map(|n| FfonElement::new_obj(escape_markup(&n)))
            .collect();

        // A folder with no subfolders would otherwise be an empty level, and
        // the app seeds those with its own "insert here" row: a typing
        // affordance wired to `create_directory`, which this provider does not
        // implement, so it would sit there doing nothing.
        if out.is_empty() {
            out.push(FfonElement::new_str(localize::t("gitclient-empty")));
        }
        out
    }

    // ---- Repository view -------------------------------------------------

    /// Dispatch on where the cursor is. Each arm renders exactly one level.
    fn repo_children(&mut self) -> Vec<FfonElement> {
        let level = self.segments.clone();
        // The labels this level had last time are about to be replaced.
        self.forget_level(&level);

        let children = match level.as_slice() {
            [] => self.root_children(),
            [Segment::Section(Section::Changes)] => self.changes_children(),
            [
                Segment::Section(Section::Changes),
                Segment::File(group, path),
            ] => {
                let (group, path) = (*group, path.clone());
                self.file_diff(group, &path)
            }

            [Segment::Section(Section::Graph)] => self.graph_children(),
            [Segment::Section(Section::Graph), Segment::Commit(oid)] => {
                let oid = oid.clone();
                self.commit_children(&oid)
            }
            [
                Segment::Section(Section::Graph),
                Segment::Commit(oid),
                Segment::Path(path),
            ] => {
                let (oid, path) = (oid.clone(), path.clone());
                self.object_file_diff(&oid, &path)
            }

            [Segment::Section(Section::Branches)] => self.scopes_children(),
            [Segment::Section(Section::Branches), Segment::Scope(scope)] => {
                let scope = *scope;
                self.branch_list_children(scope)
            }
            [
                Segment::Section(Section::Branches),
                Segment::Scope(_),
                Segment::Branch(name),
            ] => {
                let name = name.clone();
                self.branch_children(&name)
            }

            [Segment::Section(Section::Stashes)] => self.stash_list_children(),
            [Segment::Section(Section::Stashes), Segment::Stash(index)] => {
                let index = *index;
                self.stash_children(index)
            }
            [
                Segment::Section(Section::Stashes),
                Segment::Stash(index),
                Segment::Path(path),
            ] => {
                let (index, path) = (*index, path.clone());
                match self.stash_oid(index) {
                    Some(oid) => self.object_file_diff(&oid, &path),
                    None => vec![FfonElement::new_str(localize::t("gitclient-gone"))],
                }
            }

            [Segment::Section(Section::Remotes)] => self.remote_list_children(),
            [Segment::Section(Section::Remotes), Segment::Remote(name)] => {
                let name = name.clone();
                self.remote_children(&name)
            }

            // A level that no longer exists, most often because the thing it
            // described was staged, committed or deleted underneath it.
            _ => vec![FfonElement::new_str(localize::t("gitclient-gone"))],
        };

        // The app seeds its own "insert here" placeholder into an empty level,
        // which here would be a row that types into `commit_edit` and means
        // nothing. Say what is actually true instead.
        let mut children = if children.is_empty() {
            vec![FfonElement::new_str(localize::t("gitclient-empty"))]
        } else {
            children
        };

        // Both notices go at the top of whichever level the user is on, so
        // they are read without having to go looking. Neither moves the cursor:
        // the app finds it again by the row's label.
        if self.stale {
            children.insert(0, FfonElement::new_str(localize::t("gitclient-stale")));
        }
        if let Some(running) = self.network.running() {
            let mut a = localize::Args::new();
            a.set("what", running);
            children.insert(
                0,
                FfonElement::new_str(localize::t_args("gitclient-working", &a)),
            );
        }
        children
    }

    /// The repository root: one status line, then the sections.
    fn root_children(&mut self) -> Vec<FfonElement> {
        let level = self.segments.clone();
        let bare = self.repo.as_ref().is_some_and(|r| r.bare);
        let mut out = vec![FfonElement::new_str(self.head_line())];

        // A bare repository has no worktree, so there is nothing to stage.
        let sections: &[Section] = if bare {
            &[
                Section::Graph,
                Section::Branches,
                Section::Stashes,
                Section::Remotes,
            ]
        } else {
            &[
                Section::Changes,
                Section::Graph,
                Section::Branches,
                Section::Stashes,
                Section::Remotes,
            ]
        };
        for section in sections {
            let label = section_label(*section);
            self.remember(&level, &label, Segment::Section(*section));
            out.push(FfonElement::new_obj(label));
        }
        out
    }

    /// The one-line summary of where HEAD is.
    fn head_line(&mut self) -> String {
        let name = self
            .repo
            .as_ref()
            .map(|r| r.name())
            .unwrap_or_else(|| "repository".to_owned());
        let name = escape_markup(&name);

        let Some(status) = self.status() else {
            // `status()` has already put the reason on the error line.
            return localize::t_args("gitclient-head-unreadable", &{
                let mut a = localize::Args::new();
                a.set("repo", name.clone());
                a
            });
        };
        let branch = status.branch.clone();

        let mut line = if branch.unborn() {
            let mut a = localize::Args::new();
            a.set("repo", name);
            localize::t_args("gitclient-head-unborn", &a)
        } else if let Some(head) = &branch.head {
            let mut a = localize::Args::new();
            a.set("repo", name);
            a.set("branch", escape_markup(head));
            localize::t_args("gitclient-head-branch", &a)
        } else {
            let mut a = localize::Args::new();
            a.set("repo", name);
            a.set(
                "oid",
                branch
                    .oid
                    .as_deref()
                    .unwrap_or("")
                    .chars()
                    .take(7)
                    .collect::<String>(),
            );
            localize::t_args("gitclient-head-detached", &a)
        };

        if branch.upstream.is_some() {
            if branch.ahead > 0 {
                let mut a = localize::Args::new();
                a.set("n", branch.ahead);
                line.push_str(", ");
                line.push_str(&localize::t_args("gitclient-ahead", &a));
            }
            if branch.behind > 0 {
                let mut a = localize::Args::new();
                a.set("n", branch.behind);
                line.push_str(", ");
                line.push_str(&localize::t_args("gitclient-behind", &a));
            }
            if branch.ahead == 0 && branch.behind == 0 {
                line.push_str(", ");
                line.push_str(&localize::t("gitclient-in-sync"));
            }
        } else if !branch.unborn() && !branch.detached() {
            line.push_str(", ");
            line.push_str(&localize::t("gitclient-no-upstream"));
        }
        line
    }

    /// `changes`: the commit message, the four commit buttons, then every
    /// changed file.
    ///
    /// The message is the **first** row on purpose. The app's insert palette
    /// takes over `:` when the *last* row of a level is an input slot, and it
    /// would then be impossible to reach the git command palette from here.
    fn changes_children(&mut self) -> Vec<FfonElement> {
        let level = self.segments.clone();
        let mut out = Vec::new();

        out.push(FfonElement::new_str(format!(
            "{}<input>{}</input>",
            localize::t("gitclient-message-label"),
            escape_markup(&self.message)
        )));
        for (function, key) in COMMIT_BUTTONS {
            out.push(FfonElement::new_str(format!(
                "<button>{function}</button>{}",
                localize::t(key)
            )));
        }

        let Some(status) = self.status() else {
            return out;
        };
        // Conflicts first: nothing else can be committed until they are gone.
        // Then staged, then unstaged, so the list reads in the order the work
        // moves through it.
        let rows: Vec<(Group, String, Vec<u8>)> = [Group::Conflict, Group::Staged, Group::Unstaged]
            .iter()
            .flat_map(|group| {
                let group = *group;
                let entries: Vec<_> = match group {
                    Group::Conflict => status.conflicts().collect(),
                    Group::Staged => status.staged().collect(),
                    Group::Unstaged => status.unstaged().collect(),
                };
                entries
                    .into_iter()
                    .map(move |e| (group, change_label(e, group), e.path.clone()))
                    .collect::<Vec<_>>()
            })
            .collect();

        if rows.is_empty() {
            out.push(FfonElement::new_str(localize::t("gitclient-clean")));
            return out;
        }
        for (group, label, path) in rows {
            self.remember(&level, &label, Segment::File(group, path));
            out.push(FfonElement::new_obj(label));
        }
        out
    }

    /// The diff for one changed file.
    fn file_diff(&mut self, group: Group, path: &[u8]) -> Vec<FfonElement> {
        let git = self.git();
        let spec = git::literal_pathspec(path);

        // An untracked file has no diff: git has never seen it, so
        // `git diff` says nothing at all. Its content *is* the change.
        //
        // Through the accessor, not the cached field: this level can be the
        // first one fetched, with nothing cached yet. That happens on a restore
        // straight back into a diff, and on every refresh, which re-fetches the
        // deepest level first. Reading the bare field there decided "not
        // untracked" and rendered an empty diff.
        let untracked = self
            .status()
            .and_then(|s| s.entries.iter().find(|e| e.path == path))
            .is_some_and(|e| e.kind == status::EntryKind::Untracked);
        if untracked {
            return self.untracked_content(path);
        }

        let out = match group {
            Group::Staged => git.try_run([
                std::ffi::OsString::from("diff"),
                std::ffi::OsString::from("--cached"),
                std::ffi::OsString::from("--"),
                spec,
            ]),
            // A conflicted file gets a combined diff, with two status columns
            // instead of one. That is the right thing to read while resolving.
            Group::Unstaged | Group::Conflict => git.try_run([
                std::ffi::OsString::from("diff"),
                std::ffi::OsString::from("--"),
                spec,
            ]),
        };
        if !out.ok() {
            self.set_error(format!("git diff: {}", out.stderr));
            return Vec::new();
        }
        diff_rows(&out.stdout)
    }

    /// An untracked file's own contents, rendered like an all-additions diff.
    fn untracked_content(&mut self, path: &[u8]) -> Vec<FfonElement> {
        let Some(root) = self.repo.as_ref().map(|r| r.root.clone()) else {
            return Vec::new();
        };
        let full = root.join(PathBuf::from(git::os_string_from_bytes(path)));
        let Ok(bytes) = std::fs::read(&full) else {
            return vec![FfonElement::new_str(localize::t("gitclient-unreadable"))];
        };
        if is_binary(&bytes) {
            return vec![FfonElement::new_str(localize::t("gitclient-binary"))];
        }
        let mut prefixed = Vec::with_capacity(bytes.len() + 16);
        for line in bytes.split(|b| *b == b'\n') {
            prefixed.push(b'+');
            prefixed.extend_from_slice(line);
            prefixed.push(b'\n');
        }
        diff_rows(&prefixed)
    }

    // ---- Graph -----------------------------------------------------------

    /// The commit list, cached for the current page size and scope.
    fn commits(&mut self) -> Option<&[log::Commit]> {
        if self.commits.is_none() {
            // `git log` exits 128 on an unborn HEAD rather than printing
            // nothing, so a repository with no commits has to be recognised
            // before asking rather than by reading the failure.
            if self.status().is_some_and(|s| s.branch.unborn()) {
                self.commits = Some(Vec::new());
                return self.commits.as_deref();
            }
            let all = self.graph_all;
            let limit = self.graph_limit;
            match log::read(&self.git(), 0, limit, all) {
                Ok(c) => self.commits = Some(c),
                Err(e) => {
                    self.set_error(e.message());
                    return None;
                }
            }
        }
        self.commits.as_deref()
    }

    /// `graph`: one row per commit, newest first, then a button for the next
    /// page while there might be one.
    fn graph_children(&mut self) -> Vec<FfonElement> {
        let level = self.segments.clone();
        if self.status().is_some_and(|s| s.branch.unborn()) {
            return vec![FfonElement::new_str(localize::t("gitclient-no-commits"))];
        }
        let Some(commits) = self.commits() else {
            return Vec::new();
        };
        // Cloned out of the cache so the borrow ends before `remember`.
        let rows: Vec<(String, String)> = commits
            .iter()
            .map(|c| {
                (
                    format!("{} {}", c.short, escape_markup(&c.subject)),
                    c.oid.clone(),
                )
            })
            .collect();
        let page_full = rows.len() == self.graph_limit;

        let mut out = Vec::with_capacity(rows.len() + 1);
        for (label, oid) in rows {
            self.remember(&level, &label, Segment::Commit(oid));
            out.push(FfonElement::new_obj(label));
        }
        if page_full {
            let mut a = localize::Args::new();
            a.set("n", GRAPH_PAGE as i64);
            out.push(FfonElement::new_str(format!(
                "<button>{BTN_LOAD_MORE}</button>{}",
                localize::t_args("gitclient-load-more", &a)
            )));
        }
        out
    }

    /// One commit: its message, then how big it is, then what it touched, then
    /// who and when.
    ///
    /// The message comes first because it is the answer to "what is this
    /// commit", and a screen reader reads the first row without being asked.
    fn commit_children(&mut self, oid: &str) -> Vec<FfonElement> {
        let level = self.segments.clone();
        let Some(commit) = self
            .commits()
            .and_then(|cs| cs.iter().find(|c| c.oid == oid))
            .cloned()
        else {
            return vec![FfonElement::new_str(localize::t("gitclient-gone"))];
        };

        let mut out = vec![FfonElement::new_str(escape_markup(&commit.subject))];
        for line in commit.body.lines() {
            out.push(FfonElement::new_str(escape_markup(line)));
        }

        let files = match log::files(&self.git(), oid) {
            Ok(f) => f,
            Err(e) => {
                self.set_error(e.message());
                Vec::new()
            }
        };
        out.push(FfonElement::new_str(stats_line(&log::stats(&files))));

        for file in &files {
            let label = commit_file_label(file);
            self.remember(&level, &label, Segment::Path(file.path.clone()));
            out.push(FfonElement::new_obj(label));
        }

        // Author, date and refs last: they are the same shape on every commit,
        // so putting them first would make every commit start identically.
        let mut a = localize::Args::new();
        a.set("author", escape_markup(&commit.author));
        out.push(FfonElement::new_str(localize::t_args(
            "gitclient-commit-author",
            &a,
        )));
        let mut a = localize::Args::new();
        a.set("date", escape_markup(&commit.date));
        out.push(FfonElement::new_str(localize::t_args(
            "gitclient-commit-date",
            &a,
        )));
        if !commit.refs.is_empty() {
            let mut a = localize::Args::new();
            a.set("refs", escape_markup(&commit.refs));
            out.push(FfonElement::new_str(localize::t_args(
                "gitclient-commit-refs",
                &a,
            )));
        }
        out
    }

    /// The diff one commit (or stash) made to one file.
    fn object_file_diff(&mut self, oid: &str, path: &[u8]) -> Vec<FfonElement> {
        let git = self.git();
        let out = git.try_run([
            std::ffi::OsString::from("show"),
            std::ffi::OsString::from("--format="),
            std::ffi::OsString::from("-m"),
            std::ffi::OsString::from("--first-parent"),
            std::ffi::OsString::from(oid),
            std::ffi::OsString::from("--"),
            git::literal_pathspec(path),
        ]);
        if !out.ok() {
            self.set_error(format!("git show: {}", out.stderr));
            return Vec::new();
        }
        diff_rows(&out.stdout)
    }

    // ---- Branches --------------------------------------------------------

    fn branch_cache(&mut self) -> Option<&[refs::Branch]> {
        if self.branches.is_none() {
            match refs::branches(&self.git()) {
                Ok(b) => self.branches = Some(b),
                Err(e) => {
                    self.set_error(e.message());
                    return None;
                }
            }
        }
        self.branches.as_deref()
    }

    fn scopes_children(&mut self) -> Vec<FfonElement> {
        let level = self.segments.clone();
        let mut out = Vec::new();
        for scope in [Scope::Local, Scope::Remote] {
            let label = localize::t(match scope {
                Scope::Local => "gitclient-scope-local",
                Scope::Remote => "gitclient-scope-remote",
            });
            self.remember(&level, &label, Segment::Scope(scope));
            out.push(FfonElement::new_obj(label));
        }
        out
    }

    fn branch_list_children(&mut self, scope: Scope) -> Vec<FfonElement> {
        let level = self.segments.clone();
        let Some(branches) = self.branch_cache() else {
            return Vec::new();
        };
        let want_remote = scope == Scope::Remote;
        let names: Vec<String> = branches
            .iter()
            .filter(|b| b.remote == want_remote)
            .map(|b| b.name.clone())
            .collect();
        if names.is_empty() {
            return vec![FfonElement::new_str(localize::t("gitclient-no-branches"))];
        }
        let mut out = Vec::with_capacity(names.len());
        for name in names {
            // The bare branch name, with nothing about its state: "current"
            // and the ahead/behind counts move as soon as anything happens,
            // and the key is what the cursor is found by after a refresh.
            let label = escape_markup(&name);
            self.remember(&level, &label, Segment::Branch(name));
            out.push(FfonElement::new_obj(label));
        }
        out
    }

    fn branch_children(&mut self, name: &str) -> Vec<FfonElement> {
        let Some(branch) = self
            .branch_cache()
            .and_then(|bs| bs.iter().find(|b| b.name == name))
            .cloned()
        else {
            return vec![FfonElement::new_str(localize::t("gitclient-gone"))];
        };

        let mut out = Vec::new();
        if branch.current {
            out.push(FfonElement::new_str(localize::t(
                "gitclient-branch-current",
            )));
        }
        match (&branch.upstream, branch.upstream_gone) {
            (Some(_), true) => {
                out.push(FfonElement::new_str(localize::t("gitclient-branch-gone")));
            }
            (Some(up), false) => {
                let mut a = localize::Args::new();
                a.set("upstream", escape_markup(up));
                out.push(FfonElement::new_str(localize::t_args(
                    "gitclient-branch-tracking",
                    &a,
                )));
                if branch.ahead > 0 {
                    let mut a = localize::Args::new();
                    a.set("n", branch.ahead as i64);
                    out.push(FfonElement::new_str(localize::t_args(
                        "gitclient-ahead",
                        &a,
                    )));
                }
                if branch.behind > 0 {
                    let mut a = localize::Args::new();
                    a.set("n", branch.behind as i64);
                    out.push(FfonElement::new_str(localize::t_args(
                        "gitclient-behind",
                        &a,
                    )));
                }
                if branch.ahead == 0 && branch.behind == 0 {
                    out.push(FfonElement::new_str(localize::t("gitclient-in-sync")));
                }
            }
            (None, _) if !branch.remote => {
                out.push(FfonElement::new_str(localize::t("gitclient-no-upstream")));
            }
            _ => {}
        }
        if let Some(path) = &branch.checked_out_in {
            let mut a = localize::Args::new();
            a.set("path", escape_markup(path));
            out.push(FfonElement::new_str(localize::t_args(
                "gitclient-branch-elsewhere",
                &a,
            )));
        }
        out.push(FfonElement::new_str(format!(
            "{} {}",
            branch.short_oid,
            escape_markup(&branch.subject)
        )));
        out
    }

    // ---- Stashes ---------------------------------------------------------

    fn stash_cache(&mut self) -> Option<&[refs::Stash]> {
        if self.stashes.is_none() {
            match refs::stashes(&self.git()) {
                Ok(s) => self.stashes = Some(s),
                Err(e) => {
                    self.set_error(e.message());
                    return None;
                }
            }
        }
        self.stashes.as_deref()
    }

    fn stash_oid(&mut self, index: usize) -> Option<String> {
        self.stash_cache()
            .and_then(|ss| ss.iter().find(|s| s.index == index))
            .map(|s| s.oid.clone())
    }

    fn stash_list_children(&mut self) -> Vec<FfonElement> {
        let level = self.segments.clone();
        let Some(stashes) = self.stash_cache() else {
            return Vec::new();
        };
        let rows: Vec<(usize, String)> = stashes
            .iter()
            .map(|s| {
                (
                    s.index,
                    // `stash@{n}` leads, because it is what every stash command
                    // takes and what the user has to say to talk about one.
                    format!("stash@{{{}}} {}", s.index, escape_markup(&s.message)),
                )
            })
            .collect();
        if rows.is_empty() {
            return vec![FfonElement::new_str(localize::t("gitclient-no-stashes"))];
        }
        let mut out = Vec::with_capacity(rows.len());
        for (index, label) in rows {
            self.remember(&level, &label, Segment::Stash(index));
            out.push(FfonElement::new_obj(label));
        }
        out
    }

    /// A stash is a commit, so it reads like one.
    fn stash_children(&mut self, index: usize) -> Vec<FfonElement> {
        let level = self.segments.clone();
        let Some(stash) = self
            .stash_cache()
            .and_then(|ss| ss.iter().find(|s| s.index == index))
            .cloned()
        else {
            return vec![FfonElement::new_str(localize::t("gitclient-gone"))];
        };

        let mut out = vec![FfonElement::new_str(escape_markup(&stash.message))];
        let files = match log::files(&self.git(), &stash.oid) {
            Ok(f) => f,
            Err(e) => {
                self.set_error(e.message());
                Vec::new()
            }
        };
        out.push(FfonElement::new_str(stats_line(&log::stats(&files))));
        for file in &files {
            let label = commit_file_label(file);
            self.remember(&level, &label, Segment::Path(file.path.clone()));
            out.push(FfonElement::new_obj(label));
        }
        out
    }

    // ---- Remotes ---------------------------------------------------------

    fn remote_cache(&mut self) -> Option<&[refs::Remote]> {
        if self.remotes.is_none() {
            match refs::remotes(&self.git()) {
                Ok(r) => self.remotes = Some(r),
                Err(e) => {
                    self.set_error(e.message());
                    return None;
                }
            }
        }
        self.remotes.as_deref()
    }

    fn remote_list_children(&mut self) -> Vec<FfonElement> {
        let level = self.segments.clone();
        let Some(remotes) = self.remote_cache() else {
            return Vec::new();
        };
        let names: Vec<String> = remotes.iter().map(|r| r.name.clone()).collect();
        if names.is_empty() {
            return vec![FfonElement::new_str(localize::t("gitclient-no-remotes"))];
        }
        let mut out = Vec::with_capacity(names.len());
        for name in names {
            let label = escape_markup(&name);
            self.remember(&level, &label, Segment::Remote(name));
            out.push(FfonElement::new_obj(label));
        }
        out
    }

    fn remote_children(&mut self, name: &str) -> Vec<FfonElement> {
        let Some(remote) = self
            .remote_cache()
            .and_then(|rs| rs.iter().find(|r| r.name == name))
            .cloned()
        else {
            return vec![FfonElement::new_str(localize::t("gitclient-gone"))];
        };
        let mut out = Vec::new();
        let mut a = localize::Args::new();
        a.set("url", escape_markup(&remote.fetch_url));
        out.push(FfonElement::new_str(localize::t_args(
            "gitclient-remote-fetch",
            &a,
        )));
        // Both are shown even when equal: "which url does a push go to" is a
        // question worth being able to answer without inferring it.
        let mut a = localize::Args::new();
        a.set("url", escape_markup(&remote.push_url));
        out.push(FfonElement::new_str(localize::t_args(
            "gitclient-remote-push",
            &a,
        )));
        out
    }

    // ---- View swap -------------------------------------------------------

    /// Open the repository containing the browsed folder. False when there is
    /// none, which the caller reports through the command's own error channel.
    fn open_repository(&mut self) -> bool {
        let git = self.git();
        match repo::discover(&git, &self.browse_path) {
            Some(info) => {
                self.watcher = Some(worker::Watcher::start(
                    info.git_dir.clone(),
                    info.common_dir.clone(),
                ));
                self.repo = Some(info);
                self.view = View::Repo;
                self.segments.clear();
                self.labels.clear();
                self.invalidate();
                self.sync_rendered_path();
                true
            }
            None => false,
        }
    }

    /// Go back to the folder listing, at the repository's own root.
    fn close_repository(&mut self) {
        // A bare repository has no worktree, so there is nothing to go back to
        // and the browse path stays where it was.
        if let Some(info) = &self.repo
            && !info.root.as_os_str().is_empty()
        {
            self.browse_path = info.root.clone();
        }
        self.repo = None;
        // Dropping it stops its thread, so a closed tab does not leave one
        // stating a directory nobody is looking at.
        self.watcher = None;
        self.view = View::Browse;
        self.segments.clear();
        self.labels.clear();
        self.invalidate();
        self.sync_rendered_path();
    }
}

// ---------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------

/// The two answers a confirmation can have. Non-empty on purpose: the app
/// falls back to the item's *label* when its data is empty, and the label is
/// translated.
const CONFIRM_YES: &str = "yes";
const CONFIRM_NO: &str = "no";

/// Which of the four commit buttons was pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CommitMode {
    Normal,
    /// Replace the previous commit rather than adding one.
    Amend,
    /// Commit, then push in the background.
    Push,
    /// Commit, then pull with rebase and push, stopping if the pull fails.
    Sync,
}

impl GitClientProvider {
    /// Whether the cursor is somewhere a silent refresh cannot lose anyone.
    ///
    /// The repository root and the changes list are short, their rows are
    /// stable, and the app finds the cursor again by matching the row's label.
    /// Deeper, the rows are diff lines, and a label that no longer matches
    /// puts the cursor on an unrelated line of an unrelated hunk.
    fn shallow_enough_to_refresh_silently(&self) -> bool {
        matches!(
            self.segments.as_slice(),
            [] | [Segment::Section(Section::Changes)]
        )
    }

    /// The thing the cursor is on, as this level's map understands it.
    fn resolve_target(&self, element_key: &str) -> Option<Segment> {
        let label = sicompass_sdk::tags::strip_display(element_key);
        self.labels.get(&self.segments)?.get(&label).cloned()
    }

    /// What a confirmation prompt calls the thing it is about.
    fn target_description(&self, target: &Segment) -> String {
        match target {
            Segment::File(_, path) => String::from_utf8_lossy(path).into_owned(),
            Segment::Branch(name) | Segment::Remote(name) => name.clone(),
            Segment::Stash(index) => format!("stash@{{{index}}}"),
            Segment::Commit(oid) => oid.chars().take(7).collect(),
            other => other.token(),
        }
    }

    /// Run git, reporting a failure on the status line rather than swallowing
    /// it. Returns true on success.
    fn run(&mut self, args: Vec<std::ffi::OsString>) -> bool {
        match self.git().run(&args) {
            Ok(_) => {
                self.invalidate();
                true
            }
            Err(e) => {
                self.set_error(if e.is_index_locked() {
                    // git's own message here is three lines about deleting a
                    // lock file by hand, which is the wrong advice for someone
                    // who simply has a rebase running in another window.
                    localize::t("gitclient-error-locked")
                } else {
                    e.message()
                });
                false
            }
        }
    }

    fn os(args: &[&str]) -> Vec<std::ffi::OsString> {
        args.iter().map(std::ffi::OsString::from).collect()
    }

    /// Dispatch one colon command.
    fn run_command(
        &mut self,
        cmd: &str,
        element_key: &str,
        pending: Option<Pending>,
        error: &mut String,
    ) -> Option<FfonElement> {
        // A confirmation still open for *this* command is the user answering
        // it, which the app routes through `execute_command`, not here.
        let _ = pending;

        match cmd {
            CMD_OPEN => {
                // Reported through the command's own error channel rather than
                // `take_error`: the app writes this one straight onto the
                // status line, and `:` in the folder listing has to answer.
                if !self.open_repository() {
                    *error = localize::t("gitclient-error-not-a-repository");
                }
            }
            CMD_CLOSE => self.close_repository(),
            CMD_REFRESH => {
                self.invalidate();
                self.stale = false;
            }

            CMD_FETCH => self.start_network(cmd, vec![vec!["fetch".into(), "--all".into()]], error),
            CMD_PULL => {
                self.start_network(cmd, vec![vec!["pull".into(), "--rebase".into()]], error)
            }
            CMD_PUSH => self.start_network(cmd, vec![vec!["push".into()]], error),

            CMD_STAGE_ALL => {
                if self.run(Self::os(&["add", "-A"])) {
                    self.record("stage-all", &[], cmd);
                }
            }
            CMD_UNSTAGE_ALL => {
                let args = self.unstage_all_args();
                if self.run(args) {
                    self.record("unstage-all", &[], cmd);
                }
            }
            CMD_STAGE | CMD_UNSTAGE | CMD_DISCARD | CMD_IGNORE => {
                match self.resolve_target(element_key) {
                    Some(Segment::File(group, path)) => {
                        return self.file_command(cmd, group, path, error);
                    }
                    _ => *error = localize::t("gitclient-error-select-a-file"),
                }
            }

            CMD_CHECKOUT_COMMIT | CMD_BRANCH_HERE | CMD_REVERT | CMD_CHERRY_PICK => {
                match self.resolve_target(element_key) {
                    Some(Segment::Commit(oid)) => return self.commit_command(cmd, oid, error),
                    _ => *error = localize::t("gitclient-error-select-a-commit"),
                }
            }
            CMD_LOAD_MORE => {
                self.graph_limit += GRAPH_PAGE;
                self.commits = None;
            }
            CMD_ALL_BRANCHES | CMD_THIS_BRANCH => {
                self.graph_all = cmd == CMD_ALL_BRANCHES;
                self.graph_limit = GRAPH_PAGE;
                self.commits = None;
            }

            CMD_CREATE_BRANCH => {
                self.pending = Some(Pending::Text(TextOp::CreateBranch));
                return Some(FfonElement::new_str("<input></input>"));
            }
            CMD_CHECKOUT_BRANCH | CMD_DELETE_BRANCH | CMD_MERGE | CMD_REBASE => {
                match self.resolve_target(element_key) {
                    Some(Segment::Branch(name)) => return self.branch_command(cmd, name, error),
                    _ => *error = localize::t("gitclient-error-select-a-branch"),
                }
            }

            CMD_STASH => {
                self.pending = Some(Pending::Text(TextOp::StashMessage));
                return Some(FfonElement::new_str("<input></input>"));
            }
            CMD_APPLY_STASH | CMD_POP_STASH | CMD_DROP_STASH => {
                match self.resolve_target(element_key) {
                    Some(Segment::Stash(index)) => return self.stash_command(cmd, index, error),
                    _ => *error = localize::t("gitclient-error-select-a-stash"),
                }
            }

            CMD_ADD_REMOTE => {
                self.pending = Some(Pending::Text(TextOp::AddRemote));
                return Some(FfonElement::new_str("<input></input>"));
            }
            CMD_REMOVE_REMOTE | CMD_RENAME_REMOTE => match self.resolve_target(element_key) {
                Some(Segment::Remote(name)) => return self.remote_command(cmd, name, error),
                _ => *error = localize::t("gitclient-error-select-a-remote"),
            },

            _ => {}
        }
        None
    }

    /// `git reset` needs a commit to reset to, and an unborn HEAD has none.
    fn unstage_all_args(&mut self) -> Vec<std::ffi::OsString> {
        if self.status().is_some_and(|s| s.branch.unborn()) {
            Self::os(&["rm", "--cached", "-r", "-q", "--", "."])
        } else {
            Self::os(&["reset", "-q"])
        }
    }

    fn file_command(
        &mut self,
        cmd: &str,
        group: Group,
        path: Vec<u8>,
        error: &mut String,
    ) -> Option<FfonElement> {
        let spec = git::literal_pathspec(&path);
        match cmd {
            CMD_STAGE => {
                // `git add` on a conflicted file is what marks it resolved,
                // which is why conflicts share this command rather than needing
                // their own.
                if self.run(vec!["add".into(), "--".into(), spec]) {
                    self.record("stage", &[String::from_utf8_lossy(&path).into_owned()], cmd);
                }
            }
            CMD_UNSTAGE => {
                if group != Group::Staged {
                    *error = localize::t("gitclient-error-not-staged");
                    return None;
                }
                let args = if self.status().is_some_and(|s| s.branch.unborn()) {
                    // Nothing to reset *to* before the first commit, so the
                    // path is removed from the index instead.
                    vec![
                        "rm".into(),
                        "--cached".into(),
                        "-q".into(),
                        "--".into(),
                        spec,
                    ]
                } else {
                    vec![
                        "reset".into(),
                        "-q".into(),
                        "HEAD".into(),
                        "--".into(),
                        spec,
                    ]
                };
                if self.run(args) {
                    self.record(
                        "unstage",
                        &[String::from_utf8_lossy(&path).into_owned()],
                        cmd,
                    );
                }
            }
            CMD_DISCARD => {
                // The only command here that destroys work with no way back,
                // so it asks first.
                self.pending = Some(Pending::Confirm {
                    command: cmd.to_owned(),
                    target: Segment::File(group, path),
                });
            }
            CMD_IGNORE => self.ignore(&path),
            _ => {}
        }
        None
    }

    /// Append a path to `.gitignore`, creating it if need be.
    fn ignore(&mut self, path: &[u8]) {
        let Some(root) = self.repo.as_ref().map(|r| r.root.clone()) else {
            return;
        };
        let file = root.join(".gitignore");
        let existing = std::fs::read(&file).unwrap_or_default();
        // A pattern is matched against whole path components, so a leading
        // slash anchors it at the repository root and stops `src/a.rs` from
        // also ignoring `vendor/src/a.rs`.
        let mut line = vec![b'/'];
        line.extend_from_slice(path);
        if existing
            .split(|b| *b == b'\n')
            .any(|l| l.strip_suffix(b"\r").unwrap_or(l) == line.as_slice())
        {
            return;
        }
        let mut out = existing;
        if !out.is_empty() && !out.ends_with(b"\n") {
            out.push(b'\n');
        }
        out.extend_from_slice(&line);
        out.push(b'\n');
        match std::fs::write(&file, out) {
            Ok(()) => self.invalidate(),
            Err(e) => self.set_error(format!(".gitignore: {e}")),
        }
    }

    fn commit_command(
        &mut self,
        cmd: &str,
        oid: String,
        _error: &mut String,
    ) -> Option<FfonElement> {
        match cmd {
            CMD_CHECKOUT_COMMIT => {
                self.run(Self::os(&["checkout", "-q", &oid]));
            }
            CMD_BRANCH_HERE => {
                self.pending = Some(Pending::Text(TextOp::BranchAt(oid)));
                return Some(FfonElement::new_str("<input></input>"));
            }
            CMD_REVERT => {
                self.run(Self::os(&["revert", "--no-edit", &oid]));
            }
            CMD_CHERRY_PICK => {
                self.run(Self::os(&["cherry-pick", &oid]));
            }
            _ => {}
        }
        None
    }

    fn branch_command(
        &mut self,
        cmd: &str,
        name: String,
        error: &mut String,
    ) -> Option<FfonElement> {
        match cmd {
            CMD_CHECKOUT_BRANCH => {
                // git refuses a branch already checked out in another
                // worktree, with a message about the other path. Saying so
                // first is more use than passing that through.
                if let Some(other) = self
                    .branch_cache()
                    .and_then(|bs| bs.iter().find(|b| b.name == name))
                    .and_then(|b| b.checked_out_in.clone())
                {
                    let mut a = localize::Args::new();
                    a.set("path", other);
                    *error = localize::t_args("gitclient-branch-elsewhere", &a);
                    return None;
                }
                self.run(Self::os(&["checkout", "-q", &name]));
            }
            CMD_DELETE_BRANCH => {
                self.pending = Some(Pending::Confirm {
                    command: cmd.to_owned(),
                    target: Segment::Branch(name),
                });
            }
            CMD_MERGE => {
                self.run(Self::os(&["merge", "--no-edit", &name]));
            }
            CMD_REBASE => {
                self.run(Self::os(&["rebase", &name]));
            }
            _ => {}
        }
        None
    }

    fn stash_command(
        &mut self,
        cmd: &str,
        index: usize,
        _error: &mut String,
    ) -> Option<FfonElement> {
        let name = format!("stash@{{{index}}}");
        match cmd {
            CMD_APPLY_STASH => {
                self.run(Self::os(&["stash", "apply", "-q", &name]));
            }
            CMD_POP_STASH => {
                if self.run(Self::os(&["stash", "pop", "-q", &name])) {
                    self.record("stash-pop", &[], cmd);
                }
            }
            CMD_DROP_STASH => {
                self.pending = Some(Pending::Confirm {
                    command: cmd.to_owned(),
                    target: Segment::Stash(index),
                });
            }
            _ => {}
        }
        None
    }

    fn remote_command(
        &mut self,
        cmd: &str,
        name: String,
        _error: &mut String,
    ) -> Option<FfonElement> {
        match cmd {
            CMD_REMOVE_REMOTE => {
                self.pending = Some(Pending::Confirm {
                    command: cmd.to_owned(),
                    target: Segment::Remote(name),
                });
            }
            CMD_RENAME_REMOTE => {
                self.pending = Some(Pending::Text(TextOp::RenameRemote(name)));
                return Some(FfonElement::new_str("<input></input>"));
            }
            _ => {}
        }
        None
    }

    /// The second half of a confirmed destructive command.
    fn perform_confirmed(&mut self, command: &str, target: &Segment) {
        match (command, target) {
            (CMD_DISCARD, Segment::File(group, path)) => self.discard(*group, path),
            (CMD_DELETE_BRANCH, Segment::Branch(name)) => {
                // `-d`, never `-D`: git refusing to delete unmerged work is a
                // safety net, and turning it off on the user's behalf throws
                // away commits that exist nowhere else.
                self.run(Self::os(&["branch", "-d", name]));
            }
            (CMD_DROP_STASH, Segment::Stash(index)) => {
                self.run(Self::os(&[
                    "stash",
                    "drop",
                    "-q",
                    &format!("stash@{{{index}}}"),
                ]));
            }
            (CMD_REMOVE_REMOTE, Segment::Remote(name)) => {
                self.run(Self::os(&["remote", "remove", name]));
            }
            _ => {}
        }
    }

    /// Throw away one file's uncommitted changes.
    fn discard(&mut self, group: Group, path: &[u8]) {
        let untracked = self
            .status()
            .and_then(|s| s.entries.iter().find(|e| e.path == path))
            .is_some_and(|e| e.kind == status::EntryKind::Untracked);
        if untracked {
            // git has never seen the file, so there is nothing to restore it
            // to: discarding it means deleting it.
            let Some(root) = self.repo.as_ref().map(|r| r.root.clone()) else {
                return;
            };
            let full = root.join(PathBuf::from(git::os_string_from_bytes(path)));
            match std::fs::remove_file(&full) {
                Ok(()) => self.invalidate(),
                Err(e) => self.set_error(format!("{}: {e}", String::from_utf8_lossy(path))),
            }
            return;
        }
        let spec = git::literal_pathspec(path);
        let args = match group {
            // Discarding a staged change means putting the index back to HEAD
            // as well as the worktree, or the change stays staged.
            Group::Staged => vec![
                "checkout".into(),
                "-q".into(),
                "HEAD".into(),
                "--".into(),
                spec,
            ],
            Group::Unstaged | Group::Conflict => {
                vec!["checkout".into(), "-q".into(), "--".into(), spec]
            }
        };
        self.run(args);
    }

    /// The second half of a command that needed typed text.
    fn perform_text(&mut self, op: TextOp, text: &str) {
        match op {
            TextOp::CreateBranch => {
                if text.is_empty() {
                    return;
                }
                if self.run(Self::os(&["checkout", "-q", "-b", text])) {
                    self.record("create-branch", &[text.to_owned()], CMD_CREATE_BRANCH);
                }
            }
            TextOp::BranchAt(oid) => {
                if text.is_empty() {
                    return;
                }
                if self.run(Self::os(&["branch", text, &oid])) {
                    self.record("create-branch", &[text.to_owned()], CMD_BRANCH_HERE);
                }
            }
            TextOp::StashMessage => {
                let mut args = Self::os(&["stash", "push", "-q"]);
                if !text.is_empty() {
                    args.push("-m".into());
                    args.push(text.into());
                }
                if self.run(args) {
                    self.record("stash-push", &[], CMD_STASH);
                }
            }
            TextOp::AddRemote => {
                // One field for two values, because the app inserts one row to
                // type into. The split is on the first run of whitespace, so a
                // url containing spaces (there is no such thing) is not a case
                // to worry about, and a missing half is reported rather than
                // guessed at.
                let mut parts = text.split_whitespace();
                match (parts.next(), parts.next()) {
                    (Some(name), Some(url)) => {
                        self.run(Self::os(&["remote", "add", name, url]));
                    }
                    _ => self.set_error(localize::t("gitclient-error-remote-syntax")),
                }
            }
            TextOp::RenameRemote(old) => {
                if text.is_empty() {
                    return;
                }
                self.run(Self::os(&["remote", "rename", &old, text]));
            }
        }
    }

    // ---- Commit ----------------------------------------------------------

    /// The four commit buttons.
    fn commit(&mut self, mode: CommitMode) {
        let message = self.message.trim().to_owned();
        if message.is_empty() && mode != CommitMode::Amend {
            self.set_error(localize::t("gitclient-error-no-message"));
            return;
        }
        if self.status().is_some_and(|s| s.has_conflicts()) {
            self.set_error(localize::t("gitclient-error-conflicts"));
            return;
        }
        // The commit this one replaces or follows, so undo can put HEAD back
        // exactly rather than assuming there was a parent.
        let before = self
            .status()
            .and_then(|s| s.branch.oid.clone())
            .unwrap_or_default();

        let mut args = Self::os(&["commit", "-q"]);
        if mode == CommitMode::Amend {
            if before.is_empty() {
                self.set_error(localize::t("gitclient-error-nothing-to-amend"));
                return;
            }
            args.push("--amend".into());
            if message.is_empty() {
                args.push("--no-edit".into());
            }
        }
        if !message.is_empty() {
            args.push("-m".into());
            args.push(message.clone().into());
        }
        // Nothing staged means the user meant the changes they can see, which
        // is what VS Code's commit button does too. Conflicts are already ruled
        // out above, so this cannot commit a half-resolved merge.
        //
        // `git add -A` rather than `commit -a`: the latter covers only files
        // git already tracks, so the very first commit of a new repository,
        // where every file is untracked, would find nothing to commit and
        // refuse. What the changes list shows is what gets committed.
        let nothing_staged = self.status().is_some_and(|s| s.staged().count() == 0);
        if nothing_staged && mode != CommitMode::Amend && !self.run(Self::os(&["add", "-A"])) {
            return;
        }

        if !self.run(args) {
            return;
        }
        self.message.clear();
        if mode == CommitMode::Amend {
            // An amend rewrites history, so there is no clean reversal to
            // offer: the commit it replaced is unreachable by name.
            self.invalidate();
        } else {
            self.record("commit", &[before, message], "commit");
        }

        let steps: Vec<Vec<String>> = match mode {
            CommitMode::Push => vec![vec!["push".into()]],
            CommitMode::Sync => vec![vec!["pull".into(), "--rebase".into()], vec!["push".into()]],
            _ => Vec::new(),
        };
        if !steps.is_empty() {
            let mut error = String::new();
            let label = if mode == CommitMode::Sync {
                CMD_PULL
            } else {
                CMD_PUSH
            };
            self.start_network(label, steps, &mut error);
            if !error.is_empty() {
                self.set_error(error);
            }
        }
    }

    /// Whether it is time to fetch on our own.
    fn autofetch_due(&self) -> bool {
        if self.autofetch_minutes == 0 || self.repo.is_none() || self.network.busy() {
            return false;
        }
        let interval = std::time::Duration::from_secs(self.autofetch_minutes * 60);
        match self.last_autofetch {
            // The first one waits a full interval too, so opening a repository
            // is not itself a reason to contact a remote.
            None => true,
            Some(at) => at.elapsed() >= interval,
        }
    }

    fn start_network(&mut self, label: &str, steps: Vec<Vec<String>>, error: &mut String) {
        if self.repo.is_none() {
            return;
        }
        if !self.network.start(self.git(), label.to_owned(), steps) {
            *error = localize::t("gitclient-error-busy");
        }
    }

    // ---- Undo ------------------------------------------------------------

    /// Remember an action so ctrl-Z can reverse it.
    ///
    /// Only the actions that have an exact reversal are recorded. Push, pull,
    /// fetch, discard, revert, cherry-pick, merge, rebase and amend are not:
    /// see the irreversibility notes in docs/undo-redo-timeline.md.
    fn record(&mut self, command: &str, parts: &[String], label_command: &str) {
        use sicompass_sdk::timeline::TimelineEntry;
        let mut payload = FfonElement::new_obj(command);
        if let Some(obj) = payload.as_obj_mut() {
            for part in parts {
                obj.push(FfonElement::new_str(part.clone()));
            }
        }
        self.timeline.push(TimelineEntry::ProviderOp {
            // Patched by the app to the real provider index; a provider emits
            // with zero and an empty id.
            provider_idx: 0,
            command: command.to_owned(),
            payload,
            label: label_command.to_owned(),
        });
    }

    /// Apply a recorded action backwards (`undo`) or forwards again (`redo`).
    fn reverse(
        &mut self,
        entry: &sicompass_sdk::timeline::TimelineEntry,
        undo: bool,
        error: &mut String,
    ) {
        use sicompass_sdk::timeline::TimelineEntry;
        let TimelineEntry::ProviderOp {
            command, payload, ..
        } = entry
        else {
            return;
        };
        let parts: Vec<String> = payload
            .as_obj()
            .map(|o| {
                o.children
                    .iter()
                    .filter_map(|c| c.as_str().map(|s| s.to_owned()))
                    .collect()
            })
            .unwrap_or_default();

        let ok = match (command.as_str(), undo) {
            ("stage", true) | ("unstage", false) => self.reverse_stage(&parts, false),
            ("stage", false) | ("unstage", true) => self.reverse_stage(&parts, true),
            ("stage-all", true) | ("unstage-all", false) => {
                let args = self.unstage_all_args();
                self.run(args)
            }
            ("stage-all", false) | ("unstage-all", true) => self.run(Self::os(&["add", "-A"])),
            ("stash-push", true) | ("stash-pop", false) => {
                self.run(Self::os(&["stash", "pop", "-q"]))
            }
            ("stash-push", false) | ("stash-pop", true) => {
                self.run(Self::os(&["stash", "push", "-q"]))
            }
            ("create-branch", true) => {
                let Some(name) = parts.first() else { return };
                // The branch was just created, so it holds nothing that is not
                // also somewhere else and `-d` will accept it.
                self.run(Self::os(&["branch", "-d", name]))
            }
            ("create-branch", false) => {
                let Some(name) = parts.first() else { return };
                self.run(Self::os(&["branch", name]))
            }
            ("commit", true) => {
                let before = parts.first().cloned().unwrap_or_default();
                if before.is_empty() {
                    // Undoing the very first commit: there is no parent to
                    // reset to, so HEAD goes back to being unborn.
                    self.run(Self::os(&["update-ref", "-d", "HEAD"]))
                } else {
                    // `--soft`, so the staged tree and therefore the work is
                    // exactly as it was the moment before the commit.
                    self.run(Self::os(&["reset", "--soft", &before]))
                }
            }
            ("commit", false) => {
                let message = parts.get(1).cloned().unwrap_or_default();
                self.run(Self::os(&["commit", "-q", "-m", &message]))
            }
            _ => return,
        };
        if !ok {
            // `run` has already put the real reason on the status line; this
            // is what the timeline itself reports.
            *error = localize::t("gitclient-error-undo");
        }
    }

    fn reverse_stage(&mut self, parts: &[String], stage: bool) -> bool {
        let Some(path) = parts.first() else {
            return false;
        };
        let spec = git::literal_pathspec(path.as_bytes());
        if stage {
            self.run(vec!["add".into(), "--".into(), spec])
        } else {
            let args = if self.status().is_some_and(|s| s.branch.unborn()) {
                vec![
                    "rm".into(),
                    "--cached".into(),
                    "-q".into(),
                    "--".into(),
                    spec,
                ]
            } else {
                vec![
                    "reset".into(),
                    "-q".into(),
                    "HEAD".into(),
                    "--".into(),
                    spec,
                ]
            };
            self.run(args)
        }
    }
}

// ---------------------------------------------------------------------------
// Provider impl
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
impl Provider for GitClientProvider {
    fn name(&self) -> &str {
        "gitclient"
    }

    fn display_name(&self) -> String {
        register_translations();
        localize::t("gitclient-display-name")
    }

    fn version(&self) -> Option<&str> {
        Some(env!("CARGO_PKG_VERSION"))
    }

    fn fetch(&mut self) -> Vec<FfonElement> {
        register_translations();
        match self.view {
            View::Browse => self.browse_children(),
            View::Repo => self.repo_children(),
        }
    }

    // ---- Path ------------------------------------------------------------
    //
    // In the browse view the path is a real filesystem path, so the app's
    // generic navigation does all the work. In the repository it is a synthetic
    // `/`-joined list of stable tokens. `path_is_filesystem()` is true for both,
    // because what it actually selects is how the app refreshes after a command
    // (see the impl below), and the synthetic form is only ever round-tripped.

    fn push_path(&mut self, segment: &str) {
        match self.view {
            View::Browse => {
                self.browse_path
                    .push(segment.trim_end_matches('/').trim_end_matches('\\'));
            }
            View::Repo => {
                // The label the level was rendered with, first: that is what
                // the app passes when the user presses Right.
                let resolved = self
                    .labels
                    .get(&self.segments)
                    .and_then(|m| m.get(segment))
                    .cloned()
                    // A stable token, second: that is what the app passes when
                    // it rebuilds a path from `current_path()` after a restore.
                    .or_else(|| Segment::from_token(segment));
                // Neither resolves for a stale label from before a language
                // change, or a level that no longer exists. Staying put leaves
                // the cursor at a level that does exist, which is recoverable;
                // pushing a guess is not.
                if let Some(s) = resolved {
                    self.segments.push(s);
                }
            }
        }
        self.sync_rendered_path();
    }

    fn pop_path(&mut self) {
        match self.view {
            View::Browse => {
                if self.browse_path.parent().is_some() && self.browse_path != Path::new("/") {
                    self.browse_path.pop();
                }
            }
            View::Repo => {
                self.segments.pop();
            }
        }
        self.sync_rendered_path();
    }

    fn current_path(&self) -> &str {
        // The signature hands back a borrow, so the synthetic form is rendered
        // into a field rather than returned by value.
        &self.rendered_path
    }

    fn set_current_path(&mut self, path: &str) {
        // A path carrying the marker names a repository, whichever view we are
        // in. That is how a restart comes back where it left off: the app
        // writes `current_path()` out when the tab closes and hands it to a
        // freshly built provider, which starts in the folder view.
        if let Some((anchor, rest)) = split_repo_path(path) {
            let already_open = self
                .repo_anchor()
                .is_some_and(|a| a.to_string_lossy() == anchor);
            if !already_open {
                self.browse_path = PathBuf::from(anchor);
                if !self.open_repository() {
                    // The repository is gone. Browsing where it was is the
                    // nearest real place, and `browse_children` walks up from
                    // there if even that has been deleted.
                    self.view = View::Browse;
                    self.sync_rendered_path();
                    return;
                }
            }
            self.segments = parse_tokens(rest);
            self.sync_rendered_path();
            return;
        }

        match self.view {
            View::Browse => self.browse_path = PathBuf::from(path),
            // No marker while a repository is open: the app rebuilding a tree
            // from the filesystem root. Following it out of the repository
            // would silently change which one is open.
            View::Repo => self.segments.clear(),
        }
        self.sync_rendered_path();
    }

    fn at_root(&self) -> bool {
        match self.view {
            View::Browse => self.browse_path == Path::new("/"),
            View::Repo => self.segments.is_empty(),
        }
    }

    /// True, and it matters.
    ///
    /// After a colon command returns no element and no error, the app either
    /// re-fetches every level along the visible path and keeps the cursor
    /// (`path_is_filesystem() == true`) or unwinds to the provider root
    /// (`false`). Staging a file has to leave the cursor on that file: someone
    /// staging a list of files one at a time cannot be thrown back to the
    /// section list after every one.
    fn path_is_filesystem(&self) -> bool {
        true
    }

    /// Keep Ctrl+F out of this provider.
    ///
    /// `Some(_)` suppresses the app's generic FFON-tree traversal. That
    /// traversal teleports the cursor without telling the provider, and for a
    /// filesystem-path provider the app skips the resync that would otherwise
    /// repair it, so the path and the cursor would disagree from then on.
    fn collect_extended_search_items(&self) -> Option<Vec<sicompass_sdk::SearchResultItem>> {
        Some(Vec::new())
    }

    fn take_error(&mut self) -> Option<String> {
        self.error.take()
    }

    // ---- Commands --------------------------------------------------------

    fn commands(&self) -> Vec<String> {
        // Called on every keypress by the app's shortcut table, so this reads
        // cached state only: no subprocess, no filesystem access.
        if self.view == View::Browse {
            // Always the one command, so `:` always does something and says
            // why when it cannot. Offering nothing instead would put `:` into
            // an empty command mode the user has to press Escape to leave,
            // which is the common case while looking for a repository.
            return vec![CMD_OPEN.to_owned()];
        }

        let mut cmds = vec![CMD_REFRESH.to_owned()];
        let bare = self.repo.as_ref().is_some_and(|r| r.bare);
        cmds.extend([
            CMD_FETCH.to_owned(),
            CMD_PULL.to_owned(),
            CMD_PUSH.to_owned(),
        ]);

        match self.segments.first() {
            Some(Segment::Section(Section::Changes)) if !bare => {
                cmds.extend([CMD_STAGE_ALL.to_owned(), CMD_UNSTAGE_ALL.to_owned()]);
                // The per-file commands only mean anything with a file under
                // the cursor, which is exactly the level below `changes`.
                if self.segments.len() == 1 {
                    cmds.extend([
                        CMD_STAGE.to_owned(),
                        CMD_UNSTAGE.to_owned(),
                        CMD_DISCARD.to_owned(),
                        CMD_IGNORE.to_owned(),
                    ]);
                }
                cmds.push(CMD_STASH.to_owned());
            }
            Some(Segment::Section(Section::Graph)) => {
                if self.segments.len() == 1 {
                    cmds.extend([
                        CMD_CHECKOUT_COMMIT.to_owned(),
                        CMD_BRANCH_HERE.to_owned(),
                        CMD_REVERT.to_owned(),
                        CMD_CHERRY_PICK.to_owned(),
                        CMD_LOAD_MORE.to_owned(),
                    ]);
                }
                cmds.push(
                    if self.graph_all {
                        CMD_THIS_BRANCH
                    } else {
                        CMD_ALL_BRANCHES
                    }
                    .to_owned(),
                );
            }
            Some(Segment::Section(Section::Branches)) => {
                cmds.push(CMD_CREATE_BRANCH.to_owned());
                // One level below a scope is the branch list, which is where a
                // branch is under the cursor.
                if self.segments.len() == 2 {
                    cmds.extend([
                        CMD_CHECKOUT_BRANCH.to_owned(),
                        CMD_DELETE_BRANCH.to_owned(),
                        CMD_MERGE.to_owned(),
                        CMD_REBASE.to_owned(),
                    ]);
                }
            }
            Some(Segment::Section(Section::Stashes)) => {
                if !bare {
                    cmds.push(CMD_STASH.to_owned());
                }
                if self.segments.len() == 1 {
                    cmds.extend([
                        CMD_APPLY_STASH.to_owned(),
                        CMD_POP_STASH.to_owned(),
                        CMD_DROP_STASH.to_owned(),
                    ]);
                }
            }
            Some(Segment::Section(Section::Remotes)) => {
                cmds.push(CMD_ADD_REMOTE.to_owned());
                if self.segments.len() == 1 {
                    cmds.extend([CMD_REMOVE_REMOTE.to_owned(), CMD_RENAME_REMOTE.to_owned()]);
                }
            }
            _ => {}
        }
        cmds
    }

    // `command_label` is deliberately not overridden. The app renders the raw
    // ids from `commands()` and never calls it (src/sicompass/src/list.rs), so
    // an override here would be a translation surface no user ever reads, and
    // the trait default already returns the id. That is also why the ids
    // themselves are readable English rather than symbols.

    fn handle_command(
        &mut self,
        cmd: &str,
        element_key: &str,
        _element_type: i32,
        error: &mut String,
    ) -> Option<FfonElement> {
        register_translations();
        // A command starting is the end of whatever the previous one was
        // waiting for, so a half-answered confirmation cannot leak into it.
        let pending = self.pending.take();
        self.run_command(cmd, element_key, pending, error)
    }

    fn command_list_items(&self, cmd: &str) -> Vec<ListItem> {
        register_translations();
        let Some(Pending::Confirm { command, target }) = &self.pending else {
            return Vec::new();
        };
        if command != cmd {
            return Vec::new();
        }
        let mut a = localize::Args::new();
        a.set("what", self.target_description(target));
        // Cancel first, so the cursor lands on it. The app opens a list at row
        // zero, and for a command that destroys work the safe answer is the
        // one that should be under the cursor when it does.
        vec![
            ListItem {
                label: localize::t("gitclient-confirm-cancel"),
                data: CONFIRM_NO.to_owned(),
            },
            ListItem {
                label: localize::t_args("gitclient-confirm-yes", &a),
                data: CONFIRM_YES.to_owned(),
            },
        ]
    }

    fn execute_command(&mut self, cmd: &str, selection: &str) -> bool {
        register_translations();
        let Some(Pending::Confirm { command, target }) = self.pending.take() else {
            return false;
        };
        if command != cmd || selection != CONFIRM_YES {
            // Cancelling is a successful outcome: the app only drains timeline
            // entries when this returns true, and there are none to drain.
            return true;
        }
        self.perform_confirmed(&command, &target);
        true
    }

    // ---- Buttons, editing, background work -------------------------------

    fn on_button_press(&mut self, function_name: &str) {
        register_translations();
        match function_name {
            BTN_LOAD_MORE => {
                self.graph_limit += GRAPH_PAGE;
                self.commits = None;
            }
            "commit" => self.commit(CommitMode::Normal),
            "commit-amend" => self.commit(CommitMode::Amend),
            "commit-push" => self.commit(CommitMode::Push),
            "commit-sync" => self.commit(CommitMode::Sync),
            _ => {}
        }
    }

    /// Text typed into an `<input>` row.
    ///
    /// Two sources reach this: the commit message row, which is always present
    /// in `changes`, and the one-off row a text command inserts. The pending
    /// command decides which, and it is cleared either way so a later edit of
    /// the message cannot be read as an answer to an old question.
    fn commit_edit(&mut self, _old: &str, new: &str) -> bool {
        register_translations();
        match self.pending.take() {
            Some(Pending::Text(op)) => {
                self.perform_text(op, new.trim());
                true
            }
            other => {
                // Not ours to consume: put a confirmation back so the pick
                // list that is already open still resolves.
                self.pending = other;
                if self.view != View::Repo {
                    // There is no message row in the folder listing, so an edit
                    // arriving here is not one, and taking it as the commit
                    // message would set it from somewhere the user cannot see.
                    return false;
                }
                self.message = new.to_owned();
                true
            }
        }
    }

    fn tick(&mut self) -> bool {
        let mut redraw = false;

        if let Some(outcome) = self.network.take_outcome() {
            match outcome.error {
                Some(message) => self.set_error(message),
                None => {
                    let mut a = localize::Args::new();
                    a.set("what", outcome.label);
                    self.set_error(localize::t_args("gitclient-done", &a));
                }
            }
            self.invalidate();
            self.needs_refresh = true;
            redraw = true;
        }

        if self.autofetch_due() {
            self.last_autofetch = Some(std::time::Instant::now());
            let mut error = String::new();
            self.start_network(
                CMD_FETCH,
                vec![vec!["fetch".into(), "--all".into()]],
                &mut error,
            );
            // A busy worker is not worth reporting for a fetch nobody asked
            // for: the interval simply starts again.
        }

        if self.watcher.as_ref().is_some_and(|w| w.take_changed()) {
            self.invalidate();
            if self.shallow_enough_to_refresh_silently() {
                self.needs_refresh = true;
            } else {
                // Rewriting the level someone is reading a diff in is worse
                // than being one keystroke out of date, so this only says so.
                self.stale = true;
            }
            redraw = true;
        }
        redraw
    }

    /// True while a remote-contacting command is running.
    ///
    /// Only gates the Ctrl+W close confirmation, which is exactly right: a
    /// half-finished push is worth being asked about.
    fn is_busy(&self) -> bool {
        self.network.busy()
    }

    fn needs_refresh(&self) -> bool {
        self.needs_refresh
    }

    fn clear_needs_refresh(&mut self) {
        self.needs_refresh = false;
    }

    fn take_timeline_entries(&mut self) -> Vec<sicompass_sdk::timeline::TimelineEntry> {
        std::mem::take(&mut self.timeline)
    }

    async fn undo(&mut self, entry: &sicompass_sdk::timeline::TimelineEntry, error: &mut String) {
        register_translations();
        self.reverse(entry, true, error);
    }

    async fn redo(&mut self, entry: &sicompass_sdk::timeline::TimelineEntry, error: &mut String) {
        register_translations();
        self.reverse(entry, false, error);
    }

    fn on_setting_change(&mut self, key: &str, value: &str) {
        match key {
            "gitBinary" => {
                self.binary = if value.trim().is_empty() {
                    "git".to_owned()
                } else {
                    value.trim().to_owned()
                };
                self.invalidate();
            }
            "gitAutofetchMinutes" => {
                // Anything unparseable is off rather than a guess: a typo in a
                // settings field must not start contacting a remote on a timer.
                self.autofetch_minutes = value.trim().parse().unwrap_or(0);
                self.last_autofetch = None;
            }
            _ => {}
        }
    }

    fn cleanup(&mut self) {
        // Stops the watcher thread. The network job is detached and clears its
        // own flag, so there is nothing to join.
        self.watcher = None;
    }
}

// ---------------------------------------------------------------------------
// Rendering helpers
// ---------------------------------------------------------------------------

/// The four commit actions, as `(button function name, message key)`.
///
/// The function name is the stable identifier `on_button_press` receives; the
/// key resolves to what the user reads. Buttons must be `Str`s, not `Obj`s: the
/// app refuses to fire a button that is not a plain string.
const COMMIT_BUTTONS: &[(&str, &str)] = &[
    ("commit", "gitclient-button-commit"),
    ("commit-amend", "gitclient-button-amend"),
    ("commit-push", "gitclient-button-push"),
    ("commit-sync", "gitclient-button-sync"),
];

/// How many commits one page of the graph holds.
const GRAPH_PAGE: usize = 100;

/// The button that widens the graph by another page.
const BTN_LOAD_MORE: &str = "load-more";

/// A diff longer than this is truncated.
///
/// Every line of a level is an `FfonElement` the app walks to build its list,
/// so a single 100k-line diff would be 100k of them. Nobody reads that by ear,
/// and the row saying how much was cut is more use than the rest of it.
const MAX_DIFF_LINES: usize = 2000;

fn section_label(section: Section) -> String {
    localize::t(match section {
        Section::Changes => "gitclient-section-changes",
        Section::Graph => "gitclient-section-graph",
        Section::Branches => "gitclient-section-branches",
        Section::Stashes => "gitclient-section-stashes",
        Section::Remotes => "gitclient-section-remotes",
    })
}

/// The label for one changed file: what happened to it, then its path.
///
/// The verb leads because that is the part that differs between two rows for
/// the same file, and a screen reader reads left to right.
fn change_label(entry: &status::StatusEntry, group: Group) -> String {
    use status::EntryKind;

    let word = match group {
        Group::Conflict => conflict_word(entry.index, entry.worktree),
        Group::Staged => code_word(entry.index),
        Group::Unstaged => {
            if entry.kind == EntryKind::Untracked {
                "gitclient-change-untracked"
            } else {
                code_word(entry.worktree)
            }
        }
    };
    let mut label = format!(
        "{} {}",
        localize::t(word),
        escape_markup(&entry.display_path())
    );
    // A rename is the one case where the old name matters as much as the new
    // one, and it is not derivable from anything else on the row.
    if let Some(orig) = &entry.orig_path {
        let mut a = localize::Args::new();
        a.set("from", escape_markup(&String::from_utf8_lossy(orig)));
        label.push_str(&format!(
            " {}",
            localize::t_args("gitclient-renamed-from", &a)
        ));
    }
    label
}

/// "3 files changed, 42 insertions, 7 deletions", in the reader's language.
///
/// Built from counted numbers rather than passing git's own `--shortstat`
/// through, which is English whatever the locale.
fn stats_line(stats: &log::Stats) -> String {
    let mut a = localize::Args::new();
    a.set("files", stats.files as i64);
    a.set("ins", stats.insertions as i64);
    a.set("del", stats.deletions as i64);
    localize::t_args("gitclient-stats", &a)
}

/// A file inside a commit: what happened to it, then its path, the same shape
/// the changes list uses so the two read alike.
fn commit_file_label(file: &log::CommitFile) -> String {
    let mut label = format!(
        "{} {}",
        localize::t(code_word(file.status)),
        escape_markup(&file.display_path())
    );
    if let Some(orig) = &file.orig_path {
        let mut a = localize::Args::new();
        a.set("from", escape_markup(&String::from_utf8_lossy(orig)));
        label.push(' ');
        label.push_str(&localize::t_args("gitclient-renamed-from", &a));
    }
    label
}

fn code_word(code: u8) -> &'static str {
    match code {
        b'M' => "gitclient-change-modified",
        b'A' => "gitclient-change-added",
        b'D' => "gitclient-change-deleted",
        b'R' => "gitclient-change-renamed",
        b'C' => "gitclient-change-copied",
        b'T' => "gitclient-change-typechanged",
        _ => "gitclient-change-changed",
    }
}

/// Conflicts are named by *which side* did what, because that is the question
/// the user has to answer to resolve one.
fn conflict_word(index: u8, worktree: u8) -> &'static str {
    match (index, worktree) {
        (b'D', b'D') => "gitclient-conflict-both-deleted",
        (b'A', b'A') => "gitclient-conflict-both-added",
        (b'U', b'U') => "gitclient-conflict-both-modified",
        (b'A', b'U') => "gitclient-conflict-added-by-us",
        (b'U', b'A') => "gitclient-conflict-added-by-them",
        (b'D', b'U') => "gitclient-conflict-deleted-by-us",
        (b'U', b'D') => "gitclient-conflict-deleted-by-them",
        _ => "gitclient-conflict",
    }
}

/// Turn diff output into one row per line, escaped and capped.
fn diff_rows(bytes: &[u8]) -> Vec<FfonElement> {
    if is_binary(bytes) {
        return vec![FfonElement::new_str(localize::t("gitclient-binary"))];
    }
    let text = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = text
        .split('\n')
        .map(|l| l.strip_suffix('\r').unwrap_or(l))
        .collect();
    // `split` on a trailing newline leaves an empty final element that is not a
    // line of the file.
    let lines = match lines.split_last() {
        Some((&"", rest)) => rest,
        _ => &lines[..],
    };

    let mut out: Vec<FfonElement> = lines
        .iter()
        .take(MAX_DIFF_LINES)
        .map(|l| FfonElement::new_str(escape_markup(l)))
        .collect();
    if lines.len() > MAX_DIFF_LINES {
        let mut a = localize::Args::new();
        a.set("n", (lines.len() - MAX_DIFF_LINES) as i64);
        out.push(FfonElement::new_str(localize::t_args(
            "gitclient-diff-truncated",
            &a,
        )));
    }
    out
}

/// git's own heuristic: a NUL byte near the start means binary.
fn is_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8000).any(|b| *b == 0)
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub fn register() {
    register_translations();
    register_provider_factory("gitclient", || Box::new(GitClientProvider::new()));
    register_builtin_manifest(
        // Opt-in: not every user works with git, and the provider is useless
        // without the binary. `BuiltinManifest::new` already defaults to
        // opt-in, so there is no builder call to make that so.
        //
        // The section name is the display name with its space removed matched
        // against `name()`, which is why the provider is `gitclient` and not
        // `git`.
        BuiltinManifest::new("gitclient", "git client").with_settings(vec![
            SettingDecl::text("git client", "git binary path", "gitBinary", "git"),
            SettingDecl::text(
                "git client",
                "fetch from the remote every N minutes, 0 to never",
                "gitAutofetchMinutes",
                "0",
            ),
        ]),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo::fixture::Fixture;
    use sicompass_sdk::tags;

    /// A provider parked on a folder, as if the user had browsed to it.
    fn browsing(path: &Path) -> GitClientProvider {
        let mut p = GitClientProvider::new();
        p.browse_path = path.to_path_buf();
        p
    }

    /// A provider with the repository open, reached the way the user reaches
    /// it: browse the folder, then run the one command the browse view offers.
    fn opened(f: &Fixture) -> GitClientProvider {
        let mut p = browsing(&f.path());
        p.fetch();
        let mut error = String::new();
        p.handle_command(CMD_OPEN, "", 0, &mut error);
        assert!(error.is_empty(), "{error}");
        assert_eq!(p.view, View::Repo);
        p
    }

    /// What the screen reader would read: the display text of each row, with
    /// every tag resolved away.
    fn labels(elems: &[FfonElement]) -> Vec<String> {
        elems
            .iter()
            .map(|e| match e {
                FfonElement::Str(s) => tags::strip_display(s),
                FfonElement::Obj(o) => tags::strip_display(&o.key),
            })
            .collect()
    }

    /// Walk into the child with this label and return the level below it.
    fn enter(p: &mut GitClientProvider, label: &str) -> Vec<FfonElement> {
        p.push_path(label);
        p.fetch()
    }

    // ---- Path model ------------------------------------------------------

    #[test]
    fn every_segment_round_trips_through_its_token() {
        let cases = vec![
            Segment::Section(Section::Changes),
            Segment::Section(Section::Remotes),
            Segment::File(Group::Staged, b"src/lib.rs".to_vec()),
            Segment::File(Group::Unstaged, b"src/lib.rs".to_vec()),
            Segment::File(Group::Conflict, b"a b.txt".to_vec()),
            Segment::Path(b"deep/nested/file.rs".to_vec()),
            Segment::Commit("abc123".to_owned()),
            Segment::Scope(Scope::Local),
            Segment::Branch("feature/thing".to_owned()),
            Segment::Stash(3),
            Segment::Remote("origin".to_owned()),
        ];
        for seg in cases {
            let token = seg.token();
            assert_eq!(
                Segment::from_token(&token),
                Some(seg.clone()),
                "token {token:?} did not round-trip"
            );
        }
    }

    #[test]
    fn a_file_in_two_groups_has_two_different_tokens() {
        // A partially staged file is one path in two rows, and `stage` means
        // the opposite thing on each, so the group is part of its identity.
        let staged = Segment::File(Group::Staged, b"a.rs".to_vec());
        let unstaged = Segment::File(Group::Unstaged, b"a.rs".to_vec());
        assert_ne!(staged.token(), unstaged.token());
    }

    #[test]
    fn a_path_containing_a_slash_is_one_segment_not_two() {
        // `current_path()` is split on `/` when it is read back, so an
        // unescaped file path would come back as two levels that do not exist.
        let seg = Segment::File(Group::Unstaged, b"src/deep/lib.rs".to_vec());
        let token = seg.token();
        assert!(!token.contains('/'), "token was {token:?}");
        assert_eq!(token.split('/').count(), 1);
        assert_eq!(Segment::from_token(&token), Some(seg));
    }

    #[test]
    fn a_path_containing_a_percent_sign_round_trips() {
        let seg = Segment::File(Group::Unstaged, b"100%25/done.txt".to_vec());
        assert_eq!(Segment::from_token(&seg.token()), Some(seg));
    }

    #[test]
    fn the_repository_path_round_trips_through_set_current_path() {
        // The app saves `current_path()` and restores it around every
        // multi-level refresh, so anything lost here loses the user's place.
        let f = Fixture::new();
        f.write("src/deep/a.rs", "one\n");
        let mut p = opened(&f);
        p.fetch();

        p.segments = vec![
            Segment::Section(Section::Changes),
            Segment::File(Group::Unstaged, b"src/deep/a.rs".to_vec()),
        ];
        p.sync_rendered_path();
        let saved = p.current_path().to_owned();

        p.set_current_path("/");
        assert!(p.at_root());
        p.set_current_path(&saved);
        assert_eq!(p.current_path(), saved);
        assert_eq!(p.segments.len(), 2);
    }

    #[test]
    fn an_unparseable_path_lands_at_the_repository_root() {
        // A path saved by an older layout, or one from the other view. Half a
        // guess is worse than a place that certainly exists.
        let f = Fixture::new();
        let mut p = opened(&f);
        p.set_current_path("/changes/not-a-real-token");
        assert!(p.at_root(), "segments: {:?}", p.segments);
    }

    #[test]
    fn push_path_resolves_the_label_the_level_was_rendered_with() {
        let f = Fixture::new();
        let mut p = opened(&f);
        p.fetch();
        p.push_path(&localize::t("gitclient-section-changes"));
        assert_eq!(p.segments, vec![Segment::Section(Section::Changes)]);
    }

    #[test]
    fn push_path_also_accepts_a_stable_token_so_a_restore_works() {
        let f = Fixture::new();
        let mut p = opened(&f);
        p.fetch();
        p.push_path("graph");
        assert_eq!(p.segments, vec![Segment::Section(Section::Graph)]);
    }

    #[test]
    fn an_unknown_label_leaves_the_cursor_where_it_is() {
        // A stale label from before a language change. Staying put is
        // recoverable, descending into a guess is not.
        let f = Fixture::new();
        let mut p = opened(&f);
        p.fetch();
        p.push_path("something that was never rendered");
        assert!(p.segments.is_empty());
    }

    #[test]
    fn the_label_map_is_per_level_so_two_levels_cannot_collide() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit("first");
        f.write("a.txt", "two\n");
        f.run(["add", "a.txt"]);
        f.write("a.txt", "three\n");

        let mut p = opened(&f);
        p.fetch();
        let changes = enter(&mut p, &localize::t("gitclient-section-changes"));
        let rows = labels(&changes);
        // The same path in both groups, told apart by the leading verb.
        let staged = rows
            .iter()
            .find(|r| {
                r.contains("a.txt") && r.starts_with(&localize::t("gitclient-change-modified"))
            })
            .cloned()
            .expect("a.txt should be listed");
        p.push_path(&staged);
        assert!(
            matches!(p.segments.last(), Some(Segment::File(_, path)) if path == b"a.txt"),
            "segments: {:?}",
            p.segments
        );
    }

    // ---- Browse view -----------------------------------------------------

    #[test]
    fn the_browse_view_lists_directories_and_nothing_else() {
        let f = Fixture::new();
        f.subdir("src");
        f.write("a.txt", "x");
        let mut p = browsing(&f.path());
        let rows = labels(&p.fetch());
        assert!(rows.contains(&"src".to_owned()), "{rows:?}");
        assert!(!rows.contains(&"a.txt".to_owned()), "files are left out");
    }

    #[test]
    fn the_git_folder_is_listed_like_any_other() {
        // It is the plainest sign that a folder is a repository, and it is
        // searchable by name, so it stands in for an indicator row.
        let f = Fixture::new();
        let mut p = browsing(&f.path());
        assert!(labels(&p.fetch()).contains(&".git".to_owned()));
    }

    #[test]
    fn walking_into_the_git_folder_is_harmless() {
        // Nothing there is a repository to open, and `open repository` asks
        // exactly the question `is_repository` used to answer differently.
        let f = Fixture::new();
        let mut p = browsing(&f.path().join(".git/hooks"));
        p.fetch();
        let mut error = String::new();
        p.handle_command(CMD_OPEN, "", 0, &mut error);
        assert_eq!(p.view, View::Browse);
        assert_eq!(error, localize::t("gitclient-error-not-a-repository"));
    }

    #[test]
    fn a_folder_with_no_subfolders_says_so_rather_than_offering_a_dead_row() {
        // An empty level gets the app's own "insert here" row seeded into it,
        // wired to `create_directory`, which this provider does not implement.
        let dir = crate::repo::fixture::plain_dir();
        let mut p = browsing(dir.path());
        assert_eq!(labels(&p.fetch()), vec![localize::t("gitclient-empty")]);
    }

    #[test]
    fn colon_on_a_folder_that_is_not_a_repository_says_so() {
        // The command is still offered, so `:` answers rather than dropping
        // the user into an empty command mode they have to escape out of. This
        // is the only place the answer is given, now that the listing carries
        // no indicator row.
        let dir = crate::repo::fixture::plain_dir();
        let mut p = browsing(dir.path());
        p.fetch();
        assert_eq!(p.commands(), vec![CMD_OPEN.to_owned()]);
        let mut error = String::new();
        p.handle_command(CMD_OPEN, "", 0, &mut error);
        assert_eq!(p.view, View::Browse);
        assert_eq!(error, localize::t("gitclient-error-not-a-repository"));
    }

    #[test]
    fn browse_rows_carry_no_input_tag() {
        // A `+i` row is what the app's live-input-slot check looks for, and
        // nothing in a folder listing is renameable.
        let f = Fixture::new();
        f.subdir("src");
        let mut p = browsing(&f.path());
        for elem in p.fetch() {
            if let FfonElement::Obj(o) = elem {
                assert!(!tags::has_input(&o.key), "{:?} looks editable", o.key);
            }
        }
    }

    #[test]
    fn the_browse_view_offers_exactly_one_command_so_colon_fires_it() {
        // The app shows a palette for two or more and fires a lone command
        // directly. Adding a second command here silently changes what `:`
        // does in the browse view.
        let f = Fixture::new();
        let mut p = browsing(&f.path());
        p.fetch();
        assert_eq!(p.commands(), vec![CMD_OPEN.to_owned()]);
    }

    #[test]
    fn commands_answers_from_state_rather_than_probing_the_disk() {
        // The app calls `commands()` while building the shortcut table, on
        // every keypress. A `git rev-parse` per keystroke is not affordable,
        // so the browse answer is a constant and the repository answer is read
        // off `self.segments`.
        let f = Fixture::new();
        let p = browsing(&f.path());
        assert_eq!(
            p.commands(),
            vec![CMD_OPEN.to_owned()],
            "answered without a fetch having run"
        );
    }

    // ---- Opening and closing ---------------------------------------------

    #[test]
    fn opening_a_repository_from_a_subfolder_roots_at_the_worktree() {
        let f = Fixture::new();
        let sub = f.subdir("src/deep");
        let mut p = browsing(&sub);
        p.fetch();
        let mut error = String::new();
        p.handle_command(CMD_OPEN, "", 0, &mut error);
        assert_eq!(p.repo.as_ref().unwrap().root, f.path());
    }

    #[test]
    fn opening_leaves_the_provider_at_its_own_root() {
        // `at_root` is what tells the app to rebuild the subtree from the root
        // rather than re-fetching the browse levels the cursor was in. The path
        // is the repository's own folder, which is what makes a restart land
        // there rather than at the filesystem root.
        let f = Fixture::new();
        let sub = f.subdir("a/b/c");
        let mut p = browsing(&sub);
        p.fetch();
        p.handle_command(CMD_OPEN, "", 0, &mut String::new());
        assert!(p.at_root());
        assert_eq!(
            p.current_path(),
            format!("{}/{REPO_MARKER}", f.path().to_str().unwrap()),
            "the repository's own folder, plus the marker that says it is one"
        );
    }

    #[test]
    fn a_restart_comes_back_inside_the_repository() {
        // The app writes `current_path()` out when a tab closes and hands it
        // back on restart, to a provider that starts in the folder view. The
        // marker is what tells that provider the string was a repository, so
        // the tab reopens where it was rather than in a folder the user then
        // has to press `:` in again.
        let f = Fixture::new();
        let mut p = opened(&f);
        p.fetch();
        let saved = p.current_path().to_owned();

        let mut restored = GitClientProvider::new();
        restored.set_current_path(&saved);
        assert_eq!(restored.view, View::Repo);
        assert_eq!(restored.repo.as_ref().unwrap().root, f.path());
        let rows = labels(&restored.fetch());
        assert!(
            rows.contains(&localize::t("gitclient-section-changes")),
            "{rows:?}"
        );
    }

    #[test]
    fn a_restart_from_inside_the_repository_comes_back_to_the_same_level() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        let mut p = opened(&f);
        p.segments = vec![
            Segment::Section(Section::Changes),
            Segment::File(Group::Unstaged, b"a.txt".to_vec()),
        ];
        p.sync_rendered_path();
        let saved = p.current_path().to_owned();

        let mut restored = GitClientProvider::new();
        restored.set_current_path(&saved);
        assert_eq!(restored.view, View::Repo);
        assert_eq!(restored.segments, p.segments, "back in the same diff");
        assert!(labels(&restored.fetch()).iter().any(|r| r == "+one"));
    }

    #[test]
    fn a_restart_after_the_repository_is_gone_falls_back_to_browsing() {
        // Deleted, moved, or on a drive that is not mounted yet. Browsing
        // where it was is the nearest real place.
        let f = Fixture::new();
        let p = opened(&f);
        let saved = p.current_path().to_owned();
        let gone = saved.replace(
            f.path().to_str().unwrap(),
            &f.dir.path().join("not-there").to_string_lossy(),
        );

        let mut restored = GitClientProvider::new();
        restored.set_current_path(&gone);
        assert_eq!(restored.view, View::Browse);
        restored.fetch();
        assert!(
            restored.browse_path.is_dir(),
            "walked up to somewhere that exists: {:?}",
            restored.browse_path
        );
    }

    #[test]
    fn a_path_outside_the_open_repository_resets_to_its_root() {
        // `rebuild_path_from_root` hands back the filesystem root it derived
        // from the path. Following it would silently change which repository
        // is open.
        let f = Fixture::new();
        let mut p = opened(&f);
        p.segments = vec![Segment::Section(Section::Graph)];
        p.sync_rendered_path();
        p.set_current_path("/");
        assert_eq!(p.view, View::Repo);
        assert!(p.at_root());
        assert_eq!(p.repo.as_ref().unwrap().root, f.path());
    }

    #[test]
    fn opening_a_plain_folder_reports_why_it_did_not() {
        let dir = crate::repo::fixture::plain_dir();
        let mut p = browsing(dir.path());
        p.fetch();
        let mut error = String::new();
        p.handle_command(CMD_OPEN, "", 0, &mut error);
        assert_eq!(p.view, View::Browse);
        assert_eq!(error, localize::t("gitclient-error-not-a-repository"));
    }

    #[test]
    fn closing_a_repository_returns_to_its_own_folder() {
        let f = Fixture::new();
        let mut p = opened(&f);
        p.handle_command(CMD_CLOSE, "", 0, &mut String::new());
        assert_eq!(p.view, View::Browse);
        assert_eq!(p.browse_path, f.path());
    }

    #[test]
    fn closing_is_never_offered_as_a_command() {
        // Escape at the repository root is the way out. A palette entry doing
        // the same thing would be a second name for one action, and the user
        // would have to read past it every time they open the commands.
        let f = Fixture::new();
        let mut p = opened(&f);
        p.fetch();
        assert!(!p.commands().contains(&CMD_CLOSE.to_owned()));
        enter(&mut p, &localize::t("gitclient-section-graph"));
        assert!(!p.commands().contains(&CMD_CLOSE.to_owned()));
    }

    #[test]
    fn closing_still_works_when_the_app_dispatches_it_by_name() {
        // Which is how Escape reaches it, listed or not.
        let f = Fixture::new();
        let mut p = opened(&f);
        p.fetch();
        p.handle_command(CMD_CLOSE, "", 0, &mut String::new());
        assert_eq!(p.view, View::Browse);
        assert_eq!(p.browse_path, f.path());
    }

    // ---- The repository root ---------------------------------------------

    #[test]
    fn the_root_reads_a_status_line_then_the_sections() {
        let f = Fixture::new();
        f.write("a.txt", "one");
        f.commit("first");
        let mut p = opened(&f);
        let rows = labels(&p.fetch());
        assert!(rows[0].contains("main"), "first row was {:?}", rows[0]);
        assert_eq!(
            &rows[1..],
            &[
                localize::t("gitclient-section-changes"),
                localize::t("gitclient-section-graph"),
                localize::t("gitclient-section-branches"),
                localize::t("gitclient-section-stashes"),
                localize::t("gitclient-section-remotes"),
            ]
        );
    }

    #[test]
    fn a_repository_with_no_commits_says_so_rather_than_failing() {
        let f = Fixture::new();
        let mut p = opened(&f);
        let rows = labels(&p.fetch());
        let mut a = localize::Args::new();
        a.set(
            "repo",
            f.path().file_name().unwrap().to_string_lossy().into_owned(),
        );
        assert_eq!(rows[0], localize::t_args("gitclient-head-unborn", &a));
    }

    #[test]
    fn a_bare_repository_has_no_changes_section() {
        // There is no worktree, so there is nothing to stage.
        let dir = crate::repo::fixture::plain_dir();
        let git = crate::repo::fixture::git_at(dir.path());
        git.try_run(["init", "-q", "--bare"]);
        let mut p = browsing(dir.path());
        p.fetch();
        p.handle_command(CMD_OPEN, "", 0, &mut String::new());
        let rows = labels(&p.fetch());
        assert!(
            !rows.contains(&localize::t("gitclient-section-changes")),
            "{rows:?}"
        );
        assert!(rows.contains(&localize::t("gitclient-section-graph")));
    }

    #[test]
    fn no_level_is_ever_empty() {
        // An empty level gets the app's own insert placeholder seeded into it,
        // which here is a row that types into `commit_edit` and means nothing.
        let f = Fixture::new();
        let mut p = opened(&f);
        p.fetch();
        for section in [
            "gitclient-section-graph",
            "gitclient-section-branches",
            "gitclient-section-stashes",
            "gitclient-section-remotes",
        ] {
            p.segments.clear();
            p.sync_rendered_path();
            p.fetch();
            let children = enter(&mut p, &localize::t(section));
            assert!(!children.is_empty(), "{section} rendered an empty level");
        }
    }

    // ---- Changes ---------------------------------------------------------

    #[test]
    fn changes_leads_with_the_message_row_then_the_four_commit_buttons() {
        let f = Fixture::new();
        f.write("a.txt", "one");
        let mut p = opened(&f);
        p.fetch();
        let children = enter(&mut p, &localize::t("gitclient-section-changes"));

        assert!(
            tags::has_input(children[0].as_str().unwrap()),
            "the first row is the message input"
        );
        let buttons: Vec<String> = children[1..5]
            .iter()
            .map(|e| tags::extract_button_function_name(e.as_str().unwrap()).unwrap())
            .collect();
        assert_eq!(
            buttons,
            vec!["commit", "commit-amend", "commit-push", "commit-sync"]
        );
    }

    #[test]
    fn the_message_row_is_not_the_last_row_of_the_level() {
        // The app's insert palette takes over `:` when a level *ends* in an
        // input slot, and it would then be impossible to reach the git command
        // palette from the changes list.
        let f = Fixture::new();
        f.write("a.txt", "one");
        let mut p = opened(&f);
        p.fetch();
        let children = enter(&mut p, &localize::t("gitclient-section-changes"));
        let last = children.last().unwrap();
        let last_is_input = last.as_str().is_some_and(tags::has_input)
            || last.as_obj().is_some_and(|o| tags::has_input(&o.key));
        assert!(!last_is_input, "the level must not end in an input slot");
    }

    #[test]
    fn a_clean_tree_says_so() {
        let f = Fixture::new();
        f.write("a.txt", "one");
        f.commit("first");
        let mut p = opened(&f);
        p.fetch();
        let rows = labels(&enter(&mut p, &localize::t("gitclient-section-changes")));
        assert_eq!(rows.last().unwrap(), &localize::t("gitclient-clean"));
    }

    #[test]
    fn conflicts_are_listed_before_anything_else() {
        // Nothing can be committed until they are gone, so they are the first
        // thing read after the buttons.
        let f = Fixture::new();
        f.write("c.txt", "base\n");
        f.commit("base");
        f.run(["checkout", "-q", "-b", "side"]);
        f.write("c.txt", "side\n");
        f.commit("side");
        f.run(["checkout", "-q", "main"]);
        f.write("c.txt", "main\n");
        f.commit("main");
        let _ = f.git().try_run(["merge", "side"]);

        let mut p = opened(&f);
        p.fetch();
        let rows = labels(&enter(&mut p, &localize::t("gitclient-section-changes")));
        assert!(
            rows[5].starts_with(&localize::t("gitclient-conflict")),
            "row 5 was {:?}",
            rows[5]
        );
    }

    #[test]
    fn a_file_name_containing_markup_is_rendered_inert() {
        // Unescaped, this row would be a live editable field named after part
        // of the path, and Right on it would push the input's text as the
        // path segment.
        let f = Fixture::new();
        f.write("a<input>b.txt", "one\n");
        let mut p = opened(&f);
        p.fetch();
        let children = enter(&mut p, &localize::t("gitclient-section-changes"));
        let row = children
            .iter()
            .find(|e| labels(std::slice::from_ref(e))[0].contains("a<input>b.txt"))
            .expect("the file should be listed");
        let key = row.as_obj().map(|o| o.key.clone()).unwrap();
        assert!(!tags::has_input(&key), "raw key was {key:?}");
        assert!(tags::strip_display(&key).ends_with("a<input>b.txt"));
    }

    #[test]
    fn a_diff_line_that_looks_like_a_button_is_rendered_inert() {
        let f = Fixture::new();
        f.write("page.html", "<p>one</p>\n");
        f.commit("first");
        f.write("page.html", "<button>submit</button>Send\n");

        let mut p = opened(&f);
        p.fetch();
        enter(&mut p, &localize::t("gitclient-section-changes"));
        let row = labels(&p.fetch())
            .into_iter()
            .find(|r| r.contains("page.html"))
            .unwrap();
        let diff = enter(&mut p, &row);
        for elem in &diff {
            let raw = elem.as_str().unwrap();
            assert!(
                !tags::has_button(raw),
                "a diff line became a button: {raw:?}"
            );
            assert!(!tags::has_input(raw), "a diff line became a field: {raw:?}");
        }
        assert!(
            labels(&diff)
                .iter()
                .any(|l| l.contains("<button>submit</button>")),
            "the line should still read as it does in the file: {:?}",
            labels(&diff)
        );
    }

    #[test]
    fn an_untracked_file_shows_its_contents_as_additions() {
        // `git diff` says nothing about a file it has never seen, so a bare
        // diff would leave the row with nothing under it.
        let f = Fixture::new();
        f.write("new.txt", "alpha\nbeta\n");
        let mut p = opened(&f);
        p.fetch();
        enter(&mut p, &localize::t("gitclient-section-changes"));
        let row = labels(&p.fetch())
            .into_iter()
            .find(|r| r.contains("new.txt"))
            .unwrap();
        let rows = labels(&enter(&mut p, &row));
        assert!(rows.contains(&"+alpha".to_owned()), "{rows:?}");
        assert!(rows.contains(&"+beta".to_owned()), "{rows:?}");
    }

    // ---- Staging ---------------------------------------------------------

    /// The label of the one row in `changes` mentioning this path.
    fn change_row(p: &mut GitClientProvider, needle: &str) -> String {
        labels(&p.fetch())
            .into_iter()
            .find(|r| r.contains(needle))
            .unwrap_or_else(|| panic!("no row for {needle}"))
    }

    #[test]
    fn staging_a_file_moves_it_into_the_staged_half() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit("first");
        f.write("a.txt", "two\n");

        let mut p = opened(&f);
        p.fetch();
        enter(&mut p, &localize::t("gitclient-section-changes"));
        let row = change_row(&mut p, "a.txt");
        p.handle_command(CMD_STAGE, &row, 1, &mut String::new());

        let staged: Vec<String> = p
            .status()
            .unwrap()
            .staged()
            .map(|e| e.display_path())
            .collect();
        assert_eq!(staged, vec!["a.txt"]);
    }

    #[test]
    fn unstaging_puts_a_file_back() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit("first");
        f.write("a.txt", "two\n");
        f.run(["add", "a.txt"]);

        let mut p = opened(&f);
        p.fetch();
        enter(&mut p, &localize::t("gitclient-section-changes"));
        let row = change_row(&mut p, "a.txt");
        p.handle_command(CMD_UNSTAGE, &row, 1, &mut String::new());
        assert_eq!(p.status().unwrap().staged().count(), 0);
    }

    #[test]
    fn unstaging_before_the_first_commit_works_too() {
        // There is no HEAD to reset to, so `git reset HEAD -- path` fails and
        // the path has to be removed from the index instead.
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.run(["add", "a.txt"]);

        let mut p = opened(&f);
        p.fetch();
        enter(&mut p, &localize::t("gitclient-section-changes"));
        let row = change_row(&mut p, "a.txt");
        let mut error = String::new();
        p.handle_command(CMD_UNSTAGE, &row, 1, &mut error);
        assert!(error.is_empty(), "{error}");
        assert_eq!(p.take_error(), None);
        assert_eq!(p.status().unwrap().staged().count(), 0);
    }

    #[test]
    fn staging_a_command_with_the_cursor_on_the_wrong_row_says_so() {
        let f = Fixture::new();
        let mut p = opened(&f);
        p.fetch();
        let mut error = String::new();
        p.handle_command(CMD_STAGE, "not a file row", 0, &mut error);
        assert_eq!(error, localize::t("gitclient-error-select-a-file"));
    }

    #[test]
    fn staging_a_file_whose_name_looks_like_a_glob_stages_that_file() {
        // Without `:(literal)` this is read as a character class and matches
        // nothing, so the stage silently does nothing at all.
        let f = Fixture::new();
        f.write("foo[1].txt", "one\n");
        let mut p = opened(&f);
        p.fetch();
        enter(&mut p, &localize::t("gitclient-section-changes"));
        let row = change_row(&mut p, "foo[1].txt");
        p.handle_command(CMD_STAGE, &row, 1, &mut String::new());
        assert_eq!(
            p.status()
                .unwrap()
                .staged()
                .map(|e| e.display_path())
                .collect::<Vec<_>>(),
            vec!["foo[1].txt"]
        );
    }

    #[test]
    fn stage_all_and_unstage_all_are_opposites() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.write("b.txt", "two\n");
        let mut p = opened(&f);
        p.fetch();
        enter(&mut p, &localize::t("gitclient-section-changes"));

        p.handle_command(CMD_STAGE_ALL, "", 0, &mut String::new());
        assert_eq!(p.status().unwrap().staged().count(), 2);
        p.handle_command(CMD_UNSTAGE_ALL, "", 0, &mut String::new());
        assert_eq!(p.status().unwrap().staged().count(), 0);
    }

    // ---- Discarding, which asks first ------------------------------------

    #[test]
    fn discarding_asks_before_it_throws_anything_away() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit("first");
        f.write("a.txt", "two\n");

        let mut p = opened(&f);
        p.fetch();
        enter(&mut p, &localize::t("gitclient-section-changes"));
        let row = change_row(&mut p, "a.txt");
        assert!(
            p.handle_command(CMD_DISCARD, &row, 1, &mut String::new())
                .is_none(),
            "no element: the app moves on to the confirmation list"
        );
        let items = p.command_list_items(CMD_DISCARD);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].data, CONFIRM_NO, "cancel must be the first row");
        assert!(items[1].label.contains("a.txt"));
        // Nothing has happened yet.
        assert_eq!(
            std::fs::read_to_string(f.path().join("a.txt")).unwrap(),
            "two\n"
        );
    }

    #[test]
    fn cancelling_a_discard_changes_nothing() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit("first");
        f.write("a.txt", "two\n");

        let mut p = opened(&f);
        p.fetch();
        enter(&mut p, &localize::t("gitclient-section-changes"));
        let row = change_row(&mut p, "a.txt");
        p.handle_command(CMD_DISCARD, &row, 1, &mut String::new());
        assert!(p.execute_command(CMD_DISCARD, CONFIRM_NO));
        assert_eq!(
            std::fs::read_to_string(f.path().join("a.txt")).unwrap(),
            "two\n"
        );
    }

    #[test]
    fn confirming_a_discard_restores_the_file() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit("first");
        f.write("a.txt", "two\n");

        let mut p = opened(&f);
        p.fetch();
        enter(&mut p, &localize::t("gitclient-section-changes"));
        let row = change_row(&mut p, "a.txt");
        p.handle_command(CMD_DISCARD, &row, 1, &mut String::new());
        assert!(p.execute_command(CMD_DISCARD, CONFIRM_YES));
        assert_eq!(
            std::fs::read_to_string(f.path().join("a.txt")).unwrap(),
            "one\n"
        );
    }

    #[test]
    fn discarding_an_untracked_file_deletes_it() {
        // There is nothing to restore it to, so discarding it means removing
        // it, and the confirmation is the only thing standing in front of that.
        let f = Fixture::new();
        f.write("loose.txt", "one\n");
        let mut p = opened(&f);
        p.fetch();
        enter(&mut p, &localize::t("gitclient-section-changes"));
        let row = change_row(&mut p, "loose.txt");
        p.handle_command(CMD_DISCARD, &row, 1, &mut String::new());
        p.execute_command(CMD_DISCARD, CONFIRM_YES);
        assert!(!f.path().join("loose.txt").exists());
    }

    #[test]
    fn a_confirmation_does_not_survive_the_next_command() {
        // Otherwise an old answer could be applied to a new question.
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        let mut p = opened(&f);
        p.fetch();
        enter(&mut p, &localize::t("gitclient-section-changes"));
        let row = change_row(&mut p, "a.txt");
        p.handle_command(CMD_DISCARD, &row, 1, &mut String::new());
        p.handle_command(CMD_REFRESH, "", 0, &mut String::new());
        assert!(p.command_list_items(CMD_DISCARD).is_empty());
        assert!(!p.execute_command(CMD_DISCARD, CONFIRM_YES));
    }

    // ---- Ignoring --------------------------------------------------------

    #[test]
    fn ignoring_a_file_anchors_the_pattern_at_the_repository_root() {
        // An unanchored `src/a.rs` would also ignore `vendor/src/a.rs`.
        let f = Fixture::new();
        f.write("src/a.rs", "one\n");
        let mut p = opened(&f);
        p.fetch();
        enter(&mut p, &localize::t("gitclient-section-changes"));
        let row = change_row(&mut p, "src/a.rs");
        p.handle_command(CMD_IGNORE, &row, 1, &mut String::new());
        let ignore = std::fs::read_to_string(f.path().join(".gitignore")).unwrap();
        assert!(ignore.lines().any(|l| l == "/src/a.rs"), "{ignore:?}");
        assert!(
            p.status()
                .unwrap()
                .unstaged()
                .all(|e| e.display_path() != "src/a.rs"),
            "the file should no longer be listed"
        );
    }

    #[test]
    fn ignoring_the_same_file_twice_adds_one_line() {
        let f = Fixture::new();
        f.write(".gitignore", "/a.txt\n");
        f.write("a.txt", "one\n");
        let mut p = opened(&f);
        p.ignore(b"a.txt");
        let ignore = std::fs::read_to_string(f.path().join(".gitignore")).unwrap();
        assert_eq!(ignore.matches("/a.txt").count(), 1, "{ignore:?}");
    }

    // ---- Committing ------------------------------------------------------

    #[test]
    fn committing_with_no_message_refuses_and_says_why() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        let mut p = opened(&f);
        p.on_button_press("commit");
        assert_eq!(
            p.take_error(),
            Some(localize::t("gitclient-error-no-message"))
        );
    }

    #[test]
    fn committing_writes_a_commit_and_clears_the_message() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        let mut p = opened(&f);
        p.commit_edit("", "first commit");
        p.on_button_press("commit");
        assert_eq!(p.take_error(), None);
        assert_eq!(p.message, "");
        assert_eq!(f.run(["log", "-1", "--pretty=%s"]), "first commit");
    }

    #[test]
    fn committing_with_nothing_staged_commits_what_is_there() {
        // Which is what VS Code's commit button does, and what someone who has
        // not thought about the index expects.
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit("first");
        f.write("a.txt", "two\n");

        let mut p = opened(&f);
        p.commit_edit("", "second");
        p.on_button_press("commit");
        assert_eq!(p.take_error(), None);
        assert_eq!(f.run(["log", "-1", "--pretty=%s"]), "second");
        assert_eq!(f.run(["status", "--porcelain"]), "");
    }

    #[test]
    fn committing_is_refused_while_a_merge_is_unresolved() {
        let f = Fixture::new();
        f.write("c.txt", "base\n");
        f.commit("base");
        f.run(["checkout", "-q", "-b", "side"]);
        f.write("c.txt", "side\n");
        f.commit("side");
        f.run(["checkout", "-q", "main"]);
        f.write("c.txt", "main\n");
        f.commit("main");
        let _ = f.git().try_run(["merge", "side"]);

        let mut p = opened(&f);
        p.commit_edit("", "sneak it in");
        p.on_button_press("commit");
        assert_eq!(
            p.take_error(),
            Some(localize::t("gitclient-error-conflicts"))
        );
    }

    #[test]
    fn amending_before_there_is_a_commit_says_so() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        let mut p = opened(&f);
        p.on_button_press("commit-amend");
        assert_eq!(
            p.take_error(),
            Some(localize::t("gitclient-error-nothing-to-amend"))
        );
    }

    #[test]
    fn amending_replaces_the_last_commit_rather_than_adding_one() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit("typo in the subejct");
        let mut p = opened(&f);
        p.commit_edit("", "fixed subject");
        p.on_button_press("commit-amend");
        assert_eq!(p.take_error(), None);
        assert_eq!(f.run(["rev-list", "--count", "HEAD"]), "1");
        assert_eq!(f.run(["log", "-1", "--pretty=%s"]), "fixed subject");
    }

    #[test]
    fn commit_and_push_still_commits_when_the_push_cannot_run() {
        // The commit is local and already done; only the push needs a remote.
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        let mut p = opened(&f);
        p.commit_edit("", "local work");
        p.on_button_press("commit-push");
        assert_eq!(f.run(["log", "-1", "--pretty=%s"]), "local work");
    }

    // ---- Undo ------------------------------------------------------------

    #[test]
    fn staging_is_undoable() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit("first");
        f.write("a.txt", "two\n");

        let mut p = opened(&f);
        p.fetch();
        enter(&mut p, &localize::t("gitclient-section-changes"));
        let row = change_row(&mut p, "a.txt");
        p.handle_command(CMD_STAGE, &row, 1, &mut String::new());

        let entries = p.take_timeline_entries();
        assert_eq!(entries.len(), 1);
        let mut error = String::new();
        sicompass_sdk::block_on(p.undo(&entries[0], &mut error));
        assert!(error.is_empty(), "{error}");
        assert_eq!(p.status().unwrap().staged().count(), 0);

        sicompass_sdk::block_on(p.redo(&entries[0], &mut error));
        assert_eq!(p.status().unwrap().staged().count(), 1);
    }

    #[test]
    fn a_commit_is_undoable_and_leaves_the_work_staged() {
        // `--soft`, so the tree is exactly as it was the moment before.
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit("first");
        f.write("a.txt", "two\n");
        f.run(["add", "a.txt"]);

        let mut p = opened(&f);
        p.commit_edit("", "second");
        p.on_button_press("commit");
        let entries = p.take_timeline_entries();
        assert_eq!(entries.len(), 1);

        let mut error = String::new();
        sicompass_sdk::block_on(p.undo(&entries[0], &mut error));
        assert!(error.is_empty(), "{error}");
        assert_eq!(f.run(["rev-list", "--count", "HEAD"]), "1");
        p.invalidate();
        assert_eq!(
            p.status()
                .unwrap()
                .staged()
                .map(|e| e.display_path())
                .collect::<Vec<_>>(),
            vec!["a.txt"],
            "the work should still be staged"
        );
    }

    #[test]
    fn undoing_the_very_first_commit_makes_head_unborn_again() {
        // There is no parent to reset to, so `reset --soft HEAD~1` fails and
        // the ref has to be deleted instead.
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        let mut p = opened(&f);
        p.commit_edit("", "root commit");
        p.on_button_press("commit");
        let entries = p.take_timeline_entries();

        let mut error = String::new();
        sicompass_sdk::block_on(p.undo(&entries[0], &mut error));
        assert!(error.is_empty(), "{error}");
        p.invalidate();
        assert!(p.status().unwrap().branch.unborn());
    }

    #[test]
    fn an_amend_records_nothing_because_it_cannot_be_reversed() {
        // The commit it replaced is unreachable by name afterwards.
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit("first");
        let mut p = opened(&f);
        p.commit_edit("", "reworded");
        p.on_button_press("commit-amend");
        assert!(p.take_timeline_entries().is_empty());
    }

    #[test]
    fn discarding_records_nothing_because_the_work_is_gone() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit("first");
        f.write("a.txt", "two\n");
        let mut p = opened(&f);
        p.fetch();
        enter(&mut p, &localize::t("gitclient-section-changes"));
        let row = change_row(&mut p, "a.txt");
        p.handle_command(CMD_DISCARD, &row, 1, &mut String::new());
        p.execute_command(CMD_DISCARD, CONFIRM_YES);
        assert!(p.take_timeline_entries().is_empty());
    }

    // ---- Graph -----------------------------------------------------------

    #[test]
    fn the_graph_lists_commits_newest_first() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit("first");
        f.write("a.txt", "two\n");
        f.commit("second");

        let mut p = opened(&f);
        p.fetch();
        let rows = labels(&enter(&mut p, &localize::t("gitclient-section-graph")));
        assert!(rows[0].ends_with("second"), "{rows:?}");
        assert!(rows[1].ends_with("first"), "{rows:?}");
    }

    #[test]
    fn a_commit_reads_message_then_size_then_files_then_who_and_when() {
        let f = Fixture::new();
        f.write("a.txt", "one\ntwo\n");
        f.commit("the subject\n\nthe body");

        let mut p = opened(&f);
        p.fetch();
        enter(&mut p, &localize::t("gitclient-section-graph"));
        let commit_row = labels(&p.fetch())[0].clone();
        let rows = labels(&enter(&mut p, &commit_row));

        assert_eq!(rows[0], "the subject");
        assert!(rows.contains(&"the body".to_owned()), "{rows:?}");
        let stats_at = rows.iter().position(|r| r.contains("2")).unwrap();
        let file_at = rows.iter().position(|r| r.contains("a.txt")).unwrap();
        let author_at = rows.iter().position(|r| r.contains("Test")).unwrap();
        assert!(stats_at < file_at, "size comes before the files: {rows:?}");
        assert!(
            file_at < author_at,
            "files come before the author: {rows:?}"
        );
    }

    #[test]
    fn a_commits_file_expands_to_the_diff_it_made() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit("first");

        let mut p = opened(&f);
        p.fetch();
        enter(&mut p, &localize::t("gitclient-section-graph"));
        let commit_row = labels(&p.fetch())[0].clone();
        enter(&mut p, &commit_row);
        let file_row = labels(&p.fetch())
            .into_iter()
            .find(|r| r.contains("a.txt"))
            .unwrap();
        let rows = labels(&enter(&mut p, &file_row));
        assert!(rows.iter().any(|r| r == "+one"), "{rows:?}");
    }

    #[test]
    fn a_repository_with_no_commits_shows_an_empty_graph_rather_than_an_error() {
        let f = Fixture::new();
        let mut p = opened(&f);
        p.fetch();
        let rows = labels(&enter(&mut p, &localize::t("gitclient-section-graph")));
        assert_eq!(rows, vec![localize::t("gitclient-no-commits")]);
        assert_eq!(p.take_error(), None);
    }

    #[test]
    fn the_load_more_button_appears_only_when_the_page_is_full() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit("only");
        let mut p = opened(&f);
        p.fetch();
        let children = enter(&mut p, &localize::t("gitclient-section-graph"));
        assert!(
            !children
                .iter()
                .any(|e| e.as_str().is_some_and(tags::has_button)),
            "one commit is not a full page"
        );
    }

    #[test]
    fn showing_all_branches_widens_the_walk() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit("on main");
        f.run(["checkout", "-q", "-b", "side"]);
        f.write("b.txt", "two\n");
        f.commit("only on side");
        f.run(["checkout", "-q", "main"]);

        let mut p = opened(&f);
        p.fetch();
        enter(&mut p, &localize::t("gitclient-section-graph"));
        assert!(
            !labels(&p.fetch())
                .iter()
                .any(|r| r.contains("only on side"))
        );

        p.handle_command(CMD_ALL_BRANCHES, "", 0, &mut String::new());
        assert!(
            labels(&p.fetch())
                .iter()
                .any(|r| r.contains("only on side"))
        );
        // And the command flips to its opposite rather than staying on.
        assert!(p.commands().contains(&CMD_THIS_BRANCH.to_owned()));
    }

    // ---- Branches --------------------------------------------------------

    #[test]
    fn branches_are_split_into_local_and_remote() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit("first");
        let mut p = opened(&f);
        p.fetch();
        let rows = labels(&enter(&mut p, &localize::t("gitclient-section-branches")));
        assert_eq!(
            rows,
            vec![
                localize::t("gitclient-scope-local"),
                localize::t("gitclient-scope-remote")
            ]
        );
    }

    #[test]
    fn a_branch_row_is_its_bare_name_and_its_state_is_underneath() {
        // The key is the path segment and the label the cursor is restored by,
        // so anything that moves belongs in a child row.
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit("subject here");
        let mut p = opened(&f);
        p.fetch();
        enter(&mut p, &localize::t("gitclient-section-branches"));
        let rows = labels(&enter(&mut p, &localize::t("gitclient-scope-local")));
        assert_eq!(rows, vec!["main".to_owned()]);

        let details = labels(&enter(&mut p, "main"));
        assert!(details.contains(&localize::t("gitclient-branch-current")));
        assert!(details.contains(&localize::t("gitclient-no-upstream")));
        assert!(
            details.last().unwrap().ends_with("subject here"),
            "{details:?}"
        );
    }

    #[test]
    fn checking_out_a_branch_moves_head() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit("first");
        f.run(["branch", "side"]);

        let mut p = opened(&f);
        p.fetch();
        enter(&mut p, &localize::t("gitclient-section-branches"));
        enter(&mut p, &localize::t("gitclient-scope-local"));
        p.handle_command(CMD_CHECKOUT_BRANCH, "side", 1, &mut String::new());
        assert_eq!(p.take_error(), None);
        assert_eq!(f.run(["rev-parse", "--abbrev-ref", "HEAD"]), "side");
    }

    #[test]
    fn a_branch_checked_out_in_another_worktree_is_refused_with_a_useful_reason() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
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

        let mut p = opened(&f);
        p.fetch();
        enter(&mut p, &localize::t("gitclient-section-branches"));
        enter(&mut p, &localize::t("gitclient-scope-local"));
        let mut error = String::new();
        p.handle_command(CMD_CHECKOUT_BRANCH, "side", 1, &mut error);
        assert!(error.contains("wt"), "error was {error:?}");
        assert_eq!(f.run(["rev-parse", "--abbrev-ref", "HEAD"]), "main");
    }

    #[test]
    fn creating_a_branch_asks_for_a_name_and_then_switches_to_it() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit("first");
        let mut p = opened(&f);
        p.fetch();
        enter(&mut p, &localize::t("gitclient-section-branches"));

        let elem = p
            .handle_command(CMD_CREATE_BRANCH, "", 0, &mut String::new())
            .expect("a row to type into");
        assert!(tags::has_input(elem.as_str().unwrap()));
        p.commit_edit("", "feature/thing");
        assert_eq!(p.take_error(), None);
        assert_eq!(
            f.run(["rev-parse", "--abbrev-ref", "HEAD"]),
            "feature/thing"
        );
    }

    #[test]
    fn creating_a_branch_is_undoable() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit("first");
        let mut p = opened(&f);
        p.handle_command(CMD_CREATE_BRANCH, "", 0, &mut String::new());
        p.commit_edit("", "side");
        let entries = p.take_timeline_entries();
        assert_eq!(entries.len(), 1);

        // Back on main first: git will not delete the branch it is on.
        f.run(["checkout", "-q", "main"]);
        let mut error = String::new();
        sicompass_sdk::block_on(p.undo(&entries[0], &mut error));
        assert!(error.is_empty(), "{error}");
        assert!(!f.run(["branch", "--list", "side"]).contains("side"));
    }

    #[test]
    fn deleting_a_branch_asks_first_and_never_forces() {
        // `-d` refusing to drop unmerged work is a safety net, and turning it
        // off on the user's behalf would throw away commits held nowhere else.
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit("first");
        f.run(["checkout", "-q", "-b", "side"]);
        f.write("b.txt", "two\n");
        f.commit("only here");
        f.run(["checkout", "-q", "main"]);

        let mut p = opened(&f);
        p.fetch();
        enter(&mut p, &localize::t("gitclient-section-branches"));
        enter(&mut p, &localize::t("gitclient-scope-local"));
        p.handle_command(CMD_DELETE_BRANCH, "side", 1, &mut String::new());
        assert_eq!(p.command_list_items(CMD_DELETE_BRANCH).len(), 2);
        p.execute_command(CMD_DELETE_BRANCH, CONFIRM_YES);

        assert!(
            f.run(["branch", "--list", "side"]).contains("side"),
            "unmerged work must not be dropped"
        );
        assert!(p.take_error().is_some(), "and the refusal is reported");
    }

    // ---- Stashes ---------------------------------------------------------

    #[test]
    fn stashing_takes_a_message_and_the_stash_is_listed() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit("first");
        f.write("a.txt", "two\n");

        let mut p = opened(&f);
        p.fetch();
        enter(&mut p, &localize::t("gitclient-section-changes"));
        let elem = p
            .handle_command(CMD_STASH, "", 0, &mut String::new())
            .expect("a row to type into");
        assert!(tags::has_input(elem.as_str().unwrap()));
        p.commit_edit("", "work in progress");
        assert_eq!(p.take_error(), None);
        assert_eq!(
            std::fs::read_to_string(f.path().join("a.txt")).unwrap(),
            "one\n"
        );

        p.segments.clear();
        p.sync_rendered_path();
        p.fetch();
        let rows = labels(&enter(&mut p, &localize::t("gitclient-section-stashes")));
        assert!(rows[0].starts_with("stash@{0}"), "{rows:?}");
        assert!(rows[0].contains("work in progress"), "{rows:?}");
    }

    #[test]
    fn stashing_is_undoable() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit("first");
        f.write("a.txt", "two\n");

        let mut p = opened(&f);
        p.handle_command(CMD_STASH, "", 0, &mut String::new());
        p.commit_edit("", "");
        let entries = p.take_timeline_entries();
        assert_eq!(entries.len(), 1);

        let mut error = String::new();
        sicompass_sdk::block_on(p.undo(&entries[0], &mut error));
        assert!(error.is_empty(), "{error}");
        assert_eq!(
            std::fs::read_to_string(f.path().join("a.txt")).unwrap(),
            "two\n"
        );
    }

    #[test]
    fn a_stash_expands_to_what_it_holds() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit("first");
        f.write("a.txt", "two\n");
        f.run(["stash", "push", "-q", "-m", "wip"]);

        let mut p = opened(&f);
        p.fetch();
        enter(&mut p, &localize::t("gitclient-section-stashes"));
        let row = labels(&p.fetch())[0].clone();
        let rows = labels(&enter(&mut p, &row));
        assert!(rows[0].contains("wip"), "{rows:?}");
        assert!(rows.iter().any(|r| r.contains("a.txt")), "{rows:?}");
    }

    #[test]
    fn dropping_a_stash_asks_first() {
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        f.commit("first");
        f.write("a.txt", "two\n");
        f.run(["stash", "push", "-q", "-m", "wip"]);

        let mut p = opened(&f);
        p.fetch();
        enter(&mut p, &localize::t("gitclient-section-stashes"));
        let row = labels(&p.fetch())[0].clone();
        p.handle_command(CMD_DROP_STASH, &row, 1, &mut String::new());
        assert_eq!(p.command_list_items(CMD_DROP_STASH).len(), 2);
        assert_eq!(f.run(["stash", "list"]).lines().count(), 1);

        p.execute_command(CMD_DROP_STASH, CONFIRM_YES);
        assert_eq!(f.run(["stash", "list"]), "");
    }

    // ---- Remotes ---------------------------------------------------------

    #[test]
    fn adding_a_remote_takes_a_name_and_a_url_in_one_row() {
        let f = Fixture::new();
        let mut p = opened(&f);
        p.fetch();
        enter(&mut p, &localize::t("gitclient-section-remotes"));
        p.handle_command(CMD_ADD_REMOTE, "", 0, &mut String::new());
        p.commit_edit("", "origin https://example.invalid/a.git");
        assert_eq!(p.take_error(), None);

        let rows = labels(&p.fetch());
        assert_eq!(rows, vec!["origin".to_owned()]);
        let details = labels(&enter(&mut p, "origin"));
        assert!(
            details[0].contains("https://example.invalid/a.git"),
            "{details:?}"
        );
    }

    #[test]
    fn adding_a_remote_with_only_one_word_explains_the_syntax() {
        let f = Fixture::new();
        let mut p = opened(&f);
        p.handle_command(CMD_ADD_REMOTE, "", 0, &mut String::new());
        p.commit_edit("", "origin");
        assert_eq!(
            p.take_error(),
            Some(localize::t("gitclient-error-remote-syntax"))
        );
    }

    #[test]
    fn removing_a_remote_asks_first() {
        let f = Fixture::new();
        f.run(["remote", "add", "origin", "https://example.invalid/a.git"]);
        let mut p = opened(&f);
        p.fetch();
        enter(&mut p, &localize::t("gitclient-section-remotes"));
        p.handle_command(CMD_REMOVE_REMOTE, "origin", 1, &mut String::new());
        assert_eq!(p.command_list_items(CMD_REMOVE_REMOTE).len(), 2);
        p.execute_command(CMD_REMOVE_REMOTE, CONFIRM_YES);
        assert_eq!(f.run(["remote"]), "");
    }

    #[test]
    fn renaming_a_remote_keeps_its_url() {
        let f = Fixture::new();
        f.run(["remote", "add", "origin", "https://example.invalid/a.git"]);
        let mut p = opened(&f);
        p.fetch();
        enter(&mut p, &localize::t("gitclient-section-remotes"));
        p.handle_command(CMD_RENAME_REMOTE, "origin", 1, &mut String::new());
        p.commit_edit("", "upstream");
        assert_eq!(p.take_error(), None);
        assert_eq!(f.run(["remote"]), "upstream");
    }

    // ---- Refresh policy --------------------------------------------------

    #[test]
    fn a_shallow_level_refreshes_itself_and_a_deep_one_only_says_so() {
        // Rewriting the level someone is reading a diff in is worse than being
        // one keystroke out of date.
        let f = Fixture::new();
        let mut p = opened(&f);
        assert!(p.shallow_enough_to_refresh_silently(), "the root");
        p.segments = vec![Segment::Section(Section::Changes)];
        assert!(p.shallow_enough_to_refresh_silently(), "the changes list");
        p.segments
            .push(Segment::File(Group::Unstaged, b"a.txt".to_vec()));
        assert!(
            !p.shallow_enough_to_refresh_silently(),
            "a diff must not rewrite itself under the cursor"
        );
    }

    #[test]
    fn a_stale_repository_says_so_at_the_top_of_whatever_level_is_open() {
        let f = Fixture::new();
        let mut p = opened(&f);
        p.stale = true;
        let rows = labels(&p.fetch());
        assert_eq!(rows[0], localize::t("gitclient-stale"));
    }

    #[test]
    fn refreshing_clears_the_stale_notice() {
        let f = Fixture::new();
        let mut p = opened(&f);
        p.stale = true;
        p.handle_command(CMD_REFRESH, "", 0, &mut String::new());
        assert!(!p.stale);
        assert_ne!(labels(&p.fetch())[0], localize::t("gitclient-stale"));
    }

    // ---- Settings and identity -------------------------------------------

    #[test]
    fn the_provider_names_itself_the_way_the_settings_section_expects() {
        // `name_matches_provider` in the app compares the display name with
        // its spaces removed against `name()`. A mismatch loses the settings
        // section silently.
        let p = GitClientProvider::new();
        assert_eq!(p.name(), "gitclient");
        assert_eq!(p.display_name().replace(' ', ""), p.name());
    }

    #[test]
    fn the_git_binary_setting_is_picked_up() {
        let mut p = GitClientProvider::new();
        p.on_setting_change("gitBinary", "/usr/bin/git");
        assert_eq!(p.binary, "/usr/bin/git");
        // An empty value is a cleared field, not a request to run "".
        p.on_setting_change("gitBinary", "  ");
        assert_eq!(p.binary, "git");
    }

    #[test]
    fn extended_search_is_suppressed_for_this_provider() {
        // The app skips its cursor/path resync for filesystem-path providers,
        // so a Ctrl+F teleport would leave the two disagreeing from then on.
        let p = GitClientProvider::new();
        assert_eq!(p.collect_extended_search_items(), Some(Vec::new()));
    }

    #[test]
    fn no_command_id_collides_with_one_the_app_has_bound_to_a_key() {
        // `delete` takes Ctrl+D and Delete, `toggle bookmark` takes `b`, and
        // `browse` is the app's view-swap sentinel.
        let f = Fixture::new();
        f.write("a.txt", "one\n");
        let mut p = opened(&f);
        let mut all = Vec::new();
        for segments in [
            vec![],
            vec![Segment::Section(Section::Changes)],
            vec![Segment::Section(Section::Graph)],
            vec![Segment::Section(Section::Branches)],
            vec![Segment::Section(Section::Stashes)],
            vec![Segment::Section(Section::Remotes)],
        ] {
            p.segments = segments;
            all.extend(p.commands());
        }
        for reserved in ["delete", "toggle bookmark", "browse"] {
            assert!(
                !all.iter().any(|c| c == reserved),
                "{reserved:?} would hijack a key"
            );
        }
        for cmd in &all {
            assert!(
                !cmd.contains('<') && !cmd.contains('>'),
                "{cmd:?} is interpolated into a button tag unescaped"
            );
        }
    }
}

#[cfg(test)]
mod autofetch_tests {
    use super::*;
    use crate::repo::fixture::Fixture;

    #[test]
    fn autofetch_is_off_unless_it_is_switched_on() {
        // Nobody asked the app to contact a remote, and a repository needing
        // credentials would fail on a loop in the background.
        let f = Fixture::new();
        let mut p = GitClientProvider::new();
        p.browse_path = f.path();
        p.fetch();
        p.handle_command(CMD_OPEN, "", 0, &mut String::new());
        assert!(!p.autofetch_due());
    }

    #[test]
    fn a_configured_interval_makes_the_first_fetch_due() {
        let f = Fixture::new();
        let mut p = GitClientProvider::new();
        p.browse_path = f.path();
        p.fetch();
        p.handle_command(CMD_OPEN, "", 0, &mut String::new());
        p.on_setting_change("gitAutofetchMinutes", "5");
        assert!(p.autofetch_due());
        // And once it has run, not again until the interval is up.
        p.last_autofetch = Some(std::time::Instant::now());
        assert!(!p.autofetch_due());
    }

    #[test]
    fn a_typo_in_the_interval_is_off_rather_than_a_guess() {
        let mut p = GitClientProvider::new();
        p.on_setting_change("gitAutofetchMinutes", "every five");
        assert_eq!(p.autofetch_minutes, 0);
    }

    #[test]
    fn nothing_is_fetched_while_no_repository_is_open() {
        let mut p = GitClientProvider::new();
        p.on_setting_change("gitAutofetchMinutes", "1");
        assert!(!p.autofetch_due(), "there is no repository to fetch for");
    }
}

#[cfg(test)]
mod locale_tests {
    use std::collections::BTreeSet;

    const EN: &str = include_str!("../locales/en-US.ftl");
    const NL: &str = include_str!("../locales/nl-BE.ftl");
    const FR: &str = include_str!("../locales/fr-BE.ftl");
    const DE: &str = include_str!("../locales/de-BE.ftl");

    fn keys(ftl: &str) -> BTreeSet<String> {
        ftl.lines()
            .filter(|l| !l.starts_with('#') && !l.starts_with(' '))
            .filter_map(|l| l.split_once(" ="))
            .map(|(k, _)| k.trim().to_owned())
            .collect()
    }

    /// Every string has to exist in all four bundles.
    ///
    /// A missing key falls back to en-US, which is not an error but is a
    /// half-translated screen for the reader it happens to, and nothing else
    /// would ever tell us.
    #[test]
    fn all_four_bundles_carry_the_same_keys() {
        let en = keys(EN);
        for (name, other) in [("nl-BE", NL), ("fr-BE", FR), ("de-BE", DE)] {
            let other = keys(other);
            let missing: Vec<_> = en.difference(&other).collect();
            let extra: Vec<_> = other.difference(&en).collect();
            assert!(missing.is_empty(), "{name} is missing {missing:?}");
            assert!(
                extra.is_empty(),
                "{name} has {extra:?}, which en-US does not"
            );
        }
    }

    /// Every key the code asks for has to be in the bundle.
    ///
    /// `t()` echoes an unknown key back, so a typo renders as
    /// `gitclient-somthing` on screen and reads as that out loud.
    #[test]
    fn every_key_the_code_uses_exists() {
        let source = include_str!("lib.rs");
        let defined = keys(EN);
        // Built rather than written out, so this test's own needle is not one
        // of the things it finds in its own source.
        let needle = format!("\"{}-", "gitclient");
        for (index, _) in source.match_indices(needle.as_str()) {
            let rest = &source[index + 1..];
            let key = &rest[..rest.find('"').unwrap()];
            assert!(
                defined.contains(key),
                "{key} is used in the code but not defined in en-US.ftl"
            );
        }
    }

    /// And nothing is defined that nothing uses.
    #[test]
    fn every_key_in_the_bundle_is_used() {
        let source = include_str!("lib.rs");
        for key in keys(EN) {
            assert!(
                source.contains(&format!("\"{key}\"")),
                "{key} is defined but never used"
            );
        }
    }
}
