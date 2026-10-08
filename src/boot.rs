//! Application startup: build the renderer's window, then fill it the way an
//! *application* does.
//!
//! This is the half of the old `AppState::new()` that knows about
//! `settings.json`, the provider catalogue, saved tabs and the updater. The
//! other half — window, Vulkan, fonts, AccessKit — is
//! [`sicompass_ui::AppState::init_stack`], which the login greeter uses too.
//!
//! It is a free function rather than an inherent `AppState::new()` because
//! `AppState` now belongs to another crate and Rust's orphan rule does not let
//! this one add inherent methods to it.

use std::sync::{Arc, Mutex};

use sicompass_ui::accessibility::{ScreenReader, SharedAccessibility};
use sicompass_ui::app_state::{AppConfig, AppRenderer, AppState, SiError};
use sicompass_ui::registry::HostHooks;

/// The application's answers to the questions the render loop asks.
///
/// Each method forwards to `programs`, which is where the settings file, the
/// provider catalogue and the updater live. A greeter installs nothing and
/// gets [`sicompass_ui::registry::NoHooks`], whose defaults are all correct
/// for it.
#[derive(Default)]
pub struct ProgramsHooks {
    /// What the startup update check found, the app's and the plugins'
    /// ([`crate::updates`]). `None` when the check is disabled.
    pub update_state: Option<crate::updates::SharedUpdates>,
    /// The session's screen reader, started by the `screenReader` setting.
    /// Shared with the startup drain in [`app_state`]; the last clone is the
    /// renderer's, so it is dropped with the window, and dropping it stops Orca.
    pub screen_reader: Arc<Mutex<ScreenReader>>,
    /// In a desicompass session, the accessibility settings shared with the
    /// superkey (`programs::session_accessibility`). `None` otherwise, and
    /// then settings.json keeps them.
    pub shared_accessibility: Option<Arc<Mutex<SharedAccessibility>>>,
}

impl HostHooks for ProgramsHooks {
    fn apply_pending_settings(&self, renderer: &mut AppRenderer, initial: bool) {
        // The queue lives on AppState, and `view` only hands us the renderer,
        // so the caller has already established there is one to drain. The
        // renderer carries a clone for exactly this reason.
        if let Some(queue) = renderer.settings_queue.clone() {
            crate::programs::apply_pending_settings_with(
                renderer,
                &queue,
                initial,
                Some(&self.screen_reader),
            );
        }
        if let Some(shared) = self.shared_accessibility.as_deref() {
            crate::programs::follow_shared_accessibility(
                renderer,
                shared,
                Some(&self.screen_reader),
            );
        }
        // Plugins installed, updated, removed or approved elsewhere (the
        // superkey's Store in a session, another sicompass).
        if !initial {
            crate::programs::follow_plugins_folder(renderer, false);
        }
        // Called every frame. The session's Orca, once it listens, is handed
        // the focused row at once (see `ScreenReader::take_ready`).
        if self
            .screen_reader
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take_ready()
        {
            renderer.a11y_refocus_now = true;
        }
    }

    fn process_update_events(&self, renderer: &mut AppRenderer) {
        crate::updates::process_update_events(renderer, self.update_state.as_ref());
    }

    fn handle_apply_app_update(&self, renderer: &mut AppRenderer) {
        crate::updates::handle_apply_updates(renderer, self.update_state.as_ref());
    }

    fn build_content_set(
        &self,
        renderer: &mut AppRenderer,
        names: &[String],
    ) -> (
        Vec<Box<dyn sicompass_sdk::provider::Provider>>,
        Vec<sicompass_sdk::ffon::FfonElement>,
    ) {
        crate::programs::build_content_set_from_names(renderer, names)
    }

    fn write_maximized(&self, maximized: bool) {
        crate::programs::write_maximized(maximized);
    }

    fn read_font_scale(&self) -> f32 {
        crate::programs::read_font_scale_with(self.shared_accessibility.as_deref())
    }
}

/// The application's window: everything [`AppConfig`] defaults to, plus the
/// two values that come from `settings.json`.
fn app_config(shared: Option<&Mutex<SharedAccessibility>>) -> AppConfig {
    AppConfig {
        // In session mode the compositor owns the geometry and configures every
        // window itself, so a remembered "maximized" is a second opinion about
        // size that it would immediately override.
        maximized: crate::programs::read_maximized(),
        font_scale: crate::programs::read_font_scale_with(shared),
        ..AppConfig::default()
    }
}

/// Build the application. The old `AppState::new()`, verbatim in what it does.
pub fn app_state(hooks: ProgramsHooks) -> Result<AppState, SiError> {
    let shared = hooks.shared_accessibility.clone();
    let mut state = AppState::init_stack(&app_config(shared.as_deref()))?;

    // First launch = no settings.json yet. Captured before `load_programs`,
    // whose settings-provider `init()` seeds the file (after which it exists).
    let first_run = sicompass_sdk::platform::main_config_path()
        .map(|p| !p.exists())
        .unwrap_or(false);

    let screen_reader = Arc::clone(&hooks.screen_reader);
    state.renderer.hooks = Box::new(hooks);

    // Load providers (tutorial + settings by default)
    let queue = crate::programs::load_programs_with(&mut state.renderer, shared.as_deref());
    // Apply initial settings (providers already loaded above)
    crate::programs::apply_pending_settings_with(
        &mut state.renderer,
        &queue,
        true,
        Some(&screen_reader),
    );
    state.renderer.settings_queue = Some(queue);
    // In a session the accessibility settings have no rows to replay them.
    if let Some(shared) = shared.as_deref() {
        crate::programs::apply_shared_accessibility(
            &mut state.renderer,
            shared,
            Some(&screen_reader),
        );
    }

    // Restore persisted tab layout (no-op if none stored).
    // Must run AFTER providers are loaded so provider-index validation works.
    crate::programs::load_tabs_state(&mut state.renderer);

    // On first run, land the cursor on the onboarding line so a new (screen
    // reader) user is read the onboarding guide immediately. Must run AFTER
    // load_tabs_state, which otherwise resets the cursor to the bootstrap
    // tab's default (the first program in the root list).
    if first_run {
        crate::programs::focus_onboarding(&mut state.renderer);
    }

    sicompass_ui::list::create_list_current_layer(&mut state.renderer);
    Ok(state)
}

/// Install the application's HTTP client for `<link>` following and for
/// `<image>` values that point at a URL.
///
/// The renderer asks through [`sicompass_ui::http`] rather than linking an HTTP
/// client itself, so that the login greeter does not have to carry a TLS stack.
pub fn register_http_client() {
    sicompass_ui::http::register_body_fetcher(|url| {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .map_err(|e| e.to_string())?;
        let resp = client.get(url).send().map_err(|e| e.to_string())?;
        resp.bytes().map(|b| b.to_vec()).map_err(|e| e.to_string())
    });
}
