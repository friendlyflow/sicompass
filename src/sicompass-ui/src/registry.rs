//! Putting providers into a renderer, and asking the embedder for the things
//! only it knows.
//!
//! Two unrelated jobs share this file because both are the seam between the
//! renderer and whoever is driving it.

use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::provider::Provider;
use std::sync::{Arc, Mutex};

use crate::app_state::AppRenderer;

/// The default interface scale, used when nobody says otherwise.
///
/// Lives here rather than in the app's settings code because the renderer has
/// to have an answer even when there is no settings file — a greeter has none.
pub const DEFAULT_FONT_SCALE: f32 = 1.75;

/// Queue of `(key, value)` settings edits, filled by a provider's apply
/// callback and drained by the embedder.
///
/// The renderer only carries it; it never looks inside.
pub type SettingsQueue = Arc<Mutex<Vec<(String, String)>>>;

// ---------------------------------------------------------------------------
// HostHooks
// ---------------------------------------------------------------------------

/// The handful of things the render loop needs that only the embedder can
/// answer.
///
/// Every method has a do-nothing default, and those defaults are exactly right
/// for an embedder with no settings file, no updater and no tabs — which is
/// what a login screen is. The application overrides all of them.
///
/// This exists to break a dependency cycle: the app's `programs` module drives
/// providers, settings and plugins, and the render loop needs to call into it
/// at six points. Rather than move `programs` down here (it would drag the
/// provider graph with it) the calls are inverted.
pub trait HostHooks: Send {
    /// Drain the settings queue and apply what is in it. `initial` is true for
    /// the one call made during startup, where `enable_*` keys are skipped
    /// because providers were loaded already.
    fn apply_pending_settings(&self, _renderer: &mut AppRenderer, _initial: bool) {}

    /// Move the background updater's progress into the renderer's state.
    fn process_update_events(&self, _renderer: &mut AppRenderer) {}

    /// The user asked to install a staged application update.
    fn handle_apply_app_update(&self, _renderer: &mut AppRenderer) {}

    /// Build a fresh set of providers for a new tab, by provider name.
    ///
    /// The default returns nothing, which is the honest answer for an embedder
    /// that has a fixed provider list and no way to instantiate more.
    fn build_content_set(
        &self,
        _renderer: &mut AppRenderer,
        _names: &[String],
    ) -> (Vec<Box<dyn Provider>>, Vec<FfonElement>) {
        (Vec::new(), Vec::new())
    }

    /// Remember that the window was maximised or restored.
    fn write_maximized(&self, _maximized: bool) {}

    /// The current interface scale, re-read after a settings change.
    fn read_font_scale(&self) -> f32 {
        DEFAULT_FONT_SCALE
    }

    /// Asked once per frame: should the main loop stop?
    ///
    /// The application never says yes — it closes when its window does. The
    /// greeter says yes once greetd has accepted `start_session`, because
    /// greetd only launches the session after the greeter process exits.
    fn should_quit(&self) -> bool {
        false
    }
}

/// The defaults, for an embedder that has no opinions.
pub struct NoHooks;

impl HostHooks for NoHooks {}

// ---------------------------------------------------------------------------
// Provider registration
// ---------------------------------------------------------------------------

/// `init()` a provider, build its FFON root and add it to the renderer.
pub fn register_provider(renderer: &mut AppRenderer, provider: Box<dyn Provider>) {
    let (provider, root) = init_provider_root(provider, &mut renderer.error_message);
    renderer.ffon.push(root);
    renderer.providers.push(provider);
}

/// `init()` a provider and build its FFON root Obj (key = display name,
/// children = first `fetch()`). Any fetch error is surfaced through `err_sink`.
/// Shared by [`register_provider`] and the per-tab content rebuild.
pub fn init_provider_root(
    mut provider: Box<dyn Provider>,
    err_sink: &mut String,
) -> (Box<dyn Provider>, FfonElement) {
    provider.init();
    let children = provider.fetch();
    if let Some(err) = provider.take_error() {
        eprintln!(
            "provider '{}' fetch error on register: {err}",
            provider.display_name()
        );
        *err_sink = err;
    }
    let display_name = provider.display_name().to_owned();
    let mut root = FfonElement::new_obj(&display_name);
    for child in children {
        root.as_obj_mut().unwrap().push(child);
    }
    (provider, root)
}
