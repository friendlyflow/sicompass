//! Right-panel list building — equivalent to `list.c`.
//!
//! Builds `AppRenderer::total_list` from the FFON tree at the current
//! navigation path, then optionally filters it by a search string.

use crate::app_state::{AppRenderer, CommandPhase, Coordinate, RenderListItem};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use sicompass_sdk::ffon::{get_ffon_at_id, FfonElement, FfonObject, IdArray};
use sicompass_sdk::tags;
use sicompass_sdk::timeline::{
    ChatOpKind, FsSideEffect, ImapOpKind, TimelineEntry,
};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Rebuild `total_list` for `Coordinate::ExtendedSearch`.
///
/// Recursively walks the in-memory FFON tree at `current_id`, collecting all
/// elements with breadcrumb paths. This is consistent across all providers.
pub fn create_list_extended_search(renderer: &mut AppRenderer) {
    renderer.total_list.clear();
    renderer.filtered_list_indices.clear();
    renderer.error_message.clear();

    // Recursively walk the in-memory FFON tree.
    let base_id = renderer.current_id.clone();
    let ffon = &renderer.ffon;
    let arr = match get_ffon_at_id(ffon, &base_id) {
        Some(a) => a,
        None => return,
    };

    let mut items: Vec<crate::app_state::RenderListItem> = Vec::new();
    collect_items_recursive(arr, &base_id, "", false, &mut items);
    renderer.total_list = items;
    renderer.list_index = renderer.list_index.min(renderer.total_list.len().saturating_sub(1));
}

/// Recursively collect all FFON elements with breadcrumb paths.
fn collect_items_recursive(
    arr: &[FfonElement],
    base_id: &sicompass_sdk::ffon::IdArray,
    breadcrumb: &str,
    parent_has_radio: bool,
    out: &mut Vec<crate::app_state::RenderListItem>,
) {
    for (i, elem) in arr.iter().enumerate() {
        let mut item_id = base_id.clone();
        item_id.set_last(i);

        let label = build_label_for_element(elem, parent_has_radio);
        out.push(crate::app_state::RenderListItem {
            id: item_id.clone(),
            label,
            data: if breadcrumb.is_empty() { None } else { Some(breadcrumb.to_owned()) },
            nav_path: None,
        });

        // Recurse into object children
        if let FfonElement::Obj(obj) = elem {
            if !obj.children.is_empty() {
                let display = sicompass_sdk::tags::strip_display(&obj.key);
                let new_bc = if breadcrumb.is_empty() {
                    format!("{} > ", display)
                } else {
                    format!("{}{} > ", breadcrumb, display)
                };
                let mut child_id = item_id.clone();
                child_id.push(0);
                let child_parent_has_radio = sicompass_sdk::tags::has_radio(&obj.key);
                collect_items_recursive(&obj.children, &child_id, &new_bc, child_parent_has_radio, out);
            }
        }
    }
}

/// Rebuild `total_list` from the FFON tree at `current_id`, and restore
/// `list_index` to the item matching `current_id.last()`.
pub fn create_list_current_layer(renderer: &mut AppRenderer) {
    renderer.total_list.clear();
    renderer.filtered_list_indices.clear();
    renderer.error_message.clear();

    match renderer.coordinate {
        Coordinate::General
        | Coordinate::Insert
        | Coordinate::SimpleSearch => {}
        Coordinate::ExtendedSearch => {
            create_list_extended_search(renderer);
            return;
        }
        Coordinate::Command => {
            build_command_list(renderer);
            return;
        }
        Coordinate::Meta => {
            build_meta_list(renderer);
            return;
        }
        Coordinate::TimelineView => {
            build_timeline_list(renderer);
            return;
        }
        _ => {
            renderer.list_index = 0;
            return;
        }
    }

    let ffon_slice = match get_ffon_at_id(&renderer.ffon, &renderer.current_id) {
        Some(s) => s,
        None => {
            renderer.list_index = 0;
            return;
        }
    };

    // Check if parent has <radio> tag (for -r prefix on string children)
    let parent_has_radio = check_parent_has_radio(renderer);

    let base_id = renderer.current_id.clone();

    let mut items: Vec<RenderListItem> = Vec::with_capacity(ffon_slice.len());

    let filter_json = renderer.pending_file_browser_open;

    for (i, elem) in ffon_slice.iter().enumerate() {
        // In the Ctrl+O open flow, hide non-.json files (directories still shown).
        if filter_json {
            if let FfonElement::Str(s) = elem {
                let name = tags::extract_input(s)
                    .unwrap_or_else(|| s.clone());
                if !name.ends_with(".json") {
                    continue;
                }
            }
        }

        let mut item_id = base_id.clone();
        item_id.set_last(i);

        let label = build_label_for_element(elem, parent_has_radio);

        let data = match elem {
            FfonElement::Str(s) if tags::has_image(s) => tags::extract_image(s),
            _ => None,
        };

        items.push(RenderListItem { id: item_id, label, data, nav_path: None });
    }

    // Restore list_index to the item matching current_id.last()
    let selected_raw = renderer.current_id.last().unwrap_or(0);
    let new_index = items
        .iter()
        .position(|item| item.id.last() == Some(selected_raw))
        .unwrap_or(0);

    renderer.total_list = items;
    renderer.list_index = new_index;

    // Re-apply any existing search filter
    let search = renderer.search_string.clone();
    if !search.is_empty() {
        populate_list_current_layer(renderer, &search);
    }
}

/// Filter `total_list` by `search_string` using fuzzy matching and store
/// matching indices (sorted by score) in `filtered_list_indices`.
/// Matched character positions are stored in `fuzzy_match_positions`.
/// Passing an empty string clears the filter.
///
/// Pattern syntax (fzf-compatible, via `nucleo`'s `Pattern::parse`):
///
/// | Token   | Meaning                                                |
/// |---------|--------------------------------------------------------|
/// | `^foo`  | anchored prefix — match must start with `foo`          |
/// | `foo$`  | anchored suffix — match must end with `foo`            |
/// | `'foo`  | exact substring (skip fuzzy scoring)                   |
/// | `!foo`  | negation — exclude items containing `foo`              |
/// | `a\|b`  | OR — match items containing `a` or `b`                 |
/// | `a b`   | AND — both `a` and `b` must match (terms separated)    |
/// | `\$`    | literal `$` (escape any operator with `\`)             |
///
/// As a result, characters `^ $ ' ! | \` and space are interpreted as
/// operators rather than literal text. To search for them literally, escape
/// with `\` (e.g. `\$` for a literal dollar sign).
pub fn populate_list_current_layer(renderer: &mut AppRenderer, search: &str) {
    renderer.filtered_list_indices.clear();
    renderer.fuzzy_match_positions.clear();

    if search.is_empty() {
        return;
    }

    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::parse(search, CaseMatching::Ignore, Normalization::Smart);

    let mut scored: Vec<(usize, u32, Vec<u32>)> = Vec::new();
    let mut char_buf: Vec<char> = Vec::new();
    let mut indices_buf: Vec<u32> = Vec::new();

    for (i, item) in renderer.total_list.iter().enumerate() {
        char_buf.clear();
        let haystack = Utf32Str::new(&item.label, &mut char_buf);
        indices_buf.clear();
        if let Some(score) = pattern.indices(haystack, &mut matcher, &mut indices_buf) {
            indices_buf.sort_unstable();
            scored.push((i, score, indices_buf.clone()));
        }
    }

    // Sort by score descending; preserve original order for equal scores
    scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

    renderer.filtered_list_indices = scored.iter().map(|(i, _, _)| *i).collect();
    renderer.fuzzy_match_positions = scored.into_iter().map(|(_, _, pos)| pos).collect();

    // Clamp list_index into the filtered range
    let active_len = renderer.filtered_list_indices.len();
    if renderer.list_index >= active_len {
        renderer.list_index = active_len.saturating_sub(1);
    }
}

// ---------------------------------------------------------------------------
// Label building
// ---------------------------------------------------------------------------

fn build_label_for_element(elem: &FfonElement, parent_has_radio: bool) -> String {
    match elem {
        FfonElement::Str(s) => build_str_label(s, parent_has_radio),
        FfonElement::Obj(obj) => build_obj_label(obj),
    }
}

fn build_str_label(s: &str, parent_has_radio: bool) -> String {
    // Strip <one-opt> / <many-opt> first
    let stripped_opt;
    let s: &str = if tags::has_one_opt(s) {
        stripped_opt = tags::strip_one_opt(s).to_owned();
        &stripped_opt
    } else if tags::has_many_opt(s) {
        stripped_opt = tags::strip_many_opt(s).to_owned();
        &stripped_opt
    } else {
        stripped_opt = String::new();
        s
    };
    let _ = stripped_opt; // suppress unused warning

    // Editor file-content placeholder (`"ci <input></input>"`) renders as the
    // bare label `"ci"` — parallel to how I_PLACEHOLDER renders as `"i"`.
    if sicompass_sdk::placeholders::is_ci_placeholder(s) {
        return "ci".to_owned();
    }

    // Editor file-content Str — Str inside <input> with a <src=N> annotation
    // (only the editor emits these). Render as `-ci <text>`.
    if let Some(inner) = tags::extract_input(s) {
        if tags::has_src(&inner) {
            return format!("-ci {}", tags::strip_display(s));
        }
    }

    let (prefix, content): (&str, String) = if tags::has_image(s) {
        ("-p", tags::strip_display(s))
    } else if tags::has_checkbox_checked(s) {
        ("-cc", tags::extract_checkbox_checked(s)
            .unwrap_or_else(|| tags::strip_display(s)))
    } else if tags::has_checkbox(s) {
        ("-c", tags::extract_checkbox(s)
            .unwrap_or_else(|| tags::strip_display(s)))
    } else if tags::has_checked(s) {
        ("-rc", tags::extract_checked(s)
            .unwrap_or_else(|| tags::strip_display(s)))
    } else if tags::has_button(s) {
        ("-b", tags::strip_display(s))
    } else if tags::has_input(s) {
        let content = tags::strip_display(s);
        if content.trim() == "i" {
            return "i".to_owned();
        }
        ("-i", content)
    } else if parent_has_radio {
        ("-r", tags::strip_display(s))
    } else {
        ("-", tags::strip_display(s))
    };

    format!("{prefix} {content}")
}

fn build_obj_label(obj: &FfonObject) -> String {
    let raw_key = &obj.key;
    // Strip <one-opt> / <many-opt> first
    let stripped_opt;
    let key: &str = if tags::has_one_opt(raw_key) {
        stripped_opt = tags::strip_one_opt(raw_key).to_owned();
        &stripped_opt
    } else if tags::has_many_opt(raw_key) {
        stripped_opt = tags::strip_many_opt(raw_key).to_owned();
        &stripped_opt
    } else {
        stripped_opt = String::new();
        raw_key
    };
    let _ = stripped_opt;

    // Editor file-content Obj — Obj key wrapped in <input> with a <src=N>
    // annotation (only the editor emits these). Render as `+ci <text>`.
    if let Some(inner) = tags::extract_input(key) {
        if tags::has_src(&inner) {
            return format!("+ci {}", tags::strip_display(key));
        }
    }
    // Editor directory-view dir entry: `<dir><input>name</input>` → `+di name`.
    if tags::has_dir(key) {
        return format!("+di {}", tags::strip_display(key));
    }
    // Editor directory-view file entry: `<file><input>name</input>` → `+fi name`.
    if tags::has_file(key) {
        return format!("+fi {}", tags::strip_display(key));
    }

    if tags::has_checkbox_checked(key) {
        let content = tags::extract_checkbox_checked(key)
            .unwrap_or_else(|| tags::strip_display(key));
        return format!("+cc {content}");
    } else if tags::has_checkbox(key) {
        let content = tags::extract_checkbox(key)
            .unwrap_or_else(|| tags::strip_display(key));
        return format!("+c {content}");
    } else if tags::has_link(key) {
        return format!("+l {}", tags::strip_display(key));
    } else if tags::has_radio(key) {
        let group = tags::extract_radio(key)
            .unwrap_or_else(|| tags::strip_display(key));
        let state = obj.children.iter().find_map(|c| match c {
            FfonElement::Str(s) if tags::has_checked(s) => Some(
                tags::extract_checked(s)
                    .unwrap_or_else(|| tags::strip_display(s)),
            ),
            _ => None,
        }).unwrap_or_default();
        return format!("+R {group} [{state}]");
    } else if tags::has_input(key) {
        return format!("+i {}", tags::strip_display(key));
    }

    format!("+ {}", tags::strip_display(key))
}

// ---------------------------------------------------------------------------
// Helper: check if the parent element has a <radio> tag
// ---------------------------------------------------------------------------

fn check_parent_has_radio(renderer: &AppRenderer) -> bool {
    if renderer.current_id.depth() < 2 {
        return false;
    }
    // The parent is the element we navigated into to reach the current level.
    // Its id is current_id with the last component removed.
    let mut parent_id = renderer.current_id.clone();
    let _last = parent_id.pop(); // now parent_id points to the parent's siblings

    if let Some(arr) = get_ffon_at_id(&renderer.ffon, &parent_id) {
        let idx = parent_id.last().unwrap_or(0);
        if let Some(FfonElement::Obj(obj)) = arr.get(idx) {
            return tags::has_radio(&obj.key);
        }
    }
    false
}

// ---------------------------------------------------------------------------
// Command mode list building
// ---------------------------------------------------------------------------

/// Build the list for `Coordinate::Meta` — shortcut hints from the active provider.
/// Build the list for `Coordinate::TimelineView` — per-tab undo timeline.
///
/// Entries are shown in reverse order (most recent on top). The entry
/// currently at HEAD (next Ctrl+Z target) is prefixed `"> "`; entries
/// in the redo branch (already undone) are prefixed `"\u{00B7} "` (·);
/// older history below the cursor is unprefixed. ASCII `>` is used (vs
/// the more decorative U+25B6 ▶) because the bundled Consolas font's
/// rasteriser only covers codepoints 32..256.
fn build_timeline_list(renderer: &mut AppRenderer) {
    renderer.list_index = 0;

    let (entries, position) = {
        let tl = renderer.active_timeline();
        (tl.entries.clone(), tl.position)
    };

    let providers: Vec<TimelineProviderInfo> = renderer
        .providers
        .iter()
        .map(|p| TimelineProviderInfo {
            display_name: p.display_name().to_owned(),
            path_is_filesystem: p.path_is_filesystem(),
        })
        .collect();

    if entries.is_empty() {
        let mut id = IdArray::new();
        id.push(0);
        renderer.total_list = vec![RenderListItem {
            id,
            label: "  (no history)".to_string(),
            data: None,
            nav_path: None,
        }];
        return;
    }

    let len = entries.len();
    // Index of the entry currently at HEAD (= next undo target). When the
    // user has undone everything, position == len and head_idx is None.
    let head_idx: Option<usize> = if position < len {
        Some(len - position - 1)
    } else {
        None
    };
    // First index belonging to the redo branch (entries already undone).
    let redo_branch_start = len - position;

    let mut items: Vec<RenderListItem> = Vec::with_capacity(len);
    for (i, entry) in entries.iter().enumerate().rev() {
        let prefix = if Some(i) == head_idx {
            "> "
        } else if i >= redo_branch_start {
            "\u{00B7} " // ·
        } else {
            "  "
        };
        let label = format!("{}{}", prefix, timeline_entry_label(entry, &providers));
        let mut id = IdArray::new();
        id.push(i);
        items.push(RenderListItem {
            id,
            label,
            data: None,
            nav_path: None,
        });
    }
    renderer.total_list = items;
}

/// Per-provider context passed to `timeline_entry_label` so it can pick the
/// right path rendering. `display_name` is the fallback label when a Navigate
/// entry has no path (depth-1 origin/destination outside any provider).
/// `path_is_filesystem` toggles slash-separated paths (filebrowser, editor)
/// vs breadcrumb (`section › item`) for synthetic provider paths.
#[derive(Clone, Debug)]
pub struct TimelineProviderInfo {
    pub display_name: String,
    pub path_is_filesystem: bool,
}

/// Render a non-filesystem provider path as a breadcrumb. Strips the leading
/// `/` and replaces remaining slashes with ` > ` so the user reads
/// "Available programs: > Email" instead of "/Available programs:/Email".
/// Filesystem paths are passed through verbatim. ASCII `>` matches the
/// ExtendedSearch (Ctrl+F) breadcrumb and stays within the Consolas
/// glyph range covered by the text rasteriser.
fn render_nav_path(path: &str, is_fs: bool, fallback: &str) -> String {
    if is_fs {
        return path.to_owned();
    }
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        // Path is "/" or empty — at provider root, so fall back to the
        // display_name so the entry still identifies the provider.
        fallback.to_owned()
    } else {
        segments.join(" > ")
    }
}

/// One-line, human-readable summary of a `TimelineEntry` for the Z-key
/// timeline view. Keep it short — the row is rendered as a flat list item.
/// `providers` is indexed by `provider_idx`; its `display_name` is the
/// fallback when a Navigate entry has no path (cursor at depth 1, outside
/// any provider's path zone), and `path_is_filesystem` drives slash vs
/// breadcrumb rendering.
pub fn timeline_entry_label(
    entry: &TimelineEntry,
    providers: &[TimelineProviderInfo],
) -> String {
    match entry {
        TimelineEntry::Navigate {
            kind,
            from_id,
            to_id,
            from_path,
            to_path,
            ..
        } => {
            let info_for = |id: &sicompass_sdk::ffon::IdArray| -> Option<&TimelineProviderInfo> {
                id.get(0).and_then(|i| providers.get(i))
            };
            let render = |id: &sicompass_sdk::ffon::IdArray, path: &Option<String>| -> String {
                let info = info_for(id);
                let fallback = info
                    .map(|p| p.display_name.as_str())
                    .unwrap_or("?")
                    .to_owned();
                match path {
                    Some(p) => render_nav_path(
                        p,
                        info.map(|i| i.path_is_filesystem).unwrap_or(false),
                        &fallback,
                    ),
                    None => fallback,
                }
            };
            let from = render(from_id, from_path);
            let to = render(to_id, to_path);
            // Up/Down doesn't change the provider path, so from_path == to_path
            // for sibling motion inside a provider. Collapse to a single path so
            // the timeline view doesn't repeat the same string with an arrow.
            if from == to {
                format!("nav {:?} {}", kind, to)
            } else {
                format!("nav {:?} {} > {}", kind, from, to)
            }
        }
        TimelineEntry::TextChunk {
            id,
            chunk_seq,
            after,
            ..
        } => {
            let snippet = ffon_str_snippet(after, 30);
            format!(
                "type #{} id={} \"{}\"",
                chunk_seq,
                id.to_display_string(),
                snippet
            )
        }
        TimelineEntry::Structural { id, op, .. } => {
            format!("struct {:?} id={}", op, id.to_display_string())
        }
        TimelineEntry::FsOp {
            op,
            before,
            after,
            side_effect,
            ..
        } => {
            let summary = fs_summary(before.as_ref(), after.as_ref(), side_effect);
            format!("fs {:?} {}", op, summary)
        }
        TimelineEntry::ImapOp { op, .. } => format!("imap {}", imap_op_summary(op)),
        TimelineEntry::ChatOp { op, .. } => format!("chat {}", chat_op_summary(op)),
        TimelineEntry::ProviderOp { label, command, .. } => {
            format!("{} ({})", label, command)
        }
    }
}

/// Extract a short snippet from an FFON element for display. Trims the
/// type-prefix tags (e.g. `<input>`) so the user sees the text payload.
fn ffon_str_snippet(elem: &FfonElement, max_len: usize) -> String {
    let raw = match elem {
        FfonElement::Str(s) => s.clone(),
        FfonElement::Obj(o) => o.key.clone(),
    };
    let s = tags::extract_input(&raw).unwrap_or(raw);
    if s.chars().count() <= max_len {
        s
    } else {
        let truncated: String = s.chars().take(max_len).collect();
        format!("{}\u{2026}", truncated)
    }
}

fn fs_summary(
    before: Option<&FfonElement>,
    after: Option<&FfonElement>,
    side_effect: &FsSideEffect,
) -> String {
    match side_effect {
        FsSideEffect::RenameOnly { from, to } => {
            format!("{} > {}", from.display(), to.display())
        }
        FsSideEffect::TrashedFile { original_path, .. }
        | FsSideEffect::TrashedDir { original_path, .. } => {
            format!("{}", original_path.display())
        }
        FsSideEffect::None => match (before, after) {
            (Some(b), Some(a)) => format!(
                "{} > {}",
                ffon_str_snippet(b, 30),
                ffon_str_snippet(a, 30)
            ),
            (None, Some(a)) => ffon_str_snippet(a, 60),
            (Some(b), None) => ffon_str_snippet(b, 60),
            (None, None) => String::new(),
        },
    }
}

fn imap_op_summary(op: &ImapOpKind) -> String {
    match op {
        ImapOpKind::Trash { msg_id, src_folder, .. } => {
            format!("Trash {} from {}", msg_id, src_folder)
        }
        ImapOpKind::Archive { msg_id, src_folder, .. } => {
            format!("Archive {} from {}", msg_id, src_folder)
        }
        ImapOpKind::Move {
            msg_id,
            src_folder,
            dst_folder,
        } => format!("Move {} {} > {}", msg_id, src_folder, dst_folder),
        ImapOpKind::SetSeen { msg_uid, folder, new, .. } => {
            format!("SetSeen uid={} {} > {}", msg_uid, folder, new)
        }
        ImapOpKind::SetFlagged { msg_uid, folder, new, .. } => {
            format!("SetFlagged uid={} {} > {}", msg_uid, folder, new)
        }
    }
}

fn chat_op_summary(op: &ChatOpKind) -> String {
    match op {
        ChatOpKind::LeaveRoom { room_id } => format!("LeaveRoom {}", room_id),
        ChatOpKind::AcceptInvite { room_id } => format!("AcceptInvite {}", room_id),
        ChatOpKind::RejectInvite { room_id } => format!("RejectInvite {}", room_id),
        ChatOpKind::KickMember { room_id, user_id, .. } => {
            format!("KickMember {} from {}", user_id, room_id)
        }
        ChatOpKind::BanMember { room_id, user_id, .. } => {
            format!("BanMember {} from {}", user_id, room_id)
        }
        ChatOpKind::PostMessage { room_id, body, .. } => {
            let snippet = if body.chars().count() <= 30 {
                body.clone()
            } else {
                let s: String = body.chars().take(30).collect();
                format!("{}\u{2026}", s)
            };
            format!("PostMessage {} \"{}\"", room_id, snippet)
        }
    }
}

fn build_meta_list(renderer: &mut AppRenderer) {
    renderer.list_index = 0;
    let hints = crate::provider::get_meta(renderer);
    let items: Vec<RenderListItem> = hints
        .into_iter()
        .enumerate()
        .map(|(i, label)| {
            let mut id = IdArray::new();
            id.push(i);
            RenderListItem { id, label, data: None, nav_path: None }
        })
        .collect();
    renderer.total_list = items;
}

/// Build the list for `Coordinate::Command`.
///
/// - `CommandPhase::None`: show the available command names for the active element.
/// - `CommandPhase::Provider`: show the secondary selection items (e.g. "open with" apps).
fn build_command_list(renderer: &mut AppRenderer) {
    renderer.list_index = 0;

    match renderer.current_command {
        CommandPhase::None => {
            // Show provider commands as list items
            let cmds = crate::provider::get_commands(renderer);
            let items: Vec<RenderListItem> = cmds
                .into_iter()
                .enumerate()
                .map(|(i, label)| {
                    let mut id = IdArray::new();
                    id.push(i);
                    RenderListItem { id, label, data: None, nav_path: None }
                })
                .collect();
            renderer.total_list = items;
        }
        CommandPhase::Provider => {
            // Show secondary selection list (e.g. list of apps for "open with")
            let cmd_name = renderer.provider_command_name.clone();
            let items_raw = crate::provider::command_list_items(renderer, &cmd_name);
            let items: Vec<RenderListItem> = items_raw
                .into_iter()
                .enumerate()
                .map(|(i, li)| {
                    let mut id = IdArray::new();
                    id.push(i);
                    RenderListItem {
                        id,
                        label: li.label,
                        // Store the exec/data payload in nav_path — not in `data`,
                        // because the renderer treats a non-None `data` field as an
                        // image path and attempts to load it as a texture.
                        data: None,
                        nav_path: if li.data.is_empty() { None } else { Some(li.data) },
                    }
                })
                .collect();
            renderer.total_list = items;
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_state::AppRenderer;
    use sicompass_sdk::ffon::{FfonElement, IdArray};

    fn make_renderer_with_ffon(ffon: Vec<FfonElement>) -> AppRenderer {
        let mut r = AppRenderer::new();
        r.ffon = ffon;
        r.current_id = { let mut id = IdArray::new(); id.push(0); id };
        r
    }

    #[test]
    fn list_root_shows_provider() {
        let mut root = FfonElement::new_obj("tutorial");
        root.as_obj_mut().unwrap().push(FfonElement::new_str("item 0"));

        let mut r = make_renderer_with_ffon(vec![root]);
        create_list_current_layer(&mut r);

        assert_eq!(r.total_list.len(), 1);
        assert!(r.total_list[0].label.contains("tutorial"));
    }

    #[test]
    fn list_depth2_shows_children() {
        let mut root = FfonElement::new_obj("tutorial");
        root.as_obj_mut().unwrap().push(FfonElement::new_str("Hello"));
        root.as_obj_mut().unwrap().push(FfonElement::new_str("World"));

        let mut r = make_renderer_with_ffon(vec![root]);
        r.current_id = { let mut id = IdArray::new(); id.push(0); id.push(0); id };
        create_list_current_layer(&mut r);

        assert_eq!(r.total_list.len(), 2);
        assert!(r.total_list[0].label.contains("Hello"));
        assert!(r.total_list[1].label.contains("World"));
    }

    #[test]
    fn obj_element_gets_plus_prefix() {
        let mut root = FfonElement::new_obj("provider");
        let mut section = FfonElement::new_obj("Section");
        section.as_obj_mut().unwrap().push(FfonElement::new_str("child"));
        root.as_obj_mut().unwrap().push(section);

        let mut r = make_renderer_with_ffon(vec![root]);
        r.current_id = { let mut id = IdArray::new(); id.push(0); id.push(0); id };
        create_list_current_layer(&mut r);

        assert_eq!(r.total_list.len(), 1);
        assert!(r.total_list[0].label.starts_with('+'));
    }

    #[test]
    fn str_element_gets_minus_prefix() {
        let mut root = FfonElement::new_obj("provider");
        root.as_obj_mut().unwrap().push(FfonElement::new_str("leaf item"));

        let mut r = make_renderer_with_ffon(vec![root]);
        r.current_id = { let mut id = IdArray::new(); id.push(0); id.push(0); id };
        create_list_current_layer(&mut r);

        assert_eq!(r.total_list.len(), 1);
        assert!(r.total_list[0].label.starts_with('-'));
    }

    #[test]
    fn filter_by_search_string() {
        let mut root = FfonElement::new_obj("provider");
        root.as_obj_mut().unwrap().push(FfonElement::new_str("apple"));
        root.as_obj_mut().unwrap().push(FfonElement::new_str("banana"));
        root.as_obj_mut().unwrap().push(FfonElement::new_str("apricot"));

        let mut r = make_renderer_with_ffon(vec![root]);
        r.current_id = { let mut id = IdArray::new(); id.push(0); id.push(0); id };
        create_list_current_layer(&mut r);
        populate_list_current_layer(&mut r, "ap");

        assert_eq!(r.filtered_list_indices.len(), 2); // apple, apricot
    }

    #[test]
    fn checkbox_str_label() {
        assert!(build_str_label("<checkbox>item", false).starts_with("-c"));
        assert!(build_str_label("<checkbox checked>item", false).starts_with("-cc"));
    }

    #[test]
    fn input_str_label() {
        assert!(build_str_label("edit: <input>value</input>", false).starts_with("-i"));
    }

    #[test]
    fn i_placeholder_str_label_is_i() {
        // I_PLACEHOLDER must render as plain `"i"`, not `"-i "` —
        // the "i " prefix before the empty <input> tag is the sentinel.
        assert_eq!(build_str_label(sicompass_sdk::placeholders::I_PLACEHOLDER, false), "i");
    }

    #[test]
    fn ci_placeholder_str_label_is_ci() {
        // CI_PLACEHOLDER (editor file-content insert sentinel) renders as plain `"ci"`.
        assert_eq!(build_str_label(sicompass_sdk::placeholders::CI_PLACEHOLDER, false), "ci");
    }

    #[test]
    fn file_content_str_label_emits_minus_ci() {
        // <input><src=N>...</input> (file-content line) → `-ci <text>`.
        let label = build_str_label("<input><src=5>line text</input>", false);
        assert!(label.starts_with("-ci "), "expected `-ci ` prefix, got {label:?}");
        assert!(label.contains("line text"));
    }

    #[test]
    fn dir_obj_label_emits_plus_di() {
        let label = build_obj_label(&FfonObject {
            key: "<dir><input>folder</input>".to_owned(),
            children: vec![],
        });
        assert!(label.starts_with("+di "), "expected `+di ` prefix, got {label:?}");
        assert!(label.contains("folder"));
    }

    #[test]
    fn file_obj_label_emits_plus_fi() {
        let label = build_obj_label(&FfonObject {
            key: "<file><input>thing.txt</input>".to_owned(),
            children: vec![],
        });
        assert!(label.starts_with("+fi "), "expected `+fi ` prefix, got {label:?}");
        assert!(label.contains("thing.txt"));
    }

    #[test]
    fn file_content_obj_label_emits_plus_ci() {
        let label = build_obj_label(&FfonObject {
            key: "<input><src=3>section</input>".to_owned(),
            children: vec![],
        });
        assert!(label.starts_with("+ci "), "expected `+ci ` prefix, got {label:?}");
        assert!(label.contains("section"));
    }

    fn make_renderer_with_items(items: &[&str]) -> AppRenderer {
        let mut root = FfonElement::new_obj("provider");
        for &item in items {
            root.as_obj_mut().unwrap().push(FfonElement::new_str(item));
        }
        let mut r = make_renderer_with_ffon(vec![root]);
        r.current_id = { let mut id = IdArray::new(); id.push(0); id.push(0); id };
        create_list_current_layer(&mut r);
        r
    }

    #[test]
    fn create_list_clears_previous_items() {
        let mut r = make_renderer_with_items(&["a", "b"]);
        assert_eq!(r.total_list.len(), 2);
        // Replace ffon and rebuild
        r.ffon = vec![{ let mut root = FfonElement::new_obj("p"); root.as_obj_mut().unwrap().push(FfonElement::new_str("only")); root }];
        r.current_id = { let mut id = IdArray::new(); id.push(0); id.push(0); id };
        create_list_current_layer(&mut r);
        assert_eq!(r.total_list.len(), 1);
    }

    #[test]
    fn create_list_resets_filtered() {
        let mut r = make_renderer_with_items(&["hello", "world"]);
        populate_list_current_layer(&mut r, "hello");
        assert_eq!(r.filtered_list_indices.len(), 1);
        create_list_current_layer(&mut r);
        assert!(r.filtered_list_indices.is_empty());
    }

    #[test]
    fn populate_empty_search_clears_filter() {
        let mut r = make_renderer_with_items(&["hello", "world"]);
        populate_list_current_layer(&mut r, "hello");
        assert_eq!(r.filtered_list_indices.len(), 1);
        populate_list_current_layer(&mut r, ""); // empty search → clear filter
        assert!(r.filtered_list_indices.is_empty());
    }

    #[test]
    fn populate_no_matches() {
        let mut r = make_renderer_with_items(&["hello", "world"]);
        populate_list_current_layer(&mut r, "xyz");
        assert_eq!(r.filtered_list_indices.len(), 0);
    }

    #[test]
    fn populate_case_insensitive() {
        let mut r = make_renderer_with_items(&["Hello", "WORLD"]);
        populate_list_current_layer(&mut r, "hello");
        assert_eq!(r.filtered_list_indices.len(), 1);
    }

    #[test]
    fn populate_clamps_list_index() {
        let mut r = make_renderer_with_items(&["hello", "world"]);
        r.list_index = 5; // out of range
        populate_list_current_layer(&mut r, "hello"); // 1 match
        assert_eq!(r.list_index, 0);
    }

    #[test]
    fn populate_replaces_previous_filter() {
        let mut r = make_renderer_with_items(&["hello", "world", "help"]);
        populate_list_current_layer(&mut r, "hel"); // 2 matches: hello, help
        assert_eq!(r.filtered_list_indices.len(), 2);
        populate_list_current_layer(&mut r, "hello"); // 1 match
        assert_eq!(r.filtered_list_indices.len(), 1);
    }

    #[test]
    fn fuzzy_non_contiguous_match() {
        // "dcmt" should match "Documents" via fuzzy (non-contiguous subsequence)
        let mut r = make_renderer_with_items(&["Documents", "Downloads", "Desktop"]);
        populate_list_current_layer(&mut r, "dcmt");
        assert!(r.filtered_list_indices.len() >= 1);
        let labels: Vec<&str> = r.filtered_list_indices.iter()
            .map(|&i| r.total_list[i].label.as_str())
            .collect();
        assert!(labels.iter().any(|l| l.contains("Documents")), "expected Documents in {labels:?}");
    }

    #[test]
    fn fuzzy_results_sorted_by_score() {
        // Exact match should score higher than a distant fuzzy match
        let mut r = make_renderer_with_items(&["xdocx", "doc"]);
        populate_list_current_layer(&mut r, "doc");
        assert_eq!(r.filtered_list_indices.len(), 2);
        // "doc" is an exact match — should rank first
        let first_label = &r.total_list[r.filtered_list_indices[0]].label;
        assert!(first_label.contains("doc") && !first_label.contains("xdocx"),
            "expected exact match first, got {first_label}");
    }

    #[test]
    fn fuzzy_match_positions_parallel_to_indices() {
        let mut r = make_renderer_with_items(&["hello", "world"]);
        populate_list_current_layer(&mut r, "hel");
        assert_eq!(r.filtered_list_indices.len(), r.fuzzy_match_positions.len());
    }

    // -----------------------------------------------------------------------
    // Timeline-view list builder
    // -----------------------------------------------------------------------

    fn nav_entry(from: &str, to: &str) -> TimelineEntry {
        TimelineEntry::Navigate {
            provider_idx: 0,
            from_id: IdArray::new(),
            to_id: IdArray::new(),
            from_path: Some(from.to_string()),
            to_path: Some(to.to_string()),
            kind: sicompass_sdk::timeline::NavKind::ArrowDown,
        }
    }

    #[test]
    fn timeline_view_empty_shows_placeholder() {
        let mut r = AppRenderer::new();
        r.coordinate = Coordinate::TimelineView;
        create_list_current_layer(&mut r);
        assert_eq!(r.total_list.len(), 1);
        assert!(r.total_list[0].label.contains("(no history)"));
    }

    #[test]
    fn timeline_view_reverses_and_marks_head() {
        let mut r = AppRenderer::new();
        r.tab_timelines[0].entries.push(nav_entry("/a", "/b"));
        r.tab_timelines[0].entries.push(nav_entry("/b", "/c"));
        // position = 0 → HEAD is the most recent entry (entries[1])
        r.coordinate = Coordinate::TimelineView;
        create_list_current_layer(&mut r);

        assert_eq!(r.total_list.len(), 2);
        // First row = most recent (entries[1]) = HEAD → prefixed "> "
        assert!(
            r.total_list[0].label.starts_with("> "),
            "expected `> ` prefix on HEAD row, got: {:?}",
            r.total_list[0].label
        );
        // Provider info is empty here, so the renderer falls back to the
        // breadcrumb path; "/b" and "/c" surface as bare "b" and "c".
        assert!(r.total_list[0].label.contains('b') && r.total_list[0].label.contains('c'));
        // Second row = older (entries[0]) → no marker prefix
        assert!(
            r.total_list[1].label.starts_with("  "),
            "expected blank prefix on older row, got: {:?}",
            r.total_list[1].label
        );
    }

    #[test]
    fn timeline_view_marks_redo_branch_after_undo() {
        let mut r = AppRenderer::new();
        r.tab_timelines[0].entries.push(nav_entry("/a", "/b"));
        r.tab_timelines[0].entries.push(nav_entry("/b", "/c"));
        r.tab_timelines[0].entries.push(nav_entry("/c", "/d"));
        // Simulate two Ctrl+Z presses: HEAD becomes entries[0]; entries[1..3]
        // are in the redo branch.
        r.tab_timelines[0].position = 2;
        r.coordinate = Coordinate::TimelineView;
        create_list_current_layer(&mut r);

        assert_eq!(r.total_list.len(), 3);
        // Most recent (entries[2]) → redo branch
        assert!(r.total_list[0].label.starts_with("\u{00B7} "));
        // entries[1] → redo branch
        assert!(r.total_list[1].label.starts_with("\u{00B7} "));
        // entries[0] → HEAD
        assert!(r.total_list[2].label.starts_with("> "));
    }

    #[test]
    fn timeline_entry_label_provider_op_uses_label() {
        let entry = TimelineEntry::ProviderOp {
            provider_idx: 0,
            command: "radio-toggle".into(),
            payload: FfonElement::Str("payload".into()),
            label: "toggle theme".into(),
        };
        let s = timeline_entry_label(&entry, &[]);
        assert!(s.contains("toggle theme"));
        assert!(s.contains("radio-toggle"));
    }

    fn provider_info(name: &str, fs: bool) -> TimelineProviderInfo {
        TimelineProviderInfo {
            display_name: name.to_owned(),
            path_is_filesystem: fs,
        }
    }

    #[test]
    fn timeline_entry_label_navigate_falls_back_to_provider_name_when_path_none() {
        let from_id = {
            let mut a = sicompass_sdk::ffon::IdArray::new();
            a.push(0);
            a
        };
        let to_id = {
            let mut a = sicompass_sdk::ffon::IdArray::new();
            a.push(1);
            a
        };
        let entry = TimelineEntry::Navigate {
            provider_idx: 1,
            from_id,
            to_id,
            from_path: None,
            to_path: None,
            kind: sicompass_sdk::timeline::NavKind::ArrowDown,
        };
        let providers = vec![
            provider_info("File Browser", true),
            provider_info("Email Client", false),
        ];
        let s = timeline_entry_label(&entry, &providers);
        assert!(s.contains("File Browser"), "expected origin provider name in {:?}", s);
        assert!(s.contains("Email Client"), "expected destination provider name in {:?}", s);
        assert!(!s.contains("?"), "no `?` when provider names are available: {:?}", s);
    }

    #[test]
    fn timeline_entry_label_filesystem_path_keeps_slashes() {
        let mut id = sicompass_sdk::ffon::IdArray::new();
        id.push(0);
        let entry = TimelineEntry::Navigate {
            provider_idx: 0,
            from_id: id.clone(),
            to_id: id,
            from_path: Some("/home/nico".to_owned()),
            to_path: Some("/home/nico/Documents".to_owned()),
            kind: sicompass_sdk::timeline::NavKind::ArrowRight,
        };
        let providers = vec![provider_info("file browser", true)];
        let s = timeline_entry_label(&entry, &providers);
        assert!(s.contains("/home/nico"), "fs paths render verbatim: {s}");
        assert!(s.contains("/home/nico/Documents"), "fs paths keep slashes: {s}");
        // The outer from→to join is ` > ` for every Navigate label, but the
        // *segments inside an fs path* must NOT be breadcrumb-split.
        assert!(
            !s.contains("home > nico"),
            "fs path segments must not be split by breadcrumb separator: {s}",
        );
    }

    #[test]
    fn timeline_entry_label_non_filesystem_path_uses_breadcrumb() {
        let mut id = sicompass_sdk::ffon::IdArray::new();
        id.push(0);
        let entry = TimelineEntry::Navigate {
            provider_idx: 0,
            from_id: id.clone(),
            to_id: id,
            from_path: Some("/Available programs:".to_owned()),
            to_path: Some("/Available programs:/Email".to_owned()),
            kind: sicompass_sdk::timeline::NavKind::ArrowRight,
        };
        let providers = vec![provider_info("settings", false)];
        let s = timeline_entry_label(&entry, &providers);
        // No leading slashes, no `/section/item` separator — replaced with `›`.
        assert!(!s.contains("/Available"), "non-fs paths must drop leading slash: {s}");
        assert!(!s.contains(":/Email"), "non-fs paths must replace `/` with breadcrumb: {s}");
        assert!(s.contains("Available programs:"), "breadcrumb keeps segment text: {s}");
        assert!(s.contains("Email"), "breadcrumb keeps tail segment: {s}");
        assert!(s.contains(" > "), "non-fs descent uses ` > ` separator: {s}");
    }

    #[test]
    fn timeline_entry_label_non_filesystem_root_falls_back_to_display_name() {
        // current_path="/" (provider root) on a non-fs provider must read as
        // the display_name, not an empty / slash-only string.
        let mut id = sicompass_sdk::ffon::IdArray::new();
        id.push(0);
        let entry = TimelineEntry::Navigate {
            provider_idx: 0,
            from_id: id.clone(),
            to_id: id,
            from_path: Some("/".to_owned()),
            to_path: Some("/".to_owned()),
            kind: sicompass_sdk::timeline::NavKind::ArrowDown,
        };
        let providers = vec![provider_info("settings", false)];
        let s = timeline_entry_label(&entry, &providers);
        assert!(s.contains("settings"), "root non-fs path falls back to display_name: {s}");
        assert!(!s.contains('/'), "root non-fs path must not render a bare slash: {s}");
    }
}
