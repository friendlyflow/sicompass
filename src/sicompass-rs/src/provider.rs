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

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// Get available commands for the active element from the active provider.
pub fn get_commands(renderer: &AppRenderer) -> Vec<String> {
    get_active_provider_ref(renderer)
        .map(|p| p.commands())
        .unwrap_or_default()
}
