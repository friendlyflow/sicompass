//! Provider registry helpers — equivalent to `provider.c` (app-level glue).
//!
//! These functions operate on `AppRenderer` and delegate to the SDK `Provider` trait.

use crate::app_state::AppRenderer;
use sicompass_sdk::provider::Provider;

// ---------------------------------------------------------------------------
// Active provider
// ---------------------------------------------------------------------------

/// Return the index of the active provider (from `current_id.ids[0]`).
pub fn active_provider_index(renderer: &AppRenderer) -> Option<usize> {
    renderer.current_id.get(0)
}

/// Mutable reference to the currently active provider.
pub fn get_active_provider(renderer: &mut AppRenderer) -> Option<&mut Box<dyn Provider>> {
    let idx = renderer.current_id.get(0)?;
    renderer.providers.get_mut(idx)
}

/// Immutable reference to the currently active provider.
pub fn get_active_provider_ref(renderer: &AppRenderer) -> Option<&dyn Provider> {
    let idx = renderer.current_id.get(0)?;
    renderer.providers.get(idx).map(|p| p.as_ref())
}

// ---------------------------------------------------------------------------
// Path management
// ---------------------------------------------------------------------------

/// Push a path segment to the active provider (called when navigating right
/// into a provider-managed level that uses lazy fetching).
pub fn push_path(renderer: &mut AppRenderer, segment: &str) {
    if let Some(p) = get_active_provider(renderer) {
        p.push_path(segment);
    }
}

/// Pop the last path segment from the active provider.
pub fn pop_path(renderer: &mut AppRenderer) {
    if let Some(p) = get_active_provider(renderer) {
        p.pop_path();
    }
}

/// Return the current path of the active provider.
pub fn current_path(renderer: &AppRenderer) -> &str {
    get_active_provider_ref(renderer)
        .map(|p| p.current_path())
        .unwrap_or("/")
}

/// Set the current path of the active provider directly (used by undo/redo).
pub fn set_provider_path(renderer: &mut AppRenderer, path: &str) {
    let idx = match renderer.current_id.get(0) {
        Some(i) => i,
        None => return,
    };
    if let Some(p) = renderer.providers.get_mut(idx) {
        p.set_current_path(path);
    }
}

// ---------------------------------------------------------------------------
// Refresh / re-fetch
// ---------------------------------------------------------------------------

/// Re-fetch from the active provider and replace its root FfonElement in `ffon`.
///
/// Used for `no_cache` providers (e.g. file browser) that need to re-read
/// the underlying data source on every navigation step.
pub fn refresh_current_directory(renderer: &mut AppRenderer) {
    let idx = match renderer.current_id.get(0) {
        Some(i) => i,
        None => return,
    };
    if idx >= renderer.providers.len() { return; }

    let children = renderer.providers[idx].fetch();
    if let Some(err) = renderer.providers[idx].take_error() {
        renderer.error_message = err;
    }
    let cur_path = renderer.providers[idx].current_path().to_owned();
    let root_key = if cur_path == "/" {
        renderer.providers[idx].display_name().to_owned()
    } else {
        std::path::Path::new(&cur_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| renderer.providers[idx].display_name().to_owned())
    };

    let mut root = sicompass_sdk::ffon::FfonElement::new_obj(&root_key);
    for child in children {
        root.as_obj_mut().unwrap().push(child);
    }
    if idx < renderer.ffon.len() {
        renderer.ffon[idx] = root;
    }
}

/// Re-fetch only if the active provider requests it (`needs_refresh()`).
pub fn refresh_if_needed(renderer: &mut AppRenderer) {
    let needs = renderer.providers
        .get(renderer.current_id.get(0).unwrap_or(usize::MAX))
        .map(|p| p.needs_refresh())
        .unwrap_or(false);
    if needs {
        refresh_current_directory(renderer);
    }
}

// ---------------------------------------------------------------------------
// Editing
// ---------------------------------------------------------------------------

/// Commit an inline edit to the active provider.
pub fn commit_edit(renderer: &mut AppRenderer, old: &str, new: &str) -> bool {
    let idx = match renderer.current_id.get(0) {
        Some(i) => i,
        None => return false,
    };
    if let Some(p) = renderer.providers.get_mut(idx) {
        let ok = p.commit_edit(old, new);
        if !ok {
            if let Some(err) = p.take_error() {
                renderer.error_message = err;
            }
        }
        ok
    } else {
        false
    }
}

/// Delete a file or directory by name via the active provider.
pub fn delete_item_by_name(renderer: &mut AppRenderer, name: &str) -> bool {
    let idx = match renderer.current_id.get(0) {
        Some(i) => i,
        None => return false,
    };
    if let Some(p) = renderer.providers.get_mut(idx) {
        let ok = p.delete_item(name);
        if !ok {
            if let Some(err) = p.take_error() {
                renderer.error_message = err;
            }
        }
        ok
    } else {
        false
    }
}

/// Copy an item via the active provider.
///
/// Parameters mirror C `providerCopyItem`: source dir, source name, dest dir, dest name.
pub fn copy_item(renderer: &mut AppRenderer, src_dir: &str, src_name: &str, dest_dir: &str, dest_name: &str) -> bool {
    let idx = match renderer.current_id.get(0) {
        Some(i) => i,
        None => return false,
    };
    if let Some(p) = renderer.providers.get_mut(idx) {
        let ok = p.copy_item(src_dir, src_name, dest_dir, dest_name);
        if !ok {
            if let Some(err) = p.take_error() {
                renderer.error_message = err;
            }
        }
        ok
    } else {
        false
    }
}

/// Create a file via the active provider.
pub fn create_file(renderer: &mut AppRenderer, name: &str) -> bool {
    let idx = match renderer.current_id.get(0) {
        Some(i) => i,
        None => return false,
    };
    if let Some(p) = renderer.providers.get_mut(idx) {
        let ok = p.create_file(name);
        if !ok {
            if let Some(err) = p.take_error() {
                renderer.error_message = err;
            }
        }
        ok
    } else {
        false
    }
}

/// Create a directory via the active provider.
pub fn create_directory(renderer: &mut AppRenderer, name: &str) -> bool {
    let idx = match renderer.current_id.get(0) {
        Some(i) => i,
        None => return false,
    };
    if let Some(p) = renderer.providers.get_mut(idx) {
        let ok = p.create_directory(name);
        if !ok {
            if let Some(err) = p.take_error() {
                renderer.error_message = err;
            }
        }
        ok
    } else {
        false
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Get available commands for the active element from the active provider.
pub fn get_commands(renderer: &AppRenderer) -> Vec<String> {
    get_active_provider_ref(renderer)
        .map(|p| p.commands())
        .unwrap_or_default()
}

/// Get meta/shortcut hints from the active provider.
pub fn get_meta(renderer: &AppRenderer) -> Vec<String> {
    if renderer.current_id.depth() <= 1 {
        return sicompass_sdk::meta::lookup_formatted(sicompass_sdk::meta::ROOT)
            .unwrap_or_default();
    }
    get_active_provider_ref(renderer)
        .map(|p| p.meta())
        .unwrap_or_default()
}

/// Handle a command invocation (`:command`). Returns optional result element.
pub fn handle_command(
    renderer: &mut AppRenderer,
    command: &str,
    element_key: &str,
    element_type: i32,
) -> Option<sicompass_sdk::ffon::FfonElement> {
    let idx = renderer.current_id.get(0)?;
    let mut error = String::new();
    let result = renderer.providers.get_mut(idx)?.handle_command(
        command,
        element_key,
        element_type,
        &mut error,
    );
    if !error.is_empty() {
        renderer.error_message = error;
    }
    result
}

/// Get the items for a command's secondary selection list (e.g. "open with" app list).
pub fn command_list_items(
    renderer: &mut AppRenderer,
    command: &str,
) -> Vec<sicompass_sdk::provider::ListItem> {
    let idx = match renderer.current_id.get(0) {
        Some(i) => i,
        None => return Vec::new(),
    };
    renderer.providers
        .get_mut(idx)
        .map(|p| p.command_list_items(command))
        .unwrap_or_default()
}

/// Execute a command with the selected list item.
pub fn execute_command(
    renderer: &mut AppRenderer,
    command: &str,
    selected_item: &str,
) -> bool {
    let idx = match renderer.current_id.get(0) {
        Some(i) => i,
        None => return false,
    };
    renderer.providers
        .get_mut(idx)
        .map(|p| p.execute_command(command, selected_item))
        .unwrap_or(false)
}

/// Delete the currently selected item via the active provider.
pub fn delete_item(renderer: &mut AppRenderer) -> bool {
    use sicompass_sdk::ffon::get_ffon_at_id;
    use sicompass_sdk::tags;

    // Get the element name before borrowing providers mutably
    let name = {
        let arr = get_ffon_at_id(&renderer.ffon, &renderer.current_id);
        let idx = renderer.current_id.last().unwrap_or(0);
        arr.and_then(|a| a.get(idx))
            .map(|e| match e {
                sicompass_sdk::ffon::FfonElement::Str(s) => tags::strip_display(s).to_string(),
                sicompass_sdk::ffon::FfonElement::Obj(o) => tags::strip_display(&o.key).to_string(),
            })
            .unwrap_or_default()
    };

    let provider_idx = match renderer.current_id.get(0) {
        Some(i) => i,
        None => return false,
    };
    let ok = renderer.providers
        .get_mut(provider_idx)
        .map(|p| p.delete_item(&name))
        .unwrap_or(false);

    if ok {
        refresh_current_directory(renderer);
    }
    ok
}

/// Notify the active provider that a checkbox changed state.
///
/// Extracts the label and new checked state from the FFON element, then calls
/// `on_checkbox_change` on the provider (e.g. settings saves the config).
pub fn notify_checkbox_changed(renderer: &mut AppRenderer, new_elem_text: &str) {
    use sicompass_sdk::tags;

    let (label, checked) = if tags::has_checkbox_checked(new_elem_text) {
        (tags::extract_checkbox_checked(new_elem_text).unwrap_or_default(), true)
    } else if tags::has_checkbox(new_elem_text) {
        (tags::extract_checkbox(new_elem_text).unwrap_or_default(), false)
    } else {
        return;
    };

    if let Some(p) = get_active_provider(renderer) {
        p.on_checkbox_change(&label, checked);
    }
}

// ---------------------------------------------------------------------------
// Auth registry — maps URL origins to Bearer API keys
// ---------------------------------------------------------------------------

use std::sync::Mutex;

static AUTH_REGISTRY: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// Register a URL origin → API key mapping for Bearer auth.
/// Equivalent to `providerRegisterAuth` in the C code.
pub fn register_auth(origin: &str, api_key: &str) {
    AUTH_REGISTRY.lock().unwrap().push((origin.to_owned(), api_key.to_owned()));
}

/// Find an API key for a URL by prefix match.
/// Equivalent to `findApiKeyForUrl` in the C code.
pub fn find_api_key_for_url(url: &str) -> Option<String> {
    let registry = AUTH_REGISTRY.lock().unwrap();
    registry.iter()
        .find(|(origin, _)| url.starts_with(origin.as_str()))
        .map(|(_, key)| key.clone())
}

/// Clear the auth registry (for tests only).
#[cfg(test)]
fn clear_auth_registry() {
    AUTH_REGISTRY.lock().unwrap().clear();
}

/// Notify the active provider that a radio button changed value.
///
/// Equivalent to `providerNotifyRadioChanged` in the C code. Extracts the
/// selected value from the FFON tree and fires `on_radio_change` on the provider.
pub fn notify_radio_changed(renderer: &mut AppRenderer) {
    use sicompass_sdk::ffon::get_ffon_at_id;
    use sicompass_sdk::tags;

    // current_id points to the radio group parent. The selected child has <checked>.
    let mut parent_id = renderer.current_id.clone();
    let _ = parent_id.pop(); // go up one level to the radio group

    // Find the radio group key (group name)
    let group_key = {
        let arr = get_ffon_at_id(&renderer.ffon, &parent_id);
        let pidx = parent_id.last().unwrap_or(0);
        arr.and_then(|a| a.get(pidx))
            .and_then(|e| e.as_obj())
            .map(|o| tags::strip_display(&o.key).to_string())
            .unwrap_or_default()
    };

    // Find the checked child value
    let selected_value = {
        let arr = get_ffon_at_id(&renderer.ffon, &renderer.current_id);
        arr.and_then(|children| {
            children.iter().find_map(|e| {
                let s = e.as_str()?;
                if tags::has_checked(s) {
                    Some(tags::extract_checked(s).unwrap_or(s.to_string()))
                } else {
                    None
                }
            })
        })
        .unwrap_or_default()
    };

    if let Some(p) = get_active_provider(renderer) {
        p.on_radio_change(&group_key, &selected_value);
    }
}

/// Notify the active provider that a button was pressed.
///
/// Handles the "Add element:" section protocol: if a button inside an
/// "Add element:" object is pressed, calls `create_element` on the provider
/// and inserts the returned element before that section. Otherwise calls
/// `on_button_press` and refreshes the current directory.
///
/// Mirrors C `providerNotifyButtonPressed` in `provider.c`.
pub fn notify_button_pressed(renderer: &mut AppRenderer) {
    use sicompass_sdk::ffon::{FfonElement, get_ffon_at_id};
    use sicompass_sdk::tags;

    let current_id = renderer.current_id.clone();
    let depth = current_id.depth();
    let provider_idx = match current_id.get(0) { Some(i) => i, None => return };

    // Extract button function name from the current string element.
    let function_name: String = {
        let arr = get_ffon_at_id(&renderer.ffon, &current_id);
        let idx = current_id.last().unwrap_or(0);
        let text = match arr.and_then(|a| a.get(idx)).and_then(|e| e.as_str()) {
            Some(s) => s.to_owned(),
            None => return,
        };
        match tags::extract_button_function_name(&text) {
            Some(n) => n,
            None => return,
        }
    };

    let mut handled = false;

    // Check for "Add element:" section (requires depth >= 3: root / grandparent / "Add element:" / button).
    if depth >= 3 {
        let mut parent_id = current_id.clone();
        parent_id.pop();
        let parent_idx = parent_id.last().unwrap_or(0);

        // Is the parent an "Add element:" object?
        let parent_is_add_elem = {
            get_ffon_at_id(&renderer.ffon, &parent_id)
                .and_then(|a| a.get(parent_idx))
                .and_then(|e| e.as_obj())
                .map(|o| o.key.as_str() == "Add element:")
                .unwrap_or(false)
        };

        if parent_is_add_elem {
            let mut grand_id = parent_id.clone();
            grand_id.pop();
            // insert_idx = position of "Add element:" inside grandparent's children
            let insert_idx = parent_idx;
            let grand_idx = grand_id.last().unwrap_or(0);

            let grand_is_obj = {
                get_ffon_at_id(&renderer.ffon, &grand_id)
                    .and_then(|a| a.get(grand_idx))
                    .map(|e| matches!(e, FfonElement::Obj(_)))
                    .unwrap_or(false)
            };

            if grand_is_obj {
                // Pop "Add element:" from the provider path before calling create_element.
                // navigate_right_raw now pushes the path even for in-place navigation
                // (depth >= 2), so "Add element:" is on the path. Popping it first means
                // create_element sees the grandparent path (e.g. /ahu) and can construct
                // the correct child fetch path (e.g. /ahu/supply).
                if let Some(p) = renderer.providers.get_mut(provider_idx) {
                    p.pop_path();
                }

                let new_elem = renderer.providers.get_mut(provider_idx)
                    .and_then(|p| p.create_element(&function_name));

                if let Some(new_elem) = new_elem {
                    // Insert new_elem before "Add element:" in grandparent's children.
                    {
                        if let Some(slice) = get_ffon_at_id_mut(&mut renderer.ffon, &grand_id) {
                            if let Some(obj) = slice.get_mut(grand_idx).and_then(|e| e.as_obj_mut()) {
                                obj.children.insert(insert_idx.min(obj.children.len()), new_elem);
                            }
                        }
                    }

                    // Move cursor to the newly inserted element.
                    renderer.current_id = grand_id.clone();
                    renderer.current_id.push(insert_idx);

                    handled = true;

                    let add_elem_pos = insert_idx + 1;
                    let is_one_opt = function_name.starts_with("one-opt:");

                    let grand_child_count = {
                        get_ffon_at_id(&renderer.ffon, &grand_id)
                            .and_then(|a| a.get(grand_idx))
                            .and_then(|e| e.as_obj())
                            .map(|o| o.children.len())
                            .unwrap_or(0)
                    };

                    if add_elem_pos < grand_child_count.saturating_sub(1) {
                        // Clone "Add element:": remove the clone at add_elem_pos.
                        {
                            if let Some(slice) = get_ffon_at_id_mut(&mut renderer.ffon, &grand_id) {
                                if let Some(obj) = slice.get_mut(grand_idx).and_then(|e| e.as_obj_mut()) {
                                    if add_elem_pos < obj.children.len() {
                                        obj.children.remove(add_elem_pos);
                                    }
                                }
                            }
                        }
                        // Remove matching one-opt button from the original "Add element:" (last child).
                        if is_one_opt {
                            let last_idx = {
                                get_ffon_at_id(&renderer.ffon, &grand_id)
                                    .and_then(|a| a.get(grand_idx))
                                    .and_then(|e| e.as_obj())
                                    .map(|o| o.children.len().saturating_sub(1))
                                    .unwrap_or(0)
                            };
                            remove_one_opt_button_and_maybe_section(
                                &mut renderer.ffon, &grand_id, grand_idx, last_idx, &function_name,
                            );
                        }
                    } else if is_one_opt {
                        // Original "Add element:": remove button and section if empty.
                        let removed_all = remove_one_opt_button_and_maybe_section(
                            &mut renderer.ffon, &grand_id, grand_idx, add_elem_pos, &function_name,
                        );
                        if removed_all {
                            if let Some(slice) = get_ffon_at_id_mut(&mut renderer.ffon, &grand_id) {
                                if let Some(obj) = slice.get_mut(grand_idx).and_then(|e| e.as_obj_mut()) {
                                    if add_elem_pos < obj.children.len() {
                                        obj.children.remove(add_elem_pos);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if !handled {
        if let Some(p) = renderer.providers.get_mut(provider_idx) {
            p.on_button_press(&function_name);
            if let Some(err) = p.take_error() {
                renderer.error_message = err;
            }
        }
        refresh_current_directory(renderer);
    }
}

/// Remove the button matching `function_name` from the "Add element:" child at
/// `add_elem_idx` inside the grandparent object. Returns `true` if that child
/// is now empty (so the caller can remove the "Add element:" section).
fn remove_one_opt_button_and_maybe_section(
    ffon: &mut Vec<sicompass_sdk::ffon::FfonElement>,
    grand_id: &sicompass_sdk::ffon::IdArray,
    grand_idx: usize,
    add_elem_idx: usize,
    function_name: &str,
) -> bool {
    use sicompass_sdk::tags;

    let Some(slice) = get_ffon_at_id_mut(ffon, grand_id) else { return false };
    let Some(grand_obj) = slice.get_mut(grand_idx).and_then(|e| e.as_obj_mut()) else { return false };
    let Some(add_elem_obj) = grand_obj.children.get_mut(add_elem_idx).and_then(|e| e.as_obj_mut()) else { return false };

    if let Some(btn_idx) = add_elem_obj.children.iter().position(|e| {
        e.as_str()
            .and_then(|s| tags::extract_button_function_name(s))
            .as_deref() == Some(function_name)
    }) {
        add_elem_obj.children.remove(btn_idx);
    }
    add_elem_obj.children.is_empty()
}

/// Navigate a specific provider to an absolute directory path.
///
/// Resets the provider to `/`, refetches, then walks each path component
/// by matching display-stripped element names.  Sets `renderer.current_id`
/// so that the depth-2 slot points inside `absolute_dir`.
///
/// If `target_filename` is non-empty, the cursor is moved to that entry;
/// otherwise the cursor stays at 0.  Returns `true` on success.
///
/// Mirrors C `providerNavigateToPath`.
pub fn navigate_to_path(
    renderer: &mut AppRenderer,
    root_idx: usize,
    absolute_dir: &str,
    target_filename: &str,
) -> bool {
    use sicompass_sdk::ffon::{FfonElement, IdArray, get_ffon_at_id};
    use sicompass_sdk::tags;

    if root_idx >= renderer.providers.len() { return false; }

    // On Windows, absolute paths start with a drive letter ("C:\...") or UNC
    // ("\\...").  Walking component-by-component from the "/" sentinel root
    // doesn't work for these because the root shows drive entries ("C:\"), not
    // individual directory segments.  Jump directly to the target directory
    // instead, mirroring C's providerNavigateToPath (provider.c:735-769).
    #[cfg(windows)]
    {
        let b = absolute_dir.as_bytes();
        let is_windows_absolute = (b.len() >= 2 && b[1] == b':')
            || (b.len() >= 2 && b[0] == b'\\' && b[1] == b'\\');
        if is_windows_absolute {
            renderer.providers[root_idx].set_current_path(absolute_dir);
            let children = renderer.providers[root_idx].fetch();
            if let Some(FfonElement::Obj(root_obj)) = renderer.ffon.get_mut(root_idx) {
                root_obj.children = children;
            } else {
                return false;
            }
            let mut nav_id = IdArray::new();
            nav_id.push(root_idx);
            nav_id.push(0);
            renderer.current_id = nav_id;
            if !target_filename.is_empty() {
                let found = get_ffon_at_id(&renderer.ffon, &renderer.current_id)
                    .and_then(|slice| {
                        slice.iter().enumerate().find_map(|(i, e)| {
                            let raw = match e {
                                FfonElement::Str(s) => s.as_str(),
                                FfonElement::Obj(o) => o.key.as_str(),
                            };
                            if tags::strip_display(raw) == target_filename { Some(i) } else { None }
                        })
                    });
                if let Some(i) = found {
                    renderer.current_id.set_last(i);
                }
            }
            return true;
        }
    }

    // Reset provider to root and re-fetch
    renderer.providers[root_idx].set_current_path("/");
    let root_children = renderer.providers[root_idx].fetch();
    if let Some(FfonElement::Obj(root_obj)) = renderer.ffon.get_mut(root_idx) {
        root_obj.children = root_children;
    } else {
        return false;
    }

    // Start cursor at depth=2 inside this provider's root
    let mut nav_id = IdArray::new();
    nav_id.push(root_idx);
    nav_id.push(0);
    renderer.current_id = nav_id;

    // Walk each component of the absolute path (skip leading '/').
    // Split on both '/' and '\' to match C's strtok_r(start, "/\\").
    let path_stripped = absolute_dir.trim_start_matches('/');
    for component in path_stripped.split(|c| c == '/' || c == '\\') {
        if component.is_empty() { continue; }

        // Find component in current level
        let found_idx = {
            let arr = get_ffon_at_id(&renderer.ffon, &renderer.current_id);
            arr.and_then(|slice| {
                slice.iter().enumerate().find_map(|(i, e)| {
                    let raw = match e {
                        FfonElement::Str(s) => s.as_str(),
                        FfonElement::Obj(o) => o.key.as_str(),
                    };
                    if tags::strip_display(raw) == component { Some(i) } else { None }
                })
            })
        };

        let Some(idx) = found_idx else { return false; };
        renderer.current_id.set_last(idx);

        // Navigate right into this component (lazy-fetch child level)
        if !navigate_right(renderer) { return false; }
    }

    // If target_filename specified, find and select it
    if !target_filename.is_empty() {
        let found = {
            let arr = get_ffon_at_id(&renderer.ffon, &renderer.current_id);
            arr.and_then(|slice| {
                slice.iter().enumerate().find_map(|(i, e)| {
                    let raw = match e {
                        FfonElement::Str(s) => s.as_str(),
                        FfonElement::Obj(o) => o.key.as_str(),
                    };
                    if tags::strip_display(raw) == target_filename { Some(i) } else { None }
                })
            })
        };
        if let Some(i) = found {
            renderer.current_id.set_last(i);
        }
    }

    true
}

/// Navigate right into the currently selected element (enter a directory).
///
/// Pushes the element name to the provider, fetches children, appends them
/// to the FFON tree, and advances `current_id` by one depth level.
/// Returns `false` if the selected element is not an `Obj` (not a directory).
///
/// Mirrors C `providerNavigateRight`.
pub fn navigate_right(renderer: &mut AppRenderer) -> bool {
    use sicompass_sdk::ffon::{FfonElement, get_ffon_at_id};
    use sicompass_sdk::tags;

    let idx = renderer.current_id.last().unwrap_or(0);
    let root_idx = match renderer.current_id.get(0) { Some(i) => i, None => return false };

    // Get the element name (must be an Obj to navigate into)
    let (segment, is_obj) = {
        let arr = match get_ffon_at_id(&renderer.ffon, &renderer.current_id) {
            Some(a) => a,
            None => return false,
        };
        match arr.get(idx) {
            Some(FfonElement::Obj(o)) => (tags::strip_display(&o.key).to_string(), true),
            _ => return false,
        }
    };
    if !is_obj { return false; }

    // Push path to provider and fetch children
    if let Some(p) = renderer.providers.get_mut(root_idx) {
        p.push_path(&segment);
    }
    let children = if let Some(p) = renderer.providers.get_mut(root_idx) {
        p.fetch()
    } else {
        return false;
    };

    // Attach children to the selected Obj element
    {
        let arr = match get_ffon_at_id_mut(&mut renderer.ffon, &renderer.current_id) {
            Some(a) => a,
            None => return false,
        };
        if let Some(FfonElement::Obj(obj)) = arr.get_mut(idx) {
            obj.children = children;
        }
    }

    // Advance current_id one level deeper
    renderer.current_id.push(0);
    true
}

/// Mutable equivalent of `get_ffon_at_id` — walk to the parent level of `id`.
///
/// Returns a mutable slice of siblings at depth `id.depth - 1`.
pub(crate) fn get_ffon_at_id_mut<'a>(
    ffon: &'a mut Vec<sicompass_sdk::ffon::FfonElement>,
    id: &sicompass_sdk::ffon::IdArray,
) -> Option<&'a mut Vec<sicompass_sdk::ffon::FfonElement>> {
    use sicompass_sdk::ffon::FfonElement;
    let depth = id.depth();
    if depth == 0 { return None; }
    if depth == 1 {
        return Some(ffon);
    }
    // Walk down to depth-1 level
    let mut current: &mut Vec<FfonElement> = ffon;
    for level in 0..depth - 1 {
        let idx = id.get(level)?;
        // Need to go into children of the element at `idx`
        let elem = current.get_mut(idx)?;
        current = match elem {
            FfonElement::Obj(o) => &mut o.children,
            FfonElement::Str(_) => return None,
        };
    }
    Some(current)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sicompass_sdk::ffon::{FfonElement, IdArray};
    use sicompass_sdk::provider::Provider;

    /// Minimal provider for testing.
    struct MockProvider {
        name: String,
        path: String,
        items: Vec<FfonElement>,
        commit_ok: bool,
        create_dir_ok: bool,
        create_file_ok: bool,
        delete_ok: bool,
        execute_ok: bool,
        cmds: Vec<String>,
        last_commit: Option<(String, String)>,
        last_create_dir: Option<String>,
        last_create_file: Option<String>,
        last_delete: Option<String>,
        last_execute: Option<(String, String)>,
    }

    impl MockProvider {
        fn new(name: &str, items: Vec<FfonElement>) -> Self {
            MockProvider {
                name: name.to_owned(),
                path: "/".to_owned(),
                items,
                commit_ok: true,
                create_dir_ok: true,
                create_file_ok: true,
                delete_ok: true,
                execute_ok: true,
                cmds: vec![],
                last_commit: None,
                last_create_dir: None,
                last_create_file: None,
                last_delete: None,
                last_execute: None,
            }
        }
    }

    impl Provider for MockProvider {
        fn name(&self) -> &str { &self.name }
        fn fetch(&mut self) -> Vec<FfonElement> { self.items.clone() }
        fn push_path(&mut self, seg: &str) {
            if self.path == "/" { self.path = format!("/{seg}"); }
            else { self.path.push('/'); self.path.push_str(seg); }
        }
        fn pop_path(&mut self) {
            if let Some(s) = self.path.rfind('/') {
                if s == 0 { self.path = "/".to_owned(); } else { self.path.truncate(s); }
            }
        }
        fn current_path(&self) -> &str { &self.path }
        fn commit_edit(&mut self, old: &str, new: &str) -> bool {
            self.last_commit = Some((old.to_owned(), new.to_owned()));
            self.commit_ok
        }
        fn create_directory(&mut self, name: &str) -> bool {
            self.last_create_dir = Some(name.to_owned());
            self.create_dir_ok
        }
        fn create_file(&mut self, name: &str) -> bool {
            self.last_create_file = Some(name.to_owned());
            self.create_file_ok
        }
        fn delete_item(&mut self, name: &str) -> bool {
            self.last_delete = Some(name.to_owned());
            self.delete_ok
        }
        fn commands(&self) -> Vec<String> { self.cmds.clone() }
        fn execute_command(&mut self, cmd: &str, sel: &str) -> bool {
            self.last_execute = Some((cmd.to_owned(), sel.to_owned()));
            self.execute_ok
        }
    }

    fn make_renderer_with_provider(p: MockProvider) -> AppRenderer {
        let mut r = AppRenderer::new();
        // Build a minimal FFON tree mirroring what filebrowser would set up
        let mut root = FfonElement::new_obj(p.name().to_owned());
        for item in p.items.iter() { root.as_obj_mut().unwrap().push(item.clone()); }
        r.ffon = vec![root];
        r.providers = vec![Box::new(p)];
        r.current_id = { let mut id = IdArray::new(); id.push(0); id };
        r
    }

    // --- active_provider_index ---

    #[test]
    fn active_provider_index_returns_first_id() {
        let p = MockProvider::new("test", vec![]);
        let r = make_renderer_with_provider(p);
        assert_eq!(active_provider_index(&r), Some(0));
    }

    #[test]
    fn active_provider_index_second_provider() {
        let p = MockProvider::new("test", vec![]);
        let mut r = make_renderer_with_provider(p);
        // Manually set id to point to index 0 — already done by make_renderer_with_provider
        assert_eq!(active_provider_index(&r), Some(0));
        // Change to index 1 (even if no provider there)
        r.current_id.set_last(1);
        assert_eq!(active_provider_index(&r), Some(1));
    }

    // --- get_active_provider_ref ---

    #[test]
    fn get_active_provider_ref_returns_name() {
        let p = MockProvider::new("myprovider", vec![]);
        let r = make_renderer_with_provider(p);
        let name = get_active_provider_ref(&r).map(|p| p.name().to_owned());
        assert_eq!(name, Some("myprovider".to_owned()));
    }

    #[test]
    fn get_active_provider_ref_none_when_no_providers() {
        let mut r = AppRenderer::new();
        r.current_id = { let mut id = IdArray::new(); id.push(0); id };
        assert!(get_active_provider_ref(&r).is_none());
    }

    // --- push_path / pop_path / current_path ---

    #[test]
    fn push_path_updates_provider_path() {
        let p = MockProvider::new("test", vec![]);
        let mut r = make_renderer_with_provider(p);
        push_path(&mut r, "subdir");
        assert_eq!(current_path(&r), "/subdir");
    }

    #[test]
    fn pop_path_restores_root() {
        let p = MockProvider::new("test", vec![]);
        let mut r = make_renderer_with_provider(p);
        push_path(&mut r, "subdir");
        pop_path(&mut r);
        assert_eq!(current_path(&r), "/");
    }

    #[test]
    fn current_path_default_is_slash() {
        let p = MockProvider::new("test", vec![]);
        let r = make_renderer_with_provider(p);
        assert_eq!(current_path(&r), "/");
    }

    #[test]
    fn current_path_returns_slash_when_no_provider() {
        let r = AppRenderer::new();
        assert_eq!(current_path(&r), "/");
    }

    // --- commit_edit dispatch ---

    #[test]
    fn commit_edit_dispatches_to_provider() {
        let p = MockProvider::new("test", vec![]);
        let mut r = make_renderer_with_provider(p);
        // MockProvider.commit_ok defaults to true
        let ok = commit_edit(&mut r, "old", "new value");
        assert!(ok);
    }

    #[test]
    fn commit_edit_returns_false_when_no_provider() {
        let mut r = AppRenderer::new();
        r.current_id = { let mut id = IdArray::new(); id.push(0); id };
        assert!(!commit_edit(&mut r, "old", "new"));
    }

    // --- create_directory dispatch ---

    #[test]
    fn create_directory_dispatches_to_provider() {
        let p = MockProvider::new("test", vec![]);
        let mut r = make_renderer_with_provider(p);
        let ok = create_directory(&mut r, "newdir");
        assert!(ok);
    }

    #[test]
    fn create_directory_returns_false_when_no_provider() {
        let mut r = AppRenderer::new();
        r.current_id = { let mut id = IdArray::new(); id.push(0); id };
        assert!(!create_directory(&mut r, "dir"));
    }

    // --- navigate_left (pop_path equivalent) ---

    #[test]
    fn pop_path_at_root_stays_at_root() {
        let p = MockProvider::new("test", vec![]);
        let mut r = make_renderer_with_provider(p);
        // Already at root "/"
        pop_path(&mut r);
        assert_eq!(current_path(&r), "/");
    }

    #[test]
    fn pop_path_after_push_restores_parent() {
        let p = MockProvider::new("test", vec![]);
        let mut r = make_renderer_with_provider(p);
        push_path(&mut r, "inbox");
        push_path(&mut r, "subfolder");
        pop_path(&mut r);
        assert_eq!(current_path(&r), "/inbox");
    }

    // --- multiple providers ---

    #[test]
    fn get_active_provider_selects_correct_index() {
        let p0 = MockProvider::new("alpha", vec![]);
        let p1 = MockProvider::new("beta", vec![]);
        let mut r = AppRenderer::new();
        r.ffon = vec![
            FfonElement::new_obj("alpha"),
            FfonElement::new_obj("beta"),
        ];
        r.providers = vec![Box::new(p0), Box::new(p1)];
        // Select provider at index 1
        r.current_id = { let mut id = IdArray::new(); id.push(1); id };
        let name = get_active_provider_ref(&r).map(|p| p.name().to_owned());
        assert_eq!(name, Some("beta".to_owned()));
    }

    #[test]
    fn get_active_provider_out_of_bounds_returns_none() {
        let p = MockProvider::new("only", vec![]);
        let mut r = make_renderer_with_provider(p);
        r.current_id.set_last(99); // out of bounds
        assert!(get_active_provider_ref(&r).is_none());
    }

    // --- create_file dispatch ---

    #[test]
    fn create_file_dispatches_to_provider() {
        let p = MockProvider::new("test", vec![]);
        let mut r = make_renderer_with_provider(p);
        assert!(create_file(&mut r, "newfile.txt"));
    }

    #[test]
    fn create_file_returns_false_when_no_provider() {
        let mut r = AppRenderer::new();
        r.current_id = { let mut id = IdArray::new(); id.push(0); id };
        assert!(!create_file(&mut r, "f.txt"));
    }

    // --- delete_item_by_name dispatch ---

    #[test]
    fn delete_item_by_name_dispatches_to_provider() {
        let p = MockProvider::new("test", vec![]);
        let mut r = make_renderer_with_provider(p);
        assert!(delete_item_by_name(&mut r, "old.txt"));
    }

    #[test]
    fn delete_item_by_name_returns_false_when_no_provider() {
        let mut r = AppRenderer::new();
        r.current_id = { let mut id = IdArray::new(); id.push(0); id };
        assert!(!delete_item_by_name(&mut r, "f.txt"));
    }

    // --- get_commands dispatch ---

    #[test]
    fn get_commands_dispatches_to_provider() {
        let mut p = MockProvider::new("test", vec![]);
        p.cmds = vec!["open".to_string(), "rename".to_string()];
        let r = make_renderer_with_provider(p);
        let cmds = get_commands(&r);
        assert_eq!(cmds, vec!["open", "rename"]);
    }

    #[test]
    fn get_commands_returns_empty_when_no_provider() {
        let r = AppRenderer::new();
        assert!(get_commands(&r).is_empty());
    }

    // --- execute_command dispatch ---

    #[test]
    fn execute_command_dispatches_to_provider() {
        let p = MockProvider::new("test", vec![]);
        let mut r = make_renderer_with_provider(p);
        assert!(execute_command(&mut r, "open", "file.txt"));
    }

    #[test]
    fn execute_command_returns_false_when_no_provider() {
        let mut r = AppRenderer::new();
        r.current_id = { let mut id = IdArray::new(); id.push(0); id };
        assert!(!execute_command(&mut r, "open", "f"));
    }

    // --- auth registry ---

    #[test]
    fn register_auth_and_find() {
        clear_auth_registry();
        register_auth("https://example.com", "secret123");
        let key = find_api_key_for_url("https://example.com/api/data");
        assert_eq!(key.as_deref(), Some("secret123"));
        clear_auth_registry();
    }

    #[test]
    fn find_api_key_no_match() {
        clear_auth_registry();
        register_auth("https://example.com", "secret");
        assert!(find_api_key_for_url("https://other.com/foo").is_none());
        clear_auth_registry();
    }

    #[test]
    fn register_auth_multiple() {
        clear_auth_registry();
        register_auth("https://a.com", "key_a");
        register_auth("https://b.com", "key_b");
        assert_eq!(find_api_key_for_url("https://a.com/path").as_deref(), Some("key_a"));
        assert_eq!(find_api_key_for_url("https://b.com/path").as_deref(), Some("key_b"));
        clear_auth_registry();
    }

    #[test]
    fn register_auth_prefix_match() {
        clear_auth_registry();
        register_auth("https://api.example.com", "bearer_token");
        assert!(find_api_key_for_url("https://api.example.com/v1/data").is_some());
        assert!(find_api_key_for_url("https://example.com/v1/data").is_none());
        clear_auth_registry();
    }
}
