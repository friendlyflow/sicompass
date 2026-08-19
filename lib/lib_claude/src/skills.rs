//! Discovery of the Claude Code skills a session can invoke.
//!
//! Five sources, in the order the palette shows them: the user's own
//! `~/.claude/skills/`, the session folder's `.claude/skills/`, the `skills/`
//! directories of *enabled* marketplace plugins, the skills Claude Code ships
//! with, and its own slash commands. The first three are directories holding a
//! `SKILL.md` whose YAML frontmatter carries a one-line description.
//!
//! The last two are not files at all, and they are reached differently from each
//! other. The **commands** are read straight out of the installed CLI, which
//! declares each one as a literal whose keys survive minification because they
//! are read at runtime — so that list follows whatever binary is on PATH and
//! updates itself with every release, descriptions included. See
//! [`extract_builtin_commands`]. The **skills** have no such structure: they are
//! bare minified assignments with no descriptions and nothing stable to anchor
//! on, so they are a snapshot kept here. See [`BUILTIN_SKILLS`].
//!
//! The parse is hand-rolled rather than pulling in a YAML crate, matching what
//! the rest of the workspace does for small fixed-shape files (see
//! `lib_settings/build.rs`, `tests/packaging.rs`, and the SDK's `.desktop`
//! reader). Only the frontmatter is ever read; the body is never touched.

use std::collections::HashSet;
use std::path::Path;

/// Cap on the bytes read from a `SKILL.md`. Beyond this the description is
/// dropped but the skill still lists: the name comes from the directory, so an
/// unreasonably large file costs the blurb, not the entry.
const MAX_SKILL_MD_BYTES: u64 = 64 * 1024;

/// Cap on the rendered description, in characters.
const MAX_DESC_CHARS: usize = 80;

/// The skills Claude Code ships with, as of 2.1.x.
///
/// Unlike its slash commands (see [`extract_builtin_commands`]) these cannot be
/// read out of the CLI: they appear in the bundle only as bare minified
/// assignments — `rPe="artifact-design",MMo="artifact-diagramming"` — with no
/// descriptions and no stable anchor to find them by. So they are a snapshot,
/// updated here when a Claude Code release adds or drops one. It was briefly a
/// setting instead; a list of names nobody would ever edit is not worth a row on
/// the settings screen.
const BUILTIN_SKILLS: &str = "artifact-capabilities, artifact-design, \
     artifact-diagramming, artifact-pr-review, claude-api, code-review, code-walkthrough, \
     commit, commit-push-pr, dataviz, fewer-permission-prompts, init, \
     keybindings-help, loop, pr, pr-explainer, prototype, run, schedule, security-review, \
     simplify, update-config, verify, whiteboard, workshop";

/// Claude Code's own slash commands, as of 2.1.x — a snapshot merged with what
/// is read from the installed binary, never a replacement for it.
///
/// [`extract_builtin_commands`] supplies most of this list with descriptions,
/// from whichever CLI is present. It does not find quite all of them, though:
/// `doctor` and a few others are declared in a shape the scan does not match, so
/// the snapshot fills those gaps. It also covers the whole list if a future
/// bundle stops matching entirely, which turns "the scan broke" into "the list
/// is as good as this release" rather than an empty section.
const FALLBACK_BUILTIN_COMMANDS: &str = "add-dir, advisor, auto-mode-setup, autocompact, \
     autofix-pr, branch, brief, btw, bug, cd, clear, color, compact, config, context, copy, \
     daemon, design, design-consent, design-login, design-revoke, diff, doctor, effort, \
     export, feedback, focus, fork, goal, heapdump, help, hooks, ide, import, insights, \
     install, install-github-app, install-slack-app, login, logout, loops, mcp, memory, \
     model, permissions, plan, powerup, privacy-settings, radio, recap, release-notes, \
     reload-plugins, reload-skills, remote-env, resume, rewind, schedule, scroll-speed, \
     session, setup-bedrock, setup-vertex, skill-doctor, skills, status, stickers, stop, \
     subtask, team-onboarding, teleport, terminal-setup, theme, todos, tui, upgrade, usage, \
     usage-credits, version, voice, web-setup";

/// Names that are declared like commands but are not invocable: internal agent
/// plumbing, a command whose own description says it was removed, one that was
/// renamed, and two screens the app shows itself in response to a condition.
const NOT_INVOCABLE: &[&str] = &[
    "__remote-workflow",
    "workflow-launch-exec",
    "agents",
    "extra-usage",
    "pro-trial-expired",
    "rate-limit-options",
];

#[derive(Debug, Clone)]
pub(crate) struct Skill {
    /// Directory name, which is what a `/name` slash command dispatches on.
    ///
    /// Deliberately not the frontmatter's `name:` field: the directory name
    /// always exists, whereas a `SKILL.md` may have no frontmatter at all.
    pub name: String,
    /// `description:` from the frontmatter, sanitized. Empty when absent,
    /// unreadable, or oversized.
    pub description: String,
}

impl Skill {
    /// The palette row.
    ///
    /// Name first so a prefix search finds it, description after so the app's
    /// fuzzy filter — which matches on this whole string — also finds a skill by
    /// what it does rather than only by what it is called.
    pub fn label(&self) -> String {
        if self.description.is_empty() {
            self.name.clone()
        } else {
            format!("{} - {}", self.name, self.description)
        }
    }
}

// ---------------------------------------------------------------------------
// Test stub: keep discovery away from anything the test did not create.
//
// `discover` reads the real home directory *and* the installed `claude` binary,
// so any test going through the provider picks up whatever skills and commands
// the machine happens to have — the counts differ per machine, and a test
// written against a tempdir silently passes or fails on someone else's setup.
//
// Two audiences, hence both a compile-time default and a runtime setter, the
// same shape the terminal uses for its recall history:
//
// * This crate's own unit tests get it from `cfg!(test)`. Tests that care about
//   ordering call `discover_in` directly with tempdirs, and the extractor tests
//   point at a fabricated bundle, so both are unaffected.
//
// The shipped built-ins are covered too. They are not on disk, but they are just
// as much "whatever this Claude Code has" as the rest, and leaving them on would
// pad every palette assertion with rows the test did not create.
// * The app's integration tests are a different binary, where this crate is
//   compiled *without* `cfg(test)` and the provider is reached as a
//   `Box<dyn Provider>`. They call `_set_test_no_ambient_skills(true)` once.
// ---------------------------------------------------------------------------

static TEST_NO_AMBIENT: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(cfg!(test));

#[doc(hidden)]
pub fn _set_test_no_ambient_skills(enabled: bool) {
    TEST_NO_AMBIENT.store(enabled, std::sync::atomic::Ordering::Release);
}

#[inline]
fn no_ambient_skills() -> bool {
    TEST_NO_AMBIENT.load(std::sync::atomic::Ordering::Acquire)
}

/// Skills visible to a session running in `session_path`.
///
/// Five sources, in the order the palette shows them:
///
/// 1. `~/.claude/skills/` — personal, available in every project.
/// 2. `<session folder>/.claude/skills/` — this project's own.
/// 3. Enabled marketplace plugins, under `~/.claude/plugins/marketplaces/`.
/// 4. [`BUILTIN_SKILLS`] — the skills Claude Code ships with.
/// 5. Claude Code's own slash commands, read from the `program` binary.
///
/// Resolves the roots from the home directory; see [`discover_in`] for the
/// injectable form the tests use.
pub(crate) fn discover(session_path: &str, program: &str) -> Vec<Skill> {
    let project = Path::new(session_path).join(".claude").join("skills");
    let home = if no_ambient_skills() {
        None
    } else {
        sicompass_sdk::platform::home_dir()
    };
    let builtins = if no_ambient_skills() {
        ""
    } else {
        BUILTIN_SKILLS
    };
    // No home directory is not an error worth surfacing — the project skills are
    // still usable on their own, and an unreadable root is simply "no skills
    // here", so an empty path costs nothing.
    let claude_home = home.map(|h| h.join(".claude")).unwrap_or_default();
    let mut out = discover_in(
        &claude_home.join("skills"),
        &project,
        &claude_home,
        builtins,
    );
    // Claude Code's own slash commands, read from the installed CLI so the list
    // follows whatever is actually there. Last, because a skill of the same name
    // is the more specific thing.
    let mut seen: HashSet<String> = out.iter().map(|s| s.name.clone()).collect();
    let commands = if no_ambient_skills() {
        Vec::new()
    } else {
        builtin_commands(program)
    };
    for cmd in commands {
        if seen.insert(cmd.name.clone()) {
            out.push(cmd);
        }
    }
    out
}

/// The four sources in order, deduped by name.
///
/// **First occurrence wins**, so a personal skill shadows a project one, and
/// both shadow a plugin or built-in of the same name. That is the only rule
/// consistent with the listing order: if a later source won, the surviving row
/// would sit in an earlier source's block while describing something else, and a
/// list read top to bottom would be lying.
///
/// Caveat worth knowing: Claude Code itself resolves a colliding `/name` to the
/// **project** skill. Since the palette only ever inserts `/name`, the
/// invocation is identical either way — only the description shown differs.
pub(crate) fn discover_in(
    personal: &Path,
    project: &Path,
    claude_home: &Path,
    builtins: &str,
) -> Vec<Skill> {
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    collect(personal, &mut out, &mut seen);
    collect(project, &mut out, &mut seen);
    for dir in enabled_plugin_skill_dirs(claude_home) {
        collect(&dir, &mut out, &mut seen);
    }
    collect_builtins(builtins, &mut out, &mut seen);
    out
}

/// `skills/` directories of the plugins the user has actually enabled.
///
/// The marketplace tree holds every plugin that has been *downloaded*, which is
/// not the same as available: a skill from a plugin that is not enabled would
/// insert a `/name` the CLI does not resolve. So enablement is read from
/// `~/.claude/settings.json` and anything absent from it is skipped.
///
/// Layout: `<claude_home>/plugins/marketplaces/<market>/plugins/<plugin>/skills/`.
fn enabled_plugin_skill_dirs(claude_home: &Path) -> Vec<std::path::PathBuf> {
    let enabled = enabled_plugins(claude_home);
    if enabled.is_empty() {
        return Vec::new();
    }
    let markets = claude_home.join("plugins").join("marketplaces");
    let Ok(read_dir) = std::fs::read_dir(&markets) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for market in read_dir.flatten() {
        let market_name = market.file_name().to_string_lossy().into_owned();
        let plugins = market.path().join("plugins");
        let Ok(inner) = std::fs::read_dir(&plugins) else {
            continue;
        };
        for plugin in inner.flatten() {
            let name = plugin.file_name().to_string_lossy().into_owned();
            // Enablement is spelled either bare or qualified by marketplace.
            if !enabled.contains(&name) && !enabled.contains(&format!("{name}@{market_name}")) {
                continue;
            }
            let dir = plugin.path().join("skills");
            if dir.is_dir() {
                out.push(dir);
            }
        }
    }
    out.sort();
    out
}

/// Plugin ids marked enabled in `settings.json`.
///
/// Accepts both shapes the key has been seen in — an object keyed by plugin id
/// with a truthy value, and a plain array of ids — because guessing wrong would
/// silently list nothing, which looks identical to "no plugins installed".
fn enabled_plugins(claude_home: &Path) -> HashSet<String> {
    let mut out = HashSet::new();
    let Ok(text) = std::fs::read_to_string(claude_home.join("settings.json")) else {
        return out;
    };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
        return out;
    };
    match json.get("enabledPlugins") {
        Some(serde_json::Value::Object(map)) => {
            for (k, v) in map {
                let on = match v {
                    serde_json::Value::Bool(b) => *b,
                    serde_json::Value::Null => false,
                    _ => true,
                };
                if on {
                    out.insert(k.clone());
                }
            }
        }
        Some(serde_json::Value::Array(items)) => {
            for i in items {
                if let Some(s) = i.as_str() {
                    out.insert(s.to_owned());
                }
            }
        }
        _ => {}
    }
    out
}

/// Read Claude Code's own slash commands out of the installed CLI.
///
/// The bundle declares each one as a literal whose *keys* survive minification,
/// because they are read at runtime:
///
/// ```text
/// {type:"prompt",name:"insights",description:"Generate a report ...",...}
/// ```
///
/// So the list follows whichever CLI is installed and updates itself with every
/// release — including the descriptions, which makes them searchable in the
/// palette. Scanning the whole bundle costs a few hundred milliseconds even at
/// ~280 MB, and the result is cached per binary (see [`builtin_commands`]).
///
/// Reading an undocumented internal structure is a deliberate trade: it is the
/// only place this information exists, and a bundle that stops matching yields
/// an empty vector, which the caller answers with [`FALLBACK_BUILTIN_COMMANDS`].
/// The failure mode is "as good as the last snapshot", never "broken".
fn extract_builtin_commands(bundle: &Path) -> Vec<Skill> {
    use std::io::Read as _;

    let Ok(mut file) = std::fs::File::open(bundle) else {
        return Vec::new();
    };
    let mut out: Vec<Skill> = Vec::new();
    let mut seen = HashSet::new();
    // Streamed rather than read whole: the bundle is hundreds of megabytes.
    // The overlap carries any declaration straddling a chunk boundary.
    const CHUNK: usize = 4 << 20;
    const OVERLAP: usize = 4096;
    let mut buf = vec![0u8; CHUNK];
    let mut carry: Vec<u8> = Vec::new();
    while let Ok(n) = file.read(&mut buf) {
        if n == 0 {
            break;
        }
        carry.extend_from_slice(&buf[..n]);
        let text = String::from_utf8_lossy(&carry);
        scan_command_literals(&text, &mut out, &mut seen);
        let keep = carry.len().saturating_sub(OVERLAP);
        carry.drain(..keep);
    }
    out.sort_by(|a, b| natord::compare_ignore_case(&a.name, &b.name));
    out
}

/// Pull every `type:"…",name:"…",description:"…"` triple out of one window.
fn scan_command_literals(text: &str, out: &mut Vec<Skill>, seen: &mut HashSet<String>) {
    const NAME: &str = ",name:\"";
    const DESC: &str = ",description:\"";
    let mut rest = text;
    while let Some(at) = rest.find("type:\"") {
        rest = &rest[at + 6..];
        // type, then the two keys that must follow it in this exact order —
        // anything else is a different literal that merely has a `type` field.
        let Some((_ty, after)) = rest.split_once('"') else {
            break;
        };
        let Some(after) = after.strip_prefix(NAME) else {
            continue;
        };
        let Some((name, after)) = after.split_once('"') else {
            continue;
        };
        let Some(after) = after.strip_prefix(DESC) else {
            continue;
        };
        let Some((desc, _)) = after.split_once('"') else {
            continue;
        };
        if name.is_empty()
            || name.starts_with('_')
            || NOT_INVOCABLE.contains(&name)
            || !name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            continue;
        }
        // A description marking the command as gone is the CLI telling us not to
        // offer it; `agents` and `extra-usage` say exactly this today.
        if desc.starts_with("(removed)") || desc.starts_with("Renamed to") {
            continue;
        }
        if seen.insert(name.to_owned()) {
            out.push(Skill {
                name: name.to_owned(),
                description: sanitize(desc),
            });
        }
    }
}

/// Locate the bundle behind whatever `claude` the provider is configured to run.
///
/// `program` is whatever the user put in the setting: a bare name to find on
/// PATH, or a path. Symlinks are followed, and a small resolved file is treated
/// as a launcher — Nix, for one, installs a wrapper script beside the real
/// bundle as `.<name>-wrapped`.
fn resolve_bundle(program: &str) -> Option<std::path::PathBuf> {
    let direct = if program.contains(std::path::MAIN_SEPARATOR) || program.contains('/') {
        std::path::PathBuf::from(program)
    } else {
        which_on_path(program)?
    };
    let real = std::fs::canonicalize(&direct).ok()?;
    let len = std::fs::metadata(&real).ok()?.len();
    // A real bundle is tens of megabytes; anything small is a launcher.
    const LAUNCHER_MAX: u64 = 4 << 20;
    if len > LAUNCHER_MAX {
        return Some(real);
    }
    let name = real.file_name()?.to_string_lossy().into_owned();
    let wrapped = real.with_file_name(format!(".{name}-wrapped"));
    match std::fs::metadata(&wrapped) {
        Ok(m) if m.len() > LAUNCHER_MAX => Some(wrapped),
        _ => Some(real),
    }
}

fn which_on_path(program: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(program))
        .find(|candidate| candidate.is_file())
}

/// Identifies one build of the CLI: where it is, how big, and when it changed.
type BundleKey = (std::path::PathBuf, u64, Option<std::time::SystemTime>);

/// The extracted command list for one build, or nothing scanned yet.
type BundleCache = std::sync::Mutex<Option<(BundleKey, Vec<Skill>)>>;

/// Claude Code's own slash commands, read from the installed CLI and cached.
///
/// Keyed on the bundle's path, size and modification time, so an update to the
/// CLI is picked up on the next palette open and nothing else re-scans.
fn builtin_commands(program: &str) -> Vec<Skill> {
    use std::sync::Mutex;
    use std::sync::OnceLock;

    static CACHE: OnceLock<BundleCache> = OnceLock::new();

    let Some(bundle) = resolve_bundle(program) else {
        return parse_name_list(FALLBACK_BUILTIN_COMMANDS);
    };
    let key: BundleKey = match std::fs::metadata(&bundle) {
        Ok(m) => (bundle.clone(), m.len(), m.modified().ok()),
        Err(_) => return parse_name_list(FALLBACK_BUILTIN_COMMANDS),
    };

    let cache = CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(guard) = cache.lock()
        && let Some((cached_key, cached)) = guard.as_ref()
        && *cached_key == key
    {
        return cached.clone();
    }

    // Merged, not "extraction *or* fallback": the scan finds most commands but
    // not every one — `doctor`, for instance, is declared in a shape this does
    // not match — and a name the scan misses is still a real command. So the
    // snapshot fills the gaps, and covers the whole list if a future bundle
    // stops matching entirely. Extracted entries win, since they carry the
    // description.
    let mut found = extract_builtin_commands(&bundle);
    let mut seen: HashSet<String> = found.iter().map(|s| s.name.clone()).collect();
    for known in parse_name_list(FALLBACK_BUILTIN_COMMANDS) {
        if seen.insert(known.name.clone()) {
            found.push(known);
        }
    }
    found.sort_by(|a, b| natord::compare_ignore_case(&a.name, &b.name));
    if let Ok(mut guard) = cache.lock() {
        *guard = Some((key, found.clone()));
    }
    found
}

/// The skills Claude Code ships with, from a comma-separated list.
///
/// These are the one source that cannot be discovered. They are not files: the
/// names live inside the CLI's own bundle, under minified identifiers that
/// change with every build, so there is nothing stable to read and no CLI
/// command that enumerates them. Hence the snapshot in [`BUILTIN_SKILLS`].
///
/// No description is available for the same reason, so these rows show the bare
/// name.
fn collect_builtins(list: &str, out: &mut Vec<Skill>, seen: &mut HashSet<String>) {
    for skill in parse_name_list(list) {
        if seen.insert(skill.name.clone()) {
            out.push(skill);
        }
    }
}

/// A comma-separated list of names as description-less skills, sorted.
fn parse_name_list(list: &str) -> Vec<Skill> {
    let mut names: Vec<String> = list
        .split(',')
        .map(|n| n.trim().trim_start_matches('/').to_owned())
        .filter(|n| !n.is_empty() && !NOT_INVOCABLE.contains(&n.as_str()))
        .collect();
    names.sort_by(|a, b| natord::compare_ignore_case(a, b));
    names.dedup();
    names
        .into_iter()
        .map(|name| Skill {
            name,
            description: String::new(),
        })
        .collect()
}

/// Append one root's skills, sorted among themselves.
///
/// Sorting happens per group and never across the combined vector: the whole
/// point of the ordering is that personal skills come first as a block.
fn collect(dir: &Path, out: &mut Vec<Skill>, seen: &mut HashSet<String>) {
    // An absent or unreadable directory is simply "no skills here" — the same
    // shape the browse listing uses for a folder it cannot read.
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return;
    };
    let mut found: Vec<Skill> = Vec::new();
    for entry in read_dir.flatten() {
        // `metadata()` follows symlinks, so a symlinked skill directory counts —
        // which is what the CLI itself sees.
        if !entry.metadata().map(|m| m.is_dir()).unwrap_or(false) {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let md = entry.path().join("SKILL.md");
        // No SKILL.md means the directory is not a skill, whatever else it holds.
        let Ok(meta) = std::fs::metadata(&md) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let description = if meta.len() > MAX_SKILL_MD_BYTES {
            String::new()
        } else {
            std::fs::read_to_string(&md)
                .ok()
                .map(|s| parse_description(&s))
                .unwrap_or_default()
        };
        found.push(Skill { name, description });
    }
    found.sort_by(|a, b| natord::compare_ignore_case(&a.name, &b.name));
    for skill in found {
        if seen.insert(skill.name.clone()) {
            out.push(skill);
        }
    }
}

/// Pull `description:` out of a `SKILL.md` frontmatter block.
///
/// Tolerant by design: no fence, no `description`, an unterminated block, or a
/// file that is not frontmatter at all all yield an empty description rather
/// than dropping the skill.
///
/// A value split across following indented lines (a YAML folded scalar) is
/// joined back together, which is what makes the whitespace flattening in
/// [`sanitize`] worth doing.
///
/// Keys are matched after trimming, so a `description:` nested under another
/// mapping key would be picked up. Tracking indentation properly is the
/// beginning of writing a YAML parser, and a flat frontmatter block is the
/// real-world case.
fn parse_description(src: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let mut i = match lines.iter().position(|l| !l.trim().is_empty()) {
        Some(i) => i,
        None => return String::new(),
    };
    if lines[i].trim_start_matches('\u{feff}').trim() != "---" {
        return String::new();
    }
    i += 1;

    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed == "---" || trimmed == "..." {
            break;
        }
        // `split_once` takes the *first* colon, so a description containing one
        // keeps its tail intact.
        let Some((key, value)) = trimmed.split_once(':') else {
            i += 1;
            continue;
        };
        if !key.trim().eq_ignore_ascii_case("description") {
            i += 1;
            continue;
        }
        // Absorb continuation lines: indented, non-empty, not the closing
        // fence, and not themselves a `key:` pair at the top level.
        let mut value = value.trim().to_owned();
        i += 1;
        while i < lines.len() {
            let raw = lines[i];
            let trimmed = raw.trim();
            let is_continuation = !trimmed.is_empty()
                && trimmed != "---"
                && trimmed != "..."
                && raw.starts_with([' ', '\t']);
            if !is_continuation {
                break;
            }
            value.push(' ');
            value.push_str(trimmed);
            i += 1;
        }
        return sanitize(unquote(value.trim()));
    }
    String::new()
}

/// Strip one matching pair of surrounding quotes.
fn unquote(value: &str) -> &str {
    for q in ['"', '\''] {
        if value.len() >= 2 && value.starts_with(q) && value.ends_with(q) {
            return &value[1..value.len() - 1];
        }
    }
    value
}

/// Make a frontmatter value safe to use as a list label.
///
/// Not cosmetic. The app copies a command list item's label into the render list
/// verbatim, with none of the tag-stripping an ordinary FFON string gets, so a
/// description must not be able to carry markup or line breaks into the list.
fn sanitize(value: &str) -> String {
    let flattened: String = value
        .split_whitespace()
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    let stripped: String = flattened
        .chars()
        .filter(|c| *c != '<' && *c != '>')
        .collect();
    if stripped.chars().count() > MAX_DESC_CHARS {
        let head: String = stripped.chars().take(MAX_DESC_CHARS).collect();
        format!("{}…", head.trim_end())
    } else {
        stripped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write `<root>/<name>/SKILL.md` with the given contents.
    fn skill(root: &Path, name: &str, contents: &str) {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), contents).unwrap();
    }

    fn names(skills: &[Skill]) -> Vec<&str> {
        skills.iter().map(|s| s.name.as_str()).collect()
    }

    #[test]
    fn personal_skills_come_before_project_skills() {
        let home = tempfile::tempdir().unwrap();
        let proj = tempfile::tempdir().unwrap();
        skill(proj.path(), "review", "---\ndescription: review it\n---\n");
        skill(home.path(), "graphify", "---\ndescription: graph it\n---\n");

        let found = discover_in(home.path(), proj.path(), Path::new(""), "");
        assert_eq!(names(&found), vec!["graphify", "review"]);
        assert_eq!(found[0].description, "graph it", "the personal one leads");
        assert_eq!(found[1].description, "review it");
    }

    #[test]
    fn each_group_sorts_naturally() {
        // Natural, not lexicographic: item2 before item10.
        let home = tempfile::tempdir().unwrap();
        let proj = tempfile::tempdir().unwrap();
        for n in ["item10", "item2", "Item1"] {
            skill(proj.path(), n, "");
        }
        assert_eq!(
            names(&discover_in(home.path(), proj.path(), Path::new(""), "")),
            vec!["Item1", "item2", "item10"]
        );
    }

    #[test]
    fn a_name_in_both_places_keeps_the_personal_one() {
        let home = tempfile::tempdir().unwrap();
        let proj = tempfile::tempdir().unwrap();
        skill(home.path(), "sync", "---\ndescription: mine\n---\n");
        skill(proj.path(), "sync", "---\ndescription: theirs\n---\n");

        let found = discover_in(home.path(), proj.path(), Path::new(""), "");
        assert_eq!(found.len(), 1, "the shadowed one is not listed twice");
        assert_eq!(found[0].description, "mine", "the personal one survives");
    }

    #[test]
    fn description_comes_from_the_frontmatter() {
        let home = tempfile::tempdir().unwrap();
        let proj = tempfile::tempdir().unwrap();
        skill(
            proj.path(),
            "review",
            "---\nname: review\ndescription: Review the diff\n---\n\n# body\n",
        );
        assert_eq!(
            discover_in(home.path(), proj.path(), Path::new(""), "")[0].description,
            "Review the diff"
        );
    }

    #[test]
    fn a_quoted_description_is_unquoted() {
        let home = tempfile::tempdir().unwrap();
        let proj = tempfile::tempdir().unwrap();
        skill(proj.path(), "a", "---\ndescription: \"double\"\n---\n");
        skill(proj.path(), "b", "---\ndescription: 'single'\n---\n");
        let found = discover_in(home.path(), proj.path(), Path::new(""), "");
        assert_eq!(found[0].description, "double");
        assert_eq!(found[1].description, "single");
    }

    #[test]
    fn a_description_keeps_a_colon_in_its_tail() {
        let home = tempfile::tempdir().unwrap();
        let proj = tempfile::tempdir().unwrap();
        skill(
            proj.path(),
            "a",
            "---\ndescription: Use for: this and that\n---\n",
        );
        assert_eq!(
            discover_in(home.path(), proj.path(), Path::new(""), "")[0].description,
            "Use for: this and that"
        );
    }

    #[test]
    fn a_skill_without_frontmatter_still_lists() {
        // The name comes from the directory, so a plain markdown file is fine.
        let home = tempfile::tempdir().unwrap();
        let proj = tempfile::tempdir().unwrap();
        skill(proj.path(), "bare", "# just a heading\n\nsome prose\n");
        let found = discover_in(home.path(), proj.path(), Path::new(""), "");
        assert_eq!(names(&found), vec!["bare"]);
        assert!(found[0].description.is_empty());
    }

    #[test]
    fn an_unterminated_frontmatter_block_still_yields_its_description() {
        let home = tempfile::tempdir().unwrap();
        let proj = tempfile::tempdir().unwrap();
        skill(proj.path(), "a", "---\ndescription: no closing fence\n");
        assert_eq!(
            discover_in(home.path(), proj.path(), Path::new(""), "")[0].description,
            "no closing fence"
        );
    }

    #[test]
    fn frontmatter_without_a_description_yields_an_empty_one() {
        let home = tempfile::tempdir().unwrap();
        let proj = tempfile::tempdir().unwrap();
        skill(proj.path(), "a", "---\nname: a\nmodel: sonnet\n---\n");
        assert!(
            discover_in(home.path(), proj.path(), Path::new(""), "")[0]
                .description
                .is_empty()
        );
    }

    #[test]
    fn a_key_after_the_closing_fence_is_not_read() {
        let home = tempfile::tempdir().unwrap();
        let proj = tempfile::tempdir().unwrap();
        skill(
            proj.path(),
            "a",
            "---\nname: a\n---\ndescription: body text\n",
        );
        assert!(
            discover_in(home.path(), proj.path(), Path::new(""), "")[0]
                .description
                .is_empty()
        );
    }

    #[test]
    fn a_directory_without_a_skill_md_is_not_a_skill() {
        let home = tempfile::tempdir().unwrap();
        let proj = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(proj.path().join("notaskill")).unwrap();
        skill(proj.path(), "real", "");
        assert_eq!(
            names(&discover_in(home.path(), proj.path(), Path::new(""), "")),
            vec!["real"]
        );
    }

    #[test]
    fn dotted_directories_and_loose_files_are_skipped() {
        let home = tempfile::tempdir().unwrap();
        let proj = tempfile::tempdir().unwrap();
        skill(proj.path(), ".hidden", "");
        std::fs::write(proj.path().join("SKILL.md"), "").unwrap();
        skill(proj.path(), "real", "");
        assert_eq!(
            names(&discover_in(home.path(), proj.path(), Path::new(""), "")),
            vec!["real"]
        );
    }

    #[test]
    fn an_oversized_skill_md_keeps_the_name_and_drops_the_description() {
        let home = tempfile::tempdir().unwrap();
        let proj = tempfile::tempdir().unwrap();
        let mut huge = String::from("---\ndescription: never read\n---\n");
        huge.push_str(&"x".repeat(MAX_SKILL_MD_BYTES as usize + 1));
        skill(proj.path(), "big", &huge);

        let found = discover_in(home.path(), proj.path(), Path::new(""), "");
        assert_eq!(names(&found), vec!["big"], "still usable as /big");
        assert!(found[0].description.is_empty());
    }

    #[test]
    fn a_missing_directory_is_silently_empty() {
        let missing = Path::new("/definitely-not-a-directory-xyz-9000");
        assert!(discover_in(missing, missing, Path::new(""), "").is_empty());
    }

    #[test]
    fn a_description_is_flattened_stripped_and_truncated() {
        let home = tempfile::tempdir().unwrap();
        let proj = tempfile::tempdir().unwrap();
        // A folded YAML scalar leaks newlines; `<input>` would look like a tag
        // to anything reading the label downstream.
        let long = "a".repeat(200);
        skill(
            proj.path(),
            "a",
            &format!("---\ndescription: one\n  two <input> three {long}\n---\n"),
        );
        let d = &discover_in(home.path(), proj.path(), Path::new(""), "")[0].description;
        assert!(!d.contains('\n') && !d.contains("  "), "flattened: {d:?}");
        assert!(!d.contains('<') && !d.contains('>'), "tag-free: {d:?}");
        assert!(d.ends_with('…'), "truncated: {d:?}");
        assert!(d.chars().count() <= MAX_DESC_CHARS + 1);
    }

    #[test]
    fn no_unit_test_can_reach_the_real_personal_skills() {
        // The safety net for the `cfg!(test)` default. If this ever fails, some
        // test is reading the developer's own `~/.claude/skills` and its
        // assertions depend on whatever that machine happens to hold.
        assert!(
            no_ambient_skills(),
            "unit tests must not resolve the real home directory"
        );
        let session = tempfile::tempdir().unwrap();
        // `discover` joins `.claude/skills` onto the session folder itself.
        skill(&session.path().join(".claude").join("skills"), "only", "");
        assert_eq!(
            names(&discover(session.path().to_str().unwrap(), "claude")),
            vec!["only"],
            "discover() must see the project skills and nothing else"
        );
    }

    // ---- Built-ins and plugins --------------------------------------------

    /// `<claude_home>/plugins/marketplaces/<market>/plugins/<plugin>/skills/<name>/SKILL.md`
    fn plugin_skill(claude_home: &Path, market: &str, plugin: &str, name: &str, body: &str) {
        let dir = claude_home
            .join("plugins")
            .join("marketplaces")
            .join(market)
            .join("plugins")
            .join(plugin)
            .join("skills");
        skill(&dir, name, body);
    }

    fn write_settings(claude_home: &Path, json: &str) {
        std::fs::create_dir_all(claude_home).unwrap();
        std::fs::write(claude_home.join("settings.json"), json).unwrap();
    }

    #[test]
    fn builtins_come_last_and_are_sorted() {
        let home = tempfile::tempdir().unwrap();
        let proj = tempfile::tempdir().unwrap();
        skill(proj.path(), "zzz-project", "");
        let found = discover_in(
            home.path(),
            proj.path(),
            Path::new(""),
            "run, code-review, init",
        );
        assert_eq!(
            names(&found),
            vec!["zzz-project", "code-review", "init", "run"],
            "discovered skills lead, built-ins follow in their own sorted block",
        );
    }

    #[test]
    fn builtins_tolerate_slashes_blanks_and_stray_commas() {
        let home = tempfile::tempdir().unwrap();
        let proj = tempfile::tempdir().unwrap();
        let found = discover_in(home.path(), proj.path(), Path::new(""), " /run , ,, init ,");
        assert_eq!(names(&found), vec!["init", "run"]);
        assert!(found[0].description.is_empty(), "no blurb is available");
    }

    #[test]
    fn a_real_skill_shadows_a_builtin_of_the_same_name() {
        // The user's own `run` wins over the shipped one: it is the row they can
        // see, and its description is the one that is true.
        let home = tempfile::tempdir().unwrap();
        let proj = tempfile::tempdir().unwrap();
        skill(proj.path(), "run", "---\ndescription: my own\n---\n");
        let found = discover_in(home.path(), proj.path(), Path::new(""), "run, init");
        assert_eq!(names(&found), vec!["run", "init"]);
        assert_eq!(found[0].description, "my own");
    }

    #[test]
    fn a_downloaded_but_disabled_plugin_is_not_listed() {
        // Offering it would insert a `/name` the CLI does not resolve.
        let home = tempfile::tempdir().unwrap();
        let proj = tempfile::tempdir().unwrap();
        let ch = tempfile::tempdir().unwrap();
        plugin_skill(ch.path(), "official", "helper", "assist", "");
        // No `enabledPlugins` at all, which is how a fresh install looks.
        write_settings(ch.path(), r#"{"theme":"dark"}"#);
        assert!(discover_in(home.path(), proj.path(), ch.path(), "").is_empty());
    }

    #[test]
    fn an_enabled_plugins_skills_are_listed() {
        let home = tempfile::tempdir().unwrap();
        let proj = tempfile::tempdir().unwrap();
        let ch = tempfile::tempdir().unwrap();
        plugin_skill(
            ch.path(),
            "official",
            "helper",
            "assist",
            "---\ndescription: helps\n---\n",
        );
        plugin_skill(ch.path(), "official", "other", "unused", "");
        write_settings(ch.path(), r#"{"enabledPlugins":{"helper":true}}"#);

        let found = discover_in(home.path(), proj.path(), ch.path(), "");
        assert_eq!(names(&found), vec!["assist"], "only the enabled plugin's");
        assert_eq!(found[0].description, "helps");
    }

    #[test]
    fn enablement_is_accepted_qualified_by_marketplace_or_as_a_list() {
        // Both spellings occur; guessing one would silently list nothing, which
        // looks exactly like "no plugins installed".
        for settings in [
            r#"{"enabledPlugins":{"helper@official":true}}"#,
            r#"{"enabledPlugins":["helper"]}"#,
            r#"{"enabledPlugins":["helper@official"]}"#,
        ] {
            let home = tempfile::tempdir().unwrap();
            let proj = tempfile::tempdir().unwrap();
            let ch = tempfile::tempdir().unwrap();
            plugin_skill(ch.path(), "official", "helper", "assist", "");
            write_settings(ch.path(), settings);
            assert_eq!(
                names(&discover_in(home.path(), proj.path(), ch.path(), "")),
                vec!["assist"],
                "settings were {settings}"
            );
        }
    }

    #[test]
    fn a_plugin_switched_off_by_a_false_value_is_not_listed() {
        let home = tempfile::tempdir().unwrap();
        let proj = tempfile::tempdir().unwrap();
        let ch = tempfile::tempdir().unwrap();
        plugin_skill(ch.path(), "official", "helper", "assist", "");
        write_settings(ch.path(), r#"{"enabledPlugins":{"helper":false}}"#);
        assert!(discover_in(home.path(), proj.path(), ch.path(), "").is_empty());
    }

    #[test]
    fn a_missing_or_broken_settings_file_lists_no_plugins() {
        let home = tempfile::tempdir().unwrap();
        let proj = tempfile::tempdir().unwrap();
        let ch = tempfile::tempdir().unwrap();
        plugin_skill(ch.path(), "official", "helper", "assist", "");
        // No settings.json at all.
        assert!(discover_in(home.path(), proj.path(), ch.path(), "").is_empty());
        write_settings(ch.path(), "{not json");
        assert!(discover_in(home.path(), proj.path(), ch.path(), "").is_empty());
    }

    // ---- Reading the CLI's own commands out of its bundle ------------------

    fn fake_bundle(body: &str) -> tempfile::NamedTempFile {
        use std::io::Write as _;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(body.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn commands_are_read_out_of_the_bundle_with_their_descriptions() {
        let b = fake_bundle(
            r#"junk{type:"prompt",name:"insights",description:"Analyze your sessions",x:1}
               more{type:"local-jsx",name:"doctor",description:"Check the install",y:2}"#,
        );
        let found = extract_builtin_commands(b.path());
        assert_eq!(names(&found), vec!["doctor", "insights"], "sorted");
        assert_eq!(found[1].description, "Analyze your sessions");
    }

    #[test]
    fn a_command_the_cli_calls_removed_or_renamed_is_not_offered() {
        let b = fake_bundle(
            r#"{type:"local",name:"agents",description:"(removed) Ask Claude to ..."}
               {type:"local",name:"extra-usage",description:"Renamed to /usage-credits"}
               {type:"local",name:"help",description:"Show help"}"#,
        );
        assert_eq!(names(&extract_builtin_commands(b.path())), vec!["help"]);
    }

    #[test]
    fn internal_names_are_not_offered() {
        let b = fake_bundle(
            r#"{type:"agent",name:"__remote-workflow",description:"internal"}
               {type:"agent",name:"workflow-launch-exec",description:"internal"}
               {type:"local",name:"status",description:"Show status"}"#,
        );
        assert_eq!(names(&extract_builtin_commands(b.path())), vec!["status"]);
    }

    #[test]
    fn a_literal_that_merely_has_a_type_field_is_skipped() {
        // The bundle is full of other objects with a `type` key; only the exact
        // type/name/description run is a command.
        let b = fake_bundle(
            r#"{type:"number",value:3}{type:"user",message:{role:"user"}}
               {type:"local",name:"help",description:"Show help"}"#,
        );
        assert_eq!(names(&extract_builtin_commands(b.path())), vec!["help"]);
    }

    #[test]
    fn a_declaration_straddling_a_read_boundary_is_still_found() {
        // The scan is chunked, so a command landing across the seam must survive
        // via the overlap. 4 MiB of filler puts the second one past it.
        let mut body = String::from(r#"{type:"local",name:"first",description:"one"}"#);
        body.push_str(&"x".repeat(5 << 20));
        body.push_str(r#"{type:"local",name:"second",description:"two"}"#);
        let b = fake_bundle(&body);
        assert_eq!(
            names(&extract_builtin_commands(b.path())),
            vec!["first", "second"]
        );
    }

    #[test]
    fn a_bundle_with_nothing_recognisable_yields_nothing() {
        // Which is what makes the caller fall back rather than show an empty
        // section — see `builtin_commands`.
        let b = fake_bundle("a completely different bundle format");
        assert!(extract_builtin_commands(b.path()).is_empty());
    }

    #[test]
    fn a_missing_binary_yields_nothing() {
        assert!(extract_builtin_commands(Path::new("/definitely-not-here-xyz")).is_empty());
    }

    #[test]
    fn the_snapshot_fills_gaps_the_scan_leaves() {
        // The scan matches most command declarations but not every shape —
        // `doctor` is declared differently and is not found — so the two are
        // merged rather than one replacing the other, with the extracted entry
        // winning where both have the name because it carries the description.
        let b = fake_bundle(r#"{type:"local",name:"doctor",description:"From the bundle"}"#);
        let scanned = extract_builtin_commands(b.path());
        assert_eq!(scanned.len(), 1);

        let mut merged = scanned.clone();
        let mut seen: HashSet<String> = merged.iter().map(|s| s.name.clone()).collect();
        for known in parse_name_list(FALLBACK_BUILTIN_COMMANDS) {
            if seen.insert(known.name.clone()) {
                merged.push(known);
            }
        }
        let doctor = merged.iter().filter(|s| s.name == "doctor").count();
        assert_eq!(doctor, 1, "not listed twice");
        assert_eq!(
            merged
                .iter()
                .find(|s| s.name == "doctor")
                .unwrap()
                .description,
            "From the bundle",
            "the scanned entry wins, because it has the description"
        );
        assert!(
            merged.len() > 50,
            "and the snapshot still contributes the rest"
        );
    }

    #[test]
    fn the_builtin_skill_list_is_usable_on_its_own() {
        // The palette's fourth block. No scan feeds it, so this snapshot is all
        // there is: if it stops parsing, the block silently empties.
        let builtins = parse_name_list(BUILTIN_SKILLS);
        assert!(builtins.len() > 10, "got {}", builtins.len());
        let names: Vec<&str> = builtins.iter().map(|s| s.name.as_str()).collect();
        for expected in ["code-review", "init", "run", "security-review"] {
            assert!(names.contains(&expected), "missing {expected}");
        }
    }

    #[test]
    fn the_fallback_list_is_usable_on_its_own() {
        // The safety net for a future bundle the scan does not recognise.
        let fallback = parse_name_list(FALLBACK_BUILTIN_COMMANDS);
        assert!(fallback.len() > 50, "got {}", fallback.len());
        let names: Vec<&str> = fallback.iter().map(|s| s.name.as_str()).collect();
        for expected in ["doctor", "insights", "skills", "resume", "config"] {
            assert!(names.contains(&expected), "missing {expected}");
        }
        for internal in NOT_INVOCABLE {
            assert!(!names.contains(internal), "{internal} should be filtered");
        }
    }

    #[test]
    fn label_puts_the_name_first_and_omits_an_empty_description() {
        let with = Skill {
            name: "sync".to_owned(),
            description: "fast-forward main".to_owned(),
        };
        assert_eq!(with.label(), "sync - fast-forward main");
        let without = Skill {
            name: "sync".to_owned(),
            description: String::new(),
        };
        assert_eq!(without.label(), "sync");
    }
}
