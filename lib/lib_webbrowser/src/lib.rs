//! Web browser provider — Rust port of `lib_webbrowser/`.
//!
//! Fetches a URL via a real Chrome browser (chromiumoxide, kept off the user's
//! screen by xvfb-run on Linux and by off-screen window placement on Windows),
//! parses the rendered HTML with scraper (html5ever), and converts the DOM to a
//! flat FFON tree of strings and objects that mirrors the C provider's
//! lexbor-based output.
//!
//! A cookie banner is lifted out and answered as a page of its own before the
//! content is handed over (see the gate section).  Nothing else is: a language
//! switcher in particular is part of the page, not a question in front of it.
//!
//! Being part of the page is not the same as being readable, though, and
//! assuming it was cost bpost.be its language choice for a release.  A site's
//! switcher can be an `aria-hidden` modal of href-less anchors, which the prune
//! drops and the walker could not make a link of anyway.  So the languages are
//! also read from where a site declares them in machine-readable form,
//! `<link rel="alternate" hreflang>` in `<head>` — see `declared_languages`,
//! which appends them as the page's last section.  Still not a question.
//!
//! ## FFON tree layout
//!
//! ```text
//! meta             (obj)  — keyboard shortcut hints
//! <url-bar>        (obj when page loaded, str when no page)
//!   heading        (obj)  — h1-h6 → nested objects
//!   paragraph      (str)  — plain text
//!   link text      (str)  — "anchor text <link>url</link>"
//!   list           (obj)  — ul/ol wrapper
//!     - item       (str)
//!   table          (str)  — "cell1 | cell2 | …"
//!   image          (str)  — "alt text [img]"
//! ```

use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::page::AddScriptToEvaluateOnNewDocumentParams;
use futures::StreamExt as _;
use sicompass_sdk::ffon::{FfonElement, FormMap, FormNodeKind};
use sicompass_sdk::localize;
use sicompass_sdk::provider::Provider;
use std::sync::OnceLock;

/// Register this crate's translation bundles with the SDK localizer.
/// Idempotent.
pub fn register_translations() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let _ = localize::register_bundle("en-US", include_str!("../locales/en-US.ftl"));
        let _ = localize::register_bundle("nl-BE", include_str!("../locales/nl-BE.ftl"));
        let _ = localize::register_bundle("fr-BE", include_str!("../locales/fr-BE.ftl"));
        let _ = localize::register_bundle("de-BE", include_str!("../locales/de-BE.ftl"));
    });
}
#[cfg(test)]
use sicompass_sdk::ffon::html_to_ffon;
use sicompass_sdk::ffon::{html_resolve_href, html_submit_selector, html_to_ffon_with_forms};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// Test stub: skip Chrome launches entirely.
//
// Set via `_set_test_no_launch(true)` from integration tests (which can't use
// `#[cfg(test)]` across crate boundaries).  When enabled, `load_url` returns
// a placeholder FFON page immediately and `submit_form_windows` rejects with
// an error — both without ever invoking `launch_browser`.
// ---------------------------------------------------------------------------

static TEST_NO_LAUNCH: AtomicBool = AtomicBool::new(false);

#[doc(hidden)]
pub fn _set_test_no_launch(enabled: bool) {
    TEST_NO_LAUNCH.store(enabled, std::sync::atomic::Ordering::Release);
}

#[inline]
fn test_no_launch() -> bool {
    TEST_NO_LAUNCH.load(std::sync::atomic::Ordering::Acquire)
}

// ---------------------------------------------------------------------------
// Test stub: keep the URL history purely in memory.
//
// Set via `_set_test_no_history(true)` from integration tests, which reach the
// provider as a `Box<dyn Provider>` and so cannot set the per-instance
// `url_history_path` override the in-crate tests use.  When enabled,
// `resolve_url_history_path` returns `None`: `init()` reads nothing and no
// write ever lands, while the in-memory list still behaves normally, so a test
// can build history through the real UI and press a history button.
//
// It defaults to *on* under `cfg(test)`, which is what keeps this crate's own
// unit tests off the developer's real history file. Relying on each test to
// remember an override does not hold: a single test that builds a
// `WebbrowserProvider::new()` without setting `url_history_path`, calls `init()`
// and then commits a URL will read, merge and rewrite `state_home()`, seeding
// someone's real browser history with `a.invalid` and friends. That happened.
// The per-instance override still wins over this (`resolve_url_history_path`
// checks it first), so the tempdir-backed history tests are unaffected.
// ---------------------------------------------------------------------------

static TEST_NO_HISTORY: AtomicBool = AtomicBool::new(cfg!(test));

#[doc(hidden)]
pub fn _set_test_no_history(enabled: bool) {
    TEST_NO_HISTORY.store(enabled, std::sync::atomic::Ordering::Release);
}

#[inline]
fn test_no_history() -> bool {
    TEST_NO_HISTORY.load(std::sync::atomic::Ordering::Acquire)
}

// ---------------------------------------------------------------------------
// Hidden-content pruning
//
// The provider uses Chrome as a JS-capable HTML fetcher and then throws the
// engine's layout away, so a `display:none` mega-menu, a parked modal and a
// consent vendor's whole preference centre all serialise into FFON as ordinary
// readable content.  `settled_html` asks the page which nodes are actually
// hidden — Chrome computed that already — and serialises without them.
//
// Process-global rather than a provider field because `fetch_url_to_ffon` is a
// free function reached through the SDK's global URL-fetcher registry, with no
// provider instance in scope.
// ---------------------------------------------------------------------------

static PRUNE_HIDDEN: AtomicBool = AtomicBool::new(true);

/// Command labels for the prune toggle.  Plain English like the other two —
/// `commands()` is not localized anywhere in the app yet.
const CMD_SHOW_HIDDEN: &str = "show hidden content";
const CMD_HIDE_HIDDEN: &str = "hide hidden content";

/// Mark the focused history row — or, from anywhere else, the page being read —
/// as one worth keeping.  The app hardcodes this name too: it is what gates and
/// dispatches the `b` key, the same way `"delete"` gates Ctrl+D.
pub const CMD_TOGGLE_BOOKMARK: &str = "toggle bookmark";

#[inline]
fn prune_hidden() -> bool {
    PRUNE_HIDDEN.load(Ordering::Acquire)
}

// ---------------------------------------------------------------------------
// Cached page
// ---------------------------------------------------------------------------

struct CachedPage {
    #[allow(dead_code)]
    url: String,
    elements: Vec<FfonElement>,
}

/// Hand-off slot: a background load or submit task fills it, `tick` drains it.
type ReadySlot = Arc<Mutex<Option<(Vec<FfonElement>, FormMap)>>>;

// ---------------------------------------------------------------------------
// Live page session — kept alive for form interaction
//
// Non-Windows only.  On Windows the screen reader (NVDA / Narrator) would
// traverse the off-screen Chrome window via UI Automation; instead we launch
// a fresh Chrome for every page load and form submit, and keep cookies on
// disk in the persistent profile dir (`%TEMP%/sicompass-chrome`).
// ---------------------------------------------------------------------------

#[cfg(not(target_os = "windows"))]
struct LivePageSession {
    session: BrowserSession,
    page: chromiumoxide::Page,
}

// ---------------------------------------------------------------------------
// WebbrowserProvider
// ---------------------------------------------------------------------------

pub struct WebbrowserProvider {
    current_url: String,
    // Path segments pushed via push_path — kept separately so that URL
    // segments containing "://" don't confuse rfind('/') based splitting.
    path_segments: Vec<String>,
    path_cache: String, // "/" or "/seg0/seg1/…", rebuilt on every push/pop
    cached_page: Option<CachedPage>,
    form_map: FormMap,
    // Shared so the background page-load task can create and reuse the Chrome
    // session. A cold launch is the single longest operation in the app (15s
    // timeout) and used to run inline on the render thread.
    #[cfg(not(target_os = "windows"))]
    live: Arc<tokio::sync::Mutex<Option<LivePageSession>>>,
    // Guards against a second load being spawned for the same navigation.
    #[cfg(not(target_os = "windows"))]
    load_inflight: Arc<AtomicBool>,
    // Where a URL committed *during* a load waits its turn. The running task
    // picks it up when it finishes, so a second navigation is neither dropped
    // nor run concurrently with the first. Only the newest is kept: a URL the
    // user has already typed past is not worth a page load.
    #[cfg(not(target_os = "windows"))]
    pending_url: Arc<Mutex<Option<String>>>,
    // Background thread delivers refreshed content here after form submission.
    ready_content: ReadySlot,
    // Typed form values, replayed into a fresh Chrome at submit time.  Source
    // of truth for what the user has filled in between page-load and submit.
    // Cleared on URL navigation and after a successful submit-response render.
    form_field_values: HashMap<String, String>,
    // Errors surfaced by the submit thread (drift detection, launch failures,
    // network errors).  Drained by `take_error`; `Arc<Mutex>` because the
    // submit thread writes into it from a `std::thread::spawn` background.
    pending_error: Arc<Mutex<Option<String>>>,
    // Windows only: single-flight guard preventing a second submit press from
    // racing the first (each submit cold-launches its own Chrome, so two in
    // parallel would contend on the singleton profile dir).
    #[cfg(target_os = "windows")]
    submit_in_flight: Arc<AtomicBool>,
    // Set when a URL navigation starts, consumed when its content lands: the
    // cursor is parked on the URL bar the user just typed into, so the app is
    // asked to descend into the page content once there is content to read.
    // Armed only by URL navigation — a form submit response arrives while the
    // cursor is already deep inside the page, where descending would be wrong.
    pending_enter_content: bool,
    // Handed to the app through `take_navigation_request` on the next poll.
    enter_content_request: bool,
    // URLs the user has committed, newest first and deduplicated, emitted as
    // root-level `<button>` siblings *below* the URL bar. Not children of it:
    // the URL bar's children are the loaded page, and the app descends into
    // them when a load lands.
    url_history: Vec<String>,
    // Which of `url_history`'s URLs the user has marked as worth keeping. A
    // subset of `url_history` at all times: a bookmark is an annotation on a
    // row, so bookmarking a URL that is not listed yet inserts it first.
    //
    // A side set rather than a `Vec<HistoryEntry>` so the ranking machinery —
    // `dedup_keeping_first`, the move-to-top in `record_url_history`, the
    // membership test in `on_button_press` — keeps operating on plain URLs.
    bookmarks: HashSet<String>,
    // Cap on `url_history`, mirroring the terminal's `command_history_size`.
    // 0 means unbounded. Bookmarked entries do not count against it and are
    // never dropped by it: losing one silently is exactly what marking it was
    // meant to prevent.
    url_history_size: usize,
    // Once-guard for the disk read, and the gate on every disk write: nothing
    // is written before something has been read. A provider whose `init()` was
    // never called therefore cannot truncate the user's history file — which is
    // also what keeps every in-crate unit test off the real one.
    url_history_loaded: bool,
    // Per-instance path override for the in-crate tests.
    url_history_path: Option<std::path::PathBuf>,
}

impl WebbrowserProvider {
    pub fn new() -> Self {
        WebbrowserProvider {
            current_url: String::new(),
            #[cfg(not(target_os = "windows"))]
            live: Arc::new(tokio::sync::Mutex::new(None)),
            #[cfg(not(target_os = "windows"))]
            load_inflight: Arc::new(AtomicBool::new(false)),
            #[cfg(not(target_os = "windows"))]
            pending_url: Arc::new(Mutex::new(None)),
            path_segments: Vec::new(),
            path_cache: "/".to_owned(),
            cached_page: None,
            form_map: FormMap::new(),
            ready_content: Arc::new(Mutex::new(None)),
            form_field_values: HashMap::new(),
            pending_error: Arc::new(Mutex::new(None)),
            #[cfg(target_os = "windows")]
            submit_in_flight: Arc::new(AtomicBool::new(false)),
            pending_enter_content: false,
            enter_content_request: false,
            url_history: Vec::new(),
            bookmarks: HashSet::new(),
            url_history_size: 50_000,
            url_history_loaded: false,
            url_history_path: None,
        }
    }

    fn rebuild_path_cache(&mut self) {
        self.path_cache = if self.path_segments.is_empty() {
            "/".to_owned()
        } else {
            format!("/{}", self.path_segments.join("/"))
        };
    }

    /// Navigate to `url`.
    ///
    /// On non-Windows, reuses the persistent live session (or creates one
    /// first).  On Windows, cold-launches Chrome, fetches the HTML, and closes
    /// Chrome again so the off-screen window is never present in the screen
    /// reader's UI Automation tree while the user reads / fills the page.
    /// Cookies and localStorage survive in the fixed `%TEMP%/sicompass-chrome`
    /// profile dir.
    fn load_url(&mut self, url: &str) {
        // URL is changing: any form values typed for the previous page are stale.
        self.form_field_values.clear();

        // Every navigation this provider starts ends with the user in the page:
        // a typed URL, a recall-history row, and the commands that reload the
        // current page (refresh, show/hide hidden).
        //
        // The reload commands are included because a refresh rebuilds the
        // provider root, and while the load is in flight the URL bar is a
        // childless Str — so a cursor that was inside the page has nothing left
        // to stand on and gets clamped back to the bar. Without re-arming, F5
        // from inside an article would silently strand the reader on the URL
        // bar. The app only honours the request from the provider's own top
        // level, so on the synchronous path — where the cursor never left the
        // page — this is a no-op rather than a second descent.
        self.pending_enter_content = true;

        // Test stub: skip Chrome entirely, set a placeholder page.
        if test_no_launch() {
            self.cached_page = Some(CachedPage {
                url: url.to_owned(),
                elements: vec![FfonElement::new_str(format!(
                    "<test-no-launch>{url}</test-no-launch>"
                ))],
            });
            self.form_map = FormMap::new();
            self.current_url = url.to_owned();
            self.content_landed();
            return;
        }

        // Non-Windows: hand the whole load to the runtime. Launching Chrome and
        // navigating can take seconds, and doing it here froze the frame for
        // the duration — SDL events went unpolled, AT-SPI went unserviced, and
        // a screen reader dropped focus tracking. `tick` already drains
        // `ready_content` every frame, so the result lands the same way a form
        // submit response does.
        #[cfg(not(target_os = "windows"))]
        {
            self.current_url = url.to_owned();

            if self.load_inflight.swap(true, Ordering::AcqRel) {
                // A load is already running. Hand it the new destination rather
                // than dropping it: the two share one Chrome tab, so a second
                // task would interleave with the first and the loser's HTML
                // would land under the winner's URL. The running task picks
                // this up as soon as it is done.
                queue_pending(&self.pending_url, url);
                return;
            }

            let live = Arc::clone(&self.live);
            let ready = Arc::clone(&self.ready_content);
            let errors = Arc::clone(&self.pending_error);
            let inflight = Arc::clone(&self.load_inflight);
            let pending = Arc::clone(&self.pending_url);
            let mut target = url.to_owned();

            chromium_runtime().spawn(async move {
                // One navigation per turn, then drain whatever the user typed
                // while it was running. `load_inflight` stays set for the whole
                // chain, so `fetch` keeps showing "Loading…" and no second task
                // is ever spawned alongside this one.
                loop {
                    navigate_once(&live, &ready, &errors, &pending, &target).await;
                    match next_target(&inflight, &pending) {
                        Some(next) => target = next,
                        None => return,
                    }
                }
            });

            return;
        }

        #[cfg(target_os = "windows")]
        let result = chromium_runtime().block_on(fetch_html_once(url));

        #[cfg(target_os = "windows")]
        {
            match result {
                Ok(load) => {
                    let (elements, form_map) = page_to_ffon_with_forms(&load, url);
                    self.cached_page = Some(CachedPage {
                        url: url.to_owned(),
                        elements,
                    });
                    self.form_map = form_map;
                }
                Err(e) => {
                    self.cached_page = Some(CachedPage {
                        url: url.to_owned(),
                        elements: vec![FfonElement::new_str(format!("Error loading {url}: {e}"))],
                    });
                    self.form_map = FormMap::new();
                }
            }
            self.current_url = url.to_owned();
            self.content_landed();
        }
    }

    /// A page the user navigated to is now readable: turn the armed
    /// "enter the content" intent into a request for the app to act on.
    fn content_landed(&mut self) {
        if self.pending_enter_content {
            self.pending_enter_content = false;
            self.enter_content_request = true;
        }
    }

    // -----------------------------------------------------------------------
    // URL recall history
    //
    // The terminal keeps a chronological log of every command. A browser wants
    // the same recall, but ranked: revisiting a site should lift it back to the
    // top rather than add a second copy. That one difference is why the file is
    // rewritten in full on every change instead of appended to.
    // -----------------------------------------------------------------------

    /// Where the recall history lives. `None` if no usable state directory is
    /// available (e.g. `$HOME` unset), which disables the feature's persistence
    /// without disabling the in-memory list.
    ///
    /// Deliberately not next to the Chrome profile: that dir holds what *sites*
    /// remember and is what "clear cookies" wipes, whereas this is the user's
    /// own state and belongs in the state dir, next to the terminal's.
    fn resolve_url_history_path(&self) -> Option<std::path::PathBuf> {
        if let Some(p) = &self.url_history_path {
            return Some(p.clone());
        }
        if test_no_history() {
            return None;
        }
        sicompass_sdk::platform::state_home()
            .map(|s| s.join("sicompass").join("webbrowser").join("history"))
    }

    /// Read every line of the history file, newest first, plus the set of URLs
    /// whose line carried the [`BOOKMARK_PREFIX`] mark.
    ///
    /// `Some` when the history is known: the file was read, or there is no file
    /// yet (a first run — an empty list, with nothing to lose). `None` when a
    /// file is there but could not be read: no permission, a failing disk, a
    /// home directory that has not finished mounting.
    ///
    /// The distinction matters because the whole file is rewritten on every
    /// save. Treating "cannot read" as "empty" would turn a passing error into
    /// permanent loss of the user's history the moment they visit one more
    /// page. Callers must leave the file untouched on `None`. This is the same
    /// rule `load_root_for_write` applies to `settings.json` in lib_settings.
    ///
    /// No state directory at all is `None` too, for the same reason: there is no
    /// file, so there is nothing newer than memory to merge or to take a
    /// bookmark's state from. Reporting an empty file instead would make every
    /// navigation wipe the in-memory bookmarks, and make a bookmark impossible
    /// to remove — the disk set it was compared against would always be empty.
    ///
    /// A file written before bookmarks existed has no marks, so it reads back
    /// as an unbookmarked history — no migration step.
    fn read_url_history_file(&self) -> Option<(Vec<String>, HashSet<String>)> {
        let path = self.resolve_url_history_path()?;
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                let mut urls = Vec::new();
                let mut bookmarks = HashSet::new();
                for line in content.lines() {
                    // Trim before stripping the mark: a line with leading
                    // whitespace would otherwise read back unbookmarked.
                    // `sanitize_history_url` percent-encodes a leading `*`, so
                    // the mark is unambiguous by construction.
                    let trimmed = line.trim();
                    let (marked, rest) = match trimmed.strip_prefix(BOOKMARK_PREFIX) {
                        Some(rest) => (true, rest),
                        None => (false, trimmed),
                    };
                    let Some(url) = sanitize_history_url(rest) else {
                        continue;
                    };
                    if marked {
                        bookmarks.insert(url.clone());
                    }
                    urls.push(url);
                }
                Some((urls, bookmarks))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Some((Vec::new(), HashSet::new()))
            }
            Err(e) => {
                eprintln!(
                    "sicompass: {} is unreadable ({e}) — URL history left intact, \
                     this session's pages will not be remembered",
                    path.display()
                );
                None
            }
        }
    }

    /// Load the recall history from disk. Called once, from `init()`.
    ///
    /// Reading here rather than lazily from `fetch()` is deliberate: it means a
    /// provider built without `init()` — every in-crate unit test — has an empty
    /// history and never touches the user's real file.
    fn load_url_history(&mut self) {
        if self.url_history_loaded {
            return;
        }
        self.url_history_loaded = true;
        // An unreadable file starts the session with an empty list, but
        // `record_url_history` re-reads before every save and will refuse to
        // write over what it could not read.
        let (mut lines, bookmarks) = self.read_url_history_file().unwrap_or_default();
        dedup_keeping_first(&mut lines);
        self.url_history = lines;
        self.bookmarks = bookmarks;
        self.trim_url_history();
    }

    /// Drop the oldest entries past the cap. The list is newest-first, so that
    /// is a truncation from the tail.
    ///
    /// Bookmarked entries neither count against the cap nor get dropped by it.
    /// A bookmark says "do not lose this"; ageing one out of a most-recently-used
    /// ranking would be the one deletion the user explicitly asked against, and
    /// it would happen silently. So the cap governs the unbookmarked rows only.
    fn trim_url_history(&mut self) {
        if self.url_history_size == 0 {
            return;
        }
        let mut kept = 0usize;
        let bookmarks = &self.bookmarks;
        let limit = self.url_history_size;
        self.url_history.retain(|url| {
            if bookmarks.contains(url) {
                return true;
            }
            kept += 1;
            kept <= limit
        });
    }

    /// Drop bookmarks whose URL is no longer listed, restoring the
    /// `bookmarks ⊆ url_history` invariant after a merge or a trim.
    fn prune_orphan_bookmarks(&mut self) {
        if self.bookmarks.is_empty() {
            return;
        }
        let listed: HashSet<&str> = self.url_history.iter().map(String::as_str).collect();
        self.bookmarks.retain(|url| listed.contains(url.as_str()));
    }

    /// Rewrite the history file from memory.
    ///
    /// Gated on `url_history_loaded`: writing from memory that was never loaded
    /// would truncate whatever the user already had.
    ///
    /// `atomic_write` rather than `fs::write` because the whole file is
    /// rewritten on every navigation, and `fs::write` truncates the target
    /// first — a second tab starting up could read it empty.
    fn save_url_history(&self) {
        if !self.url_history_loaded {
            return;
        }
        let Some(path) = self.resolve_url_history_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            sicompass_sdk::platform::make_dirs(parent);
        }
        let mut content = String::with_capacity(self.url_history.len() * 32);
        for url in &self.url_history {
            if self.bookmarks.contains(url) {
                content.push_str(BOOKMARK_PREFIX);
            }
            content.push_str(url);
            content.push('\n');
        }
        sicompass_sdk::platform::atomic_write(&path, &content);
    }

    /// Put `url` at the top of the recall history, in memory and on disk.
    /// Already present means moved, not duplicated.
    ///
    /// Read-merge-write rather than a blind rewrite: each browser tab owns its
    /// own provider instance with its own copy of the list, so writing only
    /// what this instance remembers would drop everything another tab recorded
    /// since this one last read.
    fn record_url_history(&mut self, url: &str) {
        let Some(url) = sanitize_history_url(url) else {
            return;
        };
        self.merge_and_save(&url, true, None);
    }

    /// Read-merge-write the history file around a single `target` URL.
    ///
    /// `promote` lifts it to the top of the ranking, which is what a navigation
    /// does. Without it the target keeps whatever position the merge gives it,
    /// and is only prepended when the merge would not list it at all — a
    /// bookmark is an annotation on a row, so there has to be a row, but marking
    /// one must never move it out from under the cursor that marked it.
    ///
    /// `bookmark` sets or clears the target's mark; `None` leaves it to disk.
    ///
    /// Read-merge-write rather than a blind rewrite: each browser tab owns its
    /// own provider instance with its own copy of the list, so writing only
    /// what this instance remembers would drop everything another tab recorded
    /// since this one last read.
    ///
    /// The *flags* come from disk rather than from a union with this instance's
    /// set, and that asymmetry is deliberate. A union can only ever add, so a
    /// tab still holding a stale bookmark would resurrect one the user had just
    /// removed in another tab. Every toggle writes through immediately, so this
    /// instance never holds an unsaved flag worth preserving — except the one
    /// being set right now, which `bookmark` carries in explicitly.
    ///
    /// Nothing is written when the read reports no usable file — either it
    /// failed, where saving from a list missing everything it could not see is
    /// how a passing I/O error becomes a lost history, or there is no state
    /// directory, where the in-memory list is all there has ever been.
    fn merge_and_save(&mut self, target: &str, promote: bool, bookmark: Option<bool>) {
        let disk = self.read_url_history_file();
        let mut merged = Vec::with_capacity(1 + self.url_history.len());
        // Going in first is what promotes: `dedup_keeping_first` then drops the
        // older copy, wherever down the list it sat.
        if promote {
            merged.push(target.to_owned());
        }
        merged.append(&mut self.url_history);
        if let Some((disk_urls, _)) = &disk {
            merged.extend(disk_urls.iter().cloned());
        }
        dedup_keeping_first(&mut merged);
        if !merged.iter().any(|u| u == target) {
            merged.insert(0, target.to_owned());
        }
        self.url_history = merged;
        if let Some((_, disk_bookmarks)) = &disk {
            self.bookmarks = disk_bookmarks.clone();
        }
        match bookmark {
            Some(true) => {
                self.bookmarks.insert(target.to_owned());
            }
            Some(false) => {
                self.bookmarks.remove(target);
            }
            None => {}
        }
        self.trim_url_history();
        self.prune_orphan_bookmarks();
        if disk.is_some() {
            self.save_url_history();
        }
    }

    /// The recall history as activatable rows, newest first, each bookmarked one
    /// prefixed with the bookmark marker.
    ///
    /// Same payload shape as the terminal's history buttons: the function name
    /// is the URL itself, so `on_button_press` receives it directly. The marker
    /// goes in the *display* text after `</button>` so the function name stays
    /// the bare URL — the row's identity must not change when it is marked.
    fn history_buttons(&self) -> Vec<FfonElement> {
        // Not `init()`: the in-crate tests build providers without it, and an
        // unregistered message id resolves to the id itself.
        register_translations();
        let marker = localize::t("webbrowser-bookmark-marker");
        self.url_history
            .iter()
            .map(|u| {
                let mark = if self.bookmarks.contains(u) {
                    format!("{marker} ")
                } else {
                    String::new()
                };
                FfonElement::new_str(format!("<button>{u}</button>{mark}{u}"))
            })
            .collect()
    }

    /// Flip `url`'s bookmark, listing it first if the history does not have it
    /// yet. Returns the new state.
    ///
    /// Not undoable: the provider emits no `TimelineEntry`, so Ctrl+Z after a
    /// toggle steps the *navigation* back instead. Press `b` again to reverse it.
    fn toggle_bookmark(&mut self, url: &str) -> bool {
        // Against disk, not against memory: another tab may have changed the
        // flag since this instance last read, and its view is the newer one.
        let on = match self.read_url_history_file() {
            Some((_, disk_bookmarks)) => !disk_bookmarks.contains(url),
            None => !self.bookmarks.contains(url),
        };
        self.merge_and_save(url, false, Some(on));
        on
    }

    /// Persist a form field's new value into `cached_page` so re-fetches keep it.
    fn patch_cached_form_field(&mut self, form_key: &str, new_value: &str) {
        let Some(slash) = form_key.find('/') else {
            return;
        };
        let form_name = &form_key[..slash];
        let field_label = &form_key[slash + 1..];
        let Some(page) = &mut self.cached_page else {
            return;
        };
        let prefix = format!("{field_label}: <input>");
        let replacement = format!("{field_label}: <input>{new_value}</input>");
        patch_form_field_in_tree(&mut page.elements, form_name, &prefix, &replacement);
    }

    /// Windows submit path: cold-launch Chrome on a background thread,
    /// navigate to the current URL, refill every typed value, click submit,
    /// fetch the response page, and close Chrome again.
    ///
    /// Cookies persist via the fixed `%TEMP%/sicompass-chrome` profile dir,
    /// so login sessions survive across the close-then-reopen cycle.
    /// Cold-launch + navigate + settle adds ~5–10 s of latency between the
    /// submit press and the response page rendering — accepted for the
    /// accessibility gain of no Chrome window in the UIA tree during fill.
    #[cfg(target_os = "windows")]
    fn submit_form_windows(&mut self, form_n: usize) {
        // Test stub: never launch Chrome from tests.
        if test_no_launch() {
            set_error(
                &self.pending_error,
                "test-no-launch: submit skipped".to_owned(),
            );
            return;
        }

        // Single-flight: a second submit press while one is in flight would
        // contend on the singleton profile dir's SingletonLock.
        if self.submit_in_flight.swap(true, Ordering::AcqRel) {
            set_error(
                &self.pending_error,
                "Submit already in progress; please wait for the response.".to_owned(),
            );
            return;
        }

        let url = self.current_url.clone();
        let stored_values = self.form_field_values.clone();
        let ready = Arc::clone(&self.ready_content);
        let error_slot = Arc::clone(&self.pending_error);
        let in_flight = Arc::clone(&self.submit_in_flight);

        std::thread::spawn(move || {
            // Drop guard: clear `in_flight` on every exit path, including panic.
            struct ClearOnDrop(Arc<AtomicBool>);
            impl Drop for ClearOnDrop {
                fn drop(&mut self) {
                    self.0.store(false, Ordering::Release);
                }
            }
            let _guard = ClearOnDrop(in_flight);

            chromium_runtime().block_on(submit_form_windows_async(
                url,
                form_n,
                stored_values,
                ready,
                error_slot,
            ));
        });
    }

    /// Fill a form field via CDP. Called from `commit_edit` when the path
    /// resolves to a known form-map entry.
    ///
    /// Non-Windows only.  On Windows there is no persistent Chrome page to
    /// fill into; typed values are replayed in the submit thread.
    #[cfg(not(target_os = "windows"))]
    fn cdp_fill_field(&self, form_key: &str, value: &str) -> bool {
        let Some(node) = self.form_map.get(form_key) else {
            return false;
        };
        // Locking here still occupies the frame, but these are single CDP
        // round-trips, not a page load.
        let guard = chromium_runtime().block_on(self.live.lock());
        let Some(live) = guard.as_ref() else {
            return false;
        };
        let selector = node.css_selector.clone();
        let form_index = node.form_index;
        let match_index = node.match_index;
        // A form-less control (search box) has no submit button, so committing
        // its value also submits via an Enter key sequence.
        let submit = form_index == 0;
        let value = value.to_owned();
        let page = live.page.clone();
        let js = build_fill_js(form_index, match_index, &selector, &value, submit);
        let result = chromium_runtime().block_on(async move {
            tokio::time::timeout(tokio::time::Duration::from_secs(5), page.evaluate(js)).await
        });
        result.map(|r| r.is_ok()).unwrap_or(false)
    }
}

impl Default for WebbrowserProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for WebbrowserProvider {
    fn name(&self) -> &str {
        "webbrowser"
    }
    fn display_name(&self) -> String {
        register_translations();
        localize::t("webbrowser-display-name")
    }

    fn fetch(&mut self) -> Vec<FfonElement> {
        let mut result = Vec::new();

        // URL bar element
        let url_bar = format!(
            "<input>{}</input>",
            if self.current_url.is_empty() {
                "https://"
            } else {
                &self.current_url
            }
        );

        // A load is running: the URL bar plus a status line, so the view is not
        // silently empty for the seconds Chrome takes. The URL bar is a plain
        // row here, deliberately childless: the app dives into a committed
        // element that has children, and neither a "Loading…" placeholder nor
        // the previous page is worth dropping the user into. The descent is
        // asked for through `take_navigation_request` once the page lands.
        #[cfg(not(target_os = "windows"))]
        if self.load_inflight.load(Ordering::Acquire) {
            result.push(FfonElement::new_str(url_bar));
            result.push(FfonElement::new_str("Loading…".to_owned()));
            // History stays put while loading. Dropping it here would shrink
            // the root list to two rows and grow it back seconds later, moving
            // every index out from under a cursor parked on one.
            result.extend(self.history_buttons());
            return result;
        }

        if let Some(ref page) = self.cached_page {
            // Page loaded: wrap URL bar + page content in an Obj
            let mut page_obj = FfonElement::new_obj(&url_bar);
            let o = page_obj.as_obj_mut().unwrap();
            for elem in &page.elements {
                o.push(elem.clone());
            }
            result.push(page_obj);
        } else {
            result.push(FfonElement::new_str(url_bar));
        }

        // Recall history, newest first, as siblings *below* the URL bar. Index
        // 0 is always the URL bar and 1.. is always the history: the app relies
        // on that when it parks the cursor after a history row is pressed.
        result.extend(self.history_buttons());

        result
    }

    fn commit_edit(&mut self, _old: &str, new_content: &str) -> bool {
        // Check if the current path points to a form field.
        if let Some(form_key) = extract_form_key(&self.path_cache) {
            if self.form_map.contains_key(&form_key) {
                // Record the typed value so it can be replayed at submit time.
                // (On Windows there is no live Chrome to fill into; on other
                // platforms we still fill the live DOM AND remember the value
                // so the two stay in sync if the user resubmits after a tick.)
                self.form_field_values
                    .insert(form_key.clone(), new_content.to_owned());
                // Non-Windows: fill the live Chrome DOM as a side effect.
                #[cfg(not(target_os = "windows"))]
                {
                    self.cdp_fill_field(&form_key, new_content);
                }
                // Persist the value in cached_page so that any future re-fetch
                // returns it.  Return false so the app does NOT call
                // refresh_current_directory — that would re-invoke fetch() and
                // overwrite the value the app's unconditional local-FFON update
                // already wrote into r.ffon (handlers.rs, "Update FFON element
                // regardless of commit result").
                self.patch_cached_form_field(&form_key, new_content);
                return false;
            }
        }
        // Otherwise treat as URL navigation.
        let Some(full_url) = normalize_url_input(new_content) else {
            return false;
        };
        // Recorded here rather than in `load_url`, whose other callers — the
        // refresh, language and hidden-content commands — reload the page the
        // user is already on and must not reorder the history.
        self.record_url_history(&full_url);
        // `load_url` arms the descent into the loaded page, so the user does not
        // have to press Right after every navigation.
        self.load_url(&full_url);
        true
    }

    fn push_path(&mut self, segment: &str) {
        self.path_segments.push(segment.to_owned());
        self.rebuild_path_cache();
    }

    fn pop_path(&mut self) {
        self.path_segments.pop();
        self.rebuild_path_cache();
    }

    fn current_path(&self) -> &str {
        &self.path_cache
    }

    fn set_current_path(&mut self, path: &str) {
        // Store the path as-is; clear segment tracking since we can't reliably
        // split on '/' when segments may contain "://" (URL values).
        self.path_cache = path.to_owned();
        self.path_segments.clear();
    }

    fn on_button_press(&mut self, function_name: &str) {
        // A recall-history row: load it, and lift it back to the top so the
        // list stays ranked by last use. Identical to typing the same URL into
        // the bar, including arming the descent into the page content.
        //
        // Matched by membership rather than by shape, so a button the *page*
        // happens to carry can never be mistaken for a history row and
        // navigate the tab out from under the user.
        if self.url_history.iter().any(|u| u == function_name) {
            let url = function_name.to_owned();
            self.record_url_history(&url);
            self.load_url(&url);
            return;
        }

        // "submit:form_N" — find the submit button selector and click it.
        let Some(form_n_str) = function_name.strip_prefix("submit:form_") else {
            return;
        };
        let form_n: usize = form_n_str.parse().unwrap_or(0);

        #[cfg(target_os = "windows")]
        {
            self.submit_form_windows(form_n);
        }

        #[cfg(not(target_os = "windows"))]
        {
            let (selector, match_index) = self
                .form_map
                .iter()
                .find(|(key, node)| {
                    key.starts_with(&format!("form_{form_n}/"))
                        && matches!(node.kind, FormNodeKind::Submit)
                })
                .map(|(_, node)| (node.css_selector.clone(), node.match_index))
                .unwrap_or_else(|| (html_submit_selector("", ""), 0));

            let guard = chromium_runtime().block_on(self.live.lock());
            let Some(live) = guard.as_ref() else {
                return;
            };
            let page = live.page.clone();
            let ready = Arc::clone(&self.ready_content);
            let url = self.current_url.clone();

            std::thread::spawn(move || {
                // Resolve the submit button within its form (document.forms is in
                // document order, matching form_n), or against document for a
                // form-less control. Fall back to form.submit() for real forms.
                let js = format!(
                    "(() => {{ const fi = {form_n}; \
                     const root = fi > 0 ? (document.forms[fi-1] || document) : document; \
                     const el = root.querySelectorAll({})[{match_index}]; \
                     if (el) {{ el.click(); return true; }} \
                     if (fi > 0 && document.forms[fi-1]) {{ document.forms[fi-1].submit(); return true; }} \
                     return false; }})()",
                    js_quote(&selector)
                );
                chromium_runtime().block_on(async move {
                    let _ = tokio::time::timeout(
                        tokio::time::Duration::from_secs(5),
                        page.evaluate(js),
                    )
                    .await;
                    // Wait for the page to settle after the click. A fixed
                    // guess is not enough: a consent choice reloads in place and
                    // a submit navigates, and at a desktop viewport bpost takes
                    // longer than 2.5 s to put its content back — serialising
                    // early handed back a page with its whole middle missing.
                    await_stable_url(&page, tokio::time::Duration::from_secs(10)).await;
                    await_page_settled(&page, tokio::time::Duration::from_secs(12)).await;
                    if let Ok(Ok(html)) = tokio::time::timeout(
                        tokio::time::Duration::from_secs(20),
                        settled_html(&page),
                    )
                    .await
                    {
                        // Answering one step is what leads to the next: a
                        // language choice usually lands on a page whose cookie
                        // banner has yet to be answered. A submit can also land
                        // on a bot check just as a navigation can, so either way
                        // the response goes through the same renderer.
                        let landed =
                            tokio::time::timeout(tokio::time::Duration::from_secs(5), page.url())
                                .await
                                .ok()
                                .and_then(|r| r.ok())
                                .flatten()
                                .unwrap_or_else(|| url.clone());
                        let (load, _) = settle_gates(&page, &landed, html).await;
                        let (elements, form_map) = page_to_ffon_with_forms(&load, &landed);
                        if let Ok(mut guard) = ready.lock() {
                            *guard = Some((elements, form_map));
                        }
                    }
                });
            });
        }
    }

    fn tick(&mut self) -> bool {
        let content = self.ready_content.lock().ok().and_then(|mut g| g.take());
        if let Some((elements, form_map)) = content {
            self.cached_page = Some(CachedPage {
                url: self.current_url.clone(),
                elements,
            });
            self.form_map = form_map;
            // Submit-response page is now showing: old typed values no longer
            // apply to any field on this page.  Clear them so a subsequent
            // submit on a new form starts from a clean slate.
            self.form_field_values.clear();
            self.content_landed();
            return true;
        }
        false
    }

    fn take_navigation_request(&mut self) -> Option<sicompass_sdk::NavigationRequest> {
        std::mem::take(&mut self.enter_content_request)
            .then_some(sicompass_sdk::NavigationRequest::EnterChildren)
    }

    fn take_error(&mut self) -> Option<String> {
        self.pending_error.lock().ok().and_then(|mut g| g.take())
    }

    fn init(&mut self) {
        self.load_url_history();
    }

    fn on_setting_change(&mut self, key: &str, value: &str) {
        if key == "urlHistorySize" {
            // Garbage is ignored rather than reset to a default: a typo in the
            // settings file should not silently discard the user's history.
            if let Ok(n) = value.parse::<usize>() {
                self.url_history_size = n;
                self.trim_url_history();
            }
        }
    }

    fn cleanup(&mut self) {
        // Non-Windows: close the persistent Chrome session cleanly.
        // Windows: no persistent session — any in-flight submit thread owns
        // its own session and will close it on its own.
        #[cfg(not(target_os = "windows"))]
        {
            if let Some(live) = chromium_runtime().block_on(self.live.lock()).take() {
                use chromiumoxide::cdp::browser_protocol::browser::CloseParams;
                let _ = chromium_runtime().block_on(async {
                    tokio::time::timeout(
                        tokio::time::Duration::from_millis(500),
                        live.session.browser.execute(CloseParams::default()),
                    )
                    .await
                });
            }
        }
    }

    fn commands(&self) -> Vec<String> {
        // Labelled by what pressing it does, not by the current state.
        let hidden_toggle = if prune_hidden() {
            CMD_SHOW_HIDDEN
        } else {
            CMD_HIDE_HIDDEN
        };
        vec![
            "refresh".to_owned(),
            "clear cookies".to_owned(),
            hidden_toggle.to_owned(),
            // Advertising this is also what gates the `b` key: the app looks for
            // the name rather than for a provider called "webbrowser".
            CMD_TOGGLE_BOOKMARK.to_owned(),
        ]
    }

    fn handle_command(
        &mut self,
        cmd: &str,
        _elem_key: &str,
        _elem_type: i32,
        _error: &mut String,
    ) -> Option<FfonElement> {
        if cmd == CMD_TOGGLE_BOOKMARK {
            self.handle_toggle_bookmark_command(_elem_key, _error);
            return None;
        }
        if cmd == "refresh" {
            let url = self.current_url.clone();
            if !url.is_empty() {
                // load_url clears form_field_values, so refresh wipes any
                // typed-but-not-yet-submitted form values.  Intentional —
                // refresh means "start over from the server's current state".
                self.cached_page = None;
                self.load_url(&url);
            }
        } else if cmd == "clear cookies" {
            self.clear_cookies(_error);
        } else if cmd == CMD_SHOW_HIDDEN || cmd == CMD_HIDE_HIDDEN {
            // The prune decides what the *serialised* page contains, so the page
            // has to be fetched again to show the other version of itself.
            PRUNE_HIDDEN.store(cmd == CMD_HIDE_HIDDEN, Ordering::Release);
            let url = self.current_url.clone();
            if !url.is_empty() {
                self.cached_page = None;
                self.load_url(&url);
            }
        }
        None
    }
}

impl WebbrowserProvider {
    /// Body of the [`CMD_TOGGLE_BOOKMARK`] command.
    ///
    /// Resolves what to bookmark from the focused element's key, then reports
    /// the outcome through `error` — the slot the app announces and puts on the
    /// status line. The URL is part of the message on purpose:
    /// `announce_error_if_new` suppresses text identical to the last thing it
    /// spoke, so a bare "Bookmarked" would make a second consecutive bookmark
    /// silent.
    fn handle_toggle_bookmark_command(&mut self, elem_key: &str, error: &mut String) {
        register_translations();
        let Some(url) = self.bookmark_target(elem_key) else {
            *error = localize::t("webbrowser-bookmark-nothing");
            return;
        };
        let on = self.toggle_bookmark(&url);
        let mut args = localize::Args::new();
        args.set("url", url.as_str());
        *error = localize::t_args(
            if on {
                "webbrowser-bookmark-added"
            } else {
                "webbrowser-bookmark-removed"
            },
            &args,
        );
    }

    /// What the bookmark command acts on, given the focused element's key.
    ///
    /// In order:
    /// 1. A history row the cursor is standing on. Matched by membership in
    ///    `url_history`, not by shape, so a `<button>` the *page* carries can
    ///    never be mistaken for one — the same rule `on_button_press` follows.
    /// 2. A `<link>` the app resolved for us: the nearest one enclosing the
    ///    cursor, i.e. the page actually being read. Following an in-page link
    ///    is handled entirely app-side, so `current_url` still names the last
    ///    URL that went through `load_url` and would bookmark the wrong page.
    /// 3. Otherwise the loaded URL — the URL bar, or plain page content.
    fn bookmark_target(&self, elem_key: &str) -> Option<String> {
        if let Some(name) = sicompass_sdk::tags::extract_button_function_name(elem_key)
            && self.url_history.contains(&name)
        {
            return Some(name);
        }
        if let Some(url) = sicompass_sdk::tags::extract_link(elem_key)
            && (url.starts_with("http://") || url.starts_with("https://"))
        {
            return sanitize_history_url(&url);
        }
        sanitize_history_url(&self.current_url)
    }

    /// Clear all cookies from the persistent profile. On non-Windows there is a
    /// live browser holding the cookie store open, so clear it over CDP (which
    /// also empties the backing store). On Windows each fetch uses a throwaway
    /// browser, so there is nothing live to clear — remove the cookie files from
    /// the persistent profile dir instead.
    ///
    /// Also sweeps the language preference 0.1.17 used to keep, since this is
    /// the command for "forget what has been remembered about me".
    #[cfg_attr(target_os = "windows", allow(unused_variables))]
    fn clear_cookies(&mut self, error: &mut String) {
        remove_stale_language_pref();
        #[cfg(not(target_os = "windows"))]
        {
            let guard = chromium_runtime().block_on(self.live.lock());
            if let Some(live) = guard.as_ref() {
                use chromiumoxide::cdp::browser_protocol::network::ClearBrowserCookiesParams;
                let page = live.page.clone();
                let res = chromium_runtime().block_on(async {
                    tokio::time::timeout(
                        tokio::time::Duration::from_secs(5),
                        page.execute(ClearBrowserCookiesParams::default()),
                    )
                    .await
                });
                if !matches!(res, Ok(Ok(_))) {
                    *error = "Could not clear cookies (browser not responding)".to_owned();
                }
                return;
            }
            // No live session: nothing in memory, clear the on-disk store.
            remove_cookie_files(&chrome_profile_dir());
        }
        #[cfg(target_os = "windows")]
        {
            remove_cookie_files(&chrome_profile_dir());
        }
    }
}

/// Where 0.1.17 kept the remembered language choice, or `None` under test.
///
/// Next to the Chrome profile rather than inside it, which is what kept it out
/// of the way of `clear cookies` back when the choice was worth keeping.
/// Nothing writes this any more — the file is only still named here so the
/// leftover can be swept up. Derived rather than hardcoded so it keeps pointing
/// at the same place if the profile dir ever moves.
fn stale_language_pref_path() -> Option<std::path::PathBuf> {
    if test_no_history() {
        return None;
    }
    Some(stale_language_pref_in(&chrome_profile_dir()))
}

/// The derivation on its own, so a test can pin the location without a real
/// config dir and without turning the persistence guard off.
fn stale_language_pref_in(profile: &std::path::Path) -> std::path::PathBuf {
    profile
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(std::env::temp_dir)
        .join("webbrowser-language")
}

/// Delete the leftover language preference, if this machine ever wrote one.
///
/// The language step is gone (see the gate section), so the file is dead state
/// that would otherwise sit in the config dir forever. Safe to drop this, and
/// its caller in `clear_cookies`, once 0.1.17 is far enough behind.
fn remove_stale_language_pref() {
    if let Some(path) = stale_language_pref_path() {
        let _ = std::fs::remove_file(path);
    }
}

/// Best-effort removal of Chrome's cookie database files from a profile dir.
/// Used when there is no live browser to clear over CDP. Chrome keeps cookies in
/// `<profile>/Default/Cookies` (with a `-journal`/`-wal` sidecar); older or
/// single-profile layouts may put them at `<profile>/Cookies`.
fn remove_cookie_files(profile: &std::path::Path) {
    for rel in [
        "Default/Cookies",
        "Default/Cookies-journal",
        "Default/Cookies-wal",
        "Cookies",
        "Cookies-journal",
        "Cookies-wal",
    ] {
        let _ = std::fs::remove_file(profile.join(rel));
    }
}

// ---------------------------------------------------------------------------
// Form-interaction helpers
// ---------------------------------------------------------------------------

/// Extract the form-relative path key from a full provider path.
///
/// The provider path looks like `"/https://example.com/form_1/email"`.
/// We find the first segment that starts with `form_` followed by a digit
/// and return everything from there: `"form_1/email"`.
fn extract_form_key(path: &str) -> Option<String> {
    let trimmed = path.trim_start_matches('/');
    let segments: Vec<&str> = trimmed.split('/').collect();
    let start = segments.iter().position(|seg| {
        seg.starts_with("form_") && seg[5..].chars().next().is_some_and(|c| c.is_ascii_digit())
    })?;
    Some(segments[start..].join("/"))
}

/// Wrap a Rust string as a JSON string literal for inline JS use.
fn js_quote(s: &str) -> String {
    let escaped = s
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r");
    format!("\"{escaped}\"")
}

/// Build a JS snippet that sets an input's value and dispatches input/change
/// events so React/Vue/etc. reactive frameworks notice the change.
///
/// The control is resolved against its owning form — `document.forms[i-1]`, which
/// is in document order and matches `form_index` — or against the whole document
/// when `form_index == 0` (a control outside any `<form>`). When `submit` is set
/// (a form-less search box), an Enter key sequence is dispatched afterwards so the
/// site's search handler fires, since there is no submit button to click.
fn build_fill_js(
    form_index: usize,
    match_index: usize,
    selector: &str,
    value: &str,
    submit: bool,
) -> String {
    let root = if form_index > 0 {
        format!("(document.forms[{}] || document)", form_index - 1)
    } else {
        "document".to_owned()
    };
    let submit_js = if submit {
        "el.dispatchEvent(new KeyboardEvent('keydown', {key:'Enter', code:'Enter', keyCode:13, which:13, bubbles:true}));\n\
            el.dispatchEvent(new KeyboardEvent('keyup',   {key:'Enter', code:'Enter', keyCode:13, which:13, bubbles:true}));\n\
            const f = el.closest('form'); if (f) { (f.requestSubmit ? f.requestSubmit() : f.submit()); }"
    } else {
        ""
    };
    format!(
        r#"(() => {{
            const root = {root};
            const el = root.querySelectorAll({sel})[{match_index}];
            if (!el) return false;
            el.focus();
            const setter =
                Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')?.set
                || Object.getOwnPropertyDescriptor(window.HTMLTextAreaElement.prototype, 'value')?.set;
            if (setter) {{ setter.call(el, {val}); }}
            else {{ el.value = {val}; }}
            el.dispatchEvent(new Event('input',  {{ bubbles: true }}));
            el.dispatchEvent(new Event('change', {{ bubbles: true }}));
            {submit_js}
            return true;
        }})()"#,
        sel = js_quote(selector),
        val = js_quote(value),
    )
}

/// Walk the FFON element tree and update the first `<input>` cell whose label
/// prefix matches, inside the Obj whose key matches `form_name` (exact or suffix,
/// to handle `<id>X</id>form_N` keys produced by id-prefixed forms).
fn patch_form_field_in_tree(
    elems: &mut Vec<FfonElement>,
    form_name: &str,
    prefix: &str,
    replacement: &str,
) -> bool {
    for elem in elems.iter_mut() {
        let FfonElement::Obj(obj) = elem else {
            continue;
        };
        if obj.key == form_name || obj.key.ends_with(form_name) {
            for child in obj.children.iter_mut() {
                if let FfonElement::Str(s) = child {
                    if s.starts_with(prefix) {
                        *s = replacement.to_owned();
                        return true;
                    }
                }
            }
        }
        // Form may be nested under a heading Obj — recurse.
        if patch_form_field_in_tree(&mut obj.children, form_name, prefix, replacement) {
            return true;
        }
    }
    false
}

/// Normalise a user-typed URL bar entry: trim, reject empty, prepend
/// `https://` if no scheme is present.  Pure helper extracted so it can be
/// tested without launching Chrome.
fn normalize_url_input(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.contains("://") {
        Some(trimmed.to_owned())
    } else {
        Some(format!("https://{trimmed}"))
    }
}

/// Longest URL the recall history will store. A runaway `data:` URL is not
/// worth carrying in a file that is rewritten on every navigation.
const MAX_HISTORY_URL_LEN: usize = 4096;

/// Marks a bookmarked line in the history file.
const BOOKMARK_PREFIX: &str = "*";

/// Make `url` safe to carry as a `<button>…</button>` payload and as one line
/// of the history file, or reject it.
///
/// `tags::extract_button_function_name` slices to the *first* `</button>`, so a
/// URL containing that literal would come back truncated and press a different
/// site than the row says. `<` and `>` are excluded characters in RFC 3986, so
/// percent-encoding them denotes the same URL. Control characters and embedded
/// whitespace are rejected outright rather than encoded: the file is
/// line-based, and a URL bar entry containing them is a paste accident, not an
/// address worth remembering.
///
/// A *leading* `*` is encoded for the same reason: it is what marks a line as
/// bookmarked, so `*://x.invalid` — which `normalize_url_input` accepts as-is,
/// since it contains `://` — would otherwise read back as a bookmarked
/// `://x.invalid`. That is a different string, and `on_button_press` matches
/// history rows by membership, so the row would be dead. `*` is a sub-delim, so
/// `%2A` denotes the same URL.
fn sanitize_history_url(url: &str) -> Option<String> {
    let trimmed = url.trim();
    if trimmed.is_empty() || trimmed.len() > MAX_HISTORY_URL_LEN {
        return None;
    }
    if trimmed.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    let escaped = trimmed.replace('<', "%3C").replace('>', "%3E");
    match escaped.strip_prefix(BOOKMARK_PREFIX) {
        Some(rest) => Some(format!("%2A{rest}")),
        None => Some(escaped),
    }
}

/// Drop later duplicates, keeping the first occurrence of each value.
///
/// `Vec::dedup` only collapses *adjacent* equals, which is the wrong tool for a
/// most-recently-used list: the whole point is that a repeat visit appears at
/// the top and its older copy, arbitrarily far down, disappears.
fn dedup_keeping_first(items: &mut Vec<String>) {
    let mut seen = std::collections::HashSet::new();
    items.retain(|item| seen.insert(item.clone()));
}

/// Compare typed form values against a freshly-fetched form map.  Used by the
/// Windows submit path to detect whether the form structure has changed since
/// the user started filling it (e.g. a hidden CSRF input was renamed on reload).
///
/// Returns `Err(missing_keys)` listing every stored key that has no entry in
/// `fresh`.  An empty `stored` always succeeds.
//
// Currently exercised only by tests: the Windows submit path it supports is
// not yet wired up.
#[allow(dead_code)]
pub(crate) fn check_form_drift(
    stored: &HashMap<String, String>,
    fresh: &FormMap,
) -> Result<(), Vec<String>> {
    let mut missing: Vec<String> = stored
        .keys()
        .filter(|k| !fresh.contains_key(k.as_str()))
        .cloned()
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        missing.sort();
        Err(missing)
    }
}

/// Store an error string into a shared error slot.  Quiet on poisoned mutex —
/// the slot is best-effort communication to the UI's `take_error` poller.
pub(crate) fn set_error(slot: &Arc<Mutex<Option<String>>>, msg: String) {
    if let Ok(mut g) = slot.lock() {
        *g = Some(msg);
    }
}

/// Report a failed page load as *both* page content and a status-line error.
///
/// The content half is what unsticks the view.  `fetch` renders "Loading…"
/// while `load_inflight` is set, and `tick` — which returns true only when
/// `ready_content` has been filled — is the sole thing that makes the app
/// re-`fetch` afterwards (`needs_refresh` stays false for this provider).  A
/// failure that filled only the error slot therefore left the URL bar reading
/// "Loading…" forever, and never surfaced the error either: the app drains
/// provider errors on that same tick signal.  Filling both slots means a
/// browser that will not launch (no Chrome installed, launch timed out) shows
/// up as a readable page saying so.
#[cfg(not(target_os = "windows"))]
fn publish_load_failure(ready: &ReadySlot, errors: &Arc<Mutex<Option<String>>>, msg: String) {
    set_error(errors, msg.clone());
    if let Ok(mut g) = ready.lock() {
        *g = Some((vec![FfonElement::new_str(msg)], FormMap::new()));
    }
}

// ---------------------------------------------------------------------------
// Pending-navigation queue
//
// A one-slot, newest-wins queue between the UI thread and the running load
// task.  Committing a URL while a load is in flight puts it here instead of
// spawning a second task: both would drive the same Chrome tab, and the loser's
// HTML would be cached under the winner's URL.
// ---------------------------------------------------------------------------

/// Replace whatever was queued.  Newest wins: an older queued URL is one the
/// user has already typed past, and loading it would only show them a page they
/// no longer asked for.
#[cfg(not(target_os = "windows"))]
fn queue_pending(slot: &Arc<Mutex<Option<String>>>, url: &str) {
    if let Ok(mut g) = slot.lock() {
        *g = Some(url.to_owned());
    }
}

#[cfg(not(target_os = "windows"))]
fn take_pending(slot: &Arc<Mutex<Option<String>>>) -> Option<String> {
    slot.lock().ok().and_then(|mut g| g.take())
}

#[cfg(not(target_os = "windows"))]
fn has_pending(slot: &Arc<Mutex<Option<String>>>) -> bool {
    slot.lock().map(|g| g.is_some()).unwrap_or(false)
}

/// What the load task does after finishing one navigation: the next URL to
/// navigate, or `None` to end the chain and let the flag go.
///
/// The double take is not redundant.  Between the first take and clearing
/// `load_inflight`, a commit still sees the flag set, so it queues instead of
/// spawning — and would then wait for a task that is already exiting.  The
/// second take catches exactly that request and claims the flag back for it.
/// If the swap finds the flag already taken, a later commit won it and spawned
/// its own task for a newer URL, so the one we pulled out is stale: dropping it
/// is the same newest-wins rule `queue_pending` applies.
#[cfg(not(target_os = "windows"))]
fn next_target(inflight: &AtomicBool, pending: &Arc<Mutex<Option<String>>>) -> Option<String> {
    if let Some(next) = take_pending(pending) {
        return Some(next);
    }
    inflight.store(false, Ordering::Release);
    let next = take_pending(pending)?;
    (!inflight.swap(true, Ordering::AcqRel)).then_some(next)
}

// ---------------------------------------------------------------------------
// Async helpers for the persistent live session
// ---------------------------------------------------------------------------

/// Run one navigation to completion and publish its result, reusing the live
/// session or creating it first.
///
/// Publishing is skipped when a newer URL is already queued: its content would
/// flash on screen under the newer URL's bar before being replaced a moment
/// later.  The chain always ends with a publish, because the loop only stops
/// once the queue is empty.
#[cfg(not(target_os = "windows"))]
async fn navigate_once(
    live: &Arc<tokio::sync::Mutex<Option<LivePageSession>>>,
    ready: &ReadySlot,
    errors: &Arc<Mutex<Option<String>>>,
    pending: &Arc<Mutex<Option<String>>>,
    url: &str,
) {
    let mut guard = live.lock().await;
    if guard.is_none() {
        match init_live_session().await {
            Ok(session) => *guard = Some(session),
            Err(e) => {
                drop(guard);
                if !has_pending(pending) {
                    publish_load_failure(ready, errors, format!("Error launching browser: {e}"));
                }
                return;
            }
        }
    }
    let page = guard.as_ref().expect("initialised above").page.clone();
    // Release the session lock across the navigation so a cleanup or cookie
    // clear is not blocked behind a slow page.
    drop(guard);

    let outcome = navigate_and_get_html(&page, url).await;
    if outcome.is_err() {
        // Drop the session so the next attempt starts fresh.
        *live.lock().await = None;
    }
    if has_pending(pending) {
        return;
    }
    match outcome {
        Ok(load) => {
            let (elements, form_map) = page_to_ffon_with_forms(&load, url);
            if let Ok(mut g) = ready.lock() {
                *g = Some((elements, form_map));
            }
        }
        Err(e) => publish_load_failure(ready, errors, format!("Error loading {url}: {e}")),
    }
}

/// Initialise a long-lived Chrome session: launch the browser, open a blank
/// tab, and inject the stealth script so it applies to every page load.
///
/// Non-Windows only.  Windows uses `fetch_html_once` for each page load so the
/// off-screen Chrome window is never present while the user reads the page.
#[cfg(not(target_os = "windows"))]
async fn init_live_session() -> Result<LivePageSession, String> {
    let t = tokio::time::Duration::from_secs;
    let session = launch_browser().await?;
    let page = tokio::time::timeout(t(15), session.browser.new_page("about:blank"))
        .await
        .map_err(|_| "Chrome took >15 s to open a tab".to_owned())?
        .map_err(|e| format!("failed to open tab: {e}"))?;
    tokio::time::timeout(
        t(10),
        page.execute(AddScriptToEvaluateOnNewDocumentParams::new(STEALTH_SCRIPT)),
    )
    .await
    .map_err(|_| "stealth script injection timed out".to_owned())?
    .map_err(|e| format!("stealth script injection failed: {e}"))?;
    set_desktop_viewport(&page).await;
    Ok(LivePageSession { session, page })
}

/// Navigate an existing page to `url` and return the settled HTML.
/// Mirrors the logic of the old `fetch_page` but reuses the caller's tab.
async fn navigate_and_get_html(page: &chromiumoxide::Page, url: &str) -> Result<PageLoad, String> {
    let t = tokio::time::Duration::from_secs;

    tokio::time::timeout(t(30), page.goto(url))
        .await
        .map_err(|_| format!("navigation to {url} timed out after 30 s"))?
        .map_err(|e| format!("navigation to {url} failed: {e}"))?;

    let current_url = await_stable_url(page, t(5)).await;

    // Read the page once up front.  Google/YouTube serve their GDPR wall inline
    // on a normal URL (www.google.com), so the URL alone does not reveal it; we
    // also sniff the fetched content for the consent-save endpoint.
    let html = settled_html(page).await?;

    let (load, _) = settle_gates(page, &current_url, html).await;
    Ok(load)
}

// ---------------------------------------------------------------------------
// Windows close-after-load: per-call launch+fetch+close helpers
//
// On Windows the persistent off-screen Chrome window would be announced by
// screen readers (NVDA / Narrator) via the UI Automation tree.  Instead we
// launch a fresh Chrome for every page load and every form submit, then
// close it again.  Cookies and localStorage survive via the fixed profile
// dir (`%TEMP%/sicompass-chrome`).
// ---------------------------------------------------------------------------

/// Launch a fresh Chrome, open a tab, inject stealth, navigate to `url`,
/// fetch HTML, and close Chrome.  Used by `load_url` on Windows.
#[cfg(target_os = "windows")]
async fn fetch_html_once(url: &str) -> Result<PageLoad, String> {
    let t = tokio::time::Duration::from_secs;
    let session = launch_browser().await?;

    let result: Result<PageLoad, String> = async {
        let page = tokio::time::timeout(t(15), session.browser.new_page("about:blank"))
            .await
            .map_err(|_| "Chrome took >15 s to open a tab".to_owned())?
            .map_err(|e| format!("failed to open tab: {e}"))?;

        tokio::time::timeout(
            t(10),
            page.execute(AddScriptToEvaluateOnNewDocumentParams::new(STEALTH_SCRIPT)),
        )
        .await
        .map_err(|_| "stealth script injection timed out".to_owned())?
        .map_err(|e| format!("stealth script injection failed: {e}"))?;
        set_desktop_viewport(&page).await;

        navigate_and_get_html(&page, url).await
    }
    .await;

    close_browser(&session).await;
    result
}

/// Send `Browser.close` so Chrome exits cleanly (WebSocket close handshake
/// completes) before the `BrowserSession` is dropped.  Mirrors the pattern in
/// `fetch_html_inner`.
///
/// Also used by the browser tests on every platform: nothing kills the child on
/// drop, so a test that opens a session and walks away leaves a Chrome (and its
/// Xvfb) running until the machine is rebooted.
#[cfg(any(target_os = "windows", test))]
async fn close_browser(session: &BrowserSession) {
    use chromiumoxide::cdp::browser_protocol::browser::CloseParams;
    let _ = tokio::time::timeout(
        tokio::time::Duration::from_millis(500),
        session.browser.execute(CloseParams::default()),
    )
    .await;
    // Chrome is gone now; reclaim the foreground for the app window so the
    // screen reader re-focuses sicompass (the automatic "alt-tab back").
    #[cfg(target_os = "windows")]
    win_hide::restore_foreground(session.prev_foreground);
}

/// Poll `page.url()` until it stays the same for `settle` (or `total` elapses).
/// Used after clicking submit to give the post-submit navigation time to land.
///
/// More reliable than a fixed-duration sleep because real navigations can
/// arrive 200 ms or 4 s after the click depending on server roundtrip and
/// client-side redirects.
#[cfg(target_os = "windows")]
async fn await_navigation_stable(
    page: &chromiumoxide::Page,
    total: tokio::time::Duration,
    settle: tokio::time::Duration,
) {
    let poll = tokio::time::Duration::from_millis(200);
    let deadline = tokio::time::Instant::now() + total;
    let mut last_url = String::new();
    let mut stable_since: Option<tokio::time::Instant> = None;
    loop {
        tokio::time::sleep(poll).await;
        let url = tokio::time::timeout(tokio::time::Duration::from_secs(3), page.url())
            .await
            .ok()
            .and_then(|r| r.ok())
            .flatten()
            .unwrap_or_default();

        if url != last_url {
            last_url = url;
            stable_since = Some(tokio::time::Instant::now());
        } else if let Some(start) = stable_since {
            if start.elapsed() >= settle {
                return;
            }
        } else {
            stable_since = Some(tokio::time::Instant::now());
        }

        if tokio::time::Instant::now() >= deadline {
            return;
        }
    }
}

/// Top-level driver for the Windows submit thread.  Launches Chrome, runs the
/// fill+submit+fetch flow, surfaces success or error through the shared slots,
/// and closes Chrome on every exit path.
#[cfg(target_os = "windows")]
async fn submit_form_windows_async(
    url: String,
    form_n: usize,
    stored_values: HashMap<String, String>,
    ready: ReadySlot,
    error_slot: Arc<Mutex<Option<String>>>,
) {
    let session = match launch_browser().await {
        Ok(s) => s,
        Err(e) => {
            set_error(
                &error_slot,
                format!("Could not launch Chrome for submit: {e}"),
            );
            return;
        }
    };

    let result = submit_form_windows_inner(&session, &url, form_n, &stored_values).await;
    match result {
        Ok((elements, form_map)) => {
            if let Ok(mut guard) = ready.lock() {
                *guard = Some((elements, form_map));
            }
        }
        Err(e) => set_error(&error_slot, e),
    }
    close_browser(&session).await;
}

/// Inner submit flow: open a tab, navigate to `url`, drift-check the form,
/// refill every stored value, click submit, await navigation, fetch the
/// response page, and return its parsed FFON + new form map.
///
/// All errors are returned as `Err(String)` for the caller to push into
/// `pending_error`.
#[cfg(target_os = "windows")]
async fn submit_form_windows_inner(
    session: &BrowserSession,
    url: &str,
    form_n: usize,
    stored_values: &HashMap<String, String>,
) -> Result<(Vec<FfonElement>, FormMap), String> {
    let t = tokio::time::Duration::from_secs;

    let page = tokio::time::timeout(t(15), session.browser.new_page("about:blank"))
        .await
        .map_err(|_| "Chrome took >15 s to open a tab".to_owned())?
        .map_err(|e| format!("failed to open tab: {e}"))?;

    tokio::time::timeout(
        t(10),
        page.execute(AddScriptToEvaluateOnNewDocumentParams::new(STEALTH_SCRIPT)),
    )
    .await
    .map_err(|_| "stealth script injection timed out".to_owned())?
    .map_err(|e| format!("stealth script injection failed: {e}"))?;
    set_desktop_viewport(&page).await;

    // Re-navigate to the form URL.  Cookies from the profile dir come along.
    let reopened = navigate_and_get_html(&page, url)
        .await
        .map_err(|e| format!("Failed to reopen {url} for submit: {e}"))?;

    // Parse the fresh form so we can detect drift and look up the up-to-date
    // submit selector (form structure may have changed since the user typed).
    let (_fresh_elements, fresh_form_map) = html_to_ffon_with_forms(&reopened.html, url);

    if let Err(missing) = check_form_drift(stored_values, &fresh_form_map) {
        return Err(format!(
            "Form changed since you started filling it: missing field(s) {}. \
             Refresh the page and try again.",
            missing.join(", ")
        ));
    }

    // Refill every stored value via CDP.
    for (form_key, value) in stored_values {
        let Some(node) = fresh_form_map.get(form_key.as_str()) else {
            // Drift check already passed; this branch only hits if the map
            // changed under us between check and fill (impossible here).
            continue;
        };
        let js = build_fill_js(
            node.form_index,
            node.match_index,
            &node.css_selector,
            value,
            false,
        );
        tokio::time::timeout(t(5), page.evaluate(js))
            .await
            .map_err(|_| format!("Timed out filling field {form_key}"))?
            .map_err(|e| format!("Failed to fill field {form_key}: {e}"))?;
    }

    // Locate the submit selector in the fresh form map; fall back to the
    // generic `form:nth-of-type(N)` submit if the button isn't in the map.
    let selector = fresh_form_map
        .iter()
        .find(|(key, node)| {
            key.starts_with(&format!("form_{form_n}/")) && matches!(node.kind, FormNodeKind::Submit)
        })
        .map(|(_, node)| node.css_selector.clone())
        .unwrap_or_else(|| html_submit_selector("", ""));

    let click_js = format!(
        "(() => {{ const el = document.querySelector({}); \
         if (el) {{ el.click(); return true; }} \
         const f = document.querySelector('form:nth-of-type({form_n})'); \
         if (f) {{ f.submit(); return true; }} return false; }})()",
        js_quote(&selector)
    );
    let _ = tokio::time::timeout(t(5), page.evaluate(click_js)).await;

    // Give the post-submit navigation time to land.
    await_navigation_stable(
        &page,
        tokio::time::Duration::from_secs(10),
        tokio::time::Duration::from_millis(750),
    )
    .await;

    // Fetch the response page.
    let response_html = tokio::time::timeout(t(20), settled_html(&page))
        .await
        .map_err(|_| "timed out waiting for response page (20 s)".to_owned())??;

    let response_url = tokio::time::timeout(t(5), page.url())
        .await
        .ok()
        .and_then(|r| r.ok())
        .flatten()
        .unwrap_or_else(|| url.to_owned());

    // Answering one step leads to the next, so the response page goes through
    // the chain exactly like a navigation does.
    let (load, _) = settle_gates(&page, &response_url, response_html).await;
    Ok(page_to_ffon_with_forms(&load, &response_url))
}

// ---------------------------------------------------------------------------
// Chromium — per-fetch browser launch + shared async runtime
// ---------------------------------------------------------------------------

// Multi-thread runtime (2 workers) for chromiumoxide. Kept alive for the
// process lifetime so repeated fetches reuse the same thread pool.
static CHROMIUM_RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();

// ---------------------------------------------------------------------------
// Windows: hide Chrome windows that appear during headed launch
//
// Chrome must run headed to pass bot-detection on sites like gva.be — headless
// mode is fingerprinted and blocked.  On Linux, xvfb-run provides an invisible
// X11 display.  On Windows we use Browser::launch (which chromiumoxide manages)
// with `with_head()`, and a background thread that calls ShowWindow(SW_HIDE)
// on any Chrome windows that appear while the browser is starting up.
// The window is hidden within one paint frame (~50 ms) — invisible in practice.
// user32.dll is always linked on Windows — no extra crates.
// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Windows: keep Chrome windows off-screen (not hidden)
//
// We must never call ShowWindow(SW_HIDE) on Chrome windows.  Hiding a window
// sends WM_SHOWWINDOW(FALSE) to Chrome's message loop, which drives
// RenderWidget::SetHidden() inside Blink — this forces
// document.visibilityState = "hidden" and kills JavaScript timer resolution
// (React / consent-page apps never hydrate).  Moving the window to a position
// far off all monitors keeps Chrome thinking the window is fully visible while
// making it invisible to the user.  Chrome's JS runs at full speed.
#[cfg(target_os = "windows")]
mod win_hide {
    unsafe extern "system" {
        fn EnumWindows(
            lp_enum_func: unsafe extern "system" fn(isize, isize) -> i32,
            l_param: isize,
        ) -> i32;
        fn GetWindowThreadProcessId(hwnd: isize, lp_dw_process_id: *mut u32) -> u32;
        fn IsWindowVisible(hwnd: isize) -> i32;
        fn OpenProcess(dw_desired_access: u32, b_inherit_handle: i32, dw_process_id: u32) -> isize;
        fn CloseHandle(h_object: isize) -> i32;
        fn QueryFullProcessImageNameW(
            h_process: isize,
            dw_flags: u32,
            lp_exe_name: *mut u16,
            lp_size: *mut u32,
        ) -> i32;
        fn SetWindowPos(
            hwnd: isize,
            hwnd_insert_after: isize,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            u_flags: u32,
        ) -> i32;
        fn GetForegroundWindow() -> isize;
        fn SetForegroundWindow(hwnd: isize) -> i32;
        fn BringWindowToTop(hwnd: isize) -> i32;
        fn IsWindow(hwnd: isize) -> i32;
        fn GetCurrentThreadId() -> u32;
        fn AttachThreadInput(id_attach: u32, id_attach_to: u32, f_attach: i32) -> i32;
        fn GetWindowLongPtrW(hwnd: isize, n_index: i32) -> isize;
        fn SetWindowLongPtrW(hwnd: isize, n_index: i32, dw_new_long: isize) -> isize;
    }

    /// Index of the extended window styles in the window's memory.
    const GWL_EXSTYLE: i32 = -20;
    /// A window with this extended style does not become the foreground window
    /// when clicked/created, so it cannot steal focus from the app.
    const WS_EX_NOACTIVATE: isize = 0x0800_0000;

    /// Mark `hwnd` non-activating so it never takes the foreground again. Applied
    /// to each Chrome window the first time we see it; idempotent if reapplied.
    fn set_no_activate(hwnd: isize) {
        unsafe {
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            if ex & WS_EX_NOACTIVATE == 0 {
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | WS_EX_NOACTIVATE);
            }
        }
    }

    /// The current foreground window. Captured before Chrome launches so we know
    /// which window (the sicompass app) to hand focus back to afterwards.
    pub fn current_foreground() -> isize {
        unsafe { GetForegroundWindow() }
    }

    /// Force `hwnd` back to the foreground.
    ///
    /// When the off-screen Chrome window is created it steals foreground focus,
    /// so the screen reader follows it off into a web document (browse mode) and
    /// stops tracking the sicompass window — exactly the "first arrow-down goes
    /// silent until I alt-tab" symptom. After a page load/submit completes we
    /// call this with the window captured by `current_foreground()` to do what
    /// that manual alt-tab does: re-activate the app window so the screen reader
    /// re-enters focus mode on it.
    ///
    /// Windows blocks `SetForegroundWindow` from a process that does not own the
    /// current foreground window (foreground lock), so we briefly attach our
    /// input queue to the foreground thread first — the standard workaround.
    pub fn restore_foreground(hwnd: isize) {
        if hwnd == 0 {
            return;
        }
        unsafe {
            if IsWindow(hwnd) == 0 {
                return;
            }
            let fg = GetForegroundWindow();
            if fg == hwnd {
                return; // already focused — nothing stole it
            }
            let our_tid = GetCurrentThreadId();
            let mut _pid: u32 = 0;
            let fg_tid = if fg != 0 {
                GetWindowThreadProcessId(fg, &mut _pid)
            } else {
                0
            };
            let attached =
                fg_tid != 0 && fg_tid != our_tid && AttachThreadInput(our_tid, fg_tid, 1) != 0;
            SetForegroundWindow(hwnd);
            BringWindowToTop(hwnd);
            if attached {
                AttachThreadInput(our_tid, fg_tid, 0);
            }
        }
    }

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    /// Do not resize the window.
    const SWP_NOSIZE: u32 = 0x0001;
    /// Do not change z-order.
    const SWP_NOZORDER: u32 = 0x0004;
    /// Do not activate/focus the window.
    const SWP_NOACTIVATE: u32 = 0x0010;

    /// Off-screen position: far beyond any realistic monitor layout.
    const OFFSCREEN_X: i32 = -10_000;
    const OFFSCREEN_Y: i32 = -10_000;

    /// Collect all currently-visible top-level window handles.
    pub fn snapshot_windows() -> Vec<isize> {
        let mut hwnds: Vec<isize> = Vec::new();
        unsafe extern "system" fn callback(hwnd: isize, lparam: isize) -> i32 {
            let vec = unsafe { &mut *(lparam as *mut Vec<isize>) };
            vec.push(hwnd);
            1 // continue
        }
        unsafe { EnumWindows(callback, &mut hwnds as *mut Vec<isize> as isize) };
        hwnds
    }

    /// Move every visible top-level Chrome/Edge window that was NOT present in
    /// `before` to an off-screen position.  The window remains "visible" to
    /// Chrome (no SW_HIDE, no occlusion) so JavaScript timers are never throttled.
    ///
    /// The first time each Chrome window is seen it is also marked
    /// `WS_EX_NOACTIVATE` (so it can never take the foreground again) and the
    /// foreground is handed straight back to `prev_foreground` (the app window).
    /// Without this the newly-created Chrome window grabs the foreground on
    /// creation, the screen reader follows it into a web document, and the app's
    /// arrow keys stop working until the user alt-tabs back. `handled` carries
    /// the set of windows already processed across the mover's 50 ms ticks so the
    /// focus bounce fires once per window, not every tick.
    pub fn hide_new_browser_windows(
        before: &[isize],
        handled: &mut Vec<isize>,
        prev_foreground: isize,
    ) {
        let mut current: Vec<isize> = Vec::new();
        unsafe extern "system" fn callback(hwnd: isize, lparam: isize) -> i32 {
            let vec = unsafe { &mut *(lparam as *mut Vec<isize>) };
            vec.push(hwnd);
            1
        }
        unsafe { EnumWindows(callback, &mut current as *mut Vec<isize> as isize) };

        let mut saw_new = false;
        for hwnd in current {
            if before.contains(&hwnd) {
                continue;
            }
            if unsafe { IsWindowVisible(hwnd) } == 0 {
                continue;
            }

            // Get the process ID for this window.
            let mut pid: u32 = 0;
            unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
            if pid == 0 {
                continue;
            }

            // Open the process to query its image name.
            let h_proc = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
            if h_proc == 0 {
                continue;
            }

            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            let ok = unsafe { QueryFullProcessImageNameW(h_proc, 0, buf.as_mut_ptr(), &mut len) };
            unsafe { CloseHandle(h_proc) };

            if ok == 0 {
                continue;
            }
            let path = String::from_utf16_lossy(&buf[..len as usize]).to_lowercase();
            if path.contains("chrome") || path.contains("msedge") {
                // Move off-screen without resizing, changing z-order, or
                // activating.  Never hide — see module comment.
                unsafe {
                    SetWindowPos(
                        hwnd,
                        0,
                        OFFSCREEN_X,
                        OFFSCREEN_Y,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
                if !handled.contains(&hwnd) {
                    handled.push(hwnd);
                    // Stop this window from ever taking the foreground again.
                    set_no_activate(hwnd);
                    saw_new = true;
                }
            }
        }

        // A Chrome window just appeared (and likely grabbed the foreground on
        // creation). Hand focus straight back to the app so the screen reader
        // re-acquires it — the programmatic equivalent of the user's alt-tab.
        if saw_new {
            restore_foreground(prev_foreground);
        }
    }
}

/// How Chrome is started on Linux.
#[cfg(target_os = "linux")]
enum LinuxChrome {
    /// Path to a wrapper script that runs *headed* Chrome on an invisible
    /// virtual X11 display. The preferred mode: headed Chrome passes the
    /// bot-detection that fingerprints and blocks headless.
    VirtualDisplay(std::path::PathBuf),
    /// Path to the plain Chrome binary, to be launched in Chrome's own
    /// headless mode because no virtual display is available on this machine.
    /// Some sites block headless, but it is the only remaining way to keep
    /// Chrome off the user's screen — see `linux_chrome_launch`.
    Headless(std::path::PathBuf),
}

/// Which virtual-display helper is present on this machine.
#[cfg(target_os = "linux")]
enum XvfbHelper {
    /// `xvfb-run` — the standard wrapper; it allocates a display and cleans up.
    Run,
    /// Bare `Xvfb`, without the `xvfb-run` wrapper (e.g. some Nix setups).
    Bare,
}

/// An AT-SPI bus address that deliberately points at nothing.
///
/// Xvfb keeps Chrome off the user's *screen*, but the accessibility bus is
/// per-session, not per-display. A Chrome started on an invisible display still
/// registers itself on it, so with Orca running the user gets a second
/// application named "Google Chrome" carrying a window titled after the page
/// they just opened. The screen reader then has somewhere else to go, and the
/// arrow keys stop driving sicompass. Chrome only does this while an assistive
/// technology is actually running, which is why the app looks fine until a
/// screen reader is switched on.
///
/// Chrome resolves the a11y bus from `AT_SPI_BUS_ADDRESS` before falling back
/// to the session bus, so aiming that at a socket that cannot exist makes the
/// connection fail and keeps Chrome off the bus entirely. It is the only lever
/// that works here: `NO_AT_BRIDGE` is a GTK variable Chrome does not read, and
/// `--disable-renderer-accessibility` only trims the renderer's tree while the
/// browser process still registers.
#[cfg(target_os = "linux")]
const NO_AT_SPI_BUS: &str = "unix:path=/nonexistent/sicompass-keeps-chrome-off-the-a11y-bus";

/// Environment overrides for every Chrome sicompass starts on Linux.
///
/// Split out from `launch_browser` so the invariant the fix rests on — that the
/// address cannot resolve — is testable without launching a browser.
#[cfg(target_os = "linux")]
fn offscreen_chrome_env() -> [(&'static str, &'static str); 1] {
    [("AT_SPI_BUS_ADDRESS", NO_AT_SPI_BUS)]
}

/// The shell script that starts Chrome on an invisible X11 display.
///
/// Split out from `linux_chrome_launch` so the generated script can be tested
/// without an actual Xvfb on the machine running the tests.
#[cfg(target_os = "linux")]
fn xvfb_wrapper_script(chrome: &str, helper: XvfbHelper) -> String {
    match helper {
        // The trailing `1>&2` is what makes this work at all on Debian and its
        // derivatives. Their xvfb-run runs the command as
        // `DISPLAY=… "$@" 2>&1`, folding Chrome's stderr into stdout, and
        // chromiumoxide launches Chrome with stdout on /dev/null and only
        // stderr on a pipe, which it scans for "DevTools listening on ws://…".
        // Without the redirect that line lands in /dev/null, chromiumoxide
        // waits out its full launch timeout and reports
        // `LaunchTimeout(BrowserStderr(""))` while a perfectly healthy Chrome
        // sits on the virtual display. Sending our stdout to stderr puts the
        // line back where chromiumoxide is listening. Nixpkgs' xvfb-run has no
        // `2>&1`, so there the redirect only moves Chrome's (empty) stdout, and
        // this is why the bug never showed up in the dev shell.
        //
        // `-s "-screen …"` is not optional. Without it xvfb-run uses its own
        // default screen, which on this toolchain came out at 612x459 — Chrome
        // clamps `--window-size=1920,1080` to the screen, the viewport lands at
        // 800px, and every responsive site serves its *phone* layout. bpost
        // then hides its desktop navigation entirely and moves "Pakje
        // verzenden", "Pakje ontvangen" and the service tiles into a hamburger
        // menu, so the page read nothing like the one in a normal browser.
        XvfbHelper::Run => format!(
            "#!/bin/sh\nunset WAYLAND_DISPLAY\nexec xvfb-run -a -s \"-screen 0 {VIEWPORT_W}x{VIEWPORT_H}x24\" {chrome} --ozone-platform=x11 \"$@\" 1>&2\n"
        ),
        // Emulate xvfb-run: pick a free display number, start Xvfb on it, run
        // Chrome (backgrounded so we keep the shell alive), and kill both Xvfb
        // and Chrome on any exit signal. Chrome inherits our stdout/stderr, so
        // chromiumoxide still reads its "DevTools listening on ws://…" line.
        XvfbHelper::Bare => format!(
            "#!/bin/sh\n\
             unset WAYLAND_DISPLAY\n\
             d=99\n\
             while [ -e /tmp/.X${{d}}-lock ] || [ -e /tmp/.X11-unix/X${{d}} ]; do d=$((d+1)); done\n\
             Xvfb :$d -screen 0 {VIEWPORT_W}x{VIEWPORT_H}x24 -nolisten tcp >/dev/null 2>&1 &\n\
             xp=$!\n\
             i=0\n\
             while [ ! -e /tmp/.X11-unix/X${{d}} ]; do i=$((i+1)); [ $i -ge 100 ] && break; sleep 0.05; done\n\
             DISPLAY=:$d {chrome} --ozone-platform=x11 \"$@\" &\n\
             cp=$!\n\
             trap 'kill $cp $xp 2>/dev/null' EXIT HUP INT TERM\n\
             wait $cp\n"
        ),
    }
}

/// Decide how Chrome will be started on Linux, so that it is never visible on
/// the user's screen.
///
/// Preference order:
/// 1. `xvfb-run -a` — headed Chrome on a virtual display. Best compatibility:
///    the browser is a normal headed Chrome as far as any website can tell.
/// 2. Bare `Xvfb` — same thing, with the `xvfb-run` logic inlined into our own
///    wrapper script, for systems that ship `Xvfb` without the wrapper.
/// 3. Neither present — Chrome's own headless mode.
///
/// Step 3 exists because most desktop distributions (Mint, Ubuntu, Debian,
/// Fedora, …) do not install Xvfb by default, so a released build lands there
/// on a normal user's machine while a dev build inside `nix develop` takes
/// step 1. Launching plain headed Chrome there would put a real window on the
/// screen that takes the keyboard focus, and the screen reader follows it out
/// of sicompass. Headless risks being blocked by a few bot-detecting sites;
/// stealing focus from a screen-reader user breaks the whole app. Installing
/// `xvfb` restores step 1, which is why the .deb and .rpm depend on it.
#[cfg(target_os = "linux")]
fn linux_chrome_launch() -> Result<LinuxChrome, String> {
    let chrome = find_chrome_executable().ok_or_else(chrome_missing_message)?;
    linux_chrome_launch_with(detect_xvfb_helper(), chrome)
}

/// Which of the two virtual-display helpers this machine has, if either.
#[cfg(target_os = "linux")]
fn detect_xvfb_helper() -> Option<XvfbHelper> {
    if which::which("xvfb-run").is_ok() {
        Some(XvfbHelper::Run)
    } else if which::which("Xvfb").is_ok() {
        Some(XvfbHelper::Bare)
    } else {
        None
    }
}

/// The decision half of `linux_chrome_launch`, with the machine probe passed
/// in so a test can exercise the no-Xvfb path on a machine that has Xvfb (and
/// the reverse).
#[cfg(target_os = "linux")]
fn linux_chrome_launch_with(
    helper: Option<XvfbHelper>,
    chrome: std::path::PathBuf,
) -> Result<LinuxChrome, String> {
    let Some(helper) = helper else {
        return Ok(LinuxChrome::Headless(chrome));
    };

    let script = xvfb_wrapper_script(&chrome.to_string_lossy(), helper);
    let wrapper = std::env::temp_dir().join("sicompass-xvfb-chrome.sh");
    std::fs::write(&wrapper, &script).map_err(|e| format!("failed to write Xvfb wrapper: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755));
    }
    Ok(LinuxChrome::VirtualDisplay(wrapper))
}

fn chromium_runtime() -> &'static tokio::runtime::Runtime {
    CHROMIUM_RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("failed to build chromium tokio runtime")
    })
}

/// Locate a usable Chrome/Chromium/Edge executable.
///
/// Priority:
/// 1. `SICOMPASS_CHROME_PATH` environment variable
/// 2. Common binary names on `PATH` (works on Linux; unlikely on Windows/macOS)
/// 3. Well-known installation paths for the current OS
fn find_chrome_executable() -> Option<std::path::PathBuf> {
    // 1. Explicit override
    if let Ok(p) = std::env::var("SICOMPASS_CHROME_PATH") {
        let pb = std::path::PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }

    // 2. PATH lookup (reliable on Linux; included here for all platforms)
    const PATH_CANDIDATES: &[&str] = &[
        "google-chrome",
        "google-chrome-stable",
        "google-chrome-beta",
        "chromium",
        "chromium-browser",
        "chrome",
    ];
    if let Some(p) = PATH_CANDIDATES.iter().find_map(|n| which::which(n).ok()) {
        return Some(p);
    }

    // 3. Well-known installation locations
    #[cfg(target_os = "windows")]
    {
        let env_candidates: &[(&str, &str)] = &[
            ("ProgramFiles", r"Google\Chrome\Application\chrome.exe"),
            ("ProgramFiles", r"Chromium\Application\chrome.exe"),
            ("ProgramFiles", r"Microsoft\Edge\Application\msedge.exe"),
            ("ProgramFiles(x86)", r"Google\Chrome\Application\chrome.exe"),
            ("ProgramFiles(x86)", r"Chromium\Application\chrome.exe"),
            (
                "ProgramFiles(x86)",
                r"Microsoft\Edge\Application\msedge.exe",
            ),
            ("LocalAppData", r"Google\Chrome\Application\chrome.exe"),
        ];
        for (env, rel) in env_candidates {
            if let Ok(base) = std::env::var(env) {
                let full = std::path::PathBuf::from(base).join(rel);
                if full.exists() {
                    return Some(full);
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        const FIXED: &[&str] = &[
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        ];
        for p in FIXED {
            let pb = std::path::PathBuf::from(p);
            if pb.exists() {
                return Some(pb);
            }
        }
        // ~/Applications
        if let Ok(home) = std::env::var("HOME") {
            let p = std::path::PathBuf::from(&home)
                .join("Applications/Google Chrome.app/Contents/MacOS/Google Chrome");
            if p.exists() {
                return Some(p);
            }
        }
    }

    None
}

/// The bundle names `chrome_on_mounted_image` looks for on a mounted volume.
#[cfg(target_os = "macos")]
const MAC_BROWSER_BUNDLES: &[&str] = &[
    "Google Chrome.app",
    "Google Chrome Canary.app",
    "Chromium.app",
    "Microsoft Edge.app",
];

/// A browser sitting in its mounted `.dmg`, downloaded but never installed.
///
/// This is the common macOS half-install: the disk image is still mounted and
/// the browser gets launched from the installer window, so it *looks* installed
/// while `/Applications` stays empty. We deliberately do not drive it from
/// there — the volume is read-only and quarantined, Gatekeeper's App
/// Translocation gives the bundle a randomised path that changes per launch,
/// and ejecting the image pulls the browser out from under us mid-session. It
/// is worth detecting only so the error can say what to do about it.
#[cfg(target_os = "macos")]
fn chrome_on_mounted_image() -> Option<std::path::PathBuf> {
    for vol in std::fs::read_dir("/Volumes").ok()?.flatten() {
        for bundle in MAC_BROWSER_BUNDLES {
            let p = vol.path().join(bundle);
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

/// Why no browser could be found, in terms of what the user should do next.
fn chrome_missing_message() -> String {
    #[cfg(target_os = "macos")]
    if let Some(dmg) = chrome_on_mounted_image() {
        return format!(
            "{} is on a mounted disk image, not installed. In Finder, drag it \
             from the disk image window into Applications, eject the image, \
             then try again.",
            dmg.display()
        );
    }
    "Chrome/Chromium not found. \
     Install Chrome or set SICOMPASS_CHROME_PATH to the browser executable."
        .to_owned()
}

// ── BrowserSession ───────────────────────────────────────────────────────────
// Owns the Browser handle; chromiumoxide manages the Chrome process lifetime.
// On Windows, also owns the hider-thread stop signal: dropping it shuts the
// thread down (channel disconnects → thread exits its recv_timeout loop).

struct BrowserSession {
    browser: Browser,
    /// Windows only: dropping this signals the window-hider thread to stop.
    #[cfg(target_os = "windows")]
    _hider_stop: std::sync::mpsc::SyncSender<()>,
    /// Windows only: the app window that was foreground before Chrome launched.
    /// `close_browser` hands focus back to it so the screen reader returns to the
    /// sicompass window rather than the off-screen Chrome window.
    #[cfg(target_os = "windows")]
    prev_foreground: isize,
}

// ── Platform-specific browser launch ─────────────────────────────────────────

/// Persistent Chrome profile directory so cookies and logins survive restarts.
/// Lives alongside the app's other config (e.g. `settings.json`) rather than in
/// a temp dir that the OS wipes on reboot. Falls back to a temp dir only if no
/// config home can be resolved.
fn chrome_profile_dir() -> std::path::PathBuf {
    sicompass_sdk::platform::config_home()
        .map(|d| d.join("sicompass").join("chrome-profile"))
        .unwrap_or_else(|| std::env::temp_dir().join("sicompass-chrome"))
}

/// Linux: run Chrome headed on an invisible X11 display when Xvfb is available,
/// and in Chrome's own headless mode when it is not. Either way, no Chrome
/// window ever reaches the user's screen — see `linux_chrome_launch`.
#[cfg(target_os = "linux")]
async fn launch_browser() -> Result<BrowserSession, String> {
    let launch = linux_chrome_launch()?;

    // Persistent profile dir (see chrome_profile_dir) so cookies/logins survive
    // restarts; also clean up any stale SingletonLock from a crashed launch.
    let profile_dir = chrome_profile_dir();
    let _ = std::fs::create_dir_all(&profile_dir);
    let _ = std::fs::remove_file(profile_dir.join("SingletonLock"));

    // `--headless=new` rather than the old headless mode: it is the same
    // browser binary as headed Chrome, so the stealth script still has a real
    // window.chrome and a full DOM to patch.
    let (builder, exe, mode) = match launch {
        LinuxChrome::VirtualDisplay(exe) => (BrowserConfig::builder().with_head(), exe, "xvfb"),
        LinuxChrome::Headless(exe) => (
            BrowserConfig::builder().new_headless_mode(),
            exe,
            "headless",
        ),
    };

    // Keep this Chrome off the session's accessibility bus. Both launch modes
    // need it: headed-on-Xvfb and Chrome's own headless mode each register as
    // an application when an assistive technology is running — see
    // `NO_AT_SPI_BUS`. The wrapper script passes its environment through, so
    // setting it on the spawned process covers the Xvfb path too.
    let config = builder
        .envs(offscreen_chrome_env())
        .arg("--disable-blink-features=AutomationControlled")
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .user_data_dir(&profile_dir)
        .window_size(1920, 1080)
        .chrome_executable(exe)
        .build()
        .map_err(|e| format!("chromium config error: {e}"))?;
    let (browser, mut handler) = Browser::launch(config)
        .await
        .map_err(|e| format!("failed to launch Chrome ({mode}): {e}"))?;
    tokio::spawn(async move { while handler.next().await.is_some() {} });
    Ok(BrowserSession { browser })
}

/// Windows: launch headed Chrome positioned off-screen, with a background
/// thread that moves any newly-visible Chrome windows off-screen every 50 ms.
///
/// We never call ShowWindow(SW_HIDE) — hiding a window sends WM_SHOWWINDOW
/// to Chrome's message loop, driving RenderWidget::SetHidden(), which sets
/// document.visibilityState = "hidden" and kills JS timer resolution.
/// Instead we start Chrome with --window-position=-10000,-10000 and keep the
/// mover thread running to catch any window that Chrome opens after launch.
/// The thread stops automatically when `BrowserSession` is dropped.
#[cfg(target_os = "windows")]
async fn launch_browser() -> Result<BrowserSession, String> {
    let exe = find_chrome_executable().ok_or_else(|| {
        "Chrome/Chromium/Edge not found. \
         Install Chrome or set SICOMPASS_CHROME_PATH to the browser executable."
            .to_owned()
    })?;

    // Persistent profile dir (see chrome_profile_dir) so cookies/logins survive
    // restarts; also clean up any stale SingletonLock from a crashed launch.
    let profile_dir = chrome_profile_dir();
    let _ = std::fs::create_dir_all(&profile_dir);
    let _ = std::fs::remove_file(profile_dir.join("SingletonLock"));

    let config = BrowserConfig::builder()
        .with_head()
        .arg("--disable-blink-features=AutomationControlled")
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        // Start the window far off all monitors so it is never on-screen.
        // Negative coordinates are valid on Windows; the window is "visible"
        // to Chrome (no SW_HIDE) so JS timers and rendering run at full speed.
        .arg("--window-position=-10000,-10000")
        // Belt-and-suspenders: also disable renderer backgrounding in case
        // Chrome ever detects that its window is off all monitors.
        .arg("--disable-backgrounding-occluded-windows")
        .arg("--disable-renderer-backgrounding")
        .arg("--disable-background-timer-throttling")
        .user_data_dir(&profile_dir)
        .window_size(1920, 1080)
        .chrome_executable(&exe)
        .build()
        .map_err(|e| format!("chromium config error: {e}"))?;

    // Snapshot existing windows before launch so we only target new ones.
    let before = win_hide::snapshot_windows();

    // Capture the current foreground window (the sicompass app) before Chrome
    // exists, so `close_browser` can hand focus back to it — Chrome's window
    // grabs the foreground when created, dragging the screen reader off with it.
    let prev_foreground = win_hide::current_foreground();

    // Channel: when stop_tx is dropped (BrowserSession dropped), recv_timeout
    // returns Disconnected and the thread exits cleanly.
    let (stop_tx, stop_rx) = std::sync::mpsc::sync_channel::<()>(0);

    // Background mover: runs for the entire session lifetime.
    // Moves any new Chrome windows off-screen (never hides them), marks them
    // non-activating, and bounces focus back to the app the first time each one
    // appears so the screen reader is never dragged off into Chrome.
    std::thread::spawn(move || {
        use std::sync::mpsc::RecvTimeoutError;
        let mut handled: Vec<isize> = Vec::new();
        loop {
            match stop_rx.recv_timeout(std::time::Duration::from_millis(50)) {
                Err(RecvTimeoutError::Timeout) => {
                    win_hide::hide_new_browser_windows(&before, &mut handled, prev_foreground);
                }
                _ => break, // Disconnected → BrowserSession dropped
            }
        }
    });

    let (browser, mut handler) = Browser::launch(config).await.map_err(|e| {
        format!(
            "failed to launch Chrome at {} — \
         is Chrome installed? (set SICOMPASS_CHROME_PATH to override): {e}",
            exe.display()
        )
    })?;
    tokio::spawn(async move { while handler.next().await.is_some() {} });

    Ok(BrowserSession {
        browser,
        _hider_stop: stop_tx,
        prev_foreground,
    })
}

/// macOS / other: Chrome in its own headless mode, so no window ever reaches
/// the user's screen.
///
/// Headed Chrome here put a real window on screen and made it frontmost, which
/// is the one thing this app cannot do: the screen reader follows the focus out
/// of sicompass and the user's arrow keys go silent. The other two platforms
/// each dodge that a different way — Linux runs headed Chrome on an Xvfb
/// display, Windows parks the window at -10000,-10000 and hands focus back —
/// and neither is available here. macOS has no virtual display, and
/// `--window-position` does not help because launching still activates the app
/// and puts it in the Dock, focus and all.
///
/// So this takes the same fallback Linux takes when Xvfb is missing, which is
/// what most desktop Linux users already run. `--headless=new` rather than the
/// old headless mode: it is the same browser binary as headed Chrome, so the
/// stealth script still has a real `window.chrome` and a full DOM to patch.
/// The cost is that a few bot-detecting sites are more likely to challenge us;
/// that is a page that asks for a click, whereas stealing focus from a
/// screen-reader user breaks the whole app.
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
async fn launch_browser() -> Result<BrowserSession, String> {
    let exe = find_chrome_executable().ok_or_else(chrome_missing_message)?;
    // Persistent profile dir so cookies/logins survive restarts.
    let profile_dir = chrome_profile_dir();
    let _ = std::fs::create_dir_all(&profile_dir);
    let _ = std::fs::remove_file(profile_dir.join("SingletonLock"));
    let config = BrowserConfig::builder()
        .new_headless_mode()
        .arg("--disable-blink-features=AutomationControlled")
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .user_data_dir(&profile_dir)
        .window_size(1920, 1080)
        .chrome_executable(&exe)
        .build()
        .map_err(|e| format!("chromium config error: {e}"))?;
    let (browser, mut handler) = Browser::launch(config).await.map_err(|e| {
        format!(
            "failed to launch Chrome at {} — \
             is Chrome installed? (set SICOMPASS_CHROME_PATH to override): {e}",
            exe.display()
        )
    })?;
    tokio::spawn(async move { while handler.next().await.is_some() {} });
    Ok(BrowserSession { browser })
}

// Full stealth script injected before every page load.
// Based on puppeteer-extra-plugin-stealth patches — hides headless Chrome from
// Cloudflare's bot-detection fingerprinting.
const STEALTH_SCRIPT: &str = r#"
// ── 1. navigator.webdriver ───────────────────────────────────────────────────
// The primary signal Cloudflare checks. Remove it entirely.
Object.defineProperty(navigator, 'webdriver', { get: () => undefined });

// ── 2. navigator.plugins ─────────────────────────────────────────────────────
// Headless Chrome has zero plugins; real Chrome has several.
const _makePlugin = (name, desc, filename, mimeTypes) => {
    const plugin = Object.create(Plugin.prototype);
    Object.defineProperties(plugin, {
        name: { value: name }, description: { value: desc },
        filename: { value: filename }, length: { value: mimeTypes.length },
    });
    mimeTypes.forEach((mt, i) => { plugin[i] = mt; });
    return plugin;
};
const _pdf = Object.create(MimeType.prototype);
Object.defineProperties(_pdf, {
    type: { value: 'application/pdf' }, suffixes: { value: 'pdf' },
    description: { value: 'Portable Document Format' },
});
const _plugins = [
    _makePlugin('PDF Viewer', 'Portable Document Format', 'internal-pdf-viewer', [_pdf]),
    _makePlugin('Chrome PDF Viewer', 'Portable Document Format', 'internal-pdf-viewer', [_pdf]),
    _makePlugin('Chromium PDF Viewer', 'Portable Document Format', 'internal-pdf-viewer', [_pdf]),
    _makePlugin('Microsoft Edge PDF Viewer', 'Portable Document Format', 'internal-pdf-viewer', [_pdf]),
    _makePlugin('WebKit built-in PDF', 'Portable Document Format', 'internal-pdf-viewer', [_pdf]),
];
Object.defineProperty(navigator, 'plugins', {
    get: () => {
        const arr = [..._plugins];
        Object.setPrototypeOf(arr, PluginArray.prototype);
        arr.item = (i) => arr[i]; arr.namedItem = (n) => arr.find(p => p.name === n);
        arr.refresh = () => {};
        return arr;
    }
});
Object.defineProperty(navigator, 'mimeTypes', {
    get: () => {
        const arr = [_pdf];
        Object.setPrototypeOf(arr, MimeTypeArray.prototype);
        arr.item = (i) => arr[i]; arr.namedItem = (n) => arr.find(m => m.type === n);
        return arr;
    }
});

// ── 3. navigator.vendor + languages ──────────────────────────────────────────
Object.defineProperty(navigator, 'vendor', { get: () => 'Google Inc.' });
Object.defineProperty(navigator, 'languages', { get: () => ['nl-BE', 'nl', 'en-US', 'en'] });

// ── 4. window.chrome ─────────────────────────────────────────────────────────
// Real Chrome exposes window.chrome with loadTimes, csi, etc.
if (!window.chrome) {
    window.chrome = {
        app: {
            isInstalled: false,
            InstallState: { DISABLED: 'disabled', INSTALLED: 'installed', NOT_INSTALLED: 'not_installed' },
            RunningState: { CANNOT_RUN: 'cannot_run', READY_TO_RUN: 'ready_to_run', RUNNING: 'running' },
            getDetails: () => null, getIsInstalled: () => false,
            installState: () => 'not_installed',
        },
        runtime: {
            OnInstalledReason: { CHROME_UPDATE: 'chrome_update', INSTALL: 'install', SHARED_MODULE_UPDATE: 'shared_module_update', UPDATE: 'update' },
            OnRestartRequiredReason: { APP_UPDATE: 'app_update', GC_PRESSURE: 'gc_pressure', OS_UPDATE: 'os_update' },
            PlatformArch: { ARM: 'arm', ARM64: 'arm64', MIPS: 'mips', MIPS64: 'mips64', X86_32: 'x86-32', X86_64: 'x86-64' },
            PlatformNaclArch: { ARM: 'arm', MIPS: 'mips', MIPS64: 'mips64', X86_32: 'x86-32', X86_64: 'x86-64' },
            PlatformOs: { ANDROID: 'android', CROS: 'cros', LINUX: 'linux', MAC: 'mac', OPENBSD: 'openbsd', WIN: 'win' },
            RequestUpdateCheckStatus: { NO_UPDATE: 'no_update', THROTTLED: 'throttled', UPDATE_AVAILABLE: 'update_available' },
            connect: () => {}, sendMessage: () => {},
        },
        loadTimes: () => ({
            requestTime: performance.timing.navigationStart / 1000,
            startLoadTime: performance.timing.navigationStart / 1000,
            commitLoadTime: performance.timing.responseStart / 1000,
            finishDocumentLoadTime: performance.timing.domContentLoadedEventEnd / 1000,
            finishLoadTime: performance.timing.loadEventEnd / 1000,
            firstPaintTime: 0, firstPaintAfterLoadTime: 0,
            navigationType: 'Other', wasFetchedViaSpdy: false, wasNpnNegotiated: false,
            npnNegotiatedProtocol: 'unknown', wasAlternateProtocolAvailable: false,
            connectionInfo: 'http/1.1',
        }),
        csi: () => ({
            startE: performance.timing.navigationStart,
            onloadT: performance.timing.loadEventEnd,
            pageT: performance.now(), tran: 15,
        }),
    };
}

// ── 5. permissions.query ──────────────────────────────────────────────────────
const _origQuery = navigator.permissions.query.bind(navigator.permissions);
navigator.permissions.query = (p) =>
    p.name === 'notifications'
        ? Promise.resolve({ state: Notification.permission })
        : _origQuery(p);

// ── 6. iframe contentWindow ───────────────────────────────────────────────────
// Headless iframes expose webdriver in their contentWindow; patch them too.
const _origGet = Object.getOwnPropertyDescriptor(HTMLIFrameElement.prototype, 'contentWindow').get;
Object.defineProperty(HTMLIFrameElement.prototype, 'contentWindow', {
    get() {
        const cw = _origGet.call(this);
        if (cw) {
            try { Object.defineProperty(cw.navigator, 'webdriver', { get: () => undefined }); } catch(_) {}
        }
        return cw;
    }
});

// ── 7. Screen dimensions ──────────────────────────────────────────────────────
// Headless Chrome reports tiny/zero outerHeight. Real browsers have chrome UI.
if (window.outerHeight === 0) {
    Object.defineProperty(window, 'outerHeight', { get: () => window.innerHeight + 74 });
    Object.defineProperty(window, 'outerWidth',  { get: () => window.innerWidth });
}

// ── 8. Page visibility ────────────────────────────────────────────────────────
// On Windows the browser window is hidden via ShowWindow(SW_HIDE), which can
// cause Chrome to report visibilityState = "hidden".  Consent-page React apps
// (e.g. DPG Media / myprivacy.dpgmedia.be) check this before rendering; if the
// page appears hidden they skip hydration entirely, leaving no accept button in
// the DOM.  Always report the page as visible.
try {
    Object.defineProperty(document, 'visibilityState', { get: () => 'visible' });
    Object.defineProperty(document, 'hidden',          { get: () => false });
} catch(_) {}
"#;

// ---------------------------------------------------------------------------
// Interstitials (bot checks, CAPTCHAs, consent walls)
//
// A CAPTCHA page is still a page.  The loader never throws its HTML away: it
// renders like any other page so the user can read what the site says, and
// operate whatever it offers (a checkbox, a "continue" button, a link to the
// site's help page).  All the loader adds is a leading line naming what it
// recognised, so the user is not left guessing why an article turned into
// three lines of legal text.
// ---------------------------------------------------------------------------

/// Lowercased HTML markers that identify a specific bot-check vendor, with the
/// name to put in the notice. Matched against the raw HTML, which is where the
/// vendor's own assets and error codes live even when the visible text is bare.
const BOT_WALL_MARKERS: &[(&str, &str)] = &[
    ("sorry, you have been blocked", "Cloudflare"),
    ("cf-error-1010", "Cloudflare"),
    ("cf-error-1020", "Cloudflare"),
    ("attention required! | cloudflare", "Cloudflare"),
    ("checking your browser before accessing", "Cloudflare"),
    ("/cdn-cgi/challenge-platform/", "Cloudflare"),
    ("captcha-delivery.com", "DataDome"),
    ("geo.captcha-delivery", "DataDome"),
    ("_incapsula_resource", "Imperva"),
    ("incapsula incident id", "Imperva"),
    ("pardon our interruption", "Imperva"),
    ("px-captcha", "PerimeterX"),
    ("_px_captcha", "PerimeterX"),
    ("akam-sw.js", "Akamai"),
];

/// Generic CAPTCHA widgets. These also sit on ordinary pages (a login form with
/// a reCAPTCHA box), so they only count as an interstitial when the page has
/// almost nothing else on it.
const CAPTCHA_MARKERS: &[&str] = &["hcaptcha", "recaptcha", "turnstile", "captcha"];

/// Below this much visible text a page carrying a CAPTCHA widget is the
/// challenge itself rather than a page that happens to contain one.
const INTERSTITIAL_TEXT_LIMIT: usize = 800;

/// Total visible text in a rendered page, used to tell a challenge page apart
/// from a real page with a CAPTCHA widget somewhere on it.
fn ffon_text_len(elements: &[FfonElement]) -> usize {
    elements
        .iter()
        .map(|e| match e {
            FfonElement::Str(s) => s.chars().count(),
            FfonElement::Obj(o) => o.key.chars().count() + ffon_text_len(&o.children),
        })
        .sum()
}

/// A one-line notice when `html` is a bot check or CAPTCHA rather than the page
/// the user asked for, or `None` when it looks like a normal page.
fn challenge_notice(html: &str, elements: &[FfonElement]) -> Option<String> {
    let haystack = html.to_lowercase();
    if let Some((_, vendor)) = BOT_WALL_MARKERS.iter().find(|(m, _)| haystack.contains(m)) {
        return Some(format!(
            "Bot check ({vendor}). The page it returned is shown below."
        ));
    }
    if ffon_text_len(elements) < INTERSTITIAL_TEXT_LIMIT
        && CAPTCHA_MARKERS.iter().any(|m| haystack.contains(m))
    {
        return Some(
            "This looks like a CAPTCHA or bot check rather than the page itself. \
             Its own content is shown below."
                .to_owned(),
        );
    }
    None
}

/// A page as the loader got it: the rendered HTML, plus anything the load flow
/// itself learned along the way (e.g. a consent wall that would not accept).
/// The HTML is always carried, never replaced by the notice.
struct PageLoad {
    html: String,
    notices: Vec<String>,
}

impl PageLoad {
    fn plain(html: String) -> Self {
        PageLoad {
            html,
            notices: Vec::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// Declared language versions
// ---------------------------------------------------------------------------

/// Autonyms for the language codes a European site is likely to declare — the
/// name each language uses for itself, which is what a switcher shows.
///
/// This is a table of language codes, not a table of sites: it says nothing
/// about who is being read, and a code missing from it still lists (as the bare
/// code), it just reads less kindly.
const LANGUAGE_NAMES: &[(&str, &str)] = &[
    ("bg", "Български"),
    ("cs", "Čeština"),
    ("da", "Dansk"),
    ("de", "Deutsch"),
    ("el", "Ελληνικά"),
    ("en", "English"),
    ("es", "Español"),
    ("et", "Eesti"),
    ("fi", "Suomi"),
    ("fr", "Français"),
    ("ga", "Gaeilge"),
    ("hr", "Hrvatski"),
    ("hu", "Magyar"),
    ("is", "Íslenska"),
    ("it", "Italiano"),
    ("lb", "Lëtzebuergesch"),
    ("lt", "Lietuvių"),
    ("lv", "Latviešu"),
    ("mt", "Malti"),
    ("nl", "Nederlands"),
    ("no", "Norsk"),
    ("pl", "Polski"),
    ("pt", "Português"),
    ("ro", "Română"),
    ("ru", "Русский"),
    ("sk", "Slovenčina"),
    ("sl", "Slovenščina"),
    ("sv", "Svenska"),
    ("tr", "Türkçe"),
    ("uk", "Українська"),
];

/// The primary subtag, lowercased: `nl-BE` and `nl` are the same language.
///
/// Region matters to the site, not to a reader choosing a language, and a site
/// that declares both `nl` and `nl-BE` is offering one choice, not two.
fn primary_subtag(code: &str) -> String {
    code.trim()
        .split(['-', '_'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

/// What to call a language code: its autonym, or the bare code upper-cased.
fn language_label(code: &str) -> String {
    LANGUAGE_NAMES
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, name)| (*name).to_owned())
        .unwrap_or_else(|| code.to_ascii_uppercase())
}

/// The language versions this page declares for itself, as `(label, url)`, in
/// the order the page declares them.
///
/// The source is `<link rel="alternate" hreflang="…">` in `<head>`, which is
/// the standard, machine-readable way a site says "the same page, in that
/// language", and which reaches us with a real href. It is deliberately *not*
/// read out of the body: `hreflang` on an ordinary content link only annotates
/// what that link points at, and reading those is what made the old language
/// step offer elevenways.be's contact page as its Dutch entry.
///
/// `<head>` never reaches FFON on its own — the SDK walker skips it whole — so
/// none of this is otherwise readable, however the page is laid out. That is
/// what makes it the answer for a site like bpost, whose only in-page switcher
/// is an `aria-hidden` modal of href-less anchors that nothing can press.
///
/// Empty unless the result is a real choice (two or more languages), so an
/// ordinary monolingual page grows nothing.
fn declared_languages(html: &str, url: &str) -> Vec<(String, String)> {
    // Reachable from the standalone `fetch_url_to_ffon` bridge as well as from
    // a live provider, so it cannot assume the provider registered the bundles.
    // Idempotent.
    register_translations();
    let doc = scraper::Html::parse_document(html);

    let Ok(sel) = scraper::Selector::parse("link[rel][hreflang][href]") else {
        return Vec::new();
    };

    // Insertion-ordered: the site's own ordering is meaningful and is kept.
    let mut codes: Vec<String> = Vec::new();
    let mut hrefs: Vec<String> = Vec::new();
    for el in doc.select(&sel) {
        // `rel` is a space-separated set, so match the word rather than the
        // value. `alternate stylesheet` is the one set that carries the word
        // without meaning a translation: it is the alternate-stylesheet idiom,
        // and an `hreflang` on it describes the sheet, not a version of the
        // page.
        let rel = el.value().attr("rel").unwrap_or("");
        let mut words = rel.split_whitespace().map(str::to_ascii_lowercase);
        let mut alternate = false;
        let mut stylesheet = false;
        for w in &mut words {
            alternate |= w == "alternate";
            stylesheet |= w == "stylesheet";
        }
        if !alternate || stylesheet {
            continue;
        }
        let code = primary_subtag(el.value().attr("hreflang").unwrap_or(""));
        // `x-default` is the fallback for a language nobody asked for. It is a
        // routing hint, not a language, and it has no name to show.
        if code.is_empty() || code == "x" {
            continue;
        }
        let href = html_resolve_href(el.value().attr("href").unwrap_or(""), url);
        if href.is_empty() || codes.contains(&code) {
            continue;
        }
        codes.push(code);
        hrefs.push(href);
    }

    // The language you are already reading is routinely missing from the
    // alternates — anysurfer.be, in Dutch, declares only English and French.
    // Left as-is that is a list with no way to stay where you are, which is
    // the complaint that sank the old language step. The page says which one
    // it is in `<html lang>`, and it is the address already loaded.
    let current = scraper::Selector::parse("html")
        .ok()
        .and_then(|s| doc.select(&s).next())
        .and_then(|h| h.value().attr("lang"))
        .map(primary_subtag)
        .unwrap_or_default();
    if !current.is_empty() && current != "x" && !codes.contains(&current) {
        codes.push(current.clone());
        hrefs.push(url.to_owned());
    }

    if codes.len() < 2 {
        return Vec::new();
    }

    codes
        .into_iter()
        .zip(hrefs)
        .map(|(code, href)| {
            let mut label = language_label(&code);
            if code == current {
                label.push_str(&format!(
                    " ({})",
                    localize::t("webbrowser-language-current")
                ));
            }
            (label, href)
        })
        .collect()
}

/// Render a loaded page to FFON, with every notice as a leading line: the ones
/// the load flow recorded, then whatever the content itself gives away, and the
/// page's own language versions as a trailing section.
fn page_to_ffon_with_forms(load: &PageLoad, url: &str) -> (Vec<FfonElement>, FormMap) {
    let (mut elements, form_map) = html_to_ffon_with_forms(&load.html, url);
    let notices: Vec<String> = load
        .notices
        .iter()
        .cloned()
        .chain(challenge_notice(&load.html, &elements))
        .collect();
    for (i, notice) in notices.into_iter().enumerate() {
        elements.insert(i, FfonElement::new_str(notice));
    }

    // Last, so the page is read before its language switcher rather than after
    // it — bpost's front page alone has nineteen top-level entries, and a
    // reader who wants this knows to go to the end for it.  A question is
    // still a question: `gate_page_html` builds a bare document with no
    // `<head>` and no `lang`, so a cookie step declares nothing and grows
    // nothing, with no special case needed here.
    let languages = declared_languages(&load.html, url);
    if !languages.is_empty() {
        let mut section = FfonElement::new_obj(localize::t("webbrowser-languages"));
        if let Some(obj) = section.as_obj_mut() {
            for (label, href) in languages {
                obj.push(FfonElement::new_obj(format!("{label} <link>{href}</link>")));
            }
        }
        elements.push(section);
    }

    (elements, form_map)
}

/// Fetch a URL via Chromium, parse the HTML, and return as FFON elements.
/// Used by the main app's `fetch_url_to_elements` bridge.
pub fn fetch_url_to_ffon(url: &str) -> Vec<FfonElement> {
    if test_no_launch() {
        return vec![FfonElement::new_str(format!(
            "<test-no-launch>{url}</test-no-launch>"
        ))];
    }
    match fetch_html_chromium(url) {
        Ok(load) => page_to_ffon_with_forms(&load, url).0,
        Err(e) => vec![FfonElement::new_str(format!("Error loading {url}: {e}"))],
    }
}

fn fetch_html_chromium(url: &str) -> Result<PageLoad, String> {
    chromium_runtime().block_on(async move {
        tokio::time::timeout(tokio::time::Duration::from_secs(60), fetch_html_inner(url))
            .await
            .unwrap_or_else(|_| Err(format!("timed out loading {url} (60 s)")))
    })
}

async fn fetch_html_inner(url: &str) -> Result<PageLoad, String> {
    // Launch a fresh Chrome process for this fetch.
    let session = launch_browser().await?;

    // Do the actual page fetch.  Keeping result separate lets us close Chrome
    // gracefully on both success and error paths before dropping the session.
    let result = fetch_page(&session, url).await;

    // Send Browser.close so Chrome exits cleanly (WebSocket close handshake
    // completes) instead of being killed abruptly (which logs a spurious
    // "ConnectionReset" error from the chromiumoxide handler task).
    use chromiumoxide::cdp::browser_protocol::browser::CloseParams;
    let _ = tokio::time::timeout(
        tokio::time::Duration::from_millis(500),
        session.browser.execute(CloseParams::default()),
    )
    .await;

    // session dropped here; hider thread (Windows) and browser process stop.
    result
}

/// Open a tab, navigate to `url`, and return the rendered HTML.
/// Called by `fetch_html_inner` which handles Chrome lifecycle around it.
async fn fetch_page(session: &BrowserSession, url: &str) -> Result<PageLoad, String> {
    let t = tokio::time::Duration::from_secs;

    let page = tokio::time::timeout(t(15), session.browser.new_page("about:blank"))
        .await
        .map_err(|_| "Chrome took >15 s to open a tab".to_owned())?
        .map_err(|e| format!("failed to open tab: {e}"))?;

    tokio::time::timeout(
        t(10),
        page.execute(AddScriptToEvaluateOnNewDocumentParams::new(STEALTH_SCRIPT)),
    )
    .await
    .map_err(|_| "stealth script injection timed out".to_owned())?
    .map_err(|e| format!("stealth script injection failed: {e}"))?;
    set_desktop_viewport(&page).await;

    tokio::time::timeout(t(30), page.goto(url))
        .await
        .map_err(|_| format!("navigation to {url} timed out after 30 s"))?
        .map_err(|e| format!("navigation to {url} failed: {e}"))?;

    // Poll until the URL stabilises or a consent-wall URL is detected (up to 5 s).
    // On Windows the JS redirect from the original page to the consent wall can
    // fire well after Chrome's load event, so a single wait_for_navigation call
    // (which may return before the redirect) is not reliable cross-platform.
    let current_url = await_stable_url(&page, tokio::time::Duration::from_secs(5)).await;

    // Read the page once up front so inline consent walls (Google/YouTube serve
    // theirs on a normal URL) are visible via the content, not just the URL.
    let html = settled_html(&page).await?;

    // Surface any consent choice the page is showing, then hand the page over.
    let (load, surfaced) = settle_gates(&page, &current_url, html).await;
    if !surfaced && (is_consent_wall_str(&current_url) || html_has_consent_wall(&load.html)) {
        // Capture a snippet to diagnose why no choice could be lifted out.
        let snippet: String = load.html.chars().take(2000).collect();
        eprintln!("=== consent-wall debug ===");
        eprintln!("Consent URL : {current_url}");
        eprintln!("Page snippet:\n{snippet}");
        eprintln!("=== end consent-wall debug ===");
    }

    let _ = tokio::time::timeout(t(3), page.close()).await;

    Ok(load)
}

// ---------------------------------------------------------------------------
// Hidden-content pruning
// ---------------------------------------------------------------------------

/// The viewport every page is rendered at.
///
/// Responsive sites pick their layout from this, so it decides whether the
/// reader gets the desktop page or the phone one. Wide enough to clear the
/// usual `xl` breakpoint (1200px) with room to spare.
const VIEWPORT_W: u32 = 1920;
const VIEWPORT_H: u32 = 1080;

/// Force a desktop layout viewport, whatever the window or X screen happens to
/// be. Belt and braces next to `--window-size`: headless defaults to 800x600,
/// and under Xvfb the window is clamped to the virtual screen.
async fn set_desktop_viewport(page: &chromiumoxide::Page) {
    use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
    let params = SetDeviceMetricsOverrideParams::builder()
        .width(VIEWPORT_W as i64)
        .height(VIEWPORT_H as i64)
        .device_scale_factor(1.0)
        .mobile(false)
        .build();
    if let Ok(params) = params {
        let _ =
            tokio::time::timeout(tokio::time::Duration::from_secs(5), page.execute(params)).await;
    }
}

/// Serialise the document with every invisible subtree removed.
///
/// Runs in the page so it can use `getComputedStyle`, i.e. the layout Chrome
/// has already done.  An element is dropped when *any* of these hold:
///
///   * it has the `hidden` attribute,
///   * it has `aria-hidden="true"` — this app *is* an assistive technology, so
///     honouring that is correctness, not a heuristic,
///   * its computed `display` is `none`,
///   * its computed `visibility` is `hidden` or `collapse`.
///
/// Deliberately *not* size- or position-based: `.sr-only` / `.visually-hidden`
/// text is a clipped 1x1 box but stays `display:block; visibility:visible`, and
/// that text is exactly what a screen-reader-first browser should be reading.
///
/// The live DOM is not destroyed — the form-submit paths click against it after
/// this runs.  Hidden nodes are marked, a clone is pruned, and the markers are
/// stripped again, so the only lasting change is nothing at all.
/// Constants, the visibility verdict, and the geometry reader — everything the
/// pruning body and the banding pass both need.
///
/// The script is assembled from four consts into [`PRUNE_HIDDEN_SCRIPT`]:
/// head, [`PRUNE_JS_BODY`], [`BAND_JS`], [`PRUNE_JS_END`].  The seams exist so
/// `band_plan_js` can evaluate head + band alone and read the banding
/// decisions back as JSON, which is the only way to assert on them: the
/// algorithm is browser-side JS and there is no JS engine in the dependency
/// tree, so it is unreachable from a pure Rust test.
///
/// Assembled by concatenation, never `format!`: the JS is full of braces and
/// escaping every one of them would be a needless error surface.
const PRUNE_JS_HEAD: &str = r##"(function() {
    // Never rendered content in the first place, and FFON drops them anyway
    // (HTML_SKIP_TAGS), so keeping them costs the reader nothing — while the
    // load flow does sniff inline <script> for Google's consent values and for
    // the wall's own marker.  Checked before every other rule: Google stamps
    // aria-hidden="true" on its inline scripts, which would otherwise take the
    // consent values out with them.
    const NEVER_MARK = new Set([
        'SCRIPT','STYLE','NOSCRIPT','TEMPLATE','LINK','META','TITLE','BASE',
    ]);
    // UA stylesheets give these `display:none` (input[type=hidden]) for reasons
    // that have nothing to do with whether the user can see them.  Their
    // ancestors stay eligible, so a parked dialog full of controls still goes.
    const DISPLAY_EXEMPT = new Set(['INPUT','SELECT','OPTION','OPTGROUP','TEXTAREA']);
    const NEVER = new Set(['HTML','HEAD','BODY']);
    const MARK = 'data-sic-hidden';
    const PARK = 'data-sic-parked';
    // Geometry, carried to the clone as "x y w h flags". It has to travel as an
    // attribute: a detached clone is not laid out, so getBoundingClientRect on
    // it returns zeros — the numbers can only come from the live nodes.
    const GEO = 'data-sic-g';
    // Parked: moved to the end of the document, so it must stay there. Nothing
    // display:none has geometry to sort by, and the reading order pass would
    // otherwise pull it back into the middle on the strength of a missing box.
    const PIN = 'data-sic-pin';
    const F_FIXED = 1, F_ABS = 2, F_RTL = 4, F_NOSORT = 8, F_INLINE = 16;
    // Displays whose children are not independently positioned blocks, so
    // sorting them by geometry would scramble a line of text or a table.
    const NO_REFLOW_DISPLAY = new Set([
        'inline','inline-block','inline-flex','inline-grid','contents','list-item',
        'table','table-row','table-cell','table-row-group','table-header-group',
        'table-footer-group','ruby','ruby-text',
    ]);

    // Something the reader could act on: a link, a control, a heading. A
    // subtree holding any of these is the site's own navigation or UI, not
    // leftover markup — bpost keeps its whole main menu in a Bootstrap
    // .navbar-collapse that is display:none even on a 1920px viewport, and
    // dropping it takes "Pakje verzenden" and "Pakje ontvangen" with it.
    // Nothing here can press the hamburger to get them back, so they stay.
    const INTERACTIVE = 'a[href], button, input, select, textarea, ' +
        '[role="button"], [role="link"], [role="menuitem"], [role="tab"], ' +
        'h1, h2, h3, h4, h5, h6';
    function actionable(el) {
        try { return el.matches(INTERACTIVE) || !!el.querySelector(INTERACTIVE); }
        catch (e) { return false; }
    }

    const HIDE = 1, PARK_IT = 2, KEEP = 3;

    // One element's verdict, with no side effects. Keeping this a pure read is
    // what lets the walk below stay a read-only pass: interleaving
    // getComputedStyle with setAttribute, which is what this did when marking
    // was all it had to do, dirties style between reads and forces a fresh
    // layout flush on the next one.
    // The computed style `verdict` just read, so the geometry pass can reuse it
    // instead of paying for a second getComputedStyle. That call is the single
    // most expensive thing in this script; a rect read next to it is noise.
    let lastStyle = null;
    function verdict(el) {
        lastStyle = null;
        if (NEVER_MARK.has(el.tagName)) return KEEP;
        // `hidden` and `aria-hidden` are the author saying "assistive tech must
        // not see this". This app is assistive tech, so it obeys, links or no
        // links — that is what takes bpost's parked language modal out.
        if (el.hasAttribute('hidden')) return HIDE;
        if (el.getAttribute('aria-hidden') === 'true') return HIDE;
        let s;
        try { s = getComputedStyle(el); } catch (e) { return KEEP; }
        if (!s) return KEEP;
        lastStyle = s;
        const invisible = s.visibility === 'hidden' || s.visibility === 'collapse' ||
            (s.display === 'none' && !DISPLAY_EXEMPT.has(el.tagName));
        // Merely off-screen: dead weight goes, anything you could act on is
        // kept but gets moved out of the way (see PARK below).
        if (!invisible) return KEEP;
        if (actionable(el)) {
            // Parking is for menus. A subtree carrying the page's <h1> is the
            // content itself — bpost wraps its article in a container classed
            // `hide_section`, and moving that to the end would file the whole
            // page after the navigation. (Text length is no use as a signal:
            // textContent counts inline scripts, and innerText is empty for
            // anything not rendered.)
            let isContent = true;
            try { isContent = el.tagName === 'H1' || !!el.querySelector('h1'); } catch (e) {}
            return isContent ? KEEP : PARK_IT;
        }
        return HIDE;
    }

    // A wrapper that is `display:contents`, floats-only or a zero-height flex
    // parent measures 0x0 while its children are perfectly well placed. Take
    // the union of what is inside instead. Capped hard: this is a repair for a
    // handful of nodes, not a second traversal of the document.
    function unionOfDescendants(el) {
        let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity, seen = 0;
        const queue = [el];
        while (queue.length && seen < 32) {
            const node = queue.shift();
            for (const kid of node.children) {
                seen++;
                if (seen > 32) break;
                let r;
                try { r = kid.getBoundingClientRect(); } catch (e) { continue; }
                if (r.width > 0 && r.height > 0) {
                    x0 = Math.min(x0, r.left); y0 = Math.min(y0, r.top);
                    x1 = Math.max(x1, r.right); y1 = Math.max(y1, r.bottom);
                } else {
                    queue.push(kid);
                }
            }
        }
        if (x0 === Infinity) return null;
        return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 };
    }

    // Read the layout Chrome has already done. `s` is the style `verdict` just
    // computed for this element, so this adds a rect read and nothing else.
    function measure(el, s) {
        let r;
        try { r = el.getBoundingClientRect(); } catch (e) { return null; }
        let x = r.left, y = r.top, w = r.width, h = r.height;
        if (w === 0 || h === 0) {
            const u = unionOfDescendants(el);
            if (u) { x = u.x; y = u.y; w = u.w; h = u.h; }
        }
        let f = 0;
        if (s) {
            if (s.position === 'fixed') f |= F_FIXED;
            // `sticky` is deliberately absent: nothing scrolls this page, so at
            // offset 0 a sticky element sits at its honest in-flow position.
            else if (s.position === 'absolute') f |= F_ABS;
            if (s.direction === 'rtl') f |= F_RTL;
            // Multi-column text flows down one column then down the next, so a
            // band-then-x sort would interleave the columns into nonsense.
            if (s.columnCount !== 'auto' || s.columnWidth !== 'normal') f |= F_NOSORT;
            if (NO_REFLOW_DISPLAY.has(s.display)) f |= F_INLINE;
        }
        // scrollX/scrollY are 0 (nothing has scrolled) but cost nothing and make
        // the numbers document-absolute rather than viewport-relative.
        return [Math.round(x + scrollX), Math.round(y + scrollY),
                Math.round(w), Math.round(h), f];
    }

"##;

/// The pruning and serialization body.  Split from [`PRUNE_JS_HEAD`] only so
/// the head's constants and `measure` can be shared with the band-plan test
/// seam without a second copy of the geometry reader.
const PRUNE_JS_BODY: &str = r##"
    // ---- read pass: no DOM writes at all ----
    const toMark = [], toPark = [], geo = new Map();
    // Top-down walk: once a subtree is marked its descendants go with it, so
    // skip them rather than paying getComputedStyle on every one.  A parked
    // subtree is still walked into, because PARK only moves the outermost one
    // and its descendants each get their own verdict — but `live` goes false,
    // because everything under a display:none node measures 0x0 and there is no
    // point paying for the rect.
    (function walk(el, live) {
        for (const child of el.children) {
            if (NEVER.has(child.tagName)) {
                // <body> is never marked, but it still needs a box: the banding
                // pass measures the wrapper chain under it against its area to
                // find where the page's regions actually live.
                if (live && child.tagName === 'BODY') {
                    let s = null;
                    try { s = getComputedStyle(child); } catch (e) {}
                    const g = measure(child, s);
                    if (g) geo.set(child, g);
                }
                walk(child, live);
                continue;
            }
            const v = verdict(child);
            if (v === HIDE) { toMark.push(child); continue; }
            if (v === PARK_IT) { toPark.push(child); walk(child, false); continue; }
            if (live) {
                const g = measure(child, lastStyle);
                if (g) geo.set(child, g);
            }
            walk(child, live);
        }
    })(document.documentElement, true);

    // ---- write pass ----
    const marked = toMark;
    for (const el of toMark) el.setAttribute(MARK, '');
    for (const el of toPark) el.setAttribute(PARK, '');
    for (const [el, g] of geo) el.setAttribute(GEO, g.join(' '));

    let html;
    try {
        const clone = document.documentElement.cloneNode(true);
        // A consent banner whose buttons were lifted into the proxy form goes
        // too, whether or not the vendor bothered to hide the original.
        for (const node of clone.querySelectorAll('[data-sic-consent-src]')) node.setAttribute(MARK, '');
        for (const node of clone.querySelectorAll('[' + MARK + ']')) {
            // Dropping a <form> outright would renumber every later form, while
            // the click path resolves buttons through `document.forms[n]` on the
            // *live* page. Leave an empty shell instead: FFON still counts it,
            // and an empty form emits no node, so it stays invisible to read.
            const forms = (node.tagName === 'FORM' ? [node] : [])
                .concat(Array.from(node.querySelectorAll('form')));
            if (!forms.length) { node.remove(); continue; }
            const shells = document.createDocumentFragment();
            for (let i = 0; i < forms.length; i++) shells.appendChild(document.createElement('form'));
            node.replaceWith(shells);
        }
        // A hidden menu is navigation worth keeping, but leaving it inline lets
        // it dominate the reading order: bpost's login dropdown sits before the
        // article and opens with a heading, and FFON nests everything after a
        // heading underneath it — so the whole page ended up filed under
        // "De voordelen van je bpost-account?". Park them after the content.
        const body = clone.querySelector('body');
        if (body) {
            for (const node of clone.querySelectorAll('[' + PARK + ']')) {
                // Only top-level parked nodes; a nested one travels with its parent.
                if (node.parentElement && node.parentElement.closest('[' + PARK + ']')) continue;
                node.removeAttribute(PARK);
                // A parked <form> would have to leave its own shell behind and
                // then be appended detached. It is display:none either way, so
                // leave it exactly where it is.
                if (node.tagName === 'FORM') continue;
                // Moving a subtree to the end of the document renumbers every
                // form after it, and the click path resolves buttons through
                // `document.forms[n]` on the live page. The prune has the same
                // problem and solves it with empty shells; parking never did,
                // because an actionable subtree is not marked hidden and so
                // never reached that code. Leave a shell for each form where it
                // was, and take the real ones out of the copy that travels: a
                // parked form is display:none, so it was never operable.
                const forms = Array.from(node.querySelectorAll('form'));
                if (forms.length && node.parentNode) {
                    const shells = document.createDocumentFragment();
                    for (let i = 0; i < forms.length; i++) shells.appendChild(document.createElement('form'));
                    node.parentNode.insertBefore(shells, node);
                    for (const f of forms) f.remove();
                }
                node.setAttribute(PIN, 'end');
                body.appendChild(node);
            }
            for (const node of clone.querySelectorAll('[' + PARK + ']')) node.removeAttribute(PARK);
        }
        // Group the page into regions and read them in the order they are laid
        // out, from the geometry stamped above.  Its own try/catch on purpose:
        // a throw escaping from here would leave `html` undefined and drop the
        // whole page onto the unpruned `page.content()` fallback.
        try {
            const cbody = clone.querySelector('body');
            if (cbody) {
                sicApply(cbody, sicPlan(cbody));
                sicWrapParked(cbody);
                sicReflow(cbody, 0);
            }
        } catch (e) {}
        rehomeIds(clone);
        // Geometry has done its work; ~30 bytes times every element is 100-200 kB
        // of noise to push through the parser otherwise.
        for (const node of clone.querySelectorAll('[' + GEO + ']')) node.removeAttribute(GEO);
        for (const node of clone.querySelectorAll('[' + PIN + ']')) node.removeAttribute(PIN);
        html = '<!DOCTYPE html>' + clone.outerHTML;
    } finally {
        for (const node of marked) node.removeAttribute(MARK);
        for (const node of document.querySelectorAll('[' + PARK + ']')) node.removeAttribute(PARK);
        // Every write in this script and its matching removal happen inside one
        // synchronous task, so a MutationObserver sees the records but never a
        // dirty DOM — and the submit paths that click the live page after this
        // find it exactly as the site left it.
        for (const node of geo.keys()) node.removeAttribute(GEO);
    }
    return html;

    // An empty anchor is how most pages mark a skip-link target
    // (`<a id="main-content"></a>`), and an element with no content emits no
    // FFON node — so the id, and with it the jump target, is simply lost.
    // Hand it to the next thing that does render. Clone-only: the live DOM
    // keeps its ids, so form selectors and site scripts are untouched.
    function rehomeIds(root) {
        const CONTROL = 'input, select, textarea, button';
        // Only these four are landmarks to FFON (html_landmark_name); <section>
        // and <article> are generic containers whose id falls through to the
        // first child that renders.
        const LANDMARK = 'main, nav, aside, footer';
        // Which ids something on the page actually jumps to. Only those are
        // worth restructuring for; every other stray id is left alone.
        const targeted = new Set();
        for (const a of root.querySelectorAll('a[href^="#"]')) {
            const frag = a.getAttribute('href').slice(1);
            if (frag) targeted.add(frag);
        }
        for (const el of root.querySelectorAll('[id]')) {
            if ((el.textContent || '').trim()) continue;
            try { if (el.querySelector('img, ' + CONTROL)) continue; } catch (e) { continue; }
            let next = el.nextElementSibling;
            while (next && !(next.textContent || '').trim()) next = next.nextElementSibling;
            if (!next || next.id) continue;
            // Never onto a control itself: FFON builds a control's selector from
            // its own id, and that selector is resolved against the *live*
            // page, where this id sits somewhere else. A container that merely
            // holds controls is fine — its id stays on the container.
            try { if (next.matches(CONTROL)) continue; } catch (e) { continue; }

            // A jump target has to *contain* what you jumped to read. Handing
            // the id to a plain container is not enough: FFON passes it down to
            // whichever node renders first, and bpost's content region opens
            // with two <h1>s in a row — so the id landed on an empty heading
            // while the article nested under the next one. Wrapping the region
            // in a landmark gives the link somewhere to arrive that holds the
            // content itself.
            let host = next;
            if (targeted.has(el.id)) {
                let isLandmark = false;
                try { isLandmark = next.matches(LANDMARK); } catch (e) {}
                if (!isLandmark && next.parentNode) {
                    // <main>, because only nav/main/aside/footer are landmarks
                    // to FFON — a <section> is generic, so the id would fall
                    // straight through to the first child that renders, which
                    // is the empty <h1> we are trying to get past.
                    const doc = root.ownerDocument || document;
                    const sec = doc.createElement('main');
                    next.parentNode.insertBefore(sec, next);
                    // Everything from the target onwards belongs to it: that is
                    // what "skip to content" means, and it stops the landmark
                    // being a single-child wrapper that FFON collapses again.
                    // It stops at the next landmark, though — a <footer> after
                    // the target is not part of the main content, and swallowing
                    // one (the site's own, or one the banding pass synthesized)
                    // files the whole end of the page under "main content".
                    let node = sec.nextSibling;
                    while (node) {
                        if (node.nodeType === 1 && node.matches('footer, nav, aside, main')) break;
                        const after = node.nextSibling;
                        sec.appendChild(node);
                        node = after;
                    }
                    // A lone plain container in there is still a wrapper; lift
                    // its children so the landmark holds real structure.
                    unwrapSingleWrapper(sec);
                    host = sec;
                }
            }
            host.id = el.id;
            el.removeAttribute('id');
        }
    }
"##;

/// Region grouping and reading order, from the layout Chrome has already done.
///
/// Function declarations, hoisted into the enclosing IIFE, so this block can be
/// concatenated anywhere inside it.  Two halves, deliberately separated:
/// `sicPlan` is **pure** — it reads geometry and returns a description of what
/// should change — and `sicApply` performs it.  Only that split makes the
/// decisions assertable from a test, by evaluating `sicPlan` alone and reading
/// the plan back as JSON.
const BAND_JS: &str = r##"
    // Stamp geometry over a live tree the way the read pass does, for the band
    // plan test seam only.  The production path fuses this into its own walk so
    // it never pays for a second getComputedStyle; this exists so a test can
    // ask for a plan without running the prune.
    function sicStampLive(root) {
        const stamped = [];
        (function walk(el) {
            for (const child of el.children) {
                if (NEVER_MARK.has(child.tagName)) continue;
                let s = null;
                try { s = getComputedStyle(child); } catch (e) {}
                const g = measure(child, s);
                if (g) { child.setAttribute(GEO, g.join(' ')); stamped.push(child); }
                walk(child);
            }
        })(root);
        return stamped;
    }

    // A landmark holding one plain container is still a wrapper: lift the
    // children so it holds real structure. Shared with `rehomeIds`, which needs
    // exactly the same thing for the <main> it synthesizes.
    function unwrapSingleWrapper(el) {
        while (el.children.length === 1 &&
               /^(DIV|SECTION|ARTICLE)$/.test(el.children[0].tagName)) {
            const only = el.children[0];
            while (only.firstChild) el.insertBefore(only.firstChild, only);
            only.remove();
        }
    }

    const R_NONE = 0, R_NAV = 1, R_MAIN = 2, R_FOOTER = 3, R_ASIDE = 4;
    const ROLE_TAG = ['', 'nav', 'main', 'footer', 'aside'];
    const ROLE_NAME = ['none', 'nav', 'main', 'footer', 'aside'];
    // Pins: an overlay is not in the flow, so it has no honest place in the
    // band order. Send it where it belongs and keep it there.
    const PIN_NONE = 0, PIN_FRONT = -1, PIN_END = 1;

    function sicGeo(el) {
        const a = el.getAttribute(GEO);
        if (!a) return null;
        const p = a.split(' ');
        if (p.length < 5) return null;
        const g = { x: +p[0], y: +p[1], w: +p[2], h: +p[3], f: +p[4] };
        for (const k of ['x','y','w','h','f']) if (!isFinite(g[k])) return null;
        return g;
    }

    function sicKids(el) {
        const out = [];
        for (const c of el.children) if (!NEVER_MARK.has(c.tagName)) out.push(c);
        return out;
    }

    // The author's own markup, when there is any. Nothing here gets wrapped
    // again, and a page with two or more of these is left alone entirely.
    function sicNativeRole(el) {
        switch (el.tagName) {
            case 'NAV': case 'HEADER': return R_NAV;
            case 'MAIN': return R_MAIN;
            case 'FOOTER': return R_FOOTER;
            case 'ASIDE': return R_ASIDE;
        }
        switch ((el.getAttribute('role') || '').toLowerCase()) {
            case 'navigation': case 'banner': return R_NAV;
            case 'main': return R_MAIN;
            case 'contentinfo': return R_FOOTER;
            case 'complementary': return R_ASIDE;
        }
        return null;
    }

    // Real pages are body > div#app > div > … as often as they are a flat run.
    // Descend through wrappers that are just holding the page.
    function sicBandHost(body) {
        let host = body;
        for (let depth = 0; depth < 8; depth++) {
            const kids = sicKids(host);
            if (kids.length !== 1) break;
            const k = kids[0];
            if (!/^(DIV|SECTION|ARTICLE|MAIN)$/.test(k.tagName)) break;
            const hg = sicGeo(host), kg = sicGeo(k);
            if (!hg || !kg || hg.w * hg.h <= 0) break;
            if (kg.w * kg.h < 0.9 * hg.w * hg.h) break;
            host = k;
        }
        return host;
    }

    // Reading order: cluster into horizontal bands, then read each band across.
    // Items whose vertical extents overlap share a band, which keeps a row of
    // unequal-height cards together while a clean vertical gap splits them.
    function sicVisualOrder(items, rtl) {
        const sized = items.filter(it => it.g && it.g.w > 0 && it.g.h > 0 && !it.pin);
        if (!sized.length) return items.slice();
        const byY = sized.slice().sort((a, b) => a.g.y - b.g.y || a.i - b.i);
        let band = 0, bottom = -Infinity;
        for (const it of byY) {
            const tol = Math.min(0.5 * it.g.h, 8);
            if (it.g.y >= bottom - tol) { band++; bottom = it.g.y + it.g.h; }
            else bottom = Math.max(bottom, it.g.y + it.g.h);
            it.band = band;
        }
        // Anything with no box of its own — screen-reader-only text, an
        // absolutely positioned badge, an overlay staying put — rides along
        // with the last sized sibling before it rather than being flung to an
        // end. Its source position relative to that sibling is preserved.
        let carry = null;
        for (const it of items) {
            if (it.band !== undefined) { carry = it; continue; }
            const ref = carry || byY[0];
            it.band = ref.band;
            it.carryX = ref.g.x;
            it.carryR = ref.g.x + ref.g.w;
        }
        const keyX = it => (it.g && it.g.w > 0 && !it.pin)
            ? (rtl ? -(it.g.x + it.g.w) : it.g.x)
            : (rtl ? -it.carryR : it.carryX);
        const out = items.slice();
        out.sort((a, b) =>
            (a.pin - b.pin) || (a.band - b.band) || (keyX(a) - keyX(b)) || (a.i - b.i));
        return out;
    }

    // Boxes that overlap by most of the smaller one are not laid out relative to
    // each other; keep whatever order the markup gave them.
    function sicOverlaps(a, b) {
        if (!a || !b || a.w <= 0 || b.w <= 0) return false;
        const ox = Math.min(a.x + a.w, b.x + b.w) - Math.max(a.x, b.x);
        const oy = Math.min(a.y + a.h, b.y + b.h) - Math.max(a.y, b.y);
        if (ox <= 0 || oy <= 0) return false;
        return ox * oy >= 0.6 * Math.min(a.w * a.h, b.w * b.h);
    }

    function sicProbe(el) {
        let links = 0, textLen = 0, hasH1 = false;
        try { links = el.querySelectorAll('a[href], [role="link"]').length; } catch (e) {}
        try { textLen = (el.textContent || '').trim().length; } catch (e) {}
        try { hasH1 = el.tagName === 'H1' || !!el.querySelector('h1'); } catch (e) {}
        return { links, textLen, hasH1,
                 linkDense: links >= 4 && textLen / links < 80 };
    }

    const NAV_HINT = /header|masthead|topbar|navbar|menu/i;
    const FOOT_HINT = /footer|colophon|legal/i;
    function sicHint(el, re) {
        return re.test(el.tagName) || re.test(el.className || '') || re.test(el.id || '');
    }

    // Decide what the page's regions are and in what order they read.  Pure: it
    // returns a description, and `sicApply` performs it.  That split is what
    // makes the decisions assertable from a test.
    function sicPlan(body) {
        const plan = { host: null, order: null, groups: [], items: [], declined: null };
        if (!body) { plan.declined = 'no body'; return plan; }
        // A consent gate is in flight. The page it becomes is built in Rust
        // from labels, and its proxy forms must keep document order.
        if (body.querySelector('button[id^="sic-consent-"]')) {
            plan.declined = 'gate in flight';
            return plan;
        }
        // On a page this size the rect reads start to cost real time and the
        // signal is poor anyway. Read it the way it was written.
        let total = 0;
        try { total = document.querySelectorAll('*').length; } catch (e) {}
        if (total > 25000) { plan.declined = 'document too large'; return plan; }
        const host = sicBandHost(body);
        plan.host = host.tagName.toLowerCase() + (host.id ? '#' + host.id : '');
        const kids = sicKids(host);
        if (kids.length < 3) { plan.declined = 'too few regions'; return plan; }
        if (kids.length > 200) { plan.declined = 'too many regions'; return plan; }

        const vw = window.innerWidth || 1920;
        const vh = window.innerHeight || 1080;
        let DH = vh;
        try { DH = Math.max(document.documentElement.scrollHeight, vh); } catch (e) {}

        let rtl = false;
        const hg = sicGeo(host);
        if (hg && (hg.f & F_RTL)) rtl = true;

        const items = [];
        let missing = 0;
        for (let i = 0; i < kids.length; i++) {
            const el = kids[i], g = sicGeo(el);
            const parked = el.getAttribute(PIN) === 'end';
            // A parked menu is display:none, so it has no box by definition.
            // That is not missing geometry, it is a region already placed.
            if (!parked && (!g || g.w <= 0 || g.h <= 0)) missing++;
            const it = { i, el, g, role: R_NONE, native: sicNativeRole(el),
                         pin: parked ? PIN_END : PIN_NONE };
            // A fixed overlay has no place in the flow. A full-width strip at
            // the top reads first; one at the bottom is a cookie bar or a chat
            // widget and reads last — never as the page's footer. Anything else
            // fixed is a modal: leave it exactly where it is.
            if (g && (g.f & F_FIXED)) {
                if (g.y < 0.15 * vh && g.w >= 0.6 * vw) { it.pin = PIN_FRONT; it.role = R_NAV; }
                else if (g.y + g.h > 0.85 * vh) it.pin = PIN_END;
            }
            items.push(it);
        }
        if (missing > 0.4 * kids.length) { plan.declined = 'not enough geometry'; return plan; }

        let ordered = sicVisualOrder(items, rtl);
        // Overlapping siblings are not in flow relative to each other.
        for (let n = 1; n < ordered.length; n++) {
            const a = ordered[n - 1], b = ordered[n];
            if (a.i > b.i && sicOverlaps(a.g, b.g)) { ordered[n - 1] = b; ordered[n] = a; }
        }

        // Reordering must never change the relative order of anything holding a
        // <form>: form_index is a document-order count, resolved against the
        // live page as document.forms[n]. Wrapping cannot break that — it keeps
        // preorder — but moving a region can, so check before moving.
        const formy = kids.map(k => {
            try { return k.tagName === 'FORM' || !!k.querySelector('form'); } catch (e) { return true; }
        });
        const srcForms = [], newForms = [];
        for (let i = 0; i < kids.length; i++) if (formy[i]) srcForms.push(i);
        for (const it of ordered) if (formy[it.i]) newForms.push(it.i);
        const formSafe = srcForms.length === newForms.length
            && srcForms.every((v, n) => v === newForms[n]);
        if (!formSafe) {
            plan.declined = 'reorder would renumber forms';
            ordered = items.slice();
        }

        const perm = ordered.map(it => it.i);
        plan.order = perm.some((v, n) => v !== n) ? perm : null;

        // ---- roles, over the order the page actually reads in ----
        const natives = items.filter(it => it.native !== null).length;
        if (natives >= 2) {
            plan.declined = plan.declined || 'author already marked up landmarks';
            plan.items = ordered.map(it => sicItemReport(it));
            return plan;
        }
        for (const it of ordered) if (it.native !== null) it.role = it.native;

        const probes = new Map();
        for (const it of ordered) if (it.g && it.g.w > 0) probes.set(it, sicProbe(it.el));
        const probeOf = it => probes.get(it) || { links: 0, textLen: 0, hasH1: false, linkDense: false };

        // Footer: a trailing run only. A link-dense block in the middle of a
        // page is a card grid, not the colophon.
        const footTop = DH - Math.max(0.25 * DH, 600);
        for (let n = ordered.length - 1; n >= 0; n--) {
            const it = ordered[n];
            if (it.pin === PIN_END) continue;
            if (it.role !== R_NONE || it.native !== null) break;
            const g = it.g, p = probeOf(it);
            if (!g || g.y < footTop || g.w < 0.6 * vw) break;
            if (!(p.linkDense || sicHint(it.el, FOOT_HINT))) break;
            it.role = R_FOOTER;
        }
        // Nav: a leading run only.
        const navBottom = Math.max(0.10 * DH, 200);
        for (const it of ordered) {
            if (it.pin === PIN_FRONT) continue;
            if (it.role !== R_NONE || it.native !== null) break;
            const g = it.g, p = probeOf(it);
            if (!g || g.y > navBottom || g.w < 0.6 * vw || g.h > 0.25 * vh) break;
            if (!(p.linkDense || sicHint(it.el, NAV_HINT))) break;
            it.role = R_NAV;
        }
        // Main: the dominant remaining region, if there is one.
        let best = null, bestScore = -1, maxArea = 0, maxText = 0;
        for (const it of ordered) {
            if (it.role !== R_NONE || it.native !== null || it.pin) continue;
            if (!it.g) continue;
            maxArea = Math.max(maxArea, it.g.w * it.g.h);
            maxText = Math.max(maxText, probeOf(it).textLen);
        }
        for (const it of ordered) {
            if (it.role !== R_NONE || it.native !== null || it.pin) continue;
            if (!it.g) continue;
            const p = probeOf(it);
            let score = maxArea > 0 ? (it.g.w * it.g.h) / maxArea : 0;
            if (p.hasH1) score += 2;
            if (maxText > 0 && p.textLen === maxText) score += 1;
            if (score > bestScore) { bestScore = score; best = it; }
        }
        if (best && best.g.w * best.g.h >= 0.15 * vw * DH) {
            best.role = R_MAIN;
            // Complementary: a narrow column running alongside the main one.
            if (best.g.w >= 0.5 * vw) {
                for (const it of ordered) {
                    if (it.role !== R_NONE || it.native !== null || it.pin || !it.g) continue;
                    if (it.g.w > 0.35 * vw) continue;
                    // A complementary region is a *column* running beside the
                    // content. A one-line skip link is narrow too, and calling
                    // it "complementary" is worse than saying nothing.
                    if (it.g.h < 0.25 * best.g.h) continue;
                    const top = Math.max(it.g.y, best.g.y);
                    const bot = Math.min(it.g.y + it.g.h, best.g.y + best.g.h);
                    if (bot - top > 0.5 * it.g.h) it.role = R_ASIDE;
                }
            }
        }

        // Contiguous runs of the same role, in reading order. Contiguous is not
        // a detail: wrapping a run in place preserves document preorder, and
        // that is the whole reason this cannot renumber a form.
        for (let n = 0; n < ordered.length; ) {
            const role = ordered[n].role;
            if (role === R_NONE || ordered[n].native !== null) { n++; continue; }
            let m = n;
            while (m + 1 < ordered.length
                   && ordered[m + 1].role === role
                   && ordered[m + 1].native === null) m++;
            plan.groups.push({ role, roleName: ROLE_NAME[role], tag: ROLE_TAG[role],
                               from: n, to: m });
            n = m + 1;
        }
        plan.items = ordered.map(it => sicItemReport(it));
        return plan;
    }

    function sicItemReport(it) {
        let label = it.el.tagName.toLowerCase();
        if (it.el.id) label += '#' + it.el.id;
        else if (typeof it.el.className === 'string' && it.el.className.trim())
            label += '.' + it.el.className.trim().split(/\s+/)[0];
        return { i: it.i, label, role: ROLE_NAME[it.role], pin: it.pin,
                 x: it.g ? it.g.x : null, y: it.g ? it.g.y : null,
                 w: it.g ? it.g.w : null, h: it.g ? it.g.h : null };
    }

    const REFLOW_TAGS = /^(BODY|DIV|SECTION|ARTICLE|MAIN|HEADER|FOOTER|ASIDE|NAV)$/;
    // Containers whose child order is meaning, not layout. A list is a list in
    // the order the author wrote it, a table row is not a band of cards, and a
    // heading or paragraph holds a sentence.
    const REFLOW_BLOCKED = 'form, table, ul, ol, dl, select, label, fieldset, p,' +
        ' pre, h1, h2, h3, h4, h5, h6, blockquote, figure, picture';

    function sicReflowEligible(host) {
        if (!REFLOW_TAGS.test(host.tagName)) return false;
        const g = sicGeo(host);
        if (g && (g.f & (F_NOSORT | F_INLINE))) return false;
        try { if (host.closest(REFLOW_BLOCKED)) return false; } catch (e) { return false; }
        // Mixed inline flow: a container holding bare text between its elements
        // is a sentence, and sorting its parts by position scrambles it.
        for (const n of host.childNodes) {
            if (n.nodeType === 3 && n.textContent && n.textContent.trim()) return false;
        }
        return true;
    }

    // Sort a container's children into the order they are laid out, wherever
    // that disagrees with the order they were written in.  Recurses a few
    // levels: deeper than this the returns fall off and the risk does not.
    function sicReflow(host, depth) {
        if (depth > 4) return;
        const kids = sicKids(host);
        if (kids.length >= 2 && kids.length <= 60 && sicReflowEligible(host)) {
            // Anything holding a <form> keeps source order, full stop.
            // form_index is a document-order count resolved against the live
            // page, and no reading-order gain is worth filling the wrong field.
            let formy = false;
            for (const k of kids) {
                try {
                    if (k.tagName === 'FORM' || k.querySelector('form')) { formy = true; break; }
                } catch (e) { formy = true; break; }
            }
            if (!formy) {
                let missing = 0;
                const items = kids.map((el, i) => {
                    const g = sicGeo(el);
                    if (!g || g.w <= 0 || g.h <= 0) missing++;
                    return { i, el, g, pin: PIN_NONE };
                });
                if (missing <= 0.4 * kids.length) {
                    const hg = sicGeo(host);
                    const ordered = sicVisualOrder(items, !!(hg && (hg.f & F_RTL)));
                    // Identity permutation: touch nothing. Most of the web is
                    // written in the order it renders, and a no-op here means
                    // no DOM churn and nothing to go wrong.
                    if (ordered.some((it, n) => it.i !== n)) {
                        for (const it of ordered) host.appendChild(it.el);
                    }
                }
            }
        }
        for (const k of sicKids(host)) sicReflow(k, depth + 1);
    }

    // The parked menus end up as one contiguous run at the end of the body.
    // Give them a landmark of their own: they are hidden navigation by
    // construction, so `navigation` is honest, and — the real reason — a
    // landmark is the only boundary that stops a menu's heading swallowing
    // everything after it. Runs after the banding pass, so the wrapper it adds
    // is not counted as markup the author supplied.
    function sicWrapParked(body) {
        const parked = [];
        for (const el of sicKids(body)) {
            if (el.getAttribute(PIN) === 'end') parked.push(el);
            else if (parked.length) return false;  // not a trailing run; leave it
        }
        if (!parked.length) return false;
        for (const el of parked) el.removeAttribute(PIN);
        // Already bounded by the site's own markup. Wrapping these would only
        // stack a navigation inside a navigation and name it twice.
        if (parked.every(el => sicNativeRole(el) !== null)) return false;
        let wrap = document.createElement('nav');
        body.insertBefore(wrap, parked[0]);
        for (const el of parked) wrap.appendChild(el);
        unwrapSingleWrapper(wrap);
        // Never stack a navigation inside a navigation. If what came out is
        // already one, theirs is the better named of the two — a menu the site
        // marked up itself carries its own label.
        while (wrap.children.length === 1 && wrap.children[0].tagName === 'NAV') {
            const inner = wrap.children[0];
            wrap.replaceWith(inner);
            wrap = inner;
        }
        return true;
    }

    // Perform what `sicPlan` described.  Returns true if anything changed.
    function sicApply(body, plan) {
        if (!body || (!plan.order && !plan.groups.length)) return false;
        const host = sicBandHost(body);
        let kids = sicKids(host);
        if (plan.order) {
            if (plan.order.length !== kids.length) return false;
            const moved = plan.order.map(i => kids[i]);
            for (const el of moved) host.appendChild(el);
            kids = moved;
        }
        for (const grp of plan.groups) {
            if (grp.from < 0 || grp.to >= kids.length) continue;
            const wrap = document.createElement(grp.tag);
            const first = kids[grp.from];
            if (!first.parentNode) continue;
            first.parentNode.insertBefore(wrap, first);
            for (let n = grp.from; n <= grp.to; n++) wrap.appendChild(kids[n]);
            unwrapSingleWrapper(wrap);
        }
        return true;
    }
"##;

const PRUNE_JS_END: &str = r##"
})()"##;

/// The in-page pass: prune invisible subtrees, then group and order what is
/// left by the geometry Chrome already computed.  See [`PRUNE_JS_HEAD`].
static PRUNE_HIDDEN_SCRIPT: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    // BAND_JS goes *before* the body, and the order is not cosmetic: its `const`
    // declarations sit in the temporal dead zone until they are evaluated, and
    // the body calls into them as soon as it runs. Function declarations hoist,
    // `const` does not — with the body first, the band pass throws
    // "Cannot access 'R_NONE' before initialization" on every page, silently,
    // into the catch that guards it.
    [PRUNE_JS_HEAD, BAND_JS, PRUNE_JS_BODY, PRUNE_JS_END].concat()
});

/// Head + band alone, returning the banding decisions as JSON instead of
/// applying them.  Test-only: it is how a fixture asserts on roles and
/// permutations rather than grepping serialized HTML.
#[cfg(test)]
fn band_plan_js() -> String {
    [
        PRUNE_JS_HEAD,
        BAND_JS,
        r##"
    sicStampLive(document.documentElement);
    return JSON.stringify(sicPlan(document.body));
"##,
        PRUNE_JS_END,
    ]
    .concat()
}

/// Fetch the page's HTML, with invisible subtrees removed when pruning is on.
///
/// Falls back to a plain `page.content()` whenever the in-page pass fails or
/// runs long: a page that defeats the prune must still be readable.
async fn settled_html(page: &chromiumoxide::Page) -> Result<String, String> {
    let t = tokio::time::Duration::from_secs;
    let pruned = if prune_hidden() {
        tokio::time::timeout(t(10), page.evaluate(PRUNE_HIDDEN_SCRIPT.as_str()))
            .await
            .ok()
            .and_then(|r| r.ok())
            .and_then(|v| v.into_value::<String>().ok())
            .filter(|html| !html.is_empty())
    } else {
        None
    };
    if let Some(html) = pruned {
        return Ok(html);
    }
    tokio::time::timeout(t(15), page.content())
        .await
        .map_err(|_| "timed out waiting for page content (15 s)".to_owned())?
        .map_err(|e| format!("failed to get page content: {e}"))
}

// ---------------------------------------------------------------------------
// Consent-wall auto-accept helpers
// ---------------------------------------------------------------------------

/// CMP-specific CSS selectors for "accept all" buttons, tried in priority order.
/// Covers DPG Media, OneTrust, Didomi, TrustArc, Sourcepoint, Quantcast,
/// CookieBot, Cookie Information, Usercentrics, and generic patterns.
const CMP_SELECTORS: &[&str] = &[
    // DPG Media (hln.be, vtm.be, ad.nl, volkskrant.nl, …)
    r#"button[data-testid="pur-accept-button"]"#,
    r#"button[data-testid="pur-all-accept-button"]"#,
    r#"button[class*="pur-accept"]"#,
    // OneTrust
    "#onetrust-accept-btn-handler",
    "button.onetrust-close-btn-handler.accept-btn",
    // Didomi
    "#didomi-notice-agree-button",
    // TrustArc
    "#truste-consent-button",
    ".trustarc-agree-btn",
    // Sourcepoint
    r#"button[title="Accept All"]"#,
    ".message-button.accept-all",
    "button.sp_choice_type_11",
    // Quantcast
    "button.qc-cmp2-summary-buttons button:last-child",
    // CookieBot
    "#CybotCookiebotDialogBodyLevelButtonLevelOptinAllowAll",
    "#CybotCookiebotDialogBodyButtonAccept",
    // Cookie Information
    "#coi-banner-accept",
    // Usercentrics
    r#"button[data-testid="uc-accept-all-button"]"#,
    // Generic broad patterns
    r#"[data-role="accept-all"]"#,
    r#"[data-testid="accept-all"]"#,
    r#"button[id*="accept-all"]"#,
    r#"button[class*="accept-all"]"#,
    r#"button[class*="acceptAll"]"#,
    r#"button[id*="accept"][id*="all"]"#,
    r#"button[class*="AcceptAll"]"#,
    r#"[data-cy*="accept-all"]"#,
    r#"[aria-label*="accept all" i]"#,
    r#"[aria-label*="Alles accepteren" i]"#,
    r#"[aria-label*="akzeptieren" i]"#,
];

/// CMP-specific selectors for the one-click "reject all" button.
///
/// The mirror image of `CMP_SELECTORS`, and the reason refusing keeps working
/// in a language nobody added a keyword for: bpost's Dutch OneTrust banner
/// labels it "Alles afwijzen", but the id is the same everywhere.
const CMP_REJECT_SELECTORS: &[&str] = &[
    // OneTrust
    "#onetrust-reject-all-handler",
    ".ot-pc-refuse-all-handler",
    // Didomi
    "#didomi-notice-disagree-button",
    // CookieBot
    "#CybotCookiebotDialogBodyButtonDecline",
    "#CybotCookiebotDialogBodyLevelButtonLevelOptinDeclineAll",
    // Usercentrics
    r#"button[data-testid="uc-deny-all-button"]"#,
    // Sourcepoint
    "button.sp_choice_type_REJECT_ALL",
    // Cookie Information
    "#declineButton",
    // Quantcast
    "button.qc-cmp2-summary-buttons button:first-child",
    // Generic
    r#"button[id*="reject-all"]"#,
    r#"button[class*="reject-all"]"#,
    r#"[aria-label*="reject all" i]"#,
];

/// Button text substrings indicating a *reject* action.
/// Checked before ACCEPT_KEYWORDS to prevent classifying "decline all" buttons
/// whose text happens to contain an accept keyword fragment.
const REJECT_KEYWORDS: &[&str] = &[
    "reject",
    "decline",
    "refuse",
    "weiger",
    "afwijz",
    "niet akkoord",
    "refuser",
    "continuer sans accepter",
    "ablehnen",
    "rifiuta",
    "rechazar",
    "only necessary",
    "essential only",
    "only essential",
    "alleen noodzakelijke",
    "alleen essenti",
    "nur notwendige",
    "nur erforderliche",
];

/// Button text substrings indicating a "manage my preferences" action — a
/// button that opens a second panel rather than deciding anything.
///
/// Split out of `REJECT_KEYWORDS` when consent stopped being auto-clicked: as a
/// guard against a stray accept these behave exactly like a reject (never
/// press), but they are not a *choice* and must not be offered as one.
const SETTINGS_KEYWORDS: &[&str] = &[
    "manage",
    "settings",
    "instellingen",
    "personnaliser",
    "preferences",
    "voorkeuren",
    "einstellungen",
    "paramètres",
];

/// Button text substrings indicating an "accept all" action, in 6 languages.
const ACCEPT_KEYWORDS: &[&str] = &[
    // English
    "accept all",
    "allow all",
    "agree and continue",
    "i accept",
    "got it",
    // Dutch
    "alles accepteren",
    "accepteer alles",
    "akkoord",
    "ja, ik accepteer",
    "ik ga akkoord",
    "alles toestaan",
    // French
    "tout accepter",
    "j'accepte",
    "accepter et fermer",
    "continuer et accepter",
    // German
    "alle akzeptieren",
    "alles annehmen",
    "zustimmen",
    "einverstanden",
    "akzeptieren und weiter",
    // Italian
    "accetta tutto",
    "accetto",
    "acconsento",
    // Spanish
    "aceptar todo",
    "acepto",
    "aceptar y continuar",
];

/// Returns `true` if the button text looks like a reject *or* settings action —
/// i.e. anything that must never be treated as an accept.
#[cfg_attr(not(test), allow(dead_code))]
fn is_reject_text(text: &str) -> bool {
    let lower = text.to_lowercase();
    REJECT_KEYWORDS
        .iter()
        .chain(SETTINGS_KEYWORDS.iter())
        .any(|k| lower.contains(k))
}

/// Returns `true` if the button text only opens a preferences panel.
#[cfg_attr(not(test), allow(dead_code))]
fn is_settings_text(text: &str) -> bool {
    let lower = text.to_lowercase();
    SETTINGS_KEYWORDS.iter().any(|k| lower.contains(k))
}

/// Returns `true` if the button text looks like an "accept all" action.
/// Always returns `false` when `is_reject_text` is true (guard takes priority).
#[cfg_attr(not(test), allow(dead_code))]
fn is_accept_keyword(text: &str) -> bool {
    if is_reject_text(text) {
        return false;
    }
    let lower = text.to_lowercase();
    ACCEPT_KEYWORDS.iter().any(|k| lower.contains(k))
}

/// Serialise a Rust string slice as a JSON array for embedding in JS.
fn js_array(items: &[&str]) -> String {
    let quoted: Vec<String> = items.iter().map(|s| format!("{s:?}")).collect();
    format!("[{}]", quoted.join(","))
}

/// Consent-banner containers, as (HTML marker substring, CSS selector).
///
/// The marker decides — from the already-fetched HTML — whether the in-page
/// pass is worth running at all; the selector finds the container once we are
/// inside the page.  Vendor-specific on purpose: a generic "cookie"/"consent"
/// substring matches every page that merely links to a cookie policy.
const CONSENT_BANNERS: &[(&str, &str)] = &[
    ("onetrust-banner-sdk", "#onetrust-banner-sdk"),
    ("didomi-notice", "#didomi-notice"),
    ("CybotCookiebotDialog", "#CybotCookiebotDialog"),
    ("usercentrics-root", "#usercentrics-root"),
    ("truste_popframe", "#truste_popframe, #truste-consent-track"),
    ("qc-cmp2-container", ".qc-cmp2-container"),
    ("coi-banner-wrapper", "#coi-banner-wrapper"),
    ("sp_message_container", "[id^=\"sp_message_container\"]"),
    ("cookiescript_injected", "#cookiescript_injected"),
    ("axeptio_overlay", "#axeptio_overlay"),
    ("cmpboxbtnyes", "#cmpbox"),
    (
        "cm-btn-accept-all",
        ".klaro .cookie-modal, .klaro .cookie-notice",
    ),
];

/// Detect a consent *wall*: the site redirected to a CMP host and is serving
/// the consent page instead of the content.
fn is_consent_wall_str(url: &str) -> bool {
    url.contains("myprivacy.dpgmedia.be")
        || url.contains("sp-prod.net")
        || url.contains("privacy-mgmt.com")
        || url.contains("/consent")
        || url.contains("cookie-consent")
        || url.contains("consent.")
        || url.contains("cmp.")
}

/// Detect a consent wall that is served *inline* on an otherwise normal URL.
///
/// Google and YouTube embed the GDPR wall directly at `www.google.com` /
/// `www.youtube.com` (HTTP 200, URL unchanged) rather than redirecting to a
/// `consent.*` host, so `is_consent_wall_str` on the URL never fires for them.
/// The wall's accept button submits to a `consent.google.com/save` endpoint that
/// is present only while the wall is unaccepted.  Verified live: the walled
/// homepage references `/save` twice, the accepted homepage zero times.  (The
/// sibling `/d` endpoint is *not* usable as a marker: it persists in a footer
/// privacy link on the accepted page and would cause false positives.)
fn html_has_consent_wall(html: &str) -> bool {
    const MARKERS: &[&str] = &["consent.google.com/save", "consent.youtube.com/save"];
    MARKERS.iter().any(|m| html.contains(m))
}

/// Vendor scaffolding that is never content: preference centres, backdrops and
/// the wrappers a CMP leaves parked in the DOM whether or not it has anything
/// to say.  Removed whenever it is not actually on screen — OneTrust keeps a
/// 4 kB preference centre at `display:none` on every page of every visit, and
/// the generic prune now keeps hidden subtrees that hold buttons.
const CONSENT_DEBRIS: &[&str] = &[
    "#onetrust-consent-sdk",
    "#onetrust-pc-sdk",
    ".onetrust-pc-dark-filter",
    "#didomi-host",
    "#CybotCookiebotDialog",
    "#CybotCookiebotDialogBodyUnderlay",
    "#usercentrics-root",
    "#truste-consent-track",
    ".qc-cmp2-container",
    "#coi-banner-wrapper",
    "#cookiescript_injected",
    "#axeptio_overlay",
    "#cmpwrapper",
];

/// Detect a consent banner overlaid on an otherwise ordinary page.
///
/// This is the case `is_consent_wall_str` and `html_has_consent_wall` both miss:
/// no redirect to a consent host, HTTP 200, real content underneath — just a
/// vendor's banner on top of it.  It is also the common case.
fn html_has_inline_consent_banner(html: &str) -> bool {
    CONSENT_BANNERS
        .iter()
        .any(|(marker, _)| html.contains(marker))
}

// ---------------------------------------------------------------------------
// Google's inline consent wall
// ---------------------------------------------------------------------------

/// The consent values Google's own inline script publishes on its GDPR wall.
///
/// `sCAS` / `sCRS` are the `SOCS` cookie payloads recording "accept all" and
/// "reject all"; both carry the serving build's version, which is why a
/// hardcoded constant goes stale.
#[derive(Debug, Clone, PartialEq, Eq)]
struct GoogleConsent {
    domain: String,
    accept: String,
    reject: String,
    max_age: String,
}

/// Fallback `SOCS` payload from 2021, used only when the wall's own script
/// cannot be parsed.  It carries no build version and Google has stopped
/// honouring it reliably — treat a fallback as "probably will not work".
const GOOGLE_SOCS_FALLBACK: &str = "CAESHAgBEhIaAB";

/// Read `var name = <value>;` out of an inline script.
///
/// Tolerates single quotes, double quotes and bare numbers, and skips a match
/// where `name` is only the prefix of a longer identifier (`sL` vs `sLater`) by
/// requiring `=` as the next non-space character.
fn js_var(html: &str, name: &str) -> Option<String> {
    let needle = format!("var {name}");
    let mut base = 0usize;
    loop {
        let i = html.get(base..)?.find(&needle)?;
        let after = base + i + needle.len();
        let rest = html.get(after..)?.trim_start();
        if let Some(body) = rest.strip_prefix('=') {
            let body = body.trim_start();
            let value = if let Some(q) = body.strip_prefix('\'') {
                q.split('\'').next().unwrap_or_default().to_owned()
            } else if let Some(q) = body.strip_prefix('"') {
                q.split('"').next().unwrap_or_default().to_owned()
            } else {
                body.split([';', '\n'])
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_owned()
            };
            if !value.is_empty() {
                return Some(value);
            }
        }
        base = after;
    }
}

/// Extract Google's consent values from its wall.
///
/// Verified live (2026-08): the wall carries an inline script defining
/// `cookieDomain`, `sCAS` (accept), `sCRS` (reject) and `sL` (max-age).  Reading
/// them beats hardcoding, because both payloads embed the serving build id.
///
/// Note the sibling `aAU` / `rAU` save URLs are *not* usable as navigations:
/// `GET https://consent.google.com/save?…` answers 405, they are POST targets.
/// Setting `SOCS` and reloading is what actually clears the wall.
fn google_consent_values(html: &str) -> Option<GoogleConsent> {
    let domain = js_var(html, "cookieDomain")?;
    if !domain.starts_with('.') {
        return None;
    }
    let accept = js_var(html, "sCAS")?;
    let reject = js_var(html, "sCRS")?;
    let max_age = js_var(html, "sL")
        .filter(|v| !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()))
        .unwrap_or_else(|| "34128000".to_owned());
    Some(GoogleConsent {
        domain,
        accept,
        reject,
        max_age,
    })
}

/// Google's wall with values we could not read: still offer both choices, using
/// the stale fallback payload, rather than leaving the user with nothing.
fn google_consent_or_fallback(html: &str) -> GoogleConsent {
    google_consent_values(html).unwrap_or_else(|| GoogleConsent {
        domain: ".google.com".to_owned(),
        accept: GOOGLE_SOCS_FALLBACK.to_owned(),
        reject: GOOGLE_SOCS_FALLBACK.to_owned(),
        max_age: "34128000".to_owned(),
    })
}

// ---------------------------------------------------------------------------
// The cookie gate: the decision a site puts in front of its content
//
// A cookie banner sits between the reader and the page, so it is answered as a
// page of its own: the list of choices, and nothing else, leading to the
// content.  A decision the site already has on file is never asked again — it
// simply stops rendering its dialog, so no gate is detected and the content is
// what loads.
//
// Only the cookie question is treated this way.  A language switcher does not
// block anything, it is part of the page, and asking about it interrupted every
// multilingual site with a list the reader had not asked for — see
// `a_page_with_a_language_switcher_is_just_a_page` and the 0.1.18 CHANGELOG.
// What the page declares about its own languages is read regardless, as a
// section at the end rather than a step in front: `declared_languages`.
//
// Every choice is a single-button `<form>` whose click forwards to the real
// control (see `gate_surface_js`).  FFON can only activate a `<button>` inside
// a `<form>`, and `on_button_press` only ever receives `submit:form_N`, so one
// form per choice is what makes each choice individually pressable.
// ---------------------------------------------------------------------------

/// Fluent key for the line that introduces the step.
const CONSENT_NOTICE_KEY: &str = "webbrowser-step-consent";

/// What the in-page pass found.
#[derive(Debug, Default, serde::Deserialize)]
struct GateReport {
    /// Set when the page is showing a cookie decision that was turned into
    /// choices.
    #[serde(default)]
    gated: bool,
    /// Choice labels, in the order their forms were inserted.
    labels: Vec<String>,
    /// Set when the pass marked anything for the prune to take out, so the page
    /// is worth serialising again.
    #[serde(default)]
    marked: bool,
    /// Set when a consent vendor's script is on the page but its banner is not
    /// in the DOM yet.  bpost loads OneTrust through Google Tag Manager, so the
    /// banner arrives a beat after the page has otherwise settled.
    #[serde(default)]
    pending: bool,
}

/// Script hosts that mean "a cookie banner is coming".  Used only to decide
/// whether waiting another moment is worthwhile, never to classify a page.
const CONSENT_VENDOR_SCRIPTS: &[&str] = &[
    "cookielaw.org",
    "onetrust.com",
    "cookiebot.com",
    "didomi.io",
    "usercentrics.eu",
    "trustarc.com",
    "quantcast.com",
    "cookiescript.com",
    "axept.io",
    "consentmanager.net",
    "cookieinformation.com",
    "sourcepoint.mgr.consensu.org",
];

/// Prefix of the id given to a choice's proxy button.
const CONSENT_BUTTON_PREFIX: &str = "sic-consent-";

/// Build the in-page pass that finds the cookie decision and turns it into forms.
fn gate_surface_js(google: Option<&GoogleConsent>, is_wall: bool, rebuild: bool) -> String {
    register_translations();
    let containers = js_array(&CONSENT_BANNERS.iter().map(|(_, s)| *s).collect::<Vec<_>>());
    let cmp_sels = js_array(CMP_SELECTORS);
    let cmp_reject_sels = js_array(CMP_REJECT_SELECTORS);
    let debris_sels = js_array(CONSENT_DEBRIS);
    let reject_kws = js_array(REJECT_KEYWORDS);
    let accept_kws = js_array(ACCEPT_KEYWORDS);
    let settings_kws = js_array(SETTINGS_KEYWORDS);
    let vendor_scripts = js_array(CONSENT_VENDOR_SCRIPTS);
    let google_js = match google {
        Some(g) => format!(
            "{{domain:{:?},accept:{:?},reject:{:?},maxAge:{:?}}}",
            g.domain, g.accept, g.reject, g.max_age
        ),
        None => "null".to_owned(),
    };
    let accept_label = localize::t("webbrowser-consent-accept-all");
    let reject_label = localize::t("webbrowser-consent-reject-all");
    format!(
        r#"(function() {{
    const SELS   = {containers};
    const CMP    = {cmp_sels};
    const CMPREJ = {cmp_reject_sels};
    const DEBRIS = {debris_sels};
    const REJ    = {reject_kws};
    const ACC    = {accept_kws};
    const SET    = {settings_kws};
    const VENDORS = {vendor_scripts};
    const GOOGLE = {google_js};
    const IS_WALL = {is_wall};
    const REBUILD = {rebuild};
    const L = {{accept: {accept_label:?}, reject: {reject_label:?}}};
    const CONSENT_ID = {CONSENT_BUTTON_PREFIX:?};
    const SRC_MARK = 'data-sic-consent-src';
    const VISIBLE = 'display:block !important;visibility:visible !important;';

    const textOf = el => ((el.innerText || el.textContent || '').replace(/\s+/g, ' ').trim());
    const visible = el => {{ try {{ return el.getClientRects().length > 0; }} catch (e) {{ return false; }} }};

    // Idempotent: a second settle on the same page must not stack another copy
    // of the choices.  A deliberate retry does the opposite — it tears the
    // previous set down and rebuilds, because a CMP can render its buttons a
    // beat apart and an early scan would otherwise lock in a partial list.
    const existing = document.querySelectorAll('button[id^="' + CONSENT_ID + '"]');
    if (existing.length) {{
        if (!REBUILD) {{
            return {{gated: true, labels: Array.from(existing).map(b => b.textContent)}};
        }}
        for (const btn of existing) {{ const f = btn.closest('form'); if (f) f.remove(); else btn.remove(); }}
    }}

    // Off-screen vendor scaffolding goes regardless of what we find: it is
    // never content, and the prune deliberately keeps hidden subtrees that
    // contain buttons.  Anything actually on screen is left alone, so a
    // preference centre the user has opened stays readable.
    let marked = false;
    for (const sel of DEBRIS) {{
        let els = [];
        try {{ els = document.querySelectorAll(sel); }} catch (e) {{ continue; }}
        for (const el of els) if (!visible(el)) {{ el.setAttribute(SRC_MARK, ''); marked = true; }}
    }}

    const choices = [];
    if (GOOGLE) {{
        // Google ignores programmatic clicks on its own accept button, even
        // trusted CDP mouse events. Writing SOCS is what actually takes effect.
        const setSocs = v => function () {{
            document.cookie = 'SOCS=' + v + ';domain=' + GOOGLE.domain +
                ';path=/;max-age=' + GOOGLE.maxAge + ';secure;samesite=lax';
            location.reload();
        }};
        choices.push({{label: L.reject, kind: 'reject', run: setSocs(GOOGLE.reject)}});
        choices.push({{label: L.accept, kind: 'accept', run: setSocs(GOOGLE.accept)}});
    }} else {{
        let container = null;
        for (const sel of SELS) {{
            try {{ const el = document.querySelector(sel); if (el) {{ container = el; break; }} }} catch (e) {{}}
        }}
        // On a wall the whole page *is* the CMP, so there is no container to
        // find and scanning the document is safe — there is nothing else on it.
        const root = container || (IS_WALL ? document : null);
        if (root) {{
            const seen = new Set();
            const scan = r => {{
                let els = [];
                try {{ els = r.querySelectorAll('button, a, [role="button"], input[type="submit"]'); }} catch (e) {{ return; }}
                for (const el of els) {{
                    if (seen.has(el) || !visible(el)) continue;
                    const label = (el.tagName === 'INPUT' ? (el.value || '') : textOf(el));
                    const t = label.toLowerCase();
                    // A consent button says a few words. Anything longer is prose.
                    if (!t || t.length > 60) continue;
                    const matchesAny = list => list.some(s => {{
                        try {{ return el.matches(s); }} catch (e) {{ return false; }}
                    }});
                    let kind = null;
                    // The vendor's own id first: it survives translation, which
                    // a keyword list never fully does.
                    if (matchesAny(CMPREJ)) kind = 'reject';
                    else if (REJ.some(k => t.includes(k))) kind = 'reject';
                    else if (SET.some(k => t.includes(k))) continue;   // opens a panel, decides nothing
                    else if (ACC.some(k => t.includes(k))) kind = 'accept';
                    else if (matchesAny(CMP)) kind = 'accept';
                    if (!kind) continue;
                    seen.add(el);
                    choices.push({{label: label, kind: kind, el: el}});
                }}
            }};
            scan(root);
            if (container && container.shadowRoot) scan(container.shadowRoot);
            if (container) {{ container.setAttribute(SRC_MARK, ''); marked = true; }}
        }}
        if (!choices.length) {{
            // DPG Media (hln.be, vtm.be, ad.nl, …) publishes the site's own
            // consent callback inline, before its external consent.js loads.
            try {{
                const u = window.cmpProperties && window.cmpProperties.siteUrl;
                if (u && u.length) choices.push({{label: L.accept, kind: 'accept', href: u}});
            }} catch (e) {{}}
        }}
    }}

    if (!choices.length) {{
        // Nothing to answer *yet*: a tag manager may still be injecting the
        // vendor's banner, which is worth one more look. bpost loads OneTrust
        // this way, and it lands a beat after the page otherwise settles.
        const coming = VENDORS.some(v => {{
            try {{ return !!document.querySelector('script[src*="' + v + '"]'); }} catch (e) {{ return false; }}
        }});
        return {{gated: false, labels: [], marked: marked, pending: coming}};
    }}

    // Refusing is the choice a user has to go looking for on the real web, so
    // it goes first here. Stable within each group, i.e. the site's own order.
    const ordered = choices.filter(c => c.kind === 'reject').concat(choices.filter(c => c.kind !== 'reject'));
    ordered.forEach((c, i) => {{ c.id = CONSENT_ID + i; }});
    insert(ordered);
    // A banner that has offered only one button so far is probably mid-render:
    // worth one more look, so refusing does not go missing.
    return {{gated: true, labels: ordered.map(c => c.label), marked: true, pending: ordered.length < 2}};

    // One single-button form per choice, prepended to <body> so they are
    // document.forms[0..n-1] — the indices FFON derives from the step page have
    // to match the ones `document.forms[n]` yields here at click time.
    function insert(list) {{
        const frag = document.createDocumentFragment();
        for (const c of list) {{
            const form = document.createElement('form');
            form.setAttribute('onsubmit', 'return false');
            form.style.cssText = VISIBLE;
            const btn = document.createElement('button');
            btn.type = 'button';
            btn.id = c.id;
            btn.textContent = c.label;
            btn.style.cssText = VISIBLE;
            btn.addEventListener('click', function () {{
                const run = c.run, href = c.href, el = c.el;
                // The step is answered, so it stops existing. Without this the
                // next settle would find our own buttons still standing and
                // offer the same step forever — a CMP that hides its banner in
                // place (OneTrust) never navigates to clear them for us.
                for (const b of document.querySelectorAll('button[id^="' + CONSENT_ID + '"]')) {{
                    const f = b.closest('form');
                    if (f) f.remove(); else b.remove();
                }}
                if (run) run();
                else if (href) window.location.href = href;
                else if (el) el.click();
            }});
            form.appendChild(btn);
            frag.appendChild(form);
        }}
        document.body.insertBefore(frag, document.body.firstChild);
    }}
}})()"#
    )
}

/// Run the in-page pass once.
async fn surface_gate(
    page: &chromiumoxide::Page,
    google: Option<&GoogleConsent>,
    is_wall: bool,
    rebuild: bool,
) -> GateReport {
    let js = gate_surface_js(google, is_wall, rebuild);
    tokio::time::timeout(tokio::time::Duration::from_secs(5), page.evaluate(js))
        .await
        .ok()
        .and_then(|r| r.ok())
        .and_then(|r| r.into_value::<GateReport>().ok())
        .unwrap_or_default()
}

/// Escape a string for interpolation into HTML text or a double-quoted attribute.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Render a gate as a page containing nothing but the choices.
///
/// The forms mirror, in order, the proxy forms `gate_surface_js` prepended to
/// the live document, so `form_1..form_N` here resolve to `document.forms[0..N-1]`
/// there.  Ids match too, which is what `html_submit_selector` turns into the
/// `#sic-consent-0` selector the click is finally made through.
fn gate_page_html(labels: &[String], ids: &[String]) -> String {
    let mut body = String::new();
    for (label, id) in labels.iter().zip(ids) {
        body.push_str(&format!(
            "<form><button type=\"button\" id=\"{}\">{}</button></form>",
            html_escape(id),
            html_escape(label)
        ));
    }
    format!("<!DOCTYPE html><html><body>{body}</body></html>")
}

/// Recreate the ids `gate_surface_js` assigned, from the labels it returned.
///
/// Read back out of the live page rather than guessed, so a rebuild that
/// renumbered the choices cannot leave the ids and the labels disagreeing.
async fn gate_button_ids(page: &chromiumoxide::Page) -> Vec<String> {
    let js = format!(
        r#"Array.from(document.querySelectorAll('button[id^="{CONSENT_BUTTON_PREFIX}"]')).map(b => b.id)"#
    );
    tokio::time::timeout(tokio::time::Duration::from_secs(5), page.evaluate(js))
        .await
        .ok()
        .and_then(|r| r.ok())
        .and_then(|r| r.into_value::<Vec<String>>().ok())
        .unwrap_or_default()
}

/// Resolve whatever the site is asking before it will show its content.
///
/// Returns the page to render and whether it is a gate.  A gated page contains
/// only the cookie choices; answering one leads to the content.
async fn settle_gates(
    page: &chromiumoxide::Page,
    current_url: &str,
    html: String,
) -> (PageLoad, bool) {
    // "show hidden content" doubles as the way out: it hands back the whole
    // page, gates and all, for when a step is detected on a page that should
    // not have one.
    if !prune_hidden() {
        return (PageLoad::plain(html), false);
    }
    register_translations();

    let is_wall = is_consent_wall_str(current_url) || html_has_consent_wall(&html);
    let google = html_has_consent_wall(&html).then(|| google_consent_or_fallback(&html));

    let mut report = GateReport::default();
    // A client-rendered CMP may not have its buttons in the DOM yet — the same
    // hydration lag the old auto-accept retried for.
    for attempt in 0..4u32 {
        if attempt > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(700)).await;
        }
        report = surface_gate(page, google.as_ref(), is_wall, attempt > 0).await;
        // `pending` means the page may still be putting choices on screen:
        // a tag manager injecting the banner, or a CMP that has rendered
        // only one of its buttons so far.  It is read from the live DOM, so
        // a banner that arrived after our snapshot still counts.
        if !report.pending && (!report.labels.is_empty() || is_wall) {
            break;
        }
        if !report.pending && !html_has_inline_consent_banner(&html) {
            break;
        }
    }

    if report.gated {
        let ids = gate_button_ids(page).await;
        if ids.len() == report.labels.len() {
            return (
                PageLoad {
                    html: gate_page_html(&report.labels, &ids),
                    notices: vec![localize::t(CONSENT_NOTICE_KEY)],
                },
                true,
            );
        }
    }

    // Not gated (any more): the page is whatever the site is now serving. Only
    // worth re-serialising if something moved — a banner container marked for
    // the prune to take out.
    let html = if report.marked || html_has_inline_consent_banner(&html) {
        settled_html(page).await.unwrap_or(html)
    } else {
        html
    };
    if is_wall && html_has_consent_wall(&html) {
        // The wall is what the site served and we could not lift a choice out
        // of it, so it is what the user gets to read and operate.
        return (
            PageLoad {
                html,
                notices: vec![localize::t("webbrowser-consent-unrecognised")],
            },
            false,
        );
    }
    (PageLoad::plain(html), false)
}

/// Wait until the document has finished loading *and* its visible text has
/// stopped changing.
///
/// `await_stable_url` is not enough after a consent choice: a CMP that reloads
/// or re-renders in place never changes the URL, so that check returns after
/// its minimum wait while the page is still rebuilding itself, and the page
/// gets serialised with its content region half-built. Watching the document
/// covers both an in-place re-render and a real navigation.
async fn await_page_settled(page: &chromiumoxide::Page, budget: tokio::time::Duration) {
    const PROBE: &str = "(function(){try{return document.readyState+'|'+document.body.innerText.length;}\
         catch(e){return 'x|0';}})()";
    let deadline = tokio::time::Instant::now() + budget;
    let mut prev = String::new();
    let mut quiet = 0u32;
    loop {
        tokio::time::sleep(tokio::time::Duration::from_millis(400)).await;
        let now = tokio::time::timeout(tokio::time::Duration::from_secs(3), page.evaluate(PROBE))
            .await
            .ok()
            .and_then(|r| r.ok())
            .and_then(|v| v.into_value::<String>().ok())
            .unwrap_or_default();
        if now.starts_with("complete") && now == prev && !now.is_empty() {
            quiet += 1;
            // Two identical readings in a row, i.e. ~800 ms of nothing moving.
            if quiet >= 2 {
                return;
            }
        } else {
            quiet = 0;
        }
        prev = now;
        if tokio::time::Instant::now() >= deadline {
            return;
        }
    }
}

/// Poll `page.url()` every 300 ms until the URL stabilises or a consent-wall
/// URL is detected, up to `budget`.  Returns the final URL seen.
///
/// "Stable" means the URL was the same on two consecutive polls AND at least
/// `MIN_WAIT` has elapsed since the call started — this prevents returning
/// prematurely before a slow JS redirect has had a chance to fire (a common
/// problem on Windows where the redirect can arrive after the load event).
async fn await_stable_url(page: &chromiumoxide::Page, budget: tokio::time::Duration) -> String {
    const MIN_WAIT: tokio::time::Duration = tokio::time::Duration::from_millis(1500);
    let poll_interval = tokio::time::Duration::from_millis(300);
    let deadline = tokio::time::Instant::now() + budget;
    let start = tokio::time::Instant::now();
    let mut prev_url = String::new();
    loop {
        tokio::time::sleep(poll_interval).await;
        let url = tokio::time::timeout(tokio::time::Duration::from_secs(3), page.url())
            .await
            .ok()
            .and_then(|r| r.ok())
            .flatten()
            .unwrap_or_default();

        if is_consent_wall_str(&url) {
            return url; // Consent wall detected — stop early
        }
        if url == prev_url && start.elapsed() >= MIN_WAIT {
            return url; // URL has stabilised after minimum wait
        }
        if tokio::time::Instant::now() >= deadline {
            return url;
        }
        prev_url = url;
    }
}

// ---------------------------------------------------------------------------
// Tests — port of tests/lib_webbrowser/test_webbrowser.c (16 tests)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialises the tests that depend on `TEST_NO_LAUNCH`.
    ///
    /// The flag is process-global and cargo runs tests in parallel, so one test
    /// clearing it while another is mid-`commit_edit` would send that one off
    /// to launch a real Chrome. Every test that reads the flag takes this and
    /// states the value it wants, rather than assuming what it was left at.
    static LAUNCH_FLAG: Mutex<()> = Mutex::new(());

    fn launch_flag_guard(no_launch: bool) -> std::sync::MutexGuard<'static, ()> {
        // A test that panicked while holding this poisoned it, which says
        // nothing about the flag itself — take it regardless.
        let guard = LAUNCH_FLAG.lock().unwrap_or_else(|e| e.into_inner());
        _set_test_no_launch(no_launch);
        guard
    }

    // ---- Linux Chrome launch mode ----

    // Without xvfb-run or Xvfb — the state of a stock Mint / Ubuntu / Fedora
    // desktop, and of every released build outside `nix develop` — Chrome has
    // to run headless. The old behaviour here was a visible Chrome window that
    // stole the keyboard focus, and with it the screen reader.
    #[test]
    #[cfg(target_os = "linux")]
    fn no_xvfb_falls_back_to_headless_not_a_visible_window() {
        let chrome = std::path::PathBuf::from("/usr/bin/google-chrome");
        let launch = linux_chrome_launch_with(None, chrome.clone()).expect("decision");
        match launch {
            LinuxChrome::Headless(exe) => assert_eq!(exe, chrome),
            LinuxChrome::VirtualDisplay(_) => panic!("no Xvfb present, cannot use one"),
        }
    }

    // With a helper present, Chrome runs headed behind a wrapper script that
    // owns the invisible display.
    #[test]
    #[cfg(target_os = "linux")]
    #[test]
    fn both_xvfb_paths_use_the_same_desktop_screen_size() {
        let screen = format!("-screen 0 {VIEWPORT_W}x{VIEWPORT_H}x24");
        for helper in [XvfbHelper::Run, XvfbHelper::Bare] {
            let script = xvfb_wrapper_script("/usr/bin/google-chrome", helper);
            assert!(
                script.contains(&screen),
                "a virtual screen smaller than the layout viewport makes every \
                 responsive site serve its phone layout: {script}"
            );
        }
        assert!(VIEWPORT_W >= 1200, "must clear the usual xl breakpoint");
    }

    fn xvfb_present_runs_headed_behind_a_wrapper() {
        for helper in [XvfbHelper::Run, XvfbHelper::Bare] {
            let launch = linux_chrome_launch_with(
                Some(helper),
                std::path::PathBuf::from("/usr/bin/chromium"),
            )
            .expect("decision");
            let LinuxChrome::VirtualDisplay(wrapper) = launch else {
                panic!("a helper is present, so Chrome must run on a virtual display");
            };
            let script = std::fs::read_to_string(&wrapper).expect("wrapper written");
            assert!(
                script.starts_with("#!/bin/sh"),
                "wrapper must be a shell script"
            );
            assert!(
                script.contains("/usr/bin/chromium"),
                "wrapper must run Chrome"
            );
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn xvfb_run_script_hands_chrome_a_virtual_display() {
        let script = xvfb_wrapper_script("/usr/bin/google-chrome", XvfbHelper::Run);
        assert!(script.contains("/usr/bin/google-chrome"));
        // The screen size is the whole ballgame for a responsive site. Left to
        // its own default, xvfb-run gave a 612x459 screen, Chrome clamped its
        // window to it, the viewport came out at 800px, and every site served
        // its phone layout — bpost hid its desktop navigation completely.
        assert!(
            script.contains(&format!("-screen 0 {VIEWPORT_W}x{VIEWPORT_H}x24")),
            "xvfb-run must be given a desktop screen size: {script}"
        );
        // Chrome must not pick the compositor over the virtual X11 display.
        assert!(script.contains("unset WAYLAND_DISPLAY"));
        assert!(script.contains("--ozone-platform=x11"));
        // Debian's xvfb-run runs the command as `"$@" 2>&1`, so without this
        // redirect Chrome's "DevTools listening on ws://…" line goes to the
        // stdout chromiumoxide sends to /dev/null, and every page load on a
        // .deb/.rpm machine dies with LaunchTimeout(BrowserStderr("")).
        assert!(
            script.trim_end().ends_with("1>&2"),
            "xvfb-run's stdout must be folded into stderr, where chromiumoxide reads: {script}"
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn bare_xvfb_script_starts_and_tears_down_its_own_server() {
        let script = xvfb_wrapper_script("/usr/bin/google-chrome", XvfbHelper::Bare);
        assert!(script.contains("Xvfb :$d"), "must start its own X server");
        assert!(
            script.contains("DISPLAY=:$d /usr/bin/google-chrome"),
            "Chrome must run against that server"
        );
        assert!(
            script.contains("trap 'kill $cp $xp 2>/dev/null' EXIT HUP INT TERM"),
            "both processes must be killed on exit"
        );
    }

    /// Xvfb hides Chrome from the screen, not from the screen reader: the
    /// accessibility bus is per-session, so an off-screen Chrome would still
    /// register as a second "Google Chrome" application for Orca to wander
    /// into, and the user's arrow keys would stop reaching sicompass. The fix
    /// rests entirely on Chrome being unable to resolve the address below, so
    /// that is what this pins.
    #[test]
    #[cfg(target_os = "linux")]
    fn offscreen_chrome_cannot_reach_the_accessibility_bus() {
        let env = offscreen_chrome_env();
        let (key, addr) = env[0];
        assert_eq!(key, "AT_SPI_BUS_ADDRESS");

        let path = addr
            .strip_prefix("unix:path=")
            .expect("must be a unix socket address so the connection simply fails");
        assert!(
            !std::path::Path::new(path).exists(),
            "{path} exists, so Chrome could reach a real bus through it"
        );
        // An absolute path, or Chrome would resolve it against its own cwd.
        assert!(path.starts_with('/'), "{path} must be absolute");
    }

    // ---- html_to_ffon unit tests ----

    #[test]
    fn test_paragraph_under_heading_becomes_child() {
        let result = html_to_ffon(
            "<html><body><h2>Section</h2><p>Content</p></body></html>",
            "https://example.com",
        );
        let section = result
            .iter()
            .find(|e| e.as_obj().map_or(false, |o| o.key == "Section"));
        assert!(section.is_some(), "h2 should become an Obj");
        let children = &section.unwrap().as_obj().unwrap().children;
        assert!(
            children
                .iter()
                .any(|c| c.as_str().map_or(false, |s| s.contains("Content"))),
            "paragraph should be a child of the heading, not a sibling"
        );
    }

    #[test]
    fn test_nested_headings_build_outline() {
        let result = html_to_ffon(
            "<html><body><h1>Top</h1><h2>Sub</h2><p>Leaf</p></body></html>",
            "https://example.com",
        );
        let top = result
            .iter()
            .find(|e| e.as_obj().map_or(false, |o| o.key == "Top"));
        assert!(top.is_some(), "h1 should be at the top level");
        let top_children = &top.unwrap().as_obj().unwrap().children;
        let sub = top_children
            .iter()
            .find(|e| e.as_obj().map_or(false, |o| o.key == "Sub"));
        assert!(sub.is_some(), "h2 should be a child of h1");
        let sub_children = &sub.unwrap().as_obj().unwrap().children;
        assert!(
            sub_children
                .iter()
                .any(|c| c.as_str().map_or(false, |s| s.contains("Leaf"))),
            "paragraph should be a child of h2"
        );
    }

    #[test]
    fn test_custom_element_wrappers_do_not_collapse_to_one_blob() {
        // Regression: sites like elevenways.be wrap their real blocks (<h2>,
        // <p>) inside custom elements the converter doesn't recognize
        // (<al-widget>, <micro-copy>, web components, …). The DOM->FFON block
        // detection must look for a block descendant at any depth, not just
        // among direct children, or the whole subtree flattens into a single
        // giant string. The check is structural, so it holds for ANY custom
        // element name, not a hard-coded list.
        let result = html_to_ffon(
            "<html><body><main>\
               <al-widgets><al-widget><div>\
                 <h2>Alpha</h2><p>First paragraph</p>\
               </div></al-widget><al-widget><div>\
                 <h2>Beta</h2><p>Second paragraph</p>\
               </div></al-widget></al-widgets>\
             </main></body></html>",
            "https://example.com",
        );
        // Both headings survive as their own navigable nodes...
        fn find_key<'a>(elems: &'a [FfonElement], key: &str) -> bool {
            elems.iter().any(|e| match e {
                FfonElement::Obj(o) => o.key == key || find_key(&o.children, key),
                _ => false,
            })
        }
        assert!(find_key(&result, "Alpha"), "h2 'Alpha' lost: {result:?}");
        assert!(find_key(&result, "Beta"), "h2 'Beta' lost: {result:?}");
        // ...and no single string swallows the entire main content.
        fn any_blob(elems: &[FfonElement]) -> bool {
            elems.iter().any(|e| match e {
                FfonElement::Str(s) => {
                    s.contains("First paragraph") && s.contains("Second paragraph")
                }
                FfonElement::Obj(o) => any_blob(&o.children),
            })
        }
        assert!(
            !any_blob(&result),
            "content collapsed into one blob: {result:?}"
        );
    }

    #[test]
    fn test_whitespace_normalized_in_paragraph() {
        let result = html_to_ffon(
            "<html><body><p>Hello\n    world\t  here</p></body></html>",
            "https://example.com",
        );
        assert!(
            result
                .iter()
                .any(|e| e.as_str().map_or(false, |s| s == "Hello world here")),
            "internal whitespace should be collapsed to single spaces"
        );
    }

    #[test]
    fn test_empty_page_returns_placeholder() {
        let result = html_to_ffon("<html><body></body></html>", "https://example.com");
        // empty body → placeholder
        assert!(result.len() <= 1);
    }

    #[test]
    fn test_paragraph_becomes_str() {
        let result = html_to_ffon(
            "<html><body><p>Hello world</p></body></html>",
            "https://example.com",
        );
        assert!(
            result
                .iter()
                .any(|e| e.as_str().map_or(false, |s| s.contains("Hello world")))
        );
    }

    #[test]
    fn test_heading_becomes_obj() {
        let result = html_to_ffon(
            "<html><body><h1>Title</h1></body></html>",
            "https://example.com",
        );
        assert!(
            result
                .iter()
                .any(|e| e.as_obj().map_or(false, |o| o.key.contains("Title")))
        );
    }

    #[test]
    fn test_script_skipped() {
        let result = html_to_ffon(
            "<html><body><script>alert('x')</script><p>visible</p></body></html>",
            "https://example.com",
        );
        // No element should contain script content
        for e in &result {
            if let Some(s) = e.as_str() {
                assert!(!s.contains("alert"));
            }
        }
        assert!(
            result
                .iter()
                .any(|e| e.as_str().map_or(false, |s| s.contains("visible")))
        );
    }

    // ---- what FFON does with a synthesized landmark ----
    //
    // The banding pass wraps geometric regions in real <nav>/<main>/<footer>
    // elements, so these four pin the SDK behaviour it is built on.  They are
    // pure, so they run in the default suite — unlike the banding algorithm
    // itself, which is browser-side JS and needs Chrome.

    #[test]
    fn synthesized_landmarks_group_the_page() {
        // The reason banding is worth more than reordering. FFON nests every
        // element after a heading underneath it regardless of DOM ancestry, so
        // without a landmark boundary an <h1> swallows the whole rest of the
        // page — which is how bpost once filed itself under a login dropdown's
        // heading. A landmark is the only thing that stops it.
        let result = html_to_ffon(
            "<html><body><nav><a href='/a'>Alpha</a><a href='/b'>Beta</a></nav>\
             <main><h1>Article</h1><p>Body text</p></main>\
             <footer><a href='/p'>Privacy</a></footer></body></html>",
            "https://example.com",
        );
        let keys: Vec<String> = result
            .iter()
            .map(|e| {
                e.as_obj()
                    .map(|o| o.key.clone())
                    .or_else(|| e.as_str().map(|s| s.to_owned()))
                    .unwrap_or_default()
            })
            .collect();
        let joined = keys.join(" | ");
        assert!(
            keys.iter().any(|k| k.starts_with("navigation")),
            "expected a navigation landmark; got: {joined}"
        );
        assert!(
            keys.iter().any(|k| k == "main content"),
            "expected a main content landmark; got: {joined}"
        );
        assert!(
            keys.iter().any(|k| k == "footer"),
            "expected a footer landmark; got: {joined}"
        );
        // The heading must not have absorbed the footer.
        let article = format!(
            "{:?}",
            result
                .iter()
                .find_map(|e| e.as_obj())
                .map(|o| o.children.clone())
                .unwrap_or_default()
        );
        let main = result
            .iter()
            .find_map(|e| e.as_obj().filter(|o| o.key == "main content"))
            .expect("main content obj");
        assert!(
            !format!("{main:?}").contains("Privacy"),
            "the <h1> must not swallow what follows the landmark; got: {article}"
        );
    }

    #[test]
    fn a_landmark_boundary_holds_even_when_its_name_collapses_away() {
        // A region that opens with a heading — bpost's login dropdown, and half
        // the menus on the web — loses the landmark *name*: "navigation" is a
        // generic container key, so FFON collapses the wrapper and the
        // heading's name wins. The containment survives, and containment is the
        // point, so the banding pass wraps such a run anyway rather than
        // declining. Undoing the wrap to save the name would restore exactly
        // the bug the wrap is there to prevent, as the second half shows.
        let wrapped = html_to_ffon(
            "<html><body><nav><h2>Menu</h2><a href='/a'>Alpha</a></nav>\
             <p>AFTER-TEXT</p></body></html>",
            "https://example.com",
        );
        let after_is_sibling = wrapped
            .iter()
            .any(|e| e.as_str().is_some_and(|s| s.contains("AFTER-TEXT")));
        assert!(
            after_is_sibling,
            "the landmark must stop the heading swallowing what follows; got: {wrapped:?}"
        );

        // The same markup without the landmark: the heading takes everything
        // after it, regardless of DOM ancestry.
        let bare = html_to_ffon(
            "<html><body><h2>Menu</h2><a href='/a'>Alpha</a>\
             <p>AFTER-TEXT</p></body></html>",
            "https://example.com",
        );
        let bare_after_is_sibling = bare
            .iter()
            .any(|e| e.as_str().is_some_and(|s| s.contains("AFTER-TEXT")));
        assert!(
            !bare_after_is_sibling,
            "without a landmark the heading swallows it — this is the bug \
             banding exists to fix; got: {bare:?}"
        );
    }

    #[test]
    fn wrapping_a_run_in_a_landmark_does_not_renumber_forms() {
        // The load-bearing property of the whole banding design: wrapping a
        // contiguous run in a new parent leaves document preorder alone, so
        // `document.forms[n]` on the live page still lines up. Only reordering
        // could break that, which is why reordering skips containers holding
        // forms and verifies the sequence afterwards.
        let plain = "<html><body><form><input name='one'></form>\
                     <form><input name='two'></form></body></html>";
        let wrapped = "<html><body><form><input name='one'></form>\
                       <main><form><input name='two'></form></main></body></html>";
        let (_e, before) = html_to_ffon_with_forms(plain, "https://example.com");
        let (_e, after) = html_to_ffon_with_forms(wrapped, "https://example.com");
        let key_of = |map: &FormMap, name: &str| -> Option<String> {
            map.iter()
                .find(|(_, v)| v.css_selector.contains(name))
                .map(|(k, _)| k.clone())
        };
        assert_eq!(
            key_of(&before, "two"),
            key_of(&after, "two"),
            "wrapping must not renumber; before: {before:?} after: {after:?}"
        );
        assert!(
            key_of(&after, "two")
                .unwrap_or_default()
                .starts_with("form_2/"),
            "the second form is still form_2; got: {after:?}"
        );
    }

    #[test]
    fn a_generic_nav_is_renamed_from_its_content() {
        // A synthesized navigation landmark does not read as a bare
        // "navigation": FFON renames generic containers from what is inside.
        // That is why wrapping bpost's hidden menu still leaves "Pakje
        // verzenden" visible in the top-level keys.
        let result = html_to_ffon(
            "<html><body><nav><a href='/s'>Pakje verzenden</a>\
             <a href='/o'>Pakje ontvangen</a></nav></body></html>",
            "https://example.com",
        );
        let rendered = format!("{result:?}");
        assert!(
            rendered.contains("navigation:") && rendered.contains("Pakje verzenden"),
            "the container is named from its links; got: {rendered}"
        );
    }

    #[test]
    fn test_nav_renders_children() {
        let result = html_to_ffon(
            "<html><body><nav><a href='/'>Home</a></nav><p>content</p></body></html>",
            "https://example.com",
        );
        // Landmark elements are now wrapped in a named Obj, so the nav link is
        // nested inside the "navigation" container rather than at the top level.
        let nav = result
            .iter()
            .find_map(|e| e.as_obj())
            .filter(|o| o.key.starts_with("navigation"))
            .expect("nav should be wrapped in a navigation Obj");
        let found = nav.children.iter().any(|c| {
            c.as_obj().map_or(false, |o| {
                o.key.contains("<link>") && o.key.contains("Home")
            })
        });
        assert!(
            found,
            "nav link should render inside the navigation Obj, got: {result:?}"
        );
    }

    #[test]
    fn test_footer_renders_children() {
        let result = html_to_ffon(
            "<html><body><footer><p>Copyright</p></footer></body></html>",
            "https://example.com",
        );
        // The footer's text is now nested inside the "footer" landmark Obj.
        let footer = result
            .iter()
            .find_map(|e| e.as_obj())
            .filter(|o| o.key == "footer")
            .expect("footer should be wrapped in a footer Obj");
        let found = footer
            .children
            .iter()
            .any(|c| c.as_str().map_or(false, |s| s.contains("Copyright")));
        assert!(
            found,
            "footer children should render inside the footer Obj, got: {result:?}"
        );
    }

    #[test]
    fn test_link_gets_link_tag() {
        let result = html_to_ffon(
            "<html><body><p><a href='https://rust-lang.org'>Rust</a></p></body></html>",
            "https://example.com",
        );
        // Links inside <p> are now Obj elements with <link> in the key
        let found = result.iter().any(|e| {
            e.as_obj().map_or(false, |o| {
                o.key.contains("<link>") && o.key.contains("rust-lang.org")
            })
        });
        assert!(found, "link should be an Obj with <link> tag in key");
    }

    #[test]
    fn test_relative_link_resolved() {
        let result = html_to_ffon(
            "<html><body><p><a href='/page'>Page</a></p></body></html>",
            "https://example.com",
        );
        // Links inside <p> are now Obj elements with <link> in the key
        let found = result.iter().any(|e| {
            e.as_obj()
                .map_or(false, |o| o.key.contains("example.com/page"))
        });
        assert!(
            found,
            "relative link should be resolved and appear as Obj key"
        );
    }

    #[test]
    fn test_unordered_list_becomes_obj() {
        let result = html_to_ffon(
            "<html><body><ul><li>Alpha</li><li>Beta</li></ul></body></html>",
            "https://example.com",
        );
        // The navigability pass enriches the generic "list" key from its items.
        let list = result
            .iter()
            .find(|e| e.as_obj().map_or(false, |o| o.key.starts_with("list")));
        assert!(list.is_some());
        let children = &list.unwrap().as_obj().unwrap().children;
        assert_eq!(children.len(), 2);
        assert!(children[0].as_str().unwrap().contains("Alpha"));
        assert!(children[1].as_str().unwrap().contains("Beta"));
    }

    #[test]
    fn test_ordered_list_numbered() {
        let result = html_to_ffon(
            "<html><body><ol><li>First</li><li>Second</li></ol></body></html>",
            "https://example.com",
        );
        let list = result.iter().find(|e| {
            e.as_obj()
                .map_or(false, |o| o.key.starts_with("ordered list"))
        });
        assert!(list.is_some());
        let children = &list.unwrap().as_obj().unwrap().children;
        assert!(children[0].as_str().unwrap().starts_with("1."));
        assert!(children[1].as_str().unwrap().starts_with("2."));
    }

    #[test]
    fn test_table_row_pipe_delimited() {
        let result = html_to_ffon(
            "<html><body><table><tr><td>A</td><td>B</td></tr></table></body></html>",
            "https://example.com",
        );
        let row = result
            .iter()
            .find(|e| e.as_str().map_or(false, |s| s.contains(" | ")));
        assert!(row.is_some());
        assert!(row.unwrap().as_str().unwrap().contains("A | B"));
    }

    #[test]
    fn test_image_shows_alt() {
        let result = html_to_ffon(
            "<html><body><img alt='A diagram' src='x.png'/></body></html>",
            "https://example.com",
        );
        let img = result.iter().find(|e| {
            e.as_str()
                .map_or(false, |s| s.contains("A diagram") && s.contains("[img]"))
        });
        assert!(img.is_some());
    }

    #[test]
    fn test_fetch_returns_meta_and_url_bar() {
        let mut p = WebbrowserProvider::new();
        let items = p.fetch();
        // Index 0: url bar (no page loaded → str)
        assert!(items[0].as_str().is_some());
    }

    #[test]
    fn test_fetch_url_bar_contains_input_tag() {
        let mut p = WebbrowserProvider::new();
        let items = p.fetch();
        let url_bar = items[0].as_str().unwrap();
        assert!(url_bar.contains("<input>") && url_bar.contains("</input>"));
    }

    #[test]
    fn normalize_url_input_prepends_https_when_no_scheme() {
        assert_eq!(
            normalize_url_input("example.com"),
            Some("https://example.com".to_owned()),
        );
    }

    #[test]
    fn normalize_url_input_preserves_existing_scheme() {
        assert_eq!(
            normalize_url_input("http://example.com"),
            Some("http://example.com".to_owned()),
        );
        assert_eq!(
            normalize_url_input("https://example.com"),
            Some("https://example.com".to_owned()),
        );
    }

    #[test]
    fn normalize_url_input_trims_whitespace() {
        assert_eq!(
            normalize_url_input("   example.com  "),
            Some("https://example.com".to_owned()),
        );
    }

    #[test]
    fn normalize_url_input_returns_none_for_empty() {
        assert_eq!(normalize_url_input(""), None);
        assert_eq!(normalize_url_input("   "), None);
    }

    #[test]
    fn test_commands_includes_refresh() {
        let p = WebbrowserProvider::new();
        assert!(p.commands().contains(&"refresh".to_owned()));
    }

    #[test]
    fn test_resolve_href_absolute() {
        let result = html_resolve_href("https://other.com/page", "https://base.com");
        assert_eq!(result, "https://other.com/page");
    }

    #[test]
    fn test_resolve_href_relative() {
        let result = html_resolve_href("/path/to/page", "https://example.com/current");
        assert!(result.contains("example.com/path/to/page"));
    }

    #[test]
    fn test_resolve_href_anchor_preserved() {
        // Fragment-only hrefs are returned as-is for in-page navigation.
        let result = html_resolve_href("#section", "https://example.com");
        assert_eq!(result, "#section");
    }

    #[test]
    fn test_resolve_href_anchor_complex() {
        let result = html_resolve_href("#page-main-content", "https://www.hln.be/");
        assert_eq!(result, "#page-main-content");
    }

    // ---- fragment link parsing ----

    #[test]
    fn test_fragment_link_in_p_becomes_navigable_obj() {
        // <a href="#foo"> inside a paragraph should produce an Obj with <link>#foo</link>
        let result = html_to_ffon(
            "<html><body><p><a href=\"#foo\">skip to foo</a></p></body></html>",
            "https://example.com",
        );
        let found = result.iter().any(|e| {
            e.as_obj()
                .map_or(false, |o| o.key.contains("<link>#foo</link>"))
        });
        assert!(
            found,
            "fragment link should become an Obj with <link>#foo</link>: {result:?}"
        );
    }

    #[test]
    fn test_heading_with_id_gets_id_tag() {
        let result = html_to_ffon(
            "<html><body><h2 id=\"bar\">Section</h2></body></html>",
            "https://example.com",
        );
        let heading = result
            .iter()
            .find(|e| e.as_obj().map_or(false, |o| o.key.contains("Section")));
        assert!(heading.is_some(), "heading should exist: {result:?}");
        let key = &heading.unwrap().as_obj().unwrap().key;
        assert!(
            key.contains("<id>bar</id>"),
            "heading key should contain <id>bar</id>: {key}"
        );
    }

    #[test]
    fn test_container_with_id_propagates_to_first_child() {
        // <main id="x"><p>hi</p></main> — main is a CONTAINER_TAG, its id
        // should propagate to the first emitted element (the paragraph).
        let result = html_to_ffon(
            "<html><body><main id=\"x\"><p>hi</p></main></body></html>",
            "https://example.com",
        );
        let has_id_tag = result.iter().any(|e| match e {
            FfonElement::Str(s) => s.contains("<id>x</id>"),
            FfonElement::Obj(o) => o.key.contains("<id>x</id>"),
        });
        assert!(
            has_id_tag,
            "first element inside <main id=x> should have <id>x</id>: {result:?}"
        );
    }

    #[test]
    fn test_ul_with_id_annotates_wrapper_not_item() {
        let result = html_to_ffon(
            "<html><body><ul id=\"things\"><li>a</li><li>b</li></ul></body></html>",
            "https://example.com",
        );
        // The list wrapper Obj (not an li) should carry the id tag.
        let list = result.iter().find(|e| {
            e.as_obj().map_or(false, |o| {
                o.key.contains("list") && o.key.contains("<id>things</id>")
            })
        });
        assert!(
            list.is_some(),
            "list wrapper should have <id>things</id>: {result:?}"
        );
        // Items should NOT have the id tag
        if let Some(FfonElement::Obj(l)) = list {
            let item_has_id = l.children.iter().any(|c| match c {
                FfonElement::Str(s) => s.contains("<id>things</id>"),
                FfonElement::Obj(o) => o.key.contains("<id>things</id>"),
            });
            assert!(!item_has_id, "list items should not have the id tag");
        }
    }

    #[test]
    fn test_skip_link_and_target_end_to_end() {
        // Full skip-link scenario: <a href="#foo"> + <main id="foo">
        let result = html_to_ffon(
            "<html><body>\
             <a href=\"#foo\">skip</a>\
             <main id=\"foo\"><p>Main content</p></main>\
             </body></html>",
            "https://example.com",
        );
        // Should contain a link obj pointing to #foo
        let has_link = result.iter().any(|e| {
            e.as_obj()
                .map_or(false, |o| o.key.contains("<link>#foo</link>"))
        });
        assert!(has_link, "should have a navigable link to #foo: {result:?}");
        // Should contain an element tagged with <id>foo</id>
        let has_target = result.iter().any(|e| match e {
            FfonElement::Str(s) => s.contains("<id>foo</id>"),
            FfonElement::Obj(o) => o.key.contains("<id>foo</id>"),
        });
        assert!(
            has_target,
            "should have a target element with <id>foo</id>: {result:?}"
        );
    }

    // ---- is_consent_wall unit tests ----

    #[test]
    fn test_is_consent_wall_detects_dpgmedia() {
        assert!(is_consent_wall_str(
            "https://myprivacy.dpgmedia.be/consent?siteKey=Uqxf9TXhjmaG4pbQ&callbackUrl=https%3A%2F%2Fwww.hln.be%2F"
        ));
    }

    #[test]
    fn test_is_consent_wall_passes_normal_url() {
        assert!(!is_consent_wall_str("https://www.hln.be/sport"));
    }

    #[test]
    fn test_is_consent_wall_detects_generic_consent_path() {
        assert!(is_consent_wall_str(
            "https://example.com/consent?redirect=/"
        ));
    }

    #[test]
    fn test_is_consent_wall_detects_cookie_consent_path() {
        assert!(is_consent_wall_str(
            "https://example.com/page/cookie-consent/accept"
        ));
    }

    #[test]
    fn test_is_consent_wall_detects_sourcepoint() {
        assert!(is_consent_wall_str(
            "https://cdn.sp-prod.net/unified/v2/notice.html"
        ));
    }

    #[test]
    fn test_is_consent_wall_detects_consent_subdomain() {
        assert!(is_consent_wall_str(
            "https://consent.youtube.com/m?continue=https%3A%2F%2Fwww.youtube.com%2F"
        ));
    }

    #[test]
    fn test_is_consent_wall_detects_privacy_mgmt() {
        assert!(is_consent_wall_str(
            "https://privacy-mgmt.com/cmp?redirect=https://example.com"
        ));
    }

    #[test]
    fn test_is_consent_wall_detects_cmp_subdomain() {
        assert!(is_consent_wall_str("https://cmp.example.com/notice"));
    }

    // ---- html_has_consent_wall unit tests ----
    // Google/YouTube serve the GDPR wall inline at www.google.com (URL unchanged),
    // so is_consent_wall_str on the URL misses it; the content is the only signal.

    #[test]
    fn test_html_consent_wall_detects_google_save_endpoint() {
        let html = r#"<button jsaction="click:...">
            <div role="none">Alles accepteren</div></button>
            <script>var u="https://consent.google.com/save?continue=https://www.google.com/";</script>"#;
        assert!(html_has_consent_wall(html));
    }

    #[test]
    fn test_html_consent_wall_ignores_google_d_endpoint() {
        // The /d endpoint persists as a footer privacy link on the *accepted*
        // page (verified live: accepted homepage has /d but zero /save), so it
        // must NOT be treated as a wall on its own.
        let html =
            r#"<a href="https://consent.google.com/d?continue=https://www.google.com/">More</a>"#;
        assert!(!html_has_consent_wall(html));
    }

    #[test]
    fn test_html_consent_wall_detects_youtube() {
        let html = r#"<form action="https://consent.youtube.com/save"></form>"#;
        assert!(html_has_consent_wall(html));
    }

    #[test]
    fn test_html_consent_wall_ignores_normal_page() {
        // A plain search-results page (post-accept) or an article must not match,
        // even if it merely mentions the word "consent" or links to google.com.
        let html = r#"<html><body><form action="/search" role="search">
            <input name="q" aria-label="Zoeken"></form>
            <p>We asked for consent before continuing.</p>
            <a href="https://www.google.com/">Google</a></body></html>"#;
        assert!(!html_has_consent_wall(html));
    }

    // ---- is_reject_text unit tests ----

    #[test]
    fn test_is_reject_text_english() {
        assert!(is_reject_text("Reject all"));
        assert!(is_reject_text("Decline cookies"));
        assert!(is_reject_text("Only necessary cookies"));
        assert!(is_reject_text("Manage preferences"));
        assert!(is_reject_text("Settings"));
    }

    #[test]
    fn test_is_reject_text_dutch() {
        assert!(is_reject_text("Weigeren"));
        assert!(is_reject_text("Instellingen"));
        assert!(is_reject_text("Alleen noodzakelijke"));
    }

    #[test]
    fn test_is_reject_text_german() {
        assert!(is_reject_text("Ablehnen"));
        assert!(is_reject_text("Nur notwendige"));
    }

    #[test]
    fn test_is_reject_text_false_for_accept() {
        assert!(!is_reject_text("Accept all"));
        assert!(!is_reject_text("Alles accepteren"));
        assert!(!is_reject_text("Tout accepter"));
    }

    // ---- is_accept_keyword unit tests ----

    #[test]
    fn test_is_accept_keyword_english() {
        assert!(is_accept_keyword("Accept all cookies"));
        assert!(is_accept_keyword("Allow all"));
        assert!(is_accept_keyword("Agree and continue"));
        assert!(is_accept_keyword("I accept"));
        assert!(is_accept_keyword("Got it"));
    }

    #[test]
    fn test_is_accept_keyword_dutch() {
        assert!(is_accept_keyword("Alles accepteren"));
        assert!(is_accept_keyword("Accepteer alles"));
        assert!(is_accept_keyword("Akkoord"));
        assert!(is_accept_keyword("Ja, ik accepteer"));
        assert!(is_accept_keyword("Ik ga akkoord"));
        assert!(is_accept_keyword("Alles toestaan"));
    }

    #[test]
    fn test_is_accept_keyword_french() {
        assert!(is_accept_keyword("Tout accepter"));
        assert!(is_accept_keyword("J'accepte"));
        assert!(is_accept_keyword("Accepter et fermer"));
        assert!(is_accept_keyword("Continuer et accepter"));
    }

    #[test]
    fn test_is_accept_keyword_german() {
        assert!(is_accept_keyword("Alle akzeptieren"));
        assert!(is_accept_keyword("Alles annehmen"));
        assert!(is_accept_keyword("Zustimmen"));
        assert!(is_accept_keyword("Einverstanden"));
        assert!(is_accept_keyword("Akzeptieren und weiter"));
    }

    #[test]
    fn test_is_accept_keyword_italian() {
        assert!(is_accept_keyword("Accetta tutto"));
        assert!(is_accept_keyword("Accetto"));
        assert!(is_accept_keyword("Acconsento"));
    }

    #[test]
    fn test_is_accept_keyword_spanish() {
        assert!(is_accept_keyword("Aceptar todo"));
        assert!(is_accept_keyword("Acepto"));
        assert!(is_accept_keyword("Aceptar y continuar"));
    }

    #[test]
    fn test_is_accept_keyword_reject_guard_takes_priority() {
        // "alles ablehnen" contains "alles" (part of "alles toestaan" keyword)
        // but the reject guard must fire first
        assert!(!is_accept_keyword("Alles ablehnen"));
        assert!(!is_accept_keyword("Reject all cookies"));
        assert!(!is_accept_keyword("Decline and manage settings"));
    }

    // ---- js_array unit tests ----

    #[test]
    fn test_js_array_produces_valid_json_array() {
        let result = js_array(&["foo", "bar"]);
        assert_eq!(result, r#"["foo","bar"]"#);
    }

    #[test]
    fn test_js_array_escapes_double_quotes() {
        let result = js_array(&[r#"button[data-testid="pur-accept-button"]"#]);
        // Must produce valid JSON (double-quotes escaped)
        assert!(
            result.contains(r#"\""#),
            "embedded quotes must be escaped: {result}"
        );
    }

    // ---- Interstitial detection (bot checks, CAPTCHAs) ----

    fn notice_for(html: &str) -> Option<String> {
        let elements = html_to_ffon(html, "https://example.com");
        challenge_notice(html, &elements)
    }

    #[test]
    fn challenge_notice_detects_cloudflare_block() {
        let html = r#"<html><body><h1>Sorry, you have been blocked</h1></body></html>"#;
        let notice = notice_for(html).expect("Cloudflare block wall should be recognised");
        assert!(
            notice.contains("Cloudflare"),
            "notice should name the vendor: {notice}"
        );
    }

    #[test]
    fn challenge_notice_detects_cloudflare_error_code() {
        let html = r#"<html><body><div class="cf-error-1010">Access denied</div></body></html>"#;
        assert!(notice_for(html).is_some());
    }

    #[test]
    fn challenge_notice_passes_normal_page() {
        let html =
            r#"<html><head><title>News</title></head><body><p>Article content</p></body></html>"#;
        assert_eq!(notice_for(html), None);
    }

    #[test]
    fn challenge_notice_detects_bare_captcha_page() {
        let html = r#"<html><body><h1>Verify you are human</h1>
            <div class="h-captcha" data-sitekey="x"></div></body></html>"#;
        assert!(
            notice_for(html).is_some(),
            "a page that is nothing but a CAPTCHA widget is an interstitial"
        );
    }

    #[test]
    fn challenge_notice_ignores_captcha_widget_on_a_real_page() {
        // A login page carrying a reCAPTCHA box is not an interstitial: it is
        // the page the user asked for, and must not be labelled a bot check.
        let body = "Sign in to your account. ".repeat(60); // well past the text limit
        let html =
            format!(r#"<html><body><p>{body}</p><div class="g-recaptcha"></div></body></html>"#);
        assert_eq!(notice_for(&html), None);
    }

    #[test]
    fn interstitial_page_keeps_its_own_content() {
        // The point of the notice: it is added *in front of* the page, never
        // in place of it.
        let html = r#"<html><body><h1>Sorry, you have been blocked</h1>
            <p>You can email support@example.com to be unblocked.</p></body></html>"#;
        let load = PageLoad::plain(html.to_owned());
        let (elements, _) = page_to_ffon_with_forms(&load, "https://example.com");
        assert!(
            elements[0]
                .as_str()
                .map_or(false, |s| s.contains("Cloudflare")),
            "notice should lead the page: {:?}",
            elements[0]
        );
        let rendered = format!("{elements:?}");
        assert!(
            rendered.contains("support@example.com"),
            "the wall's own text must still be there: {rendered}"
        );
    }

    #[test]
    fn load_notices_precede_content_and_content_notices() {
        // A consent wall the loader could not accept: its notice comes first,
        // then anything sniffed from the content, then the page.
        let html = r#"<html><body><h1>Sorry, you have been blocked</h1>
            <p>Accept cookies to continue.</p></body></html>"#;
        let load = PageLoad {
            html: html.to_owned(),
            notices: vec!["Cookie-consent wall".to_owned()],
        };
        let (elements, _) = page_to_ffon_with_forms(&load, "https://example.com");
        assert!(
            elements[0]
                .as_str()
                .map_or(false, |s| s.contains("Cookie-consent wall"))
        );
        assert!(
            elements[1]
                .as_str()
                .map_or(false, |s| s.contains("Cloudflare"))
        );
    }

    // Real-browser integration test — requires Chrome/Chromium and network access.
    // Run with: cargo test -p sicompass-webbrowser -- --ignored
    #[test]
    #[ignore]
    fn test_chromium_fetches_real_cloudflare_site() {
        let result = fetch_html_chromium("https://www.gva.be");
        assert!(result.is_ok(), "fetch failed: {:?}", result.err());
        let load = result.unwrap();
        assert!(!load.html.is_empty(), "expected non-empty HTML from gva.be");
        // Whether or not the site answers with a bot check, the page itself is
        // what gets rendered; a wall only adds a leading notice.
        let (elements, _) = page_to_ffon_with_forms(&load, "https://www.gva.be");
        assert!(
            !elements.is_empty(),
            "expected rendered content, not an error"
        );
        if let Some(notice) = challenge_notice(&load.html, &elements) {
            eprintln!("gva.be answered with an interstitial: {notice}");
        }
    }

    // ---- Provider path / form helpers ----

    #[test]
    fn extract_form_key_finds_form_segment() {
        assert_eq!(
            extract_form_key("/https://example.com/form_1/email"),
            Some("form_1/email".to_owned())
        );
    }

    #[test]
    fn extract_form_key_returns_none_for_url_path() {
        assert_eq!(extract_form_key("/https://example.com"), None);
        assert_eq!(extract_form_key("/"), None);
    }

    #[test]
    fn js_quote_escapes_double_quotes() {
        assert_eq!(js_quote(r#"input[name="q"]"#), r#""input[name=\"q\"]""#);
    }

    #[test]
    fn js_quote_escapes_backslashes() {
        assert_eq!(js_quote(r"a\b"), r#""a\\b""#);
    }

    #[test]
    fn provider_push_pop_path() {
        let mut p = WebbrowserProvider::new();
        assert_eq!(p.current_path(), "/");
        p.push_path("https://example.com");
        assert_eq!(p.current_path(), "/https://example.com");
        p.push_path("form_1");
        assert_eq!(p.current_path(), "/https://example.com/form_1");
        p.pop_path();
        assert_eq!(p.current_path(), "/https://example.com");
        p.pop_path();
        assert_eq!(p.current_path(), "/");
        // pop at root is a no-op
        p.pop_path();
        assert_eq!(p.current_path(), "/");
    }

    #[test]
    fn provider_set_current_path() {
        let mut p = WebbrowserProvider::new();
        p.set_current_path("/https://x.com/form_2/q");
        assert_eq!(p.current_path(), "/https://x.com/form_2/q");
        assert_eq!(
            extract_form_key(p.current_path()),
            Some("form_2/q".to_owned())
        );
    }

    #[test]
    fn build_fill_js_contains_selector_and_value() {
        let js = build_fill_js(1, 0, "#my-input", "hello world", false);
        assert!(js.contains("\"#my-input\""), "selector missing: {js}");
        assert!(js.contains("\"hello world\""), "value missing: {js}");
        assert!(js.contains("dispatchEvent"), "event dispatch missing");
        // form_index 1 resolves against the first form in document order.
        assert!(
            js.contains("document.forms[0]"),
            "form scoping missing: {js}"
        );
    }

    #[test]
    fn build_fill_js_standalone_resolves_against_document_and_submits() {
        let js = build_fill_js(0, 0, "[name=\"q\"]", "query", true);
        assert!(
            js.contains("const root = document;"),
            "global scope missing: {js}"
        );
        assert!(
            js.contains("KeyboardEvent"),
            "Enter submit missing for standalone: {js}"
        );
    }

    #[test]
    fn provider_tick_drains_ready_content() {
        let mut p = WebbrowserProvider::new();
        // Simulate a background thread delivering content.
        {
            let mut guard = p.ready_content.lock().unwrap();
            *guard = Some((vec![FfonElement::new_str("result")], FormMap::new()));
        }
        assert!(p.tick(), "tick should return true when content is ready");
        assert!(
            p.cached_page
                .as_ref()
                .and_then(|c| c.elements.first())
                .and_then(|e| e.as_str())
                .map_or(false, |s| s == "result"),
            "cached_page should hold the delivered content"
        );
        // Second tick with no new content returns false.
        assert!(!p.tick());
    }

    #[test]
    fn page_landing_asks_app_to_enter_content() {
        let mut p = WebbrowserProvider::new();
        // What `commit_edit` does for a URL navigation.
        p.pending_enter_content = true;
        assert!(
            p.take_navigation_request().is_none(),
            "no request before the page has landed — the content isn't there to enter yet"
        );
        {
            let mut guard = p.ready_content.lock().unwrap();
            *guard = Some((vec![FfonElement::new_str("body text")], FormMap::new()));
        }
        assert!(p.tick());
        assert_eq!(
            p.take_navigation_request(),
            Some(sicompass_sdk::NavigationRequest::EnterChildren)
        );
        assert!(
            p.take_navigation_request().is_none(),
            "request must not repeat"
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn fetch_while_loading_keeps_the_url_bar_childless() {
        let mut p = WebbrowserProvider::new();
        p.current_url = "https://example.com".to_owned();
        // A page from a previous navigation, and a load now running.
        p.cached_page = Some(CachedPage {
            url: "https://old.example".to_owned(),
            elements: vec![FfonElement::new_str("stale body")],
        });
        p.load_inflight.store(true, Ordering::Release);

        let items = p.fetch();
        assert_eq!(items.len(), 2, "URL bar plus a status line: {items:?}");
        assert!(
            matches!(&items[0], FfonElement::Str(s) if s.contains("https://example.com")),
            "URL bar must be childless while loading so nothing descends into \
             the previous page: {:?}",
            items[0]
        );
        assert_eq!(items[1].as_str(), Some("Loading…"));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn browser_launch_failure_leaves_the_loading_state() {
        // A load that dies before Chrome is even up (no Chrome installed, launch
        // timed out) used to fill only the error slot. `tick` then returned
        // false, so the app never re-fetched and never drained the error: the
        // URL bar read "Loading…" for the rest of the session.
        let mut p = WebbrowserProvider::new();
        p.current_url = "https://example.com".to_owned();
        p.pending_enter_content = true;
        p.load_inflight.store(true, Ordering::Release);

        publish_load_failure(
            &p.ready_content,
            &p.pending_error,
            "Error launching browser: Chrome/Chromium not found.".to_owned(),
        );
        p.load_inflight.store(false, Ordering::Release);

        assert!(
            p.tick(),
            "the failure must signal the app to re-fetch, or the view stays on \
             \"Loading…\" forever"
        );
        let items = p.fetch();
        assert_eq!(items.len(), 1, "URL bar wrapping the failure: {items:?}");
        let body = items[0]
            .as_obj()
            .expect("URL bar gains the page as children");
        assert!(
            body.children.iter().any(|e| e
                .as_str()
                .is_some_and(|s| s.contains("Chrome/Chromium not found"))),
            "the reason has to be readable in the page: {body:?}"
        );
        assert!(
            p.take_error()
                .is_some_and(|e| e.contains("Error launching browser")),
            "and on the status line, which the app drains on the same tick"
        );
        assert_eq!(
            p.take_navigation_request(),
            Some(sicompass_sdk::NavigationRequest::EnterChildren),
            "the cursor still descends, so the error is where the user is reading"
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn url_committed_during_a_load_is_queued_not_dropped() {
        // A load is already running (the flag set here is what `load_url` would
        // have set), so committing a second URL must not spawn anything — it
        // hands the destination to the running task instead. Before, it just
        // updated the URL bar and returned, and the second page never loaded.
        let _flag = launch_flag_guard(false);
        let mut p = WebbrowserProvider::new();
        p.load_inflight.store(true, Ordering::Release);

        p.load_url("https://second.example/");

        assert_eq!(
            take_pending(&p.pending_url).as_deref(),
            Some("https://second.example/"),
            "the URL has to reach the running task, or nothing ever loads it"
        );
        assert_eq!(
            p.current_url, "https://second.example/",
            "the URL bar still shows where the user is going"
        );
        assert!(
            p.load_inflight.load(Ordering::Acquire),
            "the first load is still running; the flag stays set so `fetch` \
             keeps saying \"Loading…\""
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn queued_navigations_coalesce_to_the_newest() {
        // Typing past a URL should not cost a page load for each one.
        let _flag = launch_flag_guard(false);
        let mut p = WebbrowserProvider::new();
        p.load_inflight.store(true, Ordering::Release);
        p.load_url("https://first.example/");
        p.load_url("https://second.example/");
        p.load_url("https://third.example/");

        assert_eq!(
            take_pending(&p.pending_url).as_deref(),
            Some("https://third.example/")
        );
        assert_eq!(
            take_pending(&p.pending_url),
            None,
            "one slot, not a backlog"
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn next_target_serves_the_queue_then_releases_the_flag() {
        let inflight = AtomicBool::new(true);
        let pending = Arc::new(Mutex::new(None));

        queue_pending(&pending, "https://queued.example/");
        assert_eq!(
            next_target(&inflight, &pending).as_deref(),
            Some("https://queued.example/"),
            "a queued URL is the task's next destination"
        );
        assert!(
            inflight.load(Ordering::Acquire),
            "the chain continues, so the flag stays set and no second task spawns"
        );

        assert_eq!(
            next_target(&inflight, &pending),
            None,
            "empty queue ends the chain"
        );
        assert!(
            !inflight.load(Ordering::Acquire),
            "and releases the flag, so the next commit spawns its own task"
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn a_load_superseded_mid_flight_does_not_publish() {
        // `navigate_once` skips publishing when a newer URL is already queued.
        // Without that, the old page's content would land in `ready_content`
        // and `tick` would cache it under the newer URL now in the URL bar.
        let pending = Arc::new(Mutex::new(None));
        assert!(!has_pending(&pending));
        queue_pending(&pending, "https://newer.example/");
        assert!(
            has_pending(&pending),
            "the in-flight navigation can see that it has been superseded"
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_browser_still_on_its_disk_image_is_named_in_the_error() {
        // Chrome downloaded but never dragged to Applications: it launches from
        // the mounted installer window, so the user has every reason to believe
        // it is installed. "not found" is true but useless; the message has to
        // say what to do. Only asserts the wording when this machine is in that
        // state — the fixed half of the message is checked either way.
        let msg = chrome_missing_message();
        match chrome_on_mounted_image() {
            Some(dmg) => {
                assert!(
                    msg.contains(&dmg.display().to_string()),
                    "name the bundle we found: {msg}"
                );
                assert!(
                    msg.contains("drag it") && msg.contains("Applications"),
                    "and say how to install it: {msg}"
                );
            }
            None => assert!(
                msg.contains("SICOMPASS_CHROME_PATH"),
                "otherwise fall back to the override hint: {msg}"
            ),
        }
    }

    #[test]
    fn submit_response_does_not_move_the_cursor() {
        // Content arriving without an armed URL navigation (a form submit
        // response) must not pull the cursor out of the form the user is in.
        let mut p = WebbrowserProvider::new();
        {
            let mut guard = p.ready_content.lock().unwrap();
            *guard = Some((vec![FfonElement::new_str("results")], FormMap::new()));
        }
        assert!(p.tick());
        assert!(p.take_navigation_request().is_none());
    }

    #[test]
    fn every_navigation_arms_the_descent_into_the_page() {
        let _flag = launch_flag_guard(true);
        let mut p = WebbrowserProvider::new();
        assert!(p.commit_edit("", "https://example.invalid"));
        assert_eq!(
            p.take_navigation_request(),
            Some(sicompass_sdk::NavigationRequest::EnterChildren),
            "committing a URL should drop the user into the loaded page"
        );

        // `refresh` used to be asserted *not* to arm this, back when it grafted
        // the new content in place and so left the cursor standing on something
        // real. It now rebuilds the provider root, and while the reload is in
        // flight the URL bar is childless — a cursor inside the page is clamped
        // back to the bar. Re-arming is what carries the reader back into the
        // article they pressed F5 on. The app ignores the request unless the
        // cursor is at the provider's top level, so this cannot double-descend
        // someone who never left the page.
        let mut error = String::new();
        for cmd in ["refresh", CMD_SHOW_HIDDEN] {
            p.handle_command(cmd, "", 0, &mut error);
            assert_eq!(
                p.take_navigation_request(),
                Some(sicompass_sdk::NavigationRequest::EnterChildren),
                "`{cmd}` reloads the page, so it must land the user back in it"
            );
        }

        // The bookmark toggle is the one command that reloads nothing, so it
        // must not arm the descent: pressing `b` on a history row would yank
        // the reader into the page they were only pointing at.
        p.handle_command(CMD_TOGGLE_BOOKMARK, "", 0, &mut error);
        assert_eq!(p.take_navigation_request(), None);
    }

    // ---- URL recall history ----

    /// A provider whose history file lives in `dir`, with `init()` already run
    /// so the disk side is live. The path override is per-instance, so these
    /// tests stay parallel-safe.
    fn history_provider(dir: &std::path::Path) -> WebbrowserProvider {
        let mut p = WebbrowserProvider::new();
        p.url_history_path = Some(dir.join("history"));
        p.init();
        p
    }

    /// The URLs behind the `<button>` rows `fetch()` emitted, in order.
    fn history_rows(p: &mut WebbrowserProvider) -> Vec<String> {
        p.fetch()
            .iter()
            .filter_map(|e| e.as_str())
            .filter_map(sicompass_sdk::tags::extract_button_function_name)
            .collect()
    }

    #[test]
    fn committing_a_url_prepends_it_to_the_history() {
        let _flag = launch_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        let mut p = history_provider(dir.path());

        p.commit_edit("", "https://a.invalid");
        p.commit_edit("", "https://b.invalid");

        assert_eq!(
            p.url_history,
            vec!["https://b.invalid", "https://a.invalid"],
            "newest first"
        );
    }

    #[test]
    fn revisiting_a_url_moves_it_to_the_top_instead_of_duplicating_it() {
        let _flag = launch_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        let mut p = history_provider(dir.path());

        p.commit_edit("", "https://a.invalid");
        p.commit_edit("", "https://b.invalid");
        p.commit_edit("", "https://a.invalid");

        assert_eq!(
            p.url_history,
            vec!["https://a.invalid", "https://b.invalid"],
            "a revisit ranks, it does not accumulate"
        );
    }

    #[test]
    fn url_history_survives_a_restart() {
        let _flag = launch_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        {
            let mut p = history_provider(dir.path());
            p.commit_edit("", "https://a.invalid");
            p.commit_edit("", "https://b.invalid");
        }
        let p = history_provider(dir.path());
        assert_eq!(
            p.url_history,
            vec!["https://b.invalid", "https://a.invalid"]
        );
    }

    #[test]
    fn the_history_file_is_newest_first_one_url_per_line() {
        let _flag = launch_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        let mut p = history_provider(dir.path());
        p.commit_edit("", "https://a.invalid");
        p.commit_edit("", "https://b.invalid");

        let raw = std::fs::read_to_string(dir.path().join("history")).unwrap();
        assert_eq!(raw, "https://b.invalid\nhttps://a.invalid\n");
    }

    #[test]
    fn fetch_puts_history_below_the_url_bar_as_root_siblings() {
        let _flag = launch_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        let mut p = history_provider(dir.path());
        p.commit_edit("", "https://a.invalid");
        p.commit_edit("", "https://b.invalid");

        let items = p.fetch();
        assert_eq!(items.len(), 3, "URL bar plus two history rows: {items:?}");

        // Index 0 is still the URL bar, and it is the loaded page's parent.
        let url_bar = items[0].as_obj().expect("a page is loaded");
        assert!(sicompass_sdk::tags::has_input(&url_bar.key));
        assert!(
            !url_bar
                .children
                .iter()
                .filter_map(|c| c.as_str())
                .any(sicompass_sdk::tags::has_button),
            "history must not be mixed into the page content: {:?}",
            url_bar.children
        );

        // Everything below it is a history button, newest first.
        for row in &items[1..] {
            assert!(sicompass_sdk::tags::has_button(row.as_str().unwrap()));
        }
        assert_eq!(
            history_rows(&mut p),
            vec!["https://b.invalid", "https://a.invalid"]
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn fetch_keeps_history_visible_while_loading() {
        let _flag = launch_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        let mut p = history_provider(dir.path());
        p.commit_edit("", "https://a.invalid");

        p.load_inflight.store(true, Ordering::Release);
        let items = p.fetch();
        p.load_inflight.store(false, Ordering::Release);

        assert_eq!(
            items.len(),
            3,
            "URL bar, status line, and the history row: {items:?}"
        );
        assert_eq!(items[1].as_str().unwrap(), "Loading…");
        assert_eq!(
            sicompass_sdk::tags::extract_button_function_name(items[2].as_str().unwrap()),
            Some("https://a.invalid".to_owned()),
        );
    }

    #[test]
    fn pressing_a_history_row_moves_it_to_the_top_and_loads_it() {
        let _flag = launch_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        let mut p = history_provider(dir.path());
        p.commit_edit("", "https://a.invalid");
        p.commit_edit("", "https://b.invalid");
        let _ = p.take_navigation_request();

        p.on_button_press("https://a.invalid");

        assert_eq!(p.current_url, "https://a.invalid", "the site is loaded");
        assert_eq!(
            p.url_history,
            vec!["https://a.invalid", "https://b.invalid"],
            "the pressed row moves to the top"
        );
        assert_eq!(
            p.take_navigation_request(),
            Some(sicompass_sdk::NavigationRequest::EnterChildren),
            "and the user is dropped into the page, as if they had typed it"
        );
    }

    #[test]
    fn a_form_submit_press_is_not_mistaken_for_a_history_row() {
        let _flag = launch_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        let mut p = history_provider(dir.path());
        p.commit_edit("", "https://a.invalid");
        let before = p.url_history.clone();

        p.on_button_press("submit:form_1");

        assert_eq!(p.url_history, before);
    }

    #[test]
    fn a_page_button_carrying_a_url_does_not_navigate_the_tab() {
        let _flag = launch_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        let mut p = history_provider(dir.path());
        p.commit_edit("", "https://a.invalid");

        // Shaped like a history row, but never recorded as one.
        p.on_button_press("https://elsewhere.invalid");

        assert_eq!(p.current_url, "https://a.invalid");
        assert_eq!(p.url_history, vec!["https://a.invalid"]);
    }

    #[test]
    fn reloading_commands_do_not_reorder_the_history() {
        let _flag = launch_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        let mut p = history_provider(dir.path());
        p.commit_edit("", "https://a.invalid");
        p.commit_edit("", "https://b.invalid");
        // The user walked back to an older site, so it is on top again.
        p.on_button_press("https://a.invalid");
        let ranked = p.url_history.clone();

        let mut error = String::new();
        for cmd in [
            "refresh",
            CMD_SHOW_HIDDEN,
            // Not a reload, but held to the same rule: marking a row must not
            // move it, or it jumps out from under the cursor that marked it.
            CMD_TOGGLE_BOOKMARK,
        ] {
            p.handle_command(cmd, "", 0, &mut error);
            assert_eq!(
                p.url_history, ranked,
                "`{cmd}` reloads the current page and must not touch the ranking"
            );
        }
    }

    #[test]
    fn url_history_size_setting_drops_the_oldest() {
        let _flag = launch_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        let mut p = history_provider(dir.path());
        p.commit_edit("", "https://a.invalid");
        p.commit_edit("", "https://b.invalid");
        p.commit_edit("", "https://c.invalid");

        p.on_setting_change("urlHistorySize", "2");

        assert_eq!(
            p.url_history,
            vec!["https://c.invalid", "https://b.invalid"]
        );
        // And the cap survives the next navigation's read-merge-write, rather
        // than the dropped entry coming back off disk.
        p.commit_edit("", "https://d.invalid");
        assert_eq!(
            p.url_history,
            vec!["https://d.invalid", "https://c.invalid"]
        );
    }

    #[test]
    fn url_history_size_zero_is_unbounded() {
        let _flag = launch_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        let mut p = history_provider(dir.path());
        p.on_setting_change("urlHistorySize", "0");
        for i in 0..10 {
            p.commit_edit("", &format!("https://{i}.invalid"));
        }
        assert_eq!(p.url_history.len(), 10);
    }

    #[test]
    fn on_setting_change_ignores_a_garbage_history_size() {
        let mut p = WebbrowserProvider::new();
        p.on_setting_change("urlHistorySize", "lots");
        assert_eq!(p.url_history_size, 50_000);
    }

    #[test]
    fn a_url_containing_a_close_button_tag_round_trips() {
        let _flag = launch_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        let mut p = history_provider(dir.path());

        p.commit_edit("", "https://x.invalid/?q=</button>evil");

        assert_eq!(
            p.url_history,
            vec!["https://x.invalid/?q=%3C/button%3Eevil"],
            "the tag is encoded before it can truncate the button payload"
        );
        // Which is what the app will hand back to `on_button_press`.
        assert_eq!(history_rows(&mut p), p.url_history);
    }

    #[test]
    fn sanitize_history_url_rejects_unusable_entries() {
        assert_eq!(sanitize_history_url(""), None);
        assert_eq!(sanitize_history_url("   "), None);
        assert_eq!(sanitize_history_url("https://a.invalid/a b"), None);
        assert_eq!(sanitize_history_url("https://a.invalid\u{7}"), None);
        assert_eq!(
            sanitize_history_url(&format!(
                "https://{}.invalid",
                "x".repeat(MAX_HISTORY_URL_LEN)
            )),
            None,
        );
        assert_eq!(
            sanitize_history_url("  https://a.invalid  "),
            Some("https://a.invalid".to_owned()),
        );
        // A leading `*` is what marks a line as bookmarked in the history file.
        assert_eq!(
            sanitize_history_url("*://x.invalid"),
            Some("%2A://x.invalid".to_owned()),
        );
        assert_eq!(
            sanitize_history_url("https://x.invalid/*star"),
            Some("https://x.invalid/*star".to_owned()),
            "only the leading one is ambiguous",
        );
    }

    #[test]
    fn a_provider_that_was_never_initialised_never_touches_disk() {
        let _flag = launch_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history");
        std::fs::write(&path, "https://kept.invalid\n").unwrap();

        // No `init()`: this is every in-crate unit test, and a real provider
        // whose registration was cut short.
        let mut p = WebbrowserProvider::new();
        p.url_history_path = Some(path.clone());
        p.commit_edit("", "https://new.invalid");

        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "https://kept.invalid\n",
            "writing from a list that was never loaded would truncate the user's history"
        );
    }

    #[test]
    fn a_second_tab_recording_does_not_lose_the_first_tabs_entries() {
        let _flag = launch_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        // Two tabs, each with its own provider instance, both loaded up front.
        let mut tab_a = history_provider(dir.path());
        let mut tab_b = history_provider(dir.path());

        tab_a.commit_edit("", "https://a.invalid");
        tab_b.commit_edit("", "https://b.invalid");

        assert_eq!(
            tab_b.url_history,
            vec!["https://b.invalid", "https://a.invalid"],
            "the file is rewritten in full, so tab B has to merge what tab A wrote"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("history")).unwrap(),
            "https://b.invalid\nhttps://a.invalid\n",
        );
    }

    /// The file cannot be read, but its directory is perfectly writable — a
    /// permission problem, the everyday shape of this failure. Without the
    /// `disk.is_some()` guard in `record_url_history` the save succeeds and
    /// replaces a full history with the single URL of this session.
    ///
    /// Not a directory-in-place-of-the-file: `atomic_write` renames onto the
    /// target, which fails against a directory too, so that version of the test
    /// passes whether or not the guard is there.
    #[cfg(unix)]
    #[test]
    fn an_unreadable_history_file_is_never_overwritten() {
        use std::os::unix::fs::PermissionsExt;

        let _flag = launch_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history");
        std::fs::write(
            &path,
            "https://kept-one.invalid\nhttps://kept-two.invalid\n",
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).unwrap();

        // root ignores the mode bits, so there is nothing to test there.
        if std::fs::read_to_string(&path).is_ok() {
            return;
        }

        let mut p = WebbrowserProvider::new();
        p.url_history_path = Some(path.clone());
        p.init();
        p.commit_edit("", "https://new.invalid");

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "https://kept-one.invalid\nhttps://kept-two.invalid\n",
            "a history that could not be read must survive exactly as it was"
        );
        // The session still works; the new URL is just not persisted over
        // something we could not see.
        assert_eq!(p.url_history, vec!["https://new.invalid"]);
    }

    #[test]
    fn a_missing_history_file_is_a_first_run_not_an_error() {
        let _flag = launch_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        let mut p = history_provider(dir.path());
        p.commit_edit("", "https://a.invalid");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("history")).unwrap(),
            "https://a.invalid\n",
            "no file yet must still start recording"
        );
    }

    #[test]
    fn a_corrupt_history_file_is_filtered_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("history"),
            "https://ok.invalid\n\nnot a url with spaces\nhttps://ok.invalid\nhttps://other.invalid\n",
        )
        .unwrap();

        let p = history_provider(dir.path());

        assert_eq!(
            p.url_history,
            vec!["https://ok.invalid", "https://other.invalid"],
            "unusable lines dropped, duplicates collapsed to the first (newest) copy"
        );
    }

    // ---- Bookmarks ----

    /// The FFON key of the history row for `url`, as the app hands it to
    /// `handle_command` when the cursor is standing on that row.
    fn row_key(p: &mut WebbrowserProvider, url: &str) -> String {
        p.fetch()
            .iter()
            .filter_map(|e| e.as_str())
            .find(|k| sicompass_sdk::tags::extract_button_function_name(k).as_deref() == Some(url))
            .map(str::to_owned)
            .unwrap_or_else(|| panic!("no history row for {url}"))
    }

    /// The display text of every history row, marker included.
    fn row_labels(p: &mut WebbrowserProvider) -> Vec<String> {
        p.fetch()
            .iter()
            .filter_map(|e| e.as_str())
            .filter(|k| sicompass_sdk::tags::has_button(k))
            .filter_map(sicompass_sdk::tags::extract_button_display_text)
            .collect()
    }

    #[test]
    fn toggling_a_bookmark_marks_the_row_without_moving_it() {
        let _flag = launch_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        let mut p = history_provider(dir.path());
        p.commit_edit("", "https://a.invalid");
        p.commit_edit("", "https://b.invalid");

        let key = row_key(&mut p, "https://a.invalid");
        let mut error = String::new();
        p.handle_command(CMD_TOGGLE_BOOKMARK, &key, 0, &mut error);

        assert_eq!(
            p.url_history,
            vec!["https://b.invalid", "https://a.invalid"],
            "the ranking is untouched, so the row cannot jump under the cursor"
        );
        assert_eq!(
            row_labels(&mut p),
            vec!["https://b.invalid", "[bookmark] https://a.invalid"],
        );
        assert!(
            error.contains("https://a.invalid"),
            "the URL belongs in the message, or a second bookmark is announced \
             identically and therefore silently: {error}"
        );
    }

    #[test]
    fn history_rows_keep_the_bare_url_as_the_button_function_name() {
        let _flag = launch_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        let mut p = history_provider(dir.path());
        p.commit_edit("", "https://a.invalid");

        let key = row_key(&mut p, "https://a.invalid");
        p.handle_command(CMD_TOGGLE_BOOKMARK, &key, 0, &mut String::new());

        // The marker is display text only. `on_button_press` matches by
        // membership in `url_history`, so a marked function name is a dead row.
        assert_eq!(history_rows(&mut p), vec!["https://a.invalid"]);
        p.on_button_press("https://a.invalid");
        assert_eq!(p.current_url, "https://a.invalid");
    }

    #[test]
    fn toggling_twice_removes_the_bookmark() {
        let _flag = launch_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        let mut p = history_provider(dir.path());
        p.commit_edit("", "https://a.invalid");
        let key = row_key(&mut p, "https://a.invalid");

        let mut error = String::new();
        p.handle_command(CMD_TOGGLE_BOOKMARK, &key, 0, &mut error);
        assert!(p.bookmarks.contains("https://a.invalid"));

        p.handle_command(CMD_TOGGLE_BOOKMARK, &key, 0, &mut error);
        assert!(p.bookmarks.is_empty());
        assert_eq!(row_labels(&mut p), vec!["https://a.invalid"]);
    }

    #[test]
    fn the_history_file_marks_bookmarked_lines_with_a_star() {
        let _flag = launch_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        let mut p = history_provider(dir.path());
        p.commit_edit("", "https://a.invalid");
        p.commit_edit("", "https://b.invalid");

        let key = row_key(&mut p, "https://a.invalid");
        p.handle_command(CMD_TOGGLE_BOOKMARK, &key, 0, &mut String::new());

        assert_eq!(
            std::fs::read_to_string(dir.path().join("history")).unwrap(),
            "https://b.invalid\n*https://a.invalid\n",
            "unbookmarked lines keep the pre-bookmark format exactly"
        );
    }

    #[test]
    fn bookmarks_survive_a_restart() {
        let _flag = launch_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        {
            let mut p = history_provider(dir.path());
            p.commit_edit("", "https://a.invalid");
            p.commit_edit("", "https://b.invalid");
            let key = row_key(&mut p, "https://a.invalid");
            p.handle_command(CMD_TOGGLE_BOOKMARK, &key, 0, &mut String::new());
        }
        let mut p = history_provider(dir.path());
        assert_eq!(
            row_labels(&mut p),
            vec!["https://b.invalid", "[bookmark] https://a.invalid"],
        );
    }

    #[test]
    fn an_unprefixed_history_file_reads_as_all_unbookmarked() {
        let _flag = launch_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        // Exactly what every version before bookmarks wrote.
        std::fs::write(
            dir.path().join("history"),
            "https://a.invalid\nhttps://b.invalid\n",
        )
        .unwrap();

        let mut p = history_provider(dir.path());

        assert!(p.bookmarks.is_empty(), "no migration step to get wrong");
        assert_eq!(
            row_labels(&mut p),
            vec!["https://a.invalid", "https://b.invalid"],
        );
    }

    #[test]
    fn bookmarking_a_url_not_in_the_history_adds_a_row_at_the_top() {
        let _flag = launch_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        let mut p = history_provider(dir.path());
        p.commit_edit("", "https://typed.invalid");

        // The page the reader followed a link into: the app resolved the
        // enclosing <link> and hands its key over, since `current_url` still
        // names the last URL that went through `load_url`.
        p.handle_command(
            CMD_TOGGLE_BOOKMARK,
            "Some article <link>https://followed.invalid</link>",
            1,
            &mut String::new(),
        );

        assert_eq!(
            p.url_history,
            vec!["https://followed.invalid", "https://typed.invalid"],
            "a bookmark is an annotation on a row, so there has to be a row"
        );
        assert!(p.bookmarks.contains("https://followed.invalid"));
        assert_eq!(
            p.current_url, "https://typed.invalid",
            "bookmarking must not navigate"
        );
    }

    #[test]
    fn a_history_row_wins_over_the_loaded_url() {
        let _flag = launch_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        let mut p = history_provider(dir.path());
        p.commit_edit("", "https://a.invalid");
        p.commit_edit("", "https://b.invalid");

        let key = row_key(&mut p, "https://a.invalid");
        p.handle_command(CMD_TOGGLE_BOOKMARK, &key, 0, &mut String::new());

        assert_eq!(
            p.bookmarks.iter().collect::<Vec<_>>(),
            vec!["https://a.invalid"],
            "the cursor's row, not the page being displayed"
        );
    }

    #[test]
    fn a_page_button_is_not_mistaken_for_a_history_row() {
        let _flag = launch_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        let mut p = history_provider(dir.path());
        p.commit_edit("", "https://a.invalid");

        // A `<button>` the page carries, whose payload happens to be a URL.
        p.handle_command(
            CMD_TOGGLE_BOOKMARK,
            "<button>https://elsewhere.invalid</button>Subscribe",
            0,
            &mut String::new(),
        );

        assert_eq!(
            p.url_history,
            vec!["https://a.invalid"],
            "matched by membership, so it falls through to the loaded page"
        );
        assert!(p.bookmarks.contains("https://a.invalid"));
    }

    #[test]
    fn bookmarking_with_no_url_loaded_reports_nothing_to_bookmark() {
        let _flag = launch_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        let mut p = history_provider(dir.path());

        let mut error = String::new();
        p.handle_command(CMD_TOGGLE_BOOKMARK, "", 0, &mut error);

        assert_eq!(error, "No page to bookmark");
        assert!(p.url_history.is_empty());
        assert!(p.bookmarks.is_empty());
    }

    #[test]
    fn a_bookmark_is_not_dropped_by_the_history_cap() {
        let _flag = launch_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        let mut p = history_provider(dir.path());
        p.commit_edit("", "https://keep.invalid");
        let key = row_key(&mut p, "https://keep.invalid");
        p.handle_command(CMD_TOGGLE_BOOKMARK, &key, 0, &mut String::new());
        p.commit_edit("", "https://a.invalid");
        p.commit_edit("", "https://b.invalid");
        p.commit_edit("", "https://c.invalid");

        p.on_setting_change("urlHistorySize", "2");

        assert_eq!(
            p.url_history,
            vec![
                "https://c.invalid",
                "https://b.invalid",
                "https://keep.invalid"
            ],
            "the cap governs the unbookmarked rows; ageing a bookmark out would \
             silently delete the one thing the user asked to keep"
        );
        // And it survives the next navigation's read-merge-write too.
        p.commit_edit("", "https://d.invalid");
        assert_eq!(
            p.url_history,
            vec![
                "https://d.invalid",
                "https://c.invalid",
                "https://keep.invalid"
            ],
        );
    }

    #[test]
    fn unbookmarking_is_not_resurrected_by_another_tab() {
        let _flag = launch_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        let mut tab_a = history_provider(dir.path());
        let mut tab_b = history_provider(dir.path());

        tab_a.commit_edit("", "https://a.invalid");
        // Tab B picks the bookmark up off disk...
        let key = row_key(&mut tab_a, "https://a.invalid");
        tab_a.handle_command(CMD_TOGGLE_BOOKMARK, &key, 0, &mut String::new());
        tab_b.commit_edit("", "https://b.invalid");
        assert!(tab_b.bookmarks.contains("https://a.invalid"));

        // ...and A removes it again.
        let key = row_key(&mut tab_a, "https://a.invalid");
        tab_a.handle_command(CMD_TOGGLE_BOOKMARK, &key, 0, &mut String::new());

        // Merging the flags as a union would let B's stale copy put it back.
        tab_b.commit_edit("", "https://c.invalid");
        assert!(
            tab_b.bookmarks.is_empty(),
            "disk is authoritative for the flag: {:?}",
            tab_b.bookmarks
        );
        assert!(
            !std::fs::read_to_string(dir.path().join("history"))
                .unwrap()
                .contains('*'),
        );
    }

    /// No state directory (`$HOME` unset, or the test flag): the feature still
    /// works, purely in memory. Treating "no file" as "an empty file" would make
    /// the disk set authoritative over nothing at all — every navigation would
    /// wipe the bookmarks, and a bookmark could never be removed, because the
    /// empty set it is compared against never contains it.
    #[test]
    fn bookmarks_work_in_memory_without_a_history_file() {
        let _flag = launch_flag_guard(true);
        // No `url_history_path`, which under `cfg(test)` means no path at all —
        // see `TEST_NO_HISTORY`. Without that default this test would read,
        // merge and rewrite the developer's real history file.
        let mut p = WebbrowserProvider::new();
        p.init();
        assert!(
            p.resolve_url_history_path().is_none(),
            "this test is only meaningful, and only safe, with no file behind it"
        );
        p.commit_edit("", "https://a.invalid");

        let key = row_key(&mut p, "https://a.invalid");
        p.handle_command(CMD_TOGGLE_BOOKMARK, &key, 0, &mut String::new());
        assert!(p.bookmarks.contains("https://a.invalid"));

        p.commit_edit("", "https://b.invalid");
        assert!(
            p.bookmarks.contains("https://a.invalid"),
            "a navigation must not wipe it"
        );

        p.handle_command(CMD_TOGGLE_BOOKMARK, &key, 0, &mut String::new());
        assert!(p.bookmarks.is_empty(), "and it has to be removable");
    }

    /// No in-crate test may reach the real history file under `state_home()`.
    ///
    /// The tempdir-backed tests set `url_history_path`, but nothing forces them
    /// to, and a test that forgets does not fail — it quietly reads, merges and
    /// rewrites the developer's own browsing history, seeding it with
    /// `a.invalid`. `TEST_NO_HISTORY` defaults to `cfg!(test)` so that a
    /// forgotten override yields no file at all instead of the real one.
    #[test]
    fn no_unit_test_can_reach_the_real_history_file() {
        let p = WebbrowserProvider::new();
        assert!(
            p.resolve_url_history_path().is_none(),
            "a provider with no path override must resolve to nothing in tests"
        );
        // And the override still wins, or every history test would be inert.
        let mut p = WebbrowserProvider::new();
        p.url_history_path = Some(std::path::PathBuf::from("/tmp/somewhere/history"));
        assert!(p.resolve_url_history_path().is_some());
    }

    #[test]
    fn a_url_starting_with_a_star_is_encoded() {
        let _flag = launch_flag_guard(true);
        let dir = tempfile::tempdir().unwrap();
        let mut p = history_provider(dir.path());

        // `normalize_url_input` passes anything containing "://" through
        // verbatim, so this reaches the file as-is unless it is encoded — and
        // would then read back as a *bookmarked* "://x.invalid".
        p.commit_edit("", "*://x.invalid");

        assert_eq!(p.url_history, vec!["%2A://x.invalid"]);
        assert!(p.bookmarks.is_empty());
        assert_eq!(
            std::fs::read_to_string(dir.path().join("history")).unwrap(),
            "%2A://x.invalid\n",
        );
        // Which survives a restart as the same, still-pressable row.
        let mut p = history_provider(dir.path());
        assert!(p.bookmarks.is_empty());
        assert_eq!(history_rows(&mut p), vec!["%2A://x.invalid"]);
    }

    #[test]
    fn patch_form_field_updates_cached_str() {
        let mut elems = vec![{
            let mut form = FfonElement::new_obj("form_1");
            form.as_obj_mut()
                .unwrap()
                .push(FfonElement::new_str("q: <input></input>"));
            form
        }];
        let patched = patch_form_field_in_tree(
            &mut elems,
            "form_1",
            "q: <input>",
            "q: <input>hello</input>",
        );
        assert!(patched, "should find and patch the field");
        let child = elems[0].as_obj().unwrap().children[0].as_str().unwrap();
        assert_eq!(child, "q: <input>hello</input>");
    }

    #[test]
    fn patch_form_field_nested_under_heading() {
        // Form nested under a heading Obj (common when a page has <h1> before <form>).
        let mut elems = vec![{
            let mut heading = FfonElement::new_obj("Search");
            let mut form = FfonElement::new_obj("form_1");
            form.as_obj_mut()
                .unwrap()
                .push(FfonElement::new_str("q: <input></input>"));
            heading.as_obj_mut().unwrap().push(form);
            heading
        }];
        let patched = patch_form_field_in_tree(
            &mut elems,
            "form_1",
            "q: <input>",
            "q: <input>world</input>",
        );
        assert!(patched);
        let heading_obj = elems[0].as_obj().unwrap();
        let form_obj = heading_obj.children[0].as_obj().unwrap();
        assert_eq!(
            form_obj.children[0].as_str().unwrap(),
            "q: <input>world</input>"
        );
    }

    #[test]
    fn patch_form_field_id_prefixed_form_key() {
        // Form has an <id>X</id> prefix on its key — match by suffix.
        let mut elems = vec![{
            let mut form = FfonElement::new_obj("<id>login</id>form_1");
            form.as_obj_mut()
                .unwrap()
                .push(FfonElement::new_str("email: <input></input>"));
            form
        }];
        let patched = patch_form_field_in_tree(
            &mut elems,
            "form_1",
            "email: <input>",
            "email: <input>user@x.com</input>",
        );
        assert!(patched, "should match by suffix for id-prefixed form keys");
        assert_eq!(
            elems[0].as_obj().unwrap().children[0].as_str().unwrap(),
            "email: <input>user@x.com</input>"
        );
    }

    #[test]
    fn commit_edit_form_field_returns_false_and_patches_cache() {
        // commit_edit for a known form field must return false so the app's
        // unconditional local-FFON update isn't wiped by refresh_current_directory.
        use sicompass_sdk::ffon::FormNode;
        use sicompass_sdk::ffon::FormNodeKind;
        use sicompass_sdk::provider::Provider;

        let mut p = WebbrowserProvider::new();
        // Build a minimal cached_page with a form field.
        let mut form = FfonElement::new_obj("form_1");
        form.as_obj_mut()
            .unwrap()
            .push(FfonElement::new_str("q: <input></input>"));
        p.cached_page = Some(CachedPage {
            url: "https://example.com".into(),
            elements: vec![form],
        });
        // Seed form_map with the field.
        p.form_map.insert(
            "form_1/q".into(),
            FormNode {
                css_selector: "#q".into(),
                kind: FormNodeKind::TextInput,
                form_index: 1,
                match_index: 0,
            },
        );
        // Simulate being inside the form field.
        p.push_path("https://example.com");
        p.push_path("form_1");
        p.push_path("q");

        // commit_edit must return false (no refresh_current_directory).
        let result = p.commit_edit("", "hello");
        assert!(!result, "commit_edit for form field must return false");

        // cached_page must be patched so re-fetch preserves the value.
        let child = p.cached_page.as_ref().unwrap().elements[0]
            .as_obj()
            .unwrap()
            .children[0]
            .as_str()
            .unwrap();
        assert_eq!(child, "q: <input>hello</input>", "cached_page not patched");
    }

    // ---- Drift detection (Windows close-after-load path) ----

    fn fresh_map(keys: &[&str]) -> FormMap {
        use sicompass_sdk::ffon::{FormNode, FormNodeKind};
        let mut map = FormMap::new();
        for k in keys {
            map.insert(
                (*k).to_owned(),
                FormNode {
                    css_selector: format!("#{k}"),
                    kind: FormNodeKind::TextInput,
                    form_index: 1,
                    match_index: 0,
                },
            );
        }
        map
    }

    fn stored_map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn drift_check_all_present_returns_ok() {
        let stored = stored_map(&[("form_1/email", "a@b"), ("form_1/name", "Nico")]);
        let fresh = fresh_map(&["form_1/email", "form_1/name", "form_1/submit"]);
        assert!(check_form_drift(&stored, &fresh).is_ok());
    }

    #[test]
    fn drift_check_missing_field_returns_err_with_missing_keys() {
        let stored = stored_map(&[("form_1/email", "a@b"), ("form_1/csrf", "x")]);
        let fresh = fresh_map(&["form_1/email"]);
        let err = check_form_drift(&stored, &fresh).expect_err("should detect drift");
        assert_eq!(err, vec!["form_1/csrf".to_owned()]);
    }

    #[test]
    fn drift_check_empty_stored_returns_ok() {
        let stored = HashMap::new();
        let fresh = fresh_map(&["form_1/anything"]);
        assert!(check_form_drift(&stored, &fresh).is_ok());
    }

    #[test]
    fn drift_check_empty_fresh_with_stored_returns_err_for_all() {
        let stored = stored_map(&[("form_1/a", "1"), ("form_1/b", "2")]);
        let fresh = FormMap::new();
        let err = check_form_drift(&stored, &fresh).expect_err("should detect drift");
        assert_eq!(err, vec!["form_1/a".to_owned(), "form_1/b".to_owned()]);
    }

    // ---- form_field_values map ----

    #[test]
    fn commit_edit_records_typed_value_in_form_field_values() {
        use sicompass_sdk::ffon::{FormNode, FormNodeKind};
        let mut p = WebbrowserProvider::new();
        p.form_map.insert(
            "form_1/email".into(),
            FormNode {
                css_selector: "#email".into(),
                kind: FormNodeKind::TextInput,
                form_index: 1,
                match_index: 0,
            },
        );
        // Build cached_page so patch_cached_form_field has something to update.
        let mut form = FfonElement::new_obj("form_1");
        form.as_obj_mut()
            .unwrap()
            .push(FfonElement::new_str("email: <input></input>"));
        p.cached_page = Some(CachedPage {
            url: "https://x".into(),
            elements: vec![form],
        });

        p.push_path("https://x");
        p.push_path("form_1");
        p.push_path("email");
        p.commit_edit("", "user@example.com");

        assert_eq!(
            p.form_field_values.get("form_1/email").map(|s| s.as_str()),
            Some("user@example.com"),
        );
    }

    // ---- take_error draining ----

    #[test]
    fn take_error_drains_pending_error_once() {
        use sicompass_sdk::provider::Provider;
        let mut p = WebbrowserProvider::new();
        set_error(&p.pending_error, "boom".to_owned());
        assert_eq!(p.take_error(), Some("boom".to_owned()));
        assert_eq!(p.take_error(), None, "second drain returns None");
    }

    // ---- Windows single-flight submit guard ----

    #[cfg(target_os = "windows")]
    #[test]
    fn single_flight_rejects_second_submit() {
        use sicompass_sdk::provider::Provider;
        use std::sync::atomic::Ordering;
        let mut p = WebbrowserProvider::new();
        // Simulate an in-flight submit so the next press takes the early-return path.
        p.submit_in_flight.store(true, Ordering::SeqCst);
        p.current_url = "https://example.com".to_owned();
        p.on_button_press("submit:form_1");

        let err = p
            .take_error()
            .expect("pending_error should carry single-flight message");
        assert!(
            err.contains("already in progress"),
            "unexpected error message: {err}",
        );
    }

    // Run with: cargo test -p sicompass-webbrowser -- --ignored
    #[test]
    #[ignore]
    fn test_form_interaction_local_file() {
        // Verify that a local HTML form is parsed and the provider
        // exposes its fields as editable FFON cells.  Requires Chrome.
        let html = r#"<!DOCTYPE html><html><body>
            <form>
              <input type="search" name="q" placeholder="Search">
              <input type="submit" value="Go">
            </form>
        </body></html>"#;
        let (elems, map) = html_to_ffon_with_forms(html, "");
        let form = elems[0].as_obj().expect("expected form obj");
        assert_eq!(form.key, "form_1");
        assert!(
            map.contains_key("form_1/Search"),
            "Search field missing from map"
        );
        assert!(
            map.contains_key("form_1/Go"),
            "Submit button missing from map"
        );
    }

    // ---- inline consent banner detection ----

    #[test]
    fn inline_banner_detected_for_every_vendor() {
        for (marker, _) in CONSENT_BANNERS {
            let html = format!(r#"<html><body><div id="{marker}">Cookies</div></body></html>"#);
            assert!(
                html_has_inline_consent_banner(&html),
                "vendor marker {marker} not detected"
            );
        }
    }

    #[test]
    fn inline_banner_not_detected_on_an_ordinary_page() {
        assert!(!html_has_inline_consent_banner(
            "<html><body><h1>Hello</h1><p>Some article text.</p></body></html>"
        ));
    }

    #[test]
    fn inline_banner_not_detected_from_prose_or_a_policy_link() {
        // The whole reason the marker list is vendor-specific: a page may talk
        // about cookies at length, or link to its cookie policy, without ever
        // putting a banner on screen.
        let prose = "<html><body><p>We use cookies and a cookie consent tool. \
                     Read our cookie policy about cookies.</p>\
                     <a href=\"/en/cookie-policy\">Cookies notice</a>\
                     <a href=\"/cookie-consent\">Cookies setting</a></body></html>";
        assert!(!html_has_inline_consent_banner(prose));
    }

    #[test]
    fn bpost_style_onetrust_banner_is_detected() {
        // bpost.be loads OneTrust through GTM; the banner is overlaid on an
        // ordinary HTTP 200 page, which neither wall check ever fires for.
        let html = r#"<html><body><div id="onetrust-consent-sdk">
            <div id="onetrust-banner-sdk"><button id="onetrust-accept-btn-handler">Alles accepteren</button></div>
        </div><h1>Welkom bij Bpost</h1></body></html>"#;
        assert!(html_has_inline_consent_banner(html));
        assert!(!is_consent_wall_str("https://www.bpost.be/nl"));
        assert!(!html_has_consent_wall(html));
    }

    // ---- reject / accept / settings classification ----

    #[test]
    fn settings_buttons_are_not_a_consent_choice() {
        for text in [
            "Manage preferences",
            "Cookie settings",
            "Instellingen",
            "Personnaliser",
            "Einstellungen",
        ] {
            assert!(is_settings_text(text), "{text} should read as settings");
            // Still guarded against being mistaken for an accept.
            assert!(is_reject_text(text), "{text} must never count as accept");
            assert!(!is_accept_keyword(text));
        }
    }

    #[test]
    fn every_vendor_offers_a_reject_selector_and_it_reaches_the_page() {
        // Refusing has to keep working in a language nobody wrote a keyword
        // for: bpost's Dutch OneTrust banner says "Alles afwijzen", but the id
        // is the same everywhere.
        assert!(CMP_REJECT_SELECTORS.contains(&"#onetrust-reject-all-handler"));
        let js = gate_surface_js(None, false, false);
        for sel in CMP_REJECT_SELECTORS {
            // `js_array` emits each selector as a JSON string, so an attribute
            // selector arrives with its quotes escaped.
            assert!(
                js.contains(&format!("{sel:?}")),
                "reject selector {sel} never reaches the page"
            );
        }
        // Checked before the accept selectors, so a banner whose accept button
        // also matches a broad pattern cannot win.
        let rej = js.find("matchesAny(CMPREJ)").expect("reject check");
        let acc = js.find("matchesAny(CMP)").expect("accept check");
        assert!(rej < acc, "reject must be classified first");
    }

    #[test]
    fn dutch_and_french_reject_wordings_are_recognised() {
        // Every one of these was observed on a live Belgian consent banner.
        for text in [
            "Alles afwijzen",
            "Alles weigeren",
            "Continuer sans accepter",
            "Alleen essenti\u{eb}le cookies",
            "Nur erforderliche Cookies",
        ] {
            assert!(is_reject_text(text), "{text} should read as a refusal");
            assert!(
                !is_accept_keyword(text),
                "{text} must never count as accept"
            );
            assert!(!is_settings_text(text), "{text} decides something");
        }
    }

    #[test]
    fn a_real_reject_is_not_classified_as_settings() {
        for text in ["Reject all", "Alles weigeren", "Alleen noodzakelijke"] {
            assert!(is_reject_text(text));
            assert!(!is_settings_text(text), "{text} decides something");
        }
    }

    // ---- js_var / Google consent values ----

    #[test]
    fn js_var_reads_single_and_double_quoted_and_bare_values() {
        assert_eq!(
            js_var("var cookieDomain='.google.com';", "cookieDomain").as_deref(),
            Some(".google.com")
        );
        assert_eq!(
            js_var(r#"var sCAS = "CAIS";"#, "sCAS").as_deref(),
            Some("CAIS")
        );
        assert_eq!(
            js_var("var sL=34128000;", "sL").as_deref(),
            Some("34128000")
        );
    }

    #[test]
    fn js_var_skips_an_identifier_that_only_shares_a_prefix() {
        // `sLater` must not answer a lookup for `sL`.
        assert_eq!(
            js_var("var sLater='nope';var sL=42;", "sL").as_deref(),
            Some("42")
        );
        assert_eq!(js_var("var sLater='nope';", "sL"), None);
    }

    /// Trimmed from the real wall served by www.google.com (2026-08), including
    /// the `\x3d`-escaped save URLs that sit alongside the values we read.
    const GOOGLE_WALL_SNIPPET: &str = r#"<script nonce="JNJ">(function(){var cookieDomain='.google.com';var cookieUpdateConsentUrl='';var sCAS='CAISHAgBEhJnd3NfMjAyNjA4MDYtMF9SQzEaAm5sIAEaBgiAht_TBg';var sCRS='CAESHAgBEhJnd3NfMjAyNjA4MDYtMF9SQzEaAm5sIAEaBgiAht_TBg';var sL=34128000;var sEE=false;var aAU='https://consent.google.com/save?continue\x3dhttps://www.google.com/\x26set_eom\x3dfalse';var rAU='https://consent.google.com/save?continue\x3dhttps://www.google.com/\x26set_eom\x3dtrue';})();</script>"#;

    #[test]
    fn google_consent_values_are_read_from_the_wall() {
        let g = google_consent_values(GOOGLE_WALL_SNIPPET).expect("wall values");
        assert_eq!(g.domain, ".google.com");
        // sCAS records "accept all", sCRS records "reject all"; both carry the
        // serving build id, which is why the old hardcoded constant went stale.
        assert!(g.accept.starts_with("CAIS"), "accept payload: {}", g.accept);
        assert!(g.reject.starts_with("CAES"), "reject payload: {}", g.reject);
        assert_ne!(g.accept, g.reject);
        assert_eq!(g.max_age, "34128000");
    }

    #[test]
    fn google_consent_values_absent_on_an_ordinary_page() {
        assert_eq!(
            google_consent_values("<html><body><p>Nothing here.</p></body></html>"),
            None
        );
    }

    #[test]
    fn google_consent_falls_back_to_the_stale_constant() {
        let g = google_consent_or_fallback("<html><body></body></html>");
        assert_eq!(g.domain, ".google.com");
        assert_eq!(g.accept, GOOGLE_SOCS_FALLBACK);
    }

    #[test]
    fn google_consent_rejects_a_non_dotted_domain() {
        // A `cookieDomain` that is not a cookie domain means we parsed
        // something else entirely; better to fall back than to write junk.
        assert_eq!(
            google_consent_values(
                "var cookieDomain='https://example.com';var sCAS='a';var sCRS='b';"
            ),
            None
        );
    }

    // ---- gate surfacing script ----

    #[test]
    fn gate_script_embeds_the_google_values_it_was_given() {
        let g = google_consent_values(GOOGLE_WALL_SNIPPET).unwrap();
        let js = gate_surface_js(Some(&g), true, false);
        assert!(js.contains(&g.accept));
        assert!(js.contains(&g.reject));
        assert!(js.contains(".google.com"));
        assert!(js.contains("const IS_WALL = true;"));
    }

    #[test]
    fn gate_script_carries_the_vendor_selectors_and_no_google_block() {
        let js = gate_surface_js(None, false, false);
        assert!(js.contains("const GOOGLE = null;"));
        assert!(js.contains("const IS_WALL = false;"));
        assert!(js.contains("#onetrust-banner-sdk"));
        assert!(js.contains("#CybotCookiebotDialog"));
        // One form per choice is the whole point — several submit buttons in
        // one form are indistinguishable to `on_button_press`.
        assert!(js.contains("createElement('form')"));
    }

    #[test]
    fn the_gate_script_never_looks_for_a_language_chooser() {
        // A language switcher does not block anything, so it is left in the
        // page as the links it is. Asking about it interrupted every
        // multilingual site, and with a list that could omit the language the
        // page was already in — anysurfer.be marks its current language with a
        // <span lang> the chooser scan could not see.
        let js = gate_surface_js(None, false, false);
        for probe in ["hreflang", "data-lang", "STORED_LANG", "sic-lang-"] {
            assert!(
                !js.contains(probe),
                "the gate script still reasons about languages via {probe}"
            );
        }
    }

    #[test]
    fn gate_page_contains_only_the_choices() {
        // "Only the step, nothing else": the page a gate renders to is the list
        // and nothing more, and its forms line up 1:1 with the proxy forms in
        // the live document.
        let html = gate_page_html(
            &["Alles weigeren".to_owned(), "Alles aanvaarden".to_owned()],
            &["sic-consent-0".to_owned(), "sic-consent-1".to_owned()],
        );
        let (elems, map) = html_to_ffon_with_forms(&html, "");
        assert_eq!(elems.len(), 2, "expected exactly two choices: {elems:?}");
        assert_eq!(
            map.get("form_1/Alles weigeren")
                .map(|n| n.css_selector.as_str()),
            Some("#sic-consent-0")
        );
        assert_eq!(
            map.get("form_2/Alles aanvaarden")
                .map(|n| n.css_selector.as_str()),
            Some("#sic-consent-1")
        );
    }

    // ---- the languages a page declares ----

    /// bpost.be's `<head>`, which is the whole reason this exists: its only
    /// in-page switcher is an `aria-hidden` modal of href-less `<a data-lang>`
    /// anchors, so the prune drops it and the FFON walker would emit nothing
    /// for it anyway. The four `<link rel=alternate>` lines are the only
    /// readable statement of what languages the site has.
    const BPOST_HEAD: &str = r#"<!DOCTYPE html><html lang="nl"><head>
        <link rel="alternate" hreflang="nl" href="https://www.bpost.be/nl" />
        <link rel="alternate" hreflang="en" href="https://www.bpost.be/en" />
        <link rel="alternate" hreflang="fr" href="https://www.bpost.be/fr" />
        <link rel="alternate" hreflang="de" href="https://www.bpost.be/de" />
        </head><body>
        <div aria-hidden="true" class="modal fade pre_homepage_language_modal">
          <p><a class="choose-lang" data-lang="nl">Ik spreek Nederlands</a></p>
          <p><a class="choose-lang" data-lang="fr">Je parle Français</a></p>
        </div>
        <h1>Welkom bij Bpost</h1>
        </body></html>"#;

    #[test]
    fn a_site_declaring_four_languages_offers_all_four() {
        let langs = declared_languages(BPOST_HEAD, "https://www.bpost.be/nl");
        let codes: Vec<&str> = langs.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(langs.len(), 4, "expected bpost's four languages: {codes:?}");
        // Autonyms, in the order the site declares them.
        assert!(langs[0].0.starts_with("Nederlands"), "{codes:?}");
        assert_eq!(langs[1].0, "English");
        assert_eq!(langs[2].0, "Français");
        assert_eq!(langs[3].0, "Deutsch");
        // Real hrefs, so following one is ordinary navigation.
        assert_eq!(langs[3].1, "https://www.bpost.be/de");
        // The one you are reading is named as such, from <html lang>.
        assert!(
            langs[0]
                .0
                .contains(&localize::t("webbrowser-language-current")),
            "the current language is not marked: {codes:?}"
        );
        assert_eq!(
            langs
                .iter()
                .filter(|(l, _)| l.contains(&localize::t("webbrowser-language-current")))
                .count(),
            1,
            "exactly one language is the current one: {codes:?}"
        );
    }

    #[test]
    fn the_language_you_are_reading_is_listed_even_when_undeclared() {
        // anysurfer.be, in Dutch, declares only English and French. Listing
        // just those is a switcher with no way to stay where you are, which is
        // the complaint that sank the 0.1.17 language step. <html lang> plus
        // the loaded address is enough to put Dutch back.
        let html = r#"<!DOCTYPE html><html lang="nl"><head>
            <link href="https://www.anysurfer.be/en" rel="alternate" hreflang="en">
            <link href="https://www.anysurfer.be/fr" rel="alternate" hreflang="fr">
            </head><body><h1>Toegankelijkheid</h1></body></html>"#;
        let langs = declared_languages(html, "https://www.anysurfer.be/nl");
        assert_eq!(langs.len(), 3, "Dutch is missing: {langs:?}");
        let dutch = langs
            .iter()
            .find(|(l, _)| l.starts_with("Nederlands"))
            .expect("no Dutch entry");
        assert_eq!(dutch.1, "https://www.anysurfer.be/nl");
        assert!(
            dutch
                .0
                .contains(&localize::t("webbrowser-language-current"))
        );
    }

    #[test]
    fn a_page_offering_no_real_choice_declares_nothing() {
        // One language is not a choice, and neither is none. An ordinary
        // monolingual page must grow nothing at all.
        for html in [
            r#"<html lang="nl"><head></head><body><p>Hallo</p></body></html>"#,
            r#"<html lang="nl"><head>
               <link rel="alternate" hreflang="nl" href="/nl"></head><body></body></html>"#,
            // No <html lang> either: nothing to synthesize a current entry from.
            r#"<html><head><link rel="alternate" hreflang="fr" href="/fr"></head><body></body></html>"#,
        ] {
            assert!(
                declared_languages(html, "https://x.invalid/nl").is_empty(),
                "a page with no choice offered one: {html}"
            );
        }
    }

    #[test]
    fn region_variants_and_x_default_are_not_languages() {
        // `nl-BE` and `nl` are one choice, not two — region is the site's
        // business, not the reader's. `x-default` is a routing fallback with no
        // language and no name to show.
        let html = r#"<!DOCTYPE html><html lang="nl-BE"><head>
            <link rel="alternate" hreflang="x-default" href="https://x.invalid/">
            <link rel="alternate" hreflang="nl-BE" href="https://x.invalid/nl-be">
            <link rel="alternate" hreflang="nl" href="https://x.invalid/nl">
            <link rel="alternate" hreflang="FR" href="https://x.invalid/fr">
            </head><body></body></html>"#;
        let langs = declared_languages(html, "https://x.invalid/nl-be");
        assert_eq!(
            langs.len(),
            2,
            "expected one Dutch and one French: {langs:?}"
        );
        // First declaration wins, so the region-specific URL is the one kept.
        assert_eq!(langs[0].1, "https://x.invalid/nl-be");
        assert!(langs[0].0.starts_with("Nederlands"));
        // hreflang is case-insensitive.
        assert_eq!(langs[1].0, "Français");
    }

    #[test]
    fn only_head_alternates_count_as_a_language_version() {
        // `hreflang` on an ordinary content link says what that link points at,
        // not that the page has a version in that language. Reading those is
        // exactly how the old step came to offer elevenways.be's contact page
        // as its Dutch entry. `rel="alternate"` is also a set, and `alternate
        // stylesheet` is not a translation.
        let html = r#"<!DOCTYPE html><html lang="nl"><head>
            <link rel="alternate stylesheet" hreflang="de" href="/print.css">
            <link rel="alternate" hreflang="fr" href="/fr">
            </head><body>
            <a hreflang="en" href="/nl/contact">CONTACT</a>
            </body></html>"#;
        let langs = declared_languages(html, "https://x.invalid/nl");
        let labels: Vec<&str> = langs.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(langs.len(), 2, "expected only Dutch and French: {labels:?}");
        assert!(
            !labels.iter().any(|l| l.starts_with("English")),
            "{labels:?}"
        );
        assert!(
            !labels.iter().any(|l| l.starts_with("Deutsch")),
            "{labels:?}"
        );
    }

    #[test]
    fn relative_alternates_resolve_against_the_page() {
        let html = r#"<html lang="nl"><head>
            <link rel="alternate" hreflang="fr" href="/fr/thuis">
            </head><body></body></html>"#;
        let langs = declared_languages(html, "https://x.invalid/nl/thuis");
        let french = langs
            .iter()
            .find(|(l, _)| l == "Français")
            .expect("no French");
        assert_eq!(french.1, "https://x.invalid/fr/thuis");
    }

    #[test]
    fn an_unknown_language_code_still_lists() {
        // The autonym table is a courtesy, not a gate: a code it has never
        // heard of is still a language the site offers.
        let html = r#"<html lang="nl"><head>
            <link rel="alternate" hreflang="fy" href="/fy"></head><body></body></html>"#;
        let langs = declared_languages(html, "https://x.invalid/nl");
        assert_eq!(langs.len(), 2, "{langs:?}");
        assert!(langs.iter().any(|(l, _)| l == "FY"), "{langs:?}");
    }

    #[test]
    fn a_cookie_step_is_still_only_a_question() {
        // The gate replaces the page with a bare document that has no <head>
        // and no lang, so it declares nothing and grows nothing. This is what
        // keeps "answer the cookies first" from turning back into a page with
        // two unrelated lists on it — no special case needed to get there.
        let html = gate_page_html(
            &["Alles weigeren".to_owned(), "Alles aanvaarden".to_owned()],
            &["sic-consent-0".to_owned(), "sic-consent-1".to_owned()],
        );
        assert!(declared_languages(&html, "https://www.bpost.be/nl").is_empty());
    }

    #[test]
    fn the_language_section_is_the_last_thing_on_the_page() {
        // Content first: a reader meets the page before its switcher, and the
        // switcher is always in the same place rather than wherever the site
        // happened to put it.
        let (elements, _) = page_to_ffon_with_forms(
            &PageLoad::plain(BPOST_HEAD.to_owned()),
            "https://www.bpost.be/nl",
        );
        let last = elements.last().expect("no elements");
        let obj = last.as_obj().expect("the language section is a section");
        assert_eq!(obj.key, localize::t("webbrowser-languages"));
        assert_eq!(obj.children.len(), 4, "{:?}", obj.children);
        // Each entry is an ordinary link, read and followed like any other.
        let rendered = format!("{:?}", obj.children);
        assert!(
            rendered.contains("Deutsch <link>https://www.bpost.be/de</link>"),
            "{rendered}"
        );
        // The content is still there, and still ahead of the section.
        let all = format!("{elements:?}");
        assert!(all.contains("Welkom bij Bpost"), "{all}");
        // And the modal contributed no choice. Chrome's prune drops it outright
        // for being aria-hidden, but that runs in the page and not here, so
        // this fixture still carries its text — as bare strings. Href-less
        // anchors never become links, so even surviving it offers nothing to
        // press. The head is what spoke.
        assert!(
            !all.contains("Ik spreek Nederlands <link>"),
            "an href-less anchor became a link: {all}"
        );
    }

    // ---- the leftover language preference ----

    #[test]
    fn the_stale_language_pref_is_looked_for_where_0_1_17_wrote_it() {
        // Beside the profile dir, not inside it — that is where the old code
        // put the file precisely so `clear cookies` would leave it alone. Get
        // this wrong and the sweep quietly deletes nothing.
        let profile = std::path::Path::new("/cfg/sicompass/chrome-profile");
        assert_eq!(
            stale_language_pref_in(profile),
            std::path::PathBuf::from("/cfg/sicompass/webbrowser-language")
        );
    }

    #[test]
    fn clearing_cookies_never_touches_real_files_under_test() {
        // `remove_stale_language_pref` is a real `remove_file`, and the command
        // tests below invoke `clear cookies`. Without the same guard the URL
        // history has, running the suite would delete the developer's own file.
        assert!(
            stale_language_pref_path().is_none(),
            "the persistence guard must cover every file this provider removes"
        );
        let mut p = WebbrowserProvider::new();
        let mut error = String::new();
        p.clear_cookies(&mut error);
    }

    #[test]
    fn gate_page_escapes_a_hostile_label() {
        let html = gate_page_html(
            &["</button><script>bad()</script>".to_owned()],
            &["sic-consent-0".to_owned()],
        );
        assert!(!html.contains("<script>"), "label was not escaped: {html}");
    }

    // ---- hidden-content toggle ----

    #[test]
    fn prune_toggle_command_is_labelled_by_what_it_does() {
        let _guard = launch_flag_guard(true);
        let restore = prune_hidden();
        let mut p = WebbrowserProvider::new();
        let mut err = String::new();

        PRUNE_HIDDEN.store(true, Ordering::Release);
        assert!(p.commands().contains(&CMD_SHOW_HIDDEN.to_owned()));
        assert!(!p.commands().contains(&CMD_HIDE_HIDDEN.to_owned()));

        p.handle_command(CMD_SHOW_HIDDEN, "", 0, &mut err);
        assert!(!prune_hidden(), "pressing it should turn pruning off");
        assert!(p.commands().contains(&CMD_HIDE_HIDDEN.to_owned()));

        p.handle_command(CMD_HIDE_HIDDEN, "", 0, &mut err);
        assert!(prune_hidden(), "pressing it again should turn pruning on");

        PRUNE_HIDDEN.store(restore, Ordering::Release);
    }

    #[test]
    fn pruning_is_on_by_default() {
        assert!(PRUNE_HIDDEN.load(Ordering::Acquire) || !prune_hidden());
    }

    // ---- browser-backed tests (need Chrome; run with --ignored) ----

    /// Load an HTML fixture from a temp file through the real load path.
    #[cfg(not(target_os = "windows"))]
    fn load_fixture(name: &str, html: &str) -> PageLoad {
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, html).expect("write fixture");
        let url = format!("file://{}", path.display());
        let out = chromium_runtime().block_on(async {
            let live = init_live_session().await.expect("launch chrome");
            let out = navigate_and_get_html(&live.page, &url).await;
            close_browser(&live.session).await;
            out
        });
        let _ = std::fs::remove_file(&path);
        out.expect("fixture should load")
    }

    /// Load an HTML fixture and evaluate `js` in it, returning the result.
    ///
    /// How the geometry passes get asserted on at all: they are browser-side
    /// JS, so the only way to see their decisions is to ask the page for them.
    #[cfg(not(target_os = "windows"))]
    fn eval_fixture(name: &str, html: &str, js: &str) -> String {
        let path = std::env::temp_dir().join(name);
        std::fs::write(&path, html).expect("write fixture");
        let url = format!("file://{}", path.display());
        let out = chromium_runtime().block_on(async {
            let live = init_live_session().await.expect("launch chrome");
            let _ = navigate_and_get_html(&live.page, &url).await;
            let out = live
                .page
                .evaluate(js)
                .await
                .ok()
                .and_then(|v| v.into_value::<String>().ok());
            close_browser(&live.session).await;
            out
        });
        let _ = std::fs::remove_file(&path);
        out.expect("fixture should evaluate")
    }

    /// Stamp geometry over a fixture and read back `data-sic-g` for `selectors`.
    #[cfg(not(target_os = "windows"))]
    fn geo_probe_js(selectors: &[&str]) -> String {
        [
            PRUNE_JS_HEAD,
            BAND_JS,
            "\n    sicStampLive(document.documentElement);\n    return JSON.stringify(",
            &js_array(selectors),
            ".map(function(s) { var e = document.querySelector(s);\
             return e ? e.getAttribute(GEO) : null; }));\n",
            PRUNE_JS_END,
        ]
        .concat()
    }

    /// A page whose source order is the exact reverse of its visual order:
    /// flex `order` puts the footer first in the markup and last on screen.
    /// Nothing but geometry can tell you that.
    #[cfg(not(target_os = "windows"))]
    const BAND_FIXTURE: &str = r#"<!DOCTYPE html><html><head><style>
        body{margin:0} .page{display:flex;flex-direction:column;width:1920px}
        .hdr{order:1;height:80px} .content{order:2;display:flex;height:900px}
        .art{width:1500px} .side{width:420px} .ftr{order:3;height:300px}
        </style></head><body><div class="page">
        <div class="ftr"><a href="/p">FOOTER-PRIVACY</a><a href="/t">FOOTER-TERMS</a>
            <a href="/c">FOOTER-CONTACT</a><a href="/j">FOOTER-JOBS</a></div>
        <div class="content">
            <div class="art"><h1>ARTICLE-HEADING</h1><p>ARTICLE-BODY</p></div>
            <div class="side"><h2>SIDEBAR-HEADING</h2><p>SIDEBAR-BODY</p></div></div>
        <div class="hdr"><a href="/1">NAV-ONE</a><a href="/2">NAV-TWO</a>
            <a href="/3">NAV-THREE</a><a href="/4">NAV-FOUR</a></div>
        </div></body></html>"#;

    #[test]
    #[ignore]
    #[cfg(not(target_os = "windows"))]
    fn geometry_is_read_from_the_layout_chrome_already_did() {
        // The whole premise of the change: Chrome has already laid the page out,
        // and the numbers it computed disagree with source order. Source order
        // here is footer, content, header; the layout says otherwise.
        let _guard = launch_flag_guard(false);
        let raw = eval_fixture(
            "sic-geo-probe.html",
            BAND_FIXTURE,
            &geo_probe_js(&[".hdr", ".content", ".ftr", ".art", ".side"]),
        );
        let g: Vec<Option<String>> = serde_json::from_str(&raw).expect("probe returns JSON");
        let at = |i: usize| -> Vec<i64> {
            g[i].as_ref()
                .unwrap_or_else(|| panic!("no geometry stamped for probe {i}; got: {raw}"))
                .split(' ')
                .map(|n| n.parse().expect("integer geometry"))
                .collect()
        };
        let (hdr, content, ftr, art, side) = (at(0), at(1), at(2), at(3), at(4));
        // Vertical order is header, content, footer — the reverse of the markup.
        assert_eq!(hdr[1], 0, "header sits at the top; got {hdr:?}");
        assert_eq!(
            content[1], 80,
            "content follows the header; got {content:?}"
        );
        assert_eq!(ftr[1], 980, "footer is last on screen; got {ftr:?}");
        // And the two columns are side by side, which is what tells a wide
        // content column from a narrow complementary one.
        assert_eq!((art[0], art[2]), (0, 1500), "article column; got {art:?}");
        assert_eq!(
            (side[0], side[2]),
            (1500, 420),
            "sidebar column; got {side:?}"
        );
    }

    /// Roles the banding pass assigned, as `label => role`, in reading order.
    #[cfg(not(target_os = "windows"))]
    fn band_roles(name: &str, html: &str) -> (Vec<(String, String)>, serde_json::Value) {
        let raw = eval_fixture(name, html, &band_plan_js());
        let plan: serde_json::Value =
            serde_json::from_str(&raw).unwrap_or_else(|e| panic!("plan is JSON ({e}); got: {raw}"));
        let items = plan["items"]
            .as_array()
            .unwrap_or_else(|| panic!("plan has items; got: {plan}"))
            .iter()
            .map(|it| {
                (
                    it["label"].as_str().unwrap_or_default().to_owned(),
                    it["role"].as_str().unwrap_or_default().to_owned(),
                )
            })
            .collect();
        (items, plan)
    }

    #[test]
    #[ignore]
    #[cfg(not(target_os = "windows"))]
    fn banding_reads_regions_in_the_order_they_are_laid_out() {
        // Source order is footer, content, header. The layout says header,
        // content, footer — and that is what the page must read as.
        let _guard = launch_flag_guard(false);
        let (roles, plan) = band_roles("sic-band-plan.html", BAND_FIXTURE);
        let order: Vec<&str> = roles.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(
            order,
            vec!["div.hdr", "div.content", "div.ftr"],
            "regions must read top to bottom; plan: {plan}"
        );
        let role_of = |label: &str| -> String {
            roles
                .iter()
                .find(|(l, _)| l == label)
                .map(|(_, r)| r.clone())
                .unwrap_or_default()
        };
        assert_eq!(role_of("div.hdr"), "nav", "top link strip; plan: {plan}");
        assert_eq!(
            role_of("div.ftr"),
            "footer",
            "bottom link strip; plan: {plan}"
        );
        assert_eq!(
            role_of("div.content"),
            "main",
            "the dominant region holding the h1; plan: {plan}"
        );
    }

    #[test]
    #[ignore]
    #[cfg(not(target_os = "windows"))]
    fn banding_puts_the_page_in_reading_order_end_to_end() {
        // The same fixture through the real load path: landmarks in the HTML,
        // reading order in the FFON, and the article's heading must not have
        // swallowed the footer.
        let _guard = launch_flag_guard(false);
        let load = load_fixture("sic-band-e2e.html", BAND_FIXTURE);
        let html = &load.html;
        let at = |needle: &str| -> usize {
            html.find(needle)
                .unwrap_or_else(|| panic!("{needle} missing from output; got: {html}"))
        };
        assert!(
            at("NAV-ONE") < at("ARTICLE-HEADING") && at("ARTICLE-HEADING") < at("FOOTER-PRIVACY"),
            "reading order is nav, article, footer; got: {html}"
        );
        assert!(html.contains("<nav"), "a navigation landmark; got: {html}");
        assert!(html.contains("<main"), "a main landmark; got: {html}");
        assert!(html.contains("<footer"), "a footer landmark; got: {html}");

        let (elems, _map) = html_to_ffon_with_forms(html, "");
        let top: Vec<String> = elems
            .iter()
            .map(|e| {
                e.as_obj()
                    .map(|o| o.key.clone())
                    .or_else(|| e.as_str().map(|s| s.to_owned()))
                    .unwrap_or_default()
            })
            .collect();
        let joined = top.join(" | ");
        assert!(
            top.iter().any(|k| k == "main content"),
            "the content region is a landmark; got: {joined}"
        );
        assert!(
            top.iter().any(|k| k == "footer"),
            "the footer is its own group, not nested under the article; got: {joined}"
        );
    }

    /// A fixed cookie strip written first in the source and painted over the
    /// bottom of the window.  It is an overlay, not the page's colophon.
    #[cfg(not(target_os = "windows"))]
    const OVERLAY_FIXTURE: &str = r#"<!DOCTYPE html><html><head><style>
        body{margin:0}
        .cookie{position:fixed;bottom:0;left:0;width:1920px;height:120px}
        .art{height:1400px} .tail{height:200px}
        </style></head><body>
        <div class="cookie"><a href="/1">COOKIE-ACCEPT</a><a href="/2">COOKIE-REJECT</a>
            <a href="/3">COOKIE-MORE</a><a href="/4">COOKIE-SETTINGS</a></div>
        <div class="art"><h1>ARTICLE-HEADING</h1><p>ARTICLE-BODY</p></div>
        <div class="tail"><a href="/p">TAIL-PRIVACY</a><a href="/t">TAIL-TERMS</a>
            <a href="/c">TAIL-CONTACT</a><a href="/j">TAIL-JOBS</a></div>
        </body></html>"#;

    /// A page an author already marked up correctly, already in visual order.
    /// The pass must leave it completely alone.
    #[cfg(not(target_os = "windows"))]
    const IDENTITY_FIXTURE: &str = r#"<!DOCTYPE html><html><head><style>
        body{margin:0} nav{height:80px} main{height:900px} footer{height:200px}
        </style></head><body>
        <nav><a href="/1">NAV-ONE</a><a href="/2">NAV-TWO</a></nav>
        <main><h1>ARTICLE-HEADING</h1><p>ARTICLE-BODY</p></main>
        <footer><a href="/p">FOOT-PRIVACY</a><a href="/t">FOOT-TERMS</a></footer>
        </body></html>"#;

    #[test]
    #[ignore]
    #[cfg(not(target_os = "windows"))]
    fn a_fixed_overlay_is_not_mistaken_for_the_footer() {
        // A cookie bar is painted over the bottom of the viewport, which is
        // exactly where a footer lives. Position alone would call it one; being
        // out of flow is what says otherwise. It reads last, and it stays a
        // plain region — contentinfo would be a lie.
        let _guard = launch_flag_guard(false);
        let (roles, plan) = band_roles("sic-overlay.html", OVERLAY_FIXTURE);
        let cookie = roles
            .iter()
            .find(|(l, _)| l == "div.cookie")
            .unwrap_or_else(|| panic!("cookie strip in plan; got: {plan}"));
        assert_eq!(
            cookie.1, "none",
            "an overlay is not a landmark; plan: {plan}"
        );
        assert_eq!(
            roles.last().map(|(l, _)| l.as_str()),
            Some("div.cookie"),
            "the overlay reads last despite being first in source; plan: {plan}"
        );
        // The real trailing region is still allowed to be the footer.
        assert_eq!(
            roles
                .iter()
                .find(|(l, _)| l == "div.tail")
                .map(|(_, r)| r.as_str()),
            Some("footer"),
            "the in-flow trailing strip is the footer; plan: {plan}"
        );
    }

    #[test]
    #[ignore]
    #[cfg(not(target_os = "windows"))]
    fn a_well_authored_page_is_left_exactly_as_it_is() {
        // The author's markup beats our geometry. Nothing to reorder, nothing
        // to synthesize, and above all no second <main> stacked on the first.
        let _guard = launch_flag_guard(false);
        let load = load_fixture("sic-identity.html", IDENTITY_FIXTURE);
        let html = &load.html;
        assert_eq!(
            html.matches("<main").count(),
            1,
            "exactly one main landmark; got: {html}"
        );
        assert_eq!(
            html.matches("<footer").count(),
            1,
            "exactly one footer landmark; got: {html}"
        );
        assert_eq!(
            html.matches("<nav").count(),
            1,
            "exactly one navigation landmark; got: {html}"
        );
        let at = |needle: &str| html.find(needle).expect(needle);
        assert!(
            at("NAV-ONE") < at("ARTICLE-HEADING") && at("ARTICLE-HEADING") < at("FOOT-PRIVACY"),
            "source order is already reading order and must be preserved; got: {html}"
        );
    }

    /// Three boxes in a right-to-left flex row: the first in the source sits
    /// furthest right, and right is where reading starts.
    #[cfg(not(target_os = "windows"))]
    const RTL_FIXTURE: &str = r#"<!DOCTYPE html><html><head><style>
        body{margin:0} .row{display:flex;width:1920px}
        .row > div{width:300px;height:200px}
        </style></head><body><div class="row" dir="rtl">
        <div class="one">RTL-ONE</div>
        <div class="two">RTL-TWO</div>
        <div class="three">RTL-THREE</div>
        </div></body></html>"#;

    #[test]
    #[ignore]
    #[cfg(not(target_os = "windows"))]
    fn right_to_left_reads_from_the_right() {
        // Sorting by ascending x is only correct in a left-to-right document.
        // Here the leftmost box is the *last* one to read, and blindly sorting
        // on x would reverse the row.
        let _guard = launch_flag_guard(false);
        let (roles, plan) = band_roles("sic-rtl.html", RTL_FIXTURE);
        let order: Vec<&str> = roles.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(
            order,
            vec!["div.one", "div.two", "div.three"],
            "right-to-left reading order, which here is source order; plan: {plan}"
        );
        // And the geometry really is reversed, so this is not a no-op test.
        let x = |label: &str| -> i64 {
            plan["items"]
                .as_array()
                .unwrap()
                .iter()
                .find(|it| it["label"] == label)
                .and_then(|it| it["x"].as_i64())
                .unwrap_or_else(|| panic!("no x for {label}; plan: {plan}"))
        };
        assert!(
            x("div.one") > x("div.three"),
            "the first box is laid out furthest right; plan: {plan}"
        );
    }

    #[test]
    #[ignore]
    #[cfg(not(target_os = "windows"))]
    fn screen_reader_only_text_stays_next_to_what_it_annotates() {
        // .sr-only text is a clipped 1x1 box. It must never be pruned by
        // geometry, and it must not be sorted to an end either: with no box of
        // its own it rides along with the sibling before it.
        let _guard = launch_flag_guard(false);
        let load = load_fixture("sic-sronly-order.html", PRUNE_FIXTURE);
        let html = &load.html;
        let visible = html
            .find("VISIBLE-PARAGRAPH")
            .unwrap_or_else(|| panic!("visible text survives; got: {html}"));
        let sronly = html
            .find("SCREENREADER-ONLY")
            .unwrap_or_else(|| panic!("screen-reader text survives; got: {html}"));
        assert!(
            visible < sronly,
            "the annotation stays after what it annotates; got: {html}"
        );
    }

    /// Fixture covering every prune rule at once, including the one that must
    /// *not* fire: screen-reader-only text.
    #[cfg(not(target_os = "windows"))]
    const PRUNE_FIXTURE: &str = r#"<!DOCTYPE html><html><head><title>t</title>
        <style>.sronly{position:absolute;width:1px;height:1px;overflow:hidden;clip:rect(0 0 0 0)}</style>
        </head><body>
        <p>VISIBLE-PARAGRAPH</p>
        <span class="sronly">SCREENREADER-ONLY</span>
        <div style="display:none"><p>DISPLAY-NONE-TEXT</p></div>
        <div aria-hidden="true"><p>ARIA-HIDDEN-TEXT</p></div>
        <div hidden><p>HIDDEN-ATTR-TEXT</p></div>
        <div style="visibility:hidden"><p>VISIBILITY-HIDDEN-TEXT</p></div>
        </body></html>"#;

    #[test]
    #[ignore]
    #[cfg(not(target_os = "windows"))]
    fn hidden_nodes_are_pruned_but_screen_reader_text_survives() {
        let _guard = launch_flag_guard(false);
        let restore = prune_hidden();
        PRUNE_HIDDEN.store(true, Ordering::Release);
        let load = load_fixture("sic-prune.html", PRUNE_FIXTURE);
        PRUNE_HIDDEN.store(restore, Ordering::Release);

        assert!(load.html.contains("VISIBLE-PARAGRAPH"));
        // The rule that matters most for this app: .sr-only text is a clipped
        // 1x1 box but still display:block/visible, and it is exactly what a
        // screen reader is supposed to read.
        assert!(
            load.html.contains("SCREENREADER-ONLY"),
            "screen-reader-only text must never be pruned"
        );
        for gone in [
            "DISPLAY-NONE-TEXT",
            "ARIA-HIDDEN-TEXT",
            "HIDDEN-ATTR-TEXT",
            "VISIBILITY-HIDDEN-TEXT",
        ] {
            assert!(!load.html.contains(gone), "{gone} should have been pruned");
        }
    }

    /// A collapsed menu, a dead hidden blurb, and a skip-link target that is an
    /// empty anchor — the three shapes the prune has to tell apart.
    #[cfg(not(target_os = "windows"))]
    const NAV_FIXTURE: &str = r##"<!DOCTYPE html><html><body>
        <a href="#main-content">Skip to content</a>
        <div class="collapse" style="display:none">
          <a href="/send">MENU-SEND-PARCEL</a>
          <a href="/receive">MENU-RECEIVE-PARCEL</a>
        </div>
        <div style="display:none"><p>DEAD-HIDDEN-BLURB with no way to reach it</p></div>
        <div aria-hidden="true"><a href="/x">ARIA-HIDDEN-LINK</a></div>
        <a id="main-content"></a>
        <h1>REAL-HEADING</h1>
        <p>REAL-BODY</p>
        </body></html>"##;

    #[test]
    #[ignore]
    #[cfg(not(target_os = "windows"))]
    fn hidden_navigation_survives_but_dead_hidden_text_does_not() {
        let _guard = launch_flag_guard(false);
        let restore = prune_hidden();
        PRUNE_HIDDEN.store(true, Ordering::Release);
        let load = load_fixture("sic-nav.html", NAV_FIXTURE);
        PRUNE_HIDDEN.store(restore, Ordering::Release);

        // A collapsed menu is the site's navigation and nothing here can press
        // the hamburger to open it, so it stays.
        assert!(load.html.contains("MENU-SEND-PARCEL"));
        assert!(load.html.contains("MENU-RECEIVE-PARCEL"));
        assert!(load.html.contains("REAL-HEADING"));
        // Hidden text with nothing to act on is just weight.
        assert!(!load.html.contains("DEAD-HIDDEN-BLURB"));
        // aria-hidden is the author telling assistive tech to skip it, links
        // or no links, and this app is assistive tech.
        assert!(!load.html.contains("ARIA-HIDDEN-LINK"));
    }

    #[test]
    #[ignore]
    #[cfg(not(target_os = "windows"))]
    fn an_empty_anchors_id_moves_to_the_next_thing_that_renders() {
        let _guard = launch_flag_guard(false);
        let load = load_fixture("sic-anchor.html", NAV_FIXTURE);
        let (elems, _) = html_to_ffon_with_forms(&load.html, "");
        let rendered = format!("{elems:?}");
        // `<a id="main-content"></a>` emits no node, so without rehoming the
        // skip link points at nothing.
        assert!(
            rendered.contains("<id>main-content</id>"),
            "skip-link target was lost: {rendered}"
        );
        assert!(rendered.contains("<link>#main-content</link>"));
    }

    #[test]
    #[ignore]
    #[cfg(not(target_os = "windows"))]
    fn toggling_the_prune_off_brings_hidden_content_back() {
        let _guard = launch_flag_guard(false);
        let restore = prune_hidden();
        PRUNE_HIDDEN.store(false, Ordering::Release);
        let load = load_fixture("sic-prune-off.html", PRUNE_FIXTURE);
        PRUNE_HIDDEN.store(restore, Ordering::Release);

        for present in [
            "VISIBLE-PARAGRAPH",
            "SCREENREADER-ONLY",
            "DISPLAY-NONE-TEXT",
            "ARIA-HIDDEN-TEXT",
            "HIDDEN-ATTR-TEXT",
            "VISIBILITY-HIDDEN-TEXT",
        ] {
            assert!(
                load.html.contains(present),
                "{present} missing with prune off"
            );
        }
    }

    #[test]
    #[ignore]
    #[cfg(not(target_os = "windows"))]
    fn a_pruned_subtree_keeps_its_forms_counted() {
        // Removing a hidden <form> outright would renumber every later form,
        // while the click path resolves buttons via `document.forms[n]` on the
        // live page. The shell keeps the numbering aligned.
        let _guard = launch_flag_guard(false);
        // aria-hidden prunes regardless of what is inside, so this still
        // exercises the shell that keeps the numbering aligned.
        let html = r#"<!DOCTYPE html><html><body>
            <div aria-hidden="true"><form><input name="ghost"></form></div>
            <form><input type="search" name="q" placeholder="Search"><button>Go</button></form>
            </body></html>"#;
        let load = load_fixture("sic-form-index.html", html);
        assert!(!load.html.contains("ghost"), "hidden form contents pruned");
        let (_e, map) = html_to_ffon_with_forms(&load.html, "");
        assert!(
            map.keys().any(|k| k.starts_with("form_2/")),
            "the visible form must still be form_2; keys: {:?}",
            map.keys().collect::<Vec<_>>()
        );
    }

    #[test]
    #[ignore]
    #[cfg(not(target_os = "windows"))]
    fn a_parked_subtree_keeps_its_forms_counted() {
        // The pruner leaves an empty shell where it removes a hidden form, so
        // `document.forms[n]` keeps lining up. Parking never did: a hidden but
        // *actionable* subtree is not marked hidden, so it skips the shell path
        // entirely and gets moved to the end of the body with its forms inside
        // — renumbering every form after it. A hidden login dropdown over a
        // search box is exactly that shape, and typing in the search box would
        // fill the login form instead.
        let _guard = launch_flag_guard(false);
        let html = r#"<!DOCTYPE html><html><body>
            <div style="display:none"><a href="/login">LOGIN-LINK</a>
                <form><input name="ghost-user"></form></div>
            <form><input type="search" name="q" placeholder="Search"><button>Go</button></form>
            </body></html>"#;
        let load = load_fixture("sic-park-form-index.html", html);
        // The parked menu is kept — it is navigation, not dead weight.
        assert!(
            load.html.contains("LOGIN-LINK"),
            "the parked menu survives; got: {}",
            load.html
        );
        let (_e, map) = html_to_ffon_with_forms(&load.html, "");
        assert!(
            map.iter()
                .any(|(k, v)| k.starts_with("form_2/") && v.css_selector.contains('q')),
            "the search box is document.forms[1] on the live page, so it must \
             stay form_2 whatever parking does to the order; keys: {:?}",
            map.iter()
                .map(|(k, v)| (k, &v.css_selector))
                .collect::<Vec<_>>()
        );
    }

    /// A OneTrust-shaped banner over a real article.  Each choice rewrites the
    /// body, so a press is observable in the page the provider re-reads.
    #[cfg(not(target_os = "windows"))]
    const CONSENT_FIXTURE: &str = r#"<!DOCTYPE html><html><body>
        <h1>ARTICLE-HEADING</h1>
        <p>ARTICLE-BODY</p>
        <div id="onetrust-banner-sdk">
          <p>BANNER-PROSE about cookies</p>
          <button onclick="document.body.innerHTML='&lt;p&gt;CHOICE-REJECT&lt;/p&gt;'">Alles weigeren</button>
          <button id="onetrust-accept-btn-handler" onclick="document.body.innerHTML='&lt;p&gt;CHOICE-ACCEPT&lt;/p&gt;'">Alles accepteren</button>
          <button>Cookie settings</button>
        </div>
        </body></html>"#;

    #[test]
    #[ignore]
    #[cfg(not(target_os = "windows"))]
    fn a_consent_banner_becomes_a_step_of_its_own() {
        let _guard = launch_flag_guard(false);
        let load = load_fixture("sic-consent.html", CONSENT_FIXTURE);

        // The step page is the choices and nothing else — not the banner, and
        // not the article, which is what answering the step leads to.
        assert!(!load.html.contains("BANNER-PROSE"), "banner text leaked in");
        assert!(
            !load.html.contains("ARTICLE-HEADING"),
            "content comes after the step, not alongside it: {}",
            load.html
        );
        assert_eq!(load.notices.len(), 1, "expected the step's heading line");

        let (elems, map) = html_to_ffon_with_forms(&load.html, "");
        let rendered = format!("{elems:?}");
        assert_eq!(elems.len(), 2, "exactly two choices: {rendered}");
        assert!(rendered.contains("Alles weigeren"));
        assert!(rendered.contains("Alles accepteren"));
        // A settings button opens a panel and decides nothing, so it is never
        // offered as a choice.
        assert!(!rendered.to_lowercase().contains("cookie settings"));
        // Refusing comes first, and each choice is separately pressable.
        assert!(
            rendered.find("Alles weigeren").unwrap() < rendered.find("Alles accepteren").unwrap(),
            "reject should be offered first"
        );
        assert!(map.keys().any(|k| k.starts_with("form_1/")));
        assert!(map.keys().any(|k| k.starts_with("form_2/")));
    }

    /// An ordinary multilingual page, shaped like anysurfer.be and
    /// elevenways.be: content, a switcher in the nav, `<link rel=alternate>`
    /// for the other two languages, and `hreflang` sprayed over content links.
    ///
    /// Every part of that is a trap the old language step fell into. The
    /// current language is a `<span lang>` rather than a link, so it was left
    /// out of the list of languages entirely, and the content links carrying
    /// `hreflang="nl"` overwrote the switcher's Dutch entry with their own text
    /// and href.
    #[cfg(not(target_os = "windows"))]
    const SWITCHER_FIXTURE: &str = r#"<!DOCTYPE html><html lang="nl"><head>
        <link href="https://x.invalid/en" rel="alternate" hreflang="en">
        <link href="https://x.invalid/fr" rel="alternate" hreflang="fr">
        </head><body>
        <ul class="languages">
          <li><span lang="NL">Nederlands</span></li>
          <li><a lang="en" hreflang="en" href="https://x.invalid/en">English</a></li>
          <li><a lang="fr" hreflang="fr" href="https://x.invalid/fr">Français</a></li>
        </ul>
        <h1>ARTICLE-HEADING</h1>
        <p>ARTICLE-BODY</p>
        <a hreflang="nl" href="/nl/contact">CONTACT-LINK</a>
        </body></html>"#;

    #[test]
    #[ignore]
    #[cfg(not(target_os = "windows"))]
    fn a_page_with_a_language_switcher_is_just_a_page() {
        let _guard = launch_flag_guard(false);
        let load = load_fixture("sic-switcher.html", SWITCHER_FIXTURE);

        // The page is what loads. Nothing is asked first.
        assert!(
            load.notices.is_empty(),
            "a language switcher is not a question: {:?}",
            load.notices
        );
        // Through the provider's own renderer, not the SDK walker directly:
        // the language section is added here, and calling past it is how this
        // test used to pass while bpost had nothing at all.
        let (elems, _) = page_to_ffon_with_forms(&load, "https://x.invalid/nl");
        let rendered = format!("{elems:?}");
        for want in ["ARTICLE-HEADING", "ARTICLE-BODY", "CONTACT-LINK"] {
            assert!(
                rendered.contains(want),
                "content missing {want}: {rendered}"
            );
        }
        // The switcher reads as the links it is, current language included —
        // that entry is exactly the one the old step could not see.
        for lang in ["Nederlands", "English", "Français"] {
            assert!(
                rendered.contains(lang),
                "the switcher lost {lang}: {rendered}"
            );
        }

        // And the page's own declaration is read too, as a trailing section.
        // On a page like this it restates a switcher that was already legible;
        // on bpost it is the only thing there is. The Dutch entry comes from
        // <html lang>, since the page declares alternates for the other two
        // only — the same shape as anysurfer.be.
        let section = elems
            .last()
            .and_then(|e| e.as_obj())
            .filter(|o| o.key == localize::t("webbrowser-languages"))
            .unwrap_or_else(|| panic!("no language section: {rendered}"));
        assert_eq!(section.children.len(), 3, "{:?}", section.children);
        let listed = format!("{:?}", section.children);
        assert!(
            listed.contains("English <link>https://x.invalid/en</link>"),
            "{listed}"
        );
        assert!(
            listed.contains("Français <link>https://x.invalid/fr</link>"),
            "{listed}"
        );
        // The content link's stray hreflang="nl" must not become the Dutch
        // entry — that exact leak is what sent the old step to /nl/contact.
        assert!(
            listed.contains("<link>https://x.invalid/nl</link>"),
            "Dutch should be the page you are on, not a content link: {listed}"
        );
        assert!(
            !listed.contains("contact"),
            "a content link leaked in: {listed}"
        );
    }

    /// A provider that shuts its Chrome down when the test ends, panic or not.
    ///
    /// Nothing kills the child on drop and `cleanup` is only ever called by the
    /// app, so a browser-backed test without this leaves a Chrome and its Xvfb
    /// running until the machine is rebooted. Enough of those accumulating is
    /// what took the desktop session down twice.
    #[cfg(not(target_os = "windows"))]
    struct TestProvider(WebbrowserProvider);

    #[cfg(not(target_os = "windows"))]
    impl TestProvider {
        fn new() -> Self {
            TestProvider(WebbrowserProvider::new())
        }
    }

    #[cfg(not(target_os = "windows"))]
    impl std::ops::Deref for TestProvider {
        type Target = WebbrowserProvider;
        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    #[cfg(not(target_os = "windows"))]
    impl std::ops::DerefMut for TestProvider {
        fn deref_mut(&mut self) -> &mut Self::Target {
            &mut self.0
        }
    }

    #[cfg(not(target_os = "windows"))]
    impl Drop for TestProvider {
        fn drop(&mut self) {
            self.0.cleanup();
        }
    }

    /// Drive the provider until `tick` reports fresh content, or give up.
    #[cfg(not(target_os = "windows"))]
    fn pump(p: &mut WebbrowserProvider) -> bool {
        for _ in 0..300 {
            if p.tick() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        false
    }

    #[test]
    #[ignore]
    #[cfg(not(target_os = "windows"))]
    fn pressing_a_surfaced_choice_activates_the_real_banner_button() {
        // The whole point of the proxy form: `on_button_press` only ever gets
        // `submit:form_N`, so each choice needs its own form for the selector
        // and `document.forms[n]` lookup to land on the button it names.
        let _guard = launch_flag_guard(false);
        let path = std::env::temp_dir().join("sic-consent-press.html");
        std::fs::write(&path, CONSENT_FIXTURE).expect("write fixture");
        let url = format!("file://{}", path.display());

        let mut p = TestProvider::new();
        p.load_url(&url);
        assert!(pump(&mut p), "fixture never finished loading");

        // form_1 is the first choice offered, i.e. "Alles weigeren".
        p.on_button_press("submit:form_1");
        assert!(pump(&mut p), "no content after pressing the choice");

        let rendered = format!("{:?}", p.cached_page.as_ref().unwrap().elements);
        let _ = std::fs::remove_file(&path);
        assert!(
            rendered.contains("CHOICE-REJECT"),
            "pressing the first choice should run the site's reject button, got: {rendered}"
        );
        assert!(
            !rendered.contains("CHOICE-ACCEPT"),
            "the wrong button was clicked: {rendered}"
        );
    }

    // Live end-to-end: exercises the real Linux runtime path
    // (init_live_session -> navigate_and_get_html) against google.com with a
    // fresh cookieless profile, so Google serves its inline GDPR wall.  We no
    // longer accept on the user's behalf, so the check is that both choices are
    // surfaced from the wall's own script rather than that the wall is gone.
    //
    // Ignored by default (needs Chrome + Xvfb + network).  Run with:
    //   XDG_CONFIG_HOME=$(mktemp -d) cargo test -p sicompass-webbrowser \
    //     live_google_consent -- --ignored --nocapture
    #[test]
    #[ignore]
    #[cfg(target_os = "linux")]
    fn live_google_consent_offers_both_choices() {
        let loaded = chromium_runtime().block_on(async {
            let live = init_live_session().await.expect("launch chrome");
            let out = navigate_and_get_html(&live.page, "https://www.google.com/").await;
            close_browser(&live.session).await;
            out
        });
        let loaded = loaded.expect("navigate_and_get_html should not fail");
        assert_eq!(
            loaded.notices,
            vec![localize::t("webbrowser-step-consent")],
            "expected the cookie step's heading line"
        );

        // The step page is the two choices and nothing else — Google's wall is
        // not shown, and neither is the homepage behind it.
        let (elems, map) = html_to_ffon_with_forms(&loaded.html, "https://www.google.com/");
        let rendered = format!("{elems:?}");
        assert_eq!(elems.len(), 2, "expected exactly two choices: {rendered}");
        assert!(
            rendered.contains(&localize::t("webbrowser-consent-reject-all")),
            "reject choice missing from: {rendered}"
        );
        assert!(
            rendered.contains(&localize::t("webbrowser-consent-accept-all")),
            "accept choice missing from: {rendered}"
        );
        // Two single-button forms, so both are individually pressable.
        assert!(map.keys().any(|k| k.starts_with("form_1/")));
        assert!(map.keys().any(|k| k.starts_with("form_2/")));
    }

    // The check the SOCS rewrite exists for: pressing "reject all" on Google's
    // wall has to actually clear it.  The old hardcoded 2021 payload no longer
    // did; the values now come from the wall's own inline script.
    //
    //   XDG_CONFIG_HOME=$(mktemp -d) cargo test -p sicompass-webbrowser \
    //     live_google_reject -- --ignored --nocapture
    #[test]
    #[ignore]
    #[cfg(target_os = "linux")]
    fn live_google_reject_clears_the_wall() {
        let _guard = launch_flag_guard(false);
        let mut p = TestProvider::new();
        p.load_url("https://www.google.com/");
        assert!(pump(&mut p), "google.com never finished loading");

        // form_1 is the first choice, i.e. reject.
        p.on_button_press("submit:form_1");
        assert!(pump(&mut p), "no content after choosing reject");

        let rendered = format!("{:?}", p.cached_page.as_ref().unwrap().elements);
        assert!(
            !rendered.contains(&localize::t("webbrowser-consent-reject-all")),
            "the wall came back after rejecting: {}",
            rendered.chars().take(600).collect::<String>()
        );
        // The reloaded homepage exposes the search box.
        assert!(
            rendered.contains("<input>") || rendered.to_lowercase().contains("zoek"),
            "search box missing after rejecting: {}",
            rendered.chars().take(600).collect::<String>()
        );
    }

    // Live end-to-end against the site that prompted all of this: bpost loads a
    // OneTrust banner via GTM, so its cookie step arrives a beat after the page
    // has otherwise settled, and only then comes the content.
    //
    // It also pins that the language question is gone. bpost offers four
    // languages, so it used to ask before showing anything.
    //
    //   cargo test -p sicompass-webbrowser live_bpost -- --ignored --nocapture
    #[test]
    #[ignore]
    #[cfg(target_os = "linux")]
    fn live_bpost_answers_cookies_then_shows_content() {
        let _guard = launch_flag_guard(false);
        let mut p = TestProvider::new();
        // The language is pinned by the address, which is how a reader gets a
        // language now that nothing asks. Where the choice itself comes from is
        // asserted at the end: not from bpost's own switcher, which is an
        // aria-hidden modal of href-less anchors and unreachable either way.
        p.load_url("https://www.bpost.be/nl");
        assert!(pump(&mut p), "bpost never finished loading");

        // ---- step 1: cookies ---------------------------------------------
        let step2 = format!("{:?}", p.cached_page.as_ref().unwrap().elements);
        // Checked through the proxy ids, not the wording: bpost serves Dutch
        // here, and its reject button reads "Alles afwijzen".
        let consent: Vec<usize> = p
            .form_map
            .iter()
            .filter(|(_, n)| n.css_selector.starts_with("#sic-consent-"))
            .filter_map(|(k, _)| {
                k.trim_start_matches("form_")
                    .split('/')
                    .next()?
                    .parse()
                    .ok()
            })
            .collect();
        assert_eq!(
            consent.len(),
            2,
            "expected reject and accept as the cookie step, got: {step2}"
        );
        assert!(
            step2.to_lowercase().contains("afwijzen"),
            "refusing should be offered, got: {step2}"
        );
        // Refusing is listed first, so it is the lowest form index.
        let reject_n = *consent.iter().min().unwrap();
        p.on_button_press(&format!("submit:form_{reject_n}"));
        assert!(pump(&mut p), "no page after choosing on cookies");

        // ---- step 2: the content ------------------------------------------
        let content = format!("{:?}", p.cached_page.as_ref().unwrap().elements);
        assert!(
            content.contains("Welkom bij Bpost"),
            "expected bpost's content after the cookie step, got: {content}"
        );
        // bpost keeps its whole main menu in a .navbar-collapse that stays
        // display:none at 1920px, so an unconditional prune silently ate the
        // site's primary navigation.
        for nav in ["Pakje verzenden", "Pakje ontvangen", "Vind bpost"] {
            assert!(
                content.contains(nav),
                "hidden navigation was pruned away: {nav:?} missing"
            );
        }
        // Reachable, not buried: bpost's menus are display:none containers that
        // sit before the article, and leaving them inline let their headings
        // swallow the whole page into "De voordelen van je bpost-account?".
        let top_keys = p
            .cached_page
            .as_ref()
            .unwrap()
            .elements
            .iter()
            .map(|e| match e {
                FfonElement::Str(s) => s.clone(),
                FfonElement::Obj(o) => o.key.clone(),
            })
            .collect::<Vec<_>>();
        let top = top_keys.join(" | ");
        eprintln!("bpost top level: {top}");
        // The page reads as regions now, not as a flat run of everything the
        // template emitted: the content is a landmark of its own instead of
        // sitting eighth of nineteen entries with the header spread either side
        // of it.
        assert!(
            top.contains("main content"),
            "the content region should be a landmark of its own, got: {top}"
        );
        assert!(
            top.split(" | ").count() < 8,
            "the top level should be a handful of regions, not the whole page; got: {top}"
        );
        // The menus themselves are still reachable — the loop above checks all
        // three labels survive somewhere in the tree, which is the half that
        // matters. What this adds is that they no longer *dominate* it: the
        // login dropdown opens with a heading, and FFON nests everything after
        // a heading underneath it, so leaving it inline filed the whole page
        // under "De voordelen van je bpost-account?". It has to be nested now,
        // never a top-level entry of its own.
        // (It may still be *sampled* into a container's generated name, which is
        // just FFON labelling a group from its contents — what must not happen
        // is the heading standing as an entry of its own with the page beneath.)
        assert!(
            !top_keys
                .iter()
                .any(|k| k.starts_with("De voordelen van je bpost-account?")),
            "the login dropdown's heading must be contained, not leading the \
             page, got: {top}"
        );
        // And the content comes before the parked menus, which is the whole
        // point of parking them.
        assert!(
            top.find("main content") < top.find("navigation"),
            "content should precede the parked menus, got: {top}"
        );

        // The skip link's target is an empty <a id="main-content">, which emits
        // no node of its own, so the id has to be rehomed. Landing it on the
        // next thing that renders is not enough either: bpost opens its content
        // region with two <h1>s in a row, so the id ended up on an empty
        // heading sitting *beside* the article rather than containing it —
        // "a link to itself without the content".
        assert!(
            content.contains("<link>#main-content</link>"),
            "skip link missing: {content}"
        );
        fn find_target(e: &FfonElement) -> Option<usize> {
            match e {
                FfonElement::Str(_) => None,
                FfonElement::Obj(o) => {
                    if o.key.contains("<id>main-content</id>") {
                        return Some(o.children.len());
                    }
                    o.children.iter().find_map(find_target)
                }
            }
        }
        let kids = p
            .cached_page
            .as_ref()
            .unwrap()
            .elements
            .iter()
            .find_map(find_target)
            .unwrap_or_else(|| panic!("skip link has no target to jump to: {content}"));
        assert!(
            kids > 0,
            "the skip-link target is empty — it must contain the content, not sit next to it"
        );
        // What should still be gone: the answered steps and the vendor's parked
        // preference centre. "Ik spreek Nederlands" is the modal's own wording;
        // the section asserted below lists autonyms, so this stays a real check
        // that the aria-hidden modal is not what is being read.
        for gone in [
            "Ik spreek Nederlands",
            "Alles afwijzen",
            "Informatie over het gebruik van cookies",
        ] {
            assert!(
                !content.contains(gone),
                "{gone:?} should not be in the content"
            );
        }

        // ---- step 3: the languages ----------------------------------------
        // The regression this test now guards. bpost offers four languages and
        // its only in-page switcher is unreachable, so before this the site had
        // no language choice at all. They come from the four
        // <link rel="alternate" hreflang> lines in its <head>.
        let section = p
            .cached_page
            .as_ref()
            .unwrap()
            .elements
            .last()
            .and_then(|e| e.as_obj())
            .filter(|o| o.key == localize::t("webbrowser-languages"))
            .unwrap_or_else(|| panic!("no language section at the end of the page: {content}"));
        let listed = format!("{:?}", section.children);
        for (name, href) in [
            ("Nederlands", "https://www.bpost.be/nl"),
            ("Français", "https://www.bpost.be/fr"),
            ("English", "https://www.bpost.be/en"),
            ("Deutsch", "https://www.bpost.be/de"),
        ] {
            assert!(
                listed.contains(&format!("<link>{href}</link>")),
                "{name} is not offered: {listed}"
            );
        }
        // Followable, not just readable: every entry is a real link.
        assert_eq!(section.children.len(), 4, "{listed}");
    }
}

// ---------------------------------------------------------------------------
// SDK registration
// ---------------------------------------------------------------------------

/// Register the web browser with the SDK factory and manifest registries.
pub fn register() {
    sicompass_sdk::register_provider_factory("webbrowser", || Box::new(WebbrowserProvider::new()));
    sicompass_sdk::register_builtin_manifest(
        sicompass_sdk::BuiltinManifest::new("webbrowser", "web browser").with_settings(vec![
            // `urlHistorySize`, not `historySize`: settings are broadcast to
            // every provider in every tab by bare key, so the key namespace is
            // shared across the whole app.
            sicompass_sdk::SettingDecl::text(
                "web browser",
                "URL history",
                "urlHistorySize",
                "50000",
            ),
        ]),
    );
    sicompass_sdk::register_url_fetcher(fetch_url_to_ffon);
}
