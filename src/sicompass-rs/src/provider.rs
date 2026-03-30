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
    let display_name = renderer.providers[idx].display_name().to_owned();

    let mut root = sicompass_sdk::ffon::FfonElement::new_obj(&display_name);
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
