//! Past Claude sessions, read from Claude Code's own on-disk transcripts.
//!
//! Claude Code writes one JSONL file per session to
//! `~/.claude/projects/<dashified-cwd>/<session-uuid>.jsonl`. This module finds
//! the ones belonging to a folder (and its descendants), reads enough of each to
//! label it, replays one into a [`Conversation`], and deletes one.
//!
//! # Why not `events::StreamEvent`
//!
//! The live stream and the on-disk transcript share the `user` / `assistant`
//! record shape but not the rest: disk-only records (`ai-title`, `last-prompt`,
//! `queue-operation`, `mode`, `attachment`, `file-history-snapshot`, …) are not
//! stream events, and teaching the live-stream model about them would couple two
//! formats that only happen to overlap. So [`DiskRecord`] is the envelope here,
//! and only the *content* types (`ApiMessage`, `ContentField`, `ContentBlock`)
//! are shared.
//!
//! # Reading cheaply
//!
//! A transcript runs to several megabytes, and the list has to be built from
//! every transcript in the folder, so nothing here reads a whole file except
//! [`replay`], which runs once when a session is actually opened.
//!
//! * the **cwd** and the **first prompt** are near the start, so [`read_head`]
//!   streams with a budget and stops as soon as it has both;
//! * the **title** is near the *end* — Claude Code appends an `ai-title` record
//!   each time it refines the title, and the last one wins — so [`read_title`]
//!   seeks to a tail window instead of scanning;
//! * results are cached per file on `(mtime, len)`, which a new turn always
//!   changes, so the steady-state cost of rebuilding the list is one `stat` per
//!   transcript.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Deserialize;

use crate::events::{ApiMessage, ContentBlock};
use crate::render::Conversation;

// ---------------------------------------------------------------------------
// Where the transcripts live, and keeping tests away from the real ones
// ---------------------------------------------------------------------------
//
// `cargo test` once corrupted this developer's real browser history, terminal
// history and OS trash by writing to the live state directory, so a provider
// that *deletes* files needs the test seam built in from the start.
//
// Two layers, and the split matters:
//
// * `TEST_PROJECTS_ROOT` is **thread-local**, because it carries per-test data
//   (each test has its own `TempDir`) and `cargo test` runs many tests in one
//   process. A global would let one test observe another's root and, after a
//   teardown race, leave a test seeing `None` — which means "fall through to the
//   real `~/.claude/projects`". Thread-local makes each root private.
// * `NO_AMBIENT_PROJECTS` is a plain global defaulting to `cfg!(test)`, so a
//   unit test that forgets to inject a root sees an empty list rather than the
//   developer's transcripts. It fails closed. The app's integration tests are a
//   different binary, where this crate is compiled *without* `cfg(test)`, so
//   `ensure_builtins()` sets it once.
//
// This is sound only because `fetch()` runs on the caller's own thread. It does
// today: `Session`'s reader thread only buffers stdout lines and never reaches
// this module. If provider work ever moves to a worker thread, the thread-local
// silently stops applying and tests start reading the real home.

thread_local! {
    static TEST_PROJECTS_ROOT: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

static NO_AMBIENT_PROJECTS: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(cfg!(test));

/// Point this thread's session scanning at `path` instead of the real
/// `~/.claude/projects`. Test-only; thread-local, so it affects no other test.
#[doc(hidden)]
pub fn _set_test_projects_root(path: Option<PathBuf>) {
    TEST_PROJECTS_ROOT.with(|c| *c.borrow_mut() = path);
}

/// Refuse to fall back to the real `~/.claude/projects` when no root is
/// injected. Defaults to `true` under `cfg(test)`; the integration binary sets
/// it explicitly.
#[doc(hidden)]
pub fn _set_test_no_ambient_projects(enabled: bool) {
    NO_AMBIENT_PROJECTS.store(enabled, std::sync::atomic::Ordering::Release);
}

/// The directory holding every project's transcripts, or `None` when there is
/// none to read (no home directory, or ambient access is switched off).
pub(crate) fn projects_root() -> Option<PathBuf> {
    if let Some(p) = TEST_PROJECTS_ROOT.with(|c| c.borrow().clone()) {
        return Some(p);
    }
    if NO_AMBIENT_PROJECTS.load(std::sync::atomic::Ordering::Acquire) {
        return None;
    }
    sicompass_sdk::platform::home_dir().map(|h| h.join(".claude").join("projects"))
}

/// Claude Code's directory name for a working directory: every character that
/// is not alphanumeric becomes `-`.
///
/// The mapping is per-character, so it is **prefix-preserving**: a transcript
/// for any descendant of `/a/b` lives in a directory whose name starts with
/// `dashify("/a/b")`. That is what makes the "this folder and below" filter in
/// [`scan_in`] free of false negatives.
///
/// It is also lossy — `/a/b-c` and `/a/b/c` both dashify to `-a-b-c` — so a
/// name match is a *filter*, never a conclusion. The authoritative working
/// directory is the `cwd` recorded inside the file, and [`scan_in`] checks it.
pub(crate) fn dashify(path: &Path) -> String {
    path.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

// ---------------------------------------------------------------------------
// The records we read
// ---------------------------------------------------------------------------

/// One line of an on-disk transcript. Every field is optional because the file
/// interleaves a dozen record types and we only care about four of them.
#[derive(Debug, Default, Deserialize)]
struct DiskRecord {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    cwd: Option<String>,
    #[serde(default, rename = "aiTitle")]
    ai_title: Option<String>,
    /// Claude Code's own injected turns (command caveats and the like), which
    /// are not something the user typed.
    #[serde(default, rename = "isMeta")]
    is_meta: bool,
    #[serde(default)]
    message: Option<ApiMessage>,
}

/// One past session, as much as the list needs to know about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SessionMeta {
    /// The session uuid — the file stem, and what `--resume` takes.
    pub id: String,
    pub path: PathBuf,
    /// The folder the session actually ran in, read from the file rather than
    /// inferred from the directory name.
    pub cwd: PathBuf,
    /// Claude Code's own generated title, when it has written one yet.
    pub title: Option<String>,
    /// The session's first real user prompt, noise stripped.
    pub first_prompt: Option<String>,
    pub modified: SystemTime,
}

impl SessionMeta {
    /// What the row says: Claude's title, else the first prompt. `None` when the
    /// session has neither yet, and the caller supplies a localized placeholder.
    pub fn label(&self) -> Option<&str> {
        self.title
            .as_deref()
            .or(self.first_prompt.as_deref())
            .filter(|s| !s.is_empty())
    }
}

// ---------------------------------------------------------------------------
// Noise stripping
// ---------------------------------------------------------------------------

/// Wrappers Claude Code and its IDE integration add around, or instead of, what
/// the user typed. A first prompt is very often one of these with the real text
/// sitting after it in the same record.
const NOISE_TAGS: &[&str] = &[
    "ide_opened_file",
    "ide_selection",
    "system-reminder",
    "local-command-caveat",
    "local-command-stdout",
    "command-name",
    "command-message",
    "command-args",
    "command-stdout",
    "command-contents",
];

/// Remove every [`NOISE_TAGS`] block from `text`, keeping whatever surrounds it.
///
/// Deliberately strips rather than skipping the whole record: the real prompt is
/// frequently a *suffix* of a record that opens with `<ide_opened_file>…`, and
/// dropping the record would lose it.
pub(crate) fn strip_noise(text: &str) -> String {
    let mut out = text.to_owned();
    for tag in NOISE_TAGS {
        let open = format!("<{tag}");
        let close = format!("</{tag}>");
        while let Some(start) = out.find(&open) {
            // Skip the opening tag itself, attributes and all.
            let Some(gt) = out[start..].find('>') else {
                out.replace_range(start.., "");
                break;
            };
            let body = start + gt + 1;
            match out[body..].find(&close) {
                Some(rel) => {
                    let end = body + rel + close.len();
                    out.replace_range(start..end, "");
                }
                // An unclosed block runs to the end of the message.
                None => {
                    out.replace_range(start.., "");
                    break;
                }
            }
        }
    }
    out.trim().to_owned()
}

/// Flatten to one line and cap the length, for a row label.
pub(crate) fn one_line(text: &str, max_chars: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max_chars {
        return flat;
    }
    let cut: String = flat.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

/// The user's own words in a message, or `None` when the message carries only
/// tool results (or only noise).
fn user_text(msg: &ApiMessage) -> Option<String> {
    for block in msg.content.blocks() {
        // `blocks()` promotes a bare-string `content` to one text block, which
        // is the shape on-disk `user` records actually use.
        if let ContentBlock::Text { text } = block {
            let cleaned = strip_noise(&text);
            if !cleaned.is_empty() {
                return Some(cleaned);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Reading one transcript
// ---------------------------------------------------------------------------

/// Stop reading the head after this many lines, or this many bytes, whichever
/// comes first. Both the `cwd` and the first prompt sit within a handful of
/// records; a session that somehow opens with more noise than this gets no
/// label rather than a slow list.
const HEAD_LINE_BUDGET: usize = 200;
const HEAD_BYTE_BUDGET: usize = 1024 * 1024;

/// How far back from the end to look for the last `ai-title` record.
const TAIL_WINDOW: u64 = 256 * 1024;

/// The working directory and first user prompt, read from the start of the file.
fn read_head(path: &Path) -> (Option<PathBuf>, Option<String>) {
    let Ok(file) = File::open(path) else {
        return (None, None);
    };
    let mut cwd = None;
    let mut prompt: Option<String> = None;
    let mut bytes = 0usize;
    for (n, line) in BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .enumerate()
    {
        bytes += line.len();
        if n >= HEAD_LINE_BUDGET || bytes >= HEAD_BYTE_BUDGET {
            break;
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(rec) = serde_json::from_str::<DiskRecord>(trimmed) else {
            continue;
        };
        if cwd.is_none()
            && let Some(c) = rec.cwd.as_deref().filter(|c| !c.is_empty())
        {
            cwd = Some(PathBuf::from(c));
        }
        if prompt.is_none()
            && rec.kind == "user"
            && !rec.is_meta
            && let Some(msg) = &rec.message
        {
            prompt = user_text(msg);
        }
        if cwd.is_some() && prompt.is_some() {
            break;
        }
    }
    (cwd, prompt)
}

/// Claude's generated title, read from a window at the end of the file.
///
/// `ai-title` records are appended repeatedly as the title is refined and the
/// last one wins. Nothing in the window means the session has no title: there is
/// deliberately no full-file fallback, or a multi-megabyte transcript would cost
/// a complete scan every time the cache misses.
fn read_title(path: &Path, len: u64) -> Option<String> {
    let mut file = File::open(path).ok()?;
    let start = len.saturating_sub(TAIL_WINDOW);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).ok()?;
    let text = String::from_utf8_lossy(&buf);
    let mut lines: Vec<&str> = text.lines().collect();
    // Seeking to a byte offset almost certainly lands mid-record.
    if start > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    for line in lines.iter().rev() {
        if !line.contains(r#""type":"ai-title""#) {
            continue;
        }
        let Ok(rec) = serde_json::from_str::<DiskRecord>(line.trim()) else {
            continue;
        };
        if let Some(t) = rec.ai_title {
            let t = t.trim();
            if !t.is_empty() {
                return Some(t.to_owned());
            }
        }
    }
    None
}

/// Read one transcript's metadata. Callers go through [`SessionIndex`].
fn read_meta(path: &Path, modified: SystemTime, len: u64) -> Option<SessionMeta> {
    let id = path.file_stem()?.to_str()?.to_owned();
    let (cwd, first_prompt) = read_head(path);
    Some(SessionMeta {
        id,
        path: path.to_owned(),
        cwd: cwd?,
        title: read_title(path, len),
        first_prompt,
        modified,
    })
}

// ---------------------------------------------------------------------------
// The cache and the scan
// ---------------------------------------------------------------------------

/// Per-file metadata cache, keyed on `(mtime, len)`.
///
/// Appending a turn changes both, and the title can only change by appending, so
/// the pair is a complete validity test and a rebuilt list costs one `stat` per
/// transcript.
#[derive(Debug, Default)]
pub(crate) struct SessionIndex {
    entries: HashMap<PathBuf, (SystemTime, u64, SessionMeta)>,
}

impl SessionIndex {
    fn get(&mut self, path: &Path, modified: SystemTime, len: u64) -> Option<SessionMeta> {
        if let Some((m, l, meta)) = self.entries.get(path)
            && *m == modified
            && *l == len
        {
            return Some(meta.clone());
        }
        let meta = read_meta(path, modified, len)?;
        self.entries
            .insert(path.to_owned(), (modified, len, meta.clone()));
        Some(meta)
    }

    /// Forget one file, so a deleted transcript cannot linger in a later list.
    pub(crate) fn forget(&mut self, path: &Path) {
        self.entries.remove(path);
    }
}

/// Sessions run in `folder` or any folder below it, most recent first.
///
/// `root` is the projects directory; [`scan`] is the wrapper that resolves it.
/// Taking it explicitly is what lets the tests work against a `TempDir` without
/// touching the thread-local at all.
pub(crate) fn scan_in(root: &Path, folder: &Path, index: &mut SessionIndex) -> Vec<SessionMeta> {
    let want = dashify(folder);
    let mut out = Vec::new();
    let Ok(dirs) = std::fs::read_dir(root) else {
        return out;
    };
    for dir in dirs.flatten() {
        let name = dir.file_name().to_string_lossy().into_owned();
        // Prefix-preserving, so this filter has no false negatives. Its false
        // positives (`/a/b-c` matching a scan of `/a/b`) are settled below by
        // the `cwd` recorded in the file itself.
        if name != want && !name.starts_with(&format!("{want}-")) {
            continue;
        }
        let Ok(files) = std::fs::read_dir(dir.path()) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(meta) = file.metadata() else { continue };
            let Ok(modified) = meta.modified() else {
                continue;
            };
            let Some(session) = index.get(&path, modified, meta.len()) else {
                continue;
            };
            if session.cwd != folder && !session.cwd.starts_with(folder) {
                continue;
            }
            out.push(session);
        }
    }
    out.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.id.cmp(&b.id)));
    out
}

/// [`scan_in`] against the real projects root. Empty when there is none.
pub(crate) fn scan(folder: &Path, index: &mut SessionIndex) -> Vec<SessionMeta> {
    match projects_root() {
        Some(root) => scan_in(&root, folder, index),
        None => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Replay and delete
// ---------------------------------------------------------------------------

/// Cap on turns rendered from a past session, so a multi-megabyte transcript
/// cannot stall a frame. The *newest* turns are kept.
pub(crate) const REPLAY_TURN_CAP: usize = 2000;

/// Fold a transcript into a [`Conversation`], ready for `render::build`.
///
/// User turns are folded **here**, not through `Conversation::apply`. The live
/// stream echoes our own input back as a `user` event, so `apply` drops user
/// prose on purpose and `commit_edit` calls `push_user` instead. On a disk
/// replay nothing calls `push_user`, so leaving it to `apply` would render only
/// Claude's half of the conversation.
pub(crate) fn replay(path: &Path) -> Conversation {
    let mut convo = Conversation::default();
    let Ok(file) = File::open(path) else {
        return convo;
    };
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // A `user` record with prose is the real prompt. One without carries
        // tool results, and falls through to `apply`, which nests them under the
        // call that made them.
        if let Ok(rec) = serde_json::from_str::<DiskRecord>(trimmed)
            && rec.kind == "user"
            && !rec.is_meta
            && let Some(text) = rec.message.as_ref().and_then(user_text)
        {
            convo.push_user(&text);
            continue;
        }
        if let Some(ev) = crate::events::parse_line(trimmed) {
            convo.apply(ev);
        }
    }
    if convo.turns.len() > REPLAY_TURN_CAP {
        let drop = convo.turns.len() - REPLAY_TURN_CAP;
        convo.turns.drain(..drop);
        convo.truncated = true;
    }
    // The file is a finished transcript, not a session mid-answer: `push_user`
    // set `busy` on every prompt it folded.
    convo.busy = false;
    convo.partial.clear();
    convo
}

/// Permanently remove one transcript.
///
/// A real unlink, deliberately not `sicompass_sdk::fs_trash`: the confirmation
/// this goes through says "permanently", and routing it to the OS trash would
/// quietly make it recoverable and undoable when it promised not to be.
///
/// Only the transcript goes. Claude Code's `memory/` and `todos/` directories
/// and `~/.claude/history.jsonl` are left alone.
pub(crate) fn delete_transcript(path: &Path) -> bool {
    let Some(root) = projects_root() else {
        // No root means no ambient access (a test that forgot to inject one, or
        // no home directory). Refusing here is what keeps `cargo test` away
        // from the developer's own transcripts.
        return false;
    };
    if !path.starts_with(&root) {
        debug_assert!(false, "refusing to delete {path:?} outside {root:?}");
        return false;
    }
    std::fs::remove_file(path).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// A `user` record carrying one text block.
    fn user_line(cwd: &str, text: &str) -> String {
        format!(
            r#"{{"type":"user","cwd":"{cwd}","message":{{"role":"user","content":[{{"type":"text","text":{}}}]}}}}"#,
            serde_json::to_string(text).unwrap()
        )
    }

    /// Write a transcript and return its path.
    fn write_session(root: &Path, dir: &str, id: &str, lines: &[String]) -> PathBuf {
        let d = root.join(dir);
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join(format!("{id}.jsonl"));
        std::fs::write(&p, format!("{}\n", lines.join("\n"))).unwrap();
        p
    }

    /// The ordinary case: one session, one prompt, one title.
    fn simple(
        root: &Path,
        dir: &str,
        id: &str,
        cwd: &str,
        prompt: &str,
        title: Option<&str>,
    ) -> PathBuf {
        let mut lines = vec![user_line(cwd, prompt)];
        if let Some(t) = title {
            lines.push(format!(
                r#"{{"type":"ai-title","aiTitle":{},"sessionId":"{id}"}}"#,
                serde_json::to_string(t).unwrap()
            ));
        }
        write_session(root, dir, id, &lines)
    }

    fn set_mtime(path: &Path, secs_ago: u64) {
        let f = std::fs::OpenOptions::new().write(true).open(path).unwrap();
        f.set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000 - secs_ago))
            .unwrap();
    }

    #[test]
    fn dashify_is_prefix_preserving() {
        // What makes the "folder and below" filter free of false negatives.
        let parent = dashify(Path::new("/t/p"));
        assert!(dashify(Path::new("/t/p/sub")).starts_with(&parent));
        assert!(dashify(Path::new("/t/p/a/b")).starts_with(&parent));
        assert_eq!(parent, "-t-p");
    }

    #[test]
    fn dashify_is_lossy_so_a_name_match_is_only_a_filter() {
        // Exactly why `scan_in` re-checks the `cwd` recorded in the file.
        assert_eq!(
            dashify(Path::new("/t/p-sub")),
            dashify(Path::new("/t/p/sub"))
        );
    }

    #[test]
    fn scan_lists_the_folder_and_its_descendants() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        simple(root, "-t-p", "aaa", "/t/p", "top level", None);
        simple(root, "-t-p-sub", "bbb", "/t/p/sub", "in a subfolder", None);
        let mut idx = SessionIndex::default();

        let here = scan_in(root, Path::new("/t/p"), &mut idx);
        let mut ids: Vec<&str> = here.iter().map(|s| s.id.as_str()).collect();
        ids.sort();
        assert_eq!(ids, vec!["aaa", "bbb"], "the folder and below");

        let deeper = scan_in(root, Path::new("/t/p/sub"), &mut idx);
        assert_eq!(
            deeper.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            vec!["bbb"],
            "scanning the subfolder sees only its own"
        );
    }

    #[test]
    fn scan_rejects_a_dashify_collision() {
        // `/t/p-sub` dashifies the same as `/t/p/sub`, so the directory name
        // matches a scan of `/t/p`. The recorded cwd is what rules it out.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        simple(
            root,
            "-t-p-sub",
            "ccc",
            "/t/p-sub",
            "a sibling folder",
            None,
        );
        let mut idx = SessionIndex::default();
        assert!(scan_in(root, Path::new("/t/p"), &mut idx).is_empty());
    }

    #[test]
    fn scan_skips_a_transcript_with_no_recorded_cwd() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_session(
            root,
            "-t-p",
            "ddd",
            &[r#"{"type":"queue-operation","operation":"enqueue"}"#.to_owned()],
        );
        let mut idx = SessionIndex::default();
        assert!(scan_in(root, Path::new("/t/p"), &mut idx).is_empty());
    }

    #[test]
    fn title_comes_from_the_last_ai_title() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let p = write_session(
            root,
            "-t-p",
            "eee",
            &[
                user_line("/t/p", "first prompt"),
                r#"{"type":"ai-title","aiTitle":"early guess","sessionId":"eee"}"#.to_owned(),
                r#"{"type":"ai-title","aiTitle":"better guess","sessionId":"eee"}"#.to_owned(),
                r#"{"type":"ai-title","aiTitle":"final title","sessionId":"eee"}"#.to_owned(),
            ],
        );
        let len = std::fs::metadata(&p).unwrap().len();
        assert_eq!(read_title(&p, len).as_deref(), Some("final title"));
    }

    #[test]
    fn title_is_found_in_a_long_tail() {
        // The real files put `ai-title` near the end, after megabytes of turns,
        // which is why this is a tail seek and not a scan.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let mut lines = vec![user_line("/t/p", "first prompt")];
        for i in 0..3000 {
            lines.push(format!(
                r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"padding line {i} with enough text to push the file past the tail window boundary"}}]}}}}"#
            ));
        }
        lines.push(
            r#"{"type":"ai-title","aiTitle":"found in the tail","sessionId":"fff"}"#.to_owned(),
        );
        let p = write_session(root, "-t-p", "fff", &lines);
        let len = std::fs::metadata(&p).unwrap().len();
        assert!(len > TAIL_WINDOW, "the test file must exceed the window");
        assert_eq!(read_title(&p, len).as_deref(), Some("found in the tail"));
    }

    #[test]
    fn title_falls_back_to_the_first_user_prompt() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        simple(root, "-t-p", "ggg", "/t/p", "are you ok?", None);
        let mut idx = SessionIndex::default();
        let got = scan_in(root, Path::new("/t/p"), &mut idx);
        assert_eq!(got[0].title, None);
        assert_eq!(got[0].label(), Some("are you ok?"));
    }

    #[test]
    fn the_title_wins_over_the_first_prompt() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        simple(
            root,
            "-t-p",
            "hhh",
            "/t/p",
            "a long rambling opener",
            Some("Tidy title"),
        );
        let mut idx = SessionIndex::default();
        let got = scan_in(root, Path::new("/t/p"), &mut idx);
        assert_eq!(got[0].label(), Some("Tidy title"));
    }

    #[test]
    fn first_prompt_skips_ide_and_command_noise() {
        // The four shapes that actually occur in this developer's transcripts.
        let real = "finish this, move claude to root";
        assert_eq!(
            strip_noise(&format!(
                "<ide_opened_file>The user opened /a/b in the IDE.</ide_opened_file>{real}"
            )),
            real,
            "the real prompt is a suffix of the same record, not a later one"
        );
        assert_eq!(
            strip_noise("<ide_selection>some code</ide_selection>"),
            "",
            "a record that is only noise yields nothing, so the scan moves on"
        );
        assert_eq!(
            strip_noise(&format!(
                "<command-name>/clear</command-name><command-args></command-args>{real}"
            )),
            real
        );
        assert_eq!(
            strip_noise("<system-reminder>be careful</system-reminder>"),
            ""
        );
    }

    #[test]
    fn first_prompt_moves_past_a_record_that_is_only_noise() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_session(
            root,
            "-t-p",
            "iii",
            &[
                user_line("/t/p", "<ide_selection>fn main() {}</ide_selection>"),
                user_line("/t/p", "what does this do?"),
            ],
        );
        let mut idx = SessionIndex::default();
        let got = scan_in(root, Path::new("/t/p"), &mut idx);
        assert_eq!(got[0].first_prompt.as_deref(), Some("what does this do?"));
    }

    #[test]
    fn a_meta_record_is_not_the_first_prompt() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_session(
            root,
            "-t-p",
            "jjj",
            &[
                r#"{"type":"user","isMeta":true,"cwd":"/t/p","message":{"role":"user","content":[{"type":"text","text":"Caveat: the messages below were generated by a slash command."}]}}"#
                    .to_owned(),
                user_line("/t/p", "the real question"),
            ],
        );
        let mut idx = SessionIndex::default();
        let got = scan_in(root, Path::new("/t/p"), &mut idx);
        assert_eq!(got[0].first_prompt.as_deref(), Some("the real question"));
    }

    #[test]
    fn first_prompt_handles_bare_string_content() {
        // On-disk `user` records really do use both shapes.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        write_session(
            root,
            "-t-p",
            "kkk",
            &[
                r#"{"type":"user","cwd":"/t/p","message":{"role":"user","content":"hi!"}}"#
                    .to_owned(),
            ],
        );
        let mut idx = SessionIndex::default();
        let got = scan_in(root, Path::new("/t/p"), &mut idx);
        assert_eq!(got[0].first_prompt.as_deref(), Some("hi!"));
    }

    #[test]
    fn ordering_is_most_recent_first() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let old = simple(root, "-t-p", "old", "/t/p", "older", None);
        let new = simple(root, "-t-p", "new", "/t/p", "newer", None);
        set_mtime(&old, 3600);
        set_mtime(&new, 60);
        let mut idx = SessionIndex::default();
        let got = scan_in(root, Path::new("/t/p"), &mut idx);
        assert_eq!(
            got.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            vec!["new", "old"]
        );
    }

    #[test]
    fn the_index_is_reused_until_mtime_and_len_change() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let p = simple(root, "-t-p", "lll", "/t/p", "first", Some("Original"));
        set_mtime(&p, 100);
        let mut idx = SessionIndex::default();
        assert_eq!(
            scan_in(root, Path::new("/t/p"), &mut idx)[0].label(),
            Some("Original")
        );

        // Rewrite the body to exactly the same length and restore the mtime:
        // the cache key is unchanged, so the stale title is served and no file
        // was re-read.
        let original = std::fs::read_to_string(&p).unwrap();
        let swapped = original.replace("Original", "Rewrites");
        assert_eq!(swapped.len(), original.len());
        std::fs::write(&p, &swapped).unwrap();
        set_mtime(&p, 100);
        assert_eq!(
            scan_in(root, Path::new("/t/p"), &mut idx)[0].label(),
            Some("Original"),
            "same (mtime, len) means the cached metadata is served"
        );

        // A real append moves both, so the entry is re-read.
        set_mtime(&p, 10);
        assert_eq!(
            scan_in(root, Path::new("/t/p"), &mut idx)[0].label(),
            Some("Rewrites")
        );
    }

    #[test]
    fn replay_keeps_user_turns() {
        // `Conversation::apply` drops user prose on purpose (the live stream
        // echoes our own input back). On a disk replay nothing calls
        // `push_user`, so without the fold in `replay` the user's half of the
        // conversation vanishes and only Claude's answers render.
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let p = write_session(
            root,
            "-t-p",
            "mmm",
            &[
                user_line("/t/p", "what is two plus two?"),
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Four."}]}}"#.to_owned(),
            ],
        );
        let convo = replay(&p);
        assert_eq!(convo.turns.len(), 2, "both halves, got {:?}", convo.turns);
        assert!(
            format!("{:?}", convo.turns[0]).contains("what is two plus two?"),
            "the user's own words survive the replay"
        );
        assert!(!convo.busy, "a finished transcript is not mid-answer");
    }

    #[test]
    fn replay_leaves_tool_results_to_apply() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let p = write_session(
            root,
            "-t-p",
            "nnn",
            &[
                user_line("/t/p", "read the file"),
                r#"{"type":"assistant","message":{"role":"assistant","content":[{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/a"}}]}}"#.to_owned(),
                r#"{"type":"user","cwd":"/t/p","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"t1","content":"hello"}]}}"#.to_owned(),
            ],
        );
        let convo = replay(&p);
        // Three turns: the prompt, the call, and the result. The result must
        // arrive as a `ToolResult` (which `render::build` nests under its call),
        // never folded into a second user turn by the `push_user` branch.
        let shapes: Vec<&str> = convo
            .turns
            .iter()
            .map(|t| match t {
                crate::render::Turn::User { .. } => "user",
                crate::render::Turn::Assistant { .. } => "assistant",
                crate::render::Turn::ToolResult { .. } => "tool-result",
            })
            .collect();
        assert_eq!(shapes, vec!["user", "assistant", "tool-result"]);
    }

    #[test]
    fn replay_caps_a_runaway_transcript_and_keeps_the_newest() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let mut lines = Vec::new();
        for i in 0..(REPLAY_TURN_CAP + 50) {
            lines.push(user_line("/t/p", &format!("prompt {i}")));
        }
        let p = write_session(root, "-t-p", "ooo", &lines);
        let convo = replay(&p);
        assert_eq!(convo.turns.len(), REPLAY_TURN_CAP);
        assert!(convo.truncated);
        let last = format!("{:?}", convo.turns.last().unwrap());
        assert!(
            last.contains(&format!("prompt {}", REPLAY_TURN_CAP + 49)),
            "the newest turns are the ones kept, got {last}"
        );
    }

    #[test]
    fn delete_refuses_without_an_injected_root() {
        // The guard that keeps `cargo test` away from the real transcripts: a
        // test that forgets to inject a root deletes nothing at all.
        let tmp = tempfile::tempdir().unwrap();
        let p = simple(tmp.path(), "-t-p", "ppp", "/t/p", "keep me", None);
        _set_test_projects_root(None);
        assert!(!delete_transcript(&p));
        assert!(p.exists(), "nothing was removed");
    }

    #[test]
    fn delete_removes_only_the_transcript() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let doomed = simple(root, "-t-p", "qqq", "/t/p", "delete me", None);
        let keeper = simple(root, "-t-p", "rrr", "/t/p", "keep me", None);
        let memory = root.join("-t-p").join("memory");
        std::fs::create_dir_all(&memory).unwrap();
        std::fs::write(memory.join("MEMORY.md"), "notes").unwrap();

        _set_test_projects_root(Some(root.to_owned()));
        assert!(delete_transcript(&doomed));
        _set_test_projects_root(None);

        assert!(!doomed.exists());
        assert!(keeper.exists(), "a sibling session is untouched");
        assert!(memory.join("MEMORY.md").exists(), "memory/ is untouched");
    }

    #[test]
    fn delete_refuses_a_path_outside_the_root() {
        let tmp = tempfile::tempdir().unwrap();
        let outside = tmp.path().join("elsewhere.jsonl");
        std::fs::write(&outside, "x").unwrap();
        _set_test_projects_root(Some(tmp.path().join("projects")));
        // `debug_assert!` fires in a debug test build, so only the release
        // behaviour (refuse, change nothing) is assertable here.
        if !cfg!(debug_assertions) {
            assert!(!delete_transcript(&outside));
        }
        _set_test_projects_root(None);
        assert!(outside.exists());
    }

    #[test]
    fn one_line_flattens_and_caps() {
        assert_eq!(one_line("a\n  b\tc", 40), "a b c");
        assert_eq!(one_line("abcdefghij", 5), "abcd…");
    }

    #[test]
    fn no_projects_root_means_an_empty_list_not_a_panic() {
        _set_test_projects_root(None);
        let mut idx = SessionIndex::default();
        assert!(scan(Path::new("/t/p"), &mut idx).is_empty());
    }
}
