//! Right-panel list building — equivalent to `list.c`.
//!
//! Builds `AppRenderer::total_list` from the FFON tree at the current
//! navigation path, then optionally filters it by a search string.

use crate::app_state::{AppRenderer, CommandPhase, Coordinate, RenderListItem};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use sicompass_sdk::ffon::{get_ffon_at_id, FfonElement, IdArray};
use sicompass_sdk::tags;

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
        Coordinate::OperatorGeneral
        | Coordinate::OperatorInsert
        | Coordinate::SimpleSearch
        | Coordinate::EditorGeneral
        | Coordinate::EditorInsert => {}
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
                    .or_else(|| tags::extract_input_all(s))
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
        FfonElement::Obj(obj) => build_obj_label(&obj.key),
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
    } else if tags::has_input_all(s) {
        ("-i", tags::strip_display(s))
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

fn build_obj_label(key: &str) -> String {
    // Strip <one-opt> / <many-opt> first
    let stripped_opt;
    let key: &str = if tags::has_one_opt(key) {
        stripped_opt = tags::strip_one_opt(key).to_owned();
        &stripped_opt
    } else if tags::has_many_opt(key) {
        stripped_opt = tags::strip_many_opt(key).to_owned();
        &stripped_opt
    } else {
        stripped_opt = String::new();
        key
    };
    let _ = stripped_opt;

    let (prefix, content): (&str, String) = if tags::has_checkbox_checked(key) {
        ("+cc", tags::extract_checkbox_checked(key)
            .unwrap_or_else(|| tags::strip_display(key)))
    } else if tags::has_checkbox(key) {
        ("+c", tags::extract_checkbox(key)
            .unwrap_or_else(|| tags::strip_display(key)))
    } else if tags::has_link(key) {
        ("+l", tags::strip_display(key))
    } else if tags::has_radio(key) {
        ("+R", tags::extract_radio(key)
            .unwrap_or_else(|| tags::strip_display(key)))
    } else if tags::has_input_all(key) {
        ("+i", tags::strip_display(key))
    } else if tags::has_input(key) {
        ("+i", tags::strip_display(key))
    } else {
        ("+", tags::strip_display(key))
    };

    format!("{prefix} {content}")
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
        // The compose-body placeholder `"i <input></input>"` must render as plain `"i"`,
        // not `"-i "` — the "i " prefix before the empty <input> tag is the sentinel.
        assert_eq!(build_str_label("i <input></input>", false), "i");
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
}
