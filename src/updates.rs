//! The updates ready to install, in one message: the app's own (which brings
//! its built-in programs and the renderer with it) and the plugins'.
//!
//! A background thread at startup fills [`UpdateState`] (`main.rs`), gated by
//! the `autoUpdateCheck` setting. The render loop calls
//! [`process_update_events`] every frame, which keeps the banner up to date,
//! and Ctrl+U calls [`handle_apply_updates`], which installs:
//!
//! - every plugin update that asks for nothing new, on a thread of its own,
//!   the way the Store's Update button does (`sicompass_builtins::plugin_updates`).
//!   The app follows the plugins folder, so each one is reloaded without a
//!   restart. An update that asks for more access is only named, and left to
//!   the Store, which shows what it asks for;
//! - then the app's own, which on Windows runs the installer and exits, so it
//!   waits for the plugins.
//!
//! desicompass and the login screen are not here: they arrive with
//! `nixos-rebuild`. Neither is the app itself where Nix installed it
//! ([`app_updates_itself`]).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sicompass_builtins::plugin_updates::{self, PendingUpdate};
use sicompass_sdk::localize;
use sicompass_ui::app_state::AppRenderer;

/// What the startup check found, and what Ctrl+U is doing about it.
#[derive(Default)]
pub struct UpdateState {
    pub app: sicompass_updater::UpdateStatus,
    pub plugins: Vec<PendingUpdate>,
    /// The plugin updates are being installed.
    pub installing: bool,
    /// Apply the app's update once the plugins are installed.
    pub apply_app_after_plugins: bool,
    /// What the install did, one line per plugin, shown once.
    pub results: Vec<String>,
    /// When [`forget_installed`] last looked at the plugins folder.
    checked: Option<Instant>,
}

pub type SharedUpdates = Arc<Mutex<UpdateState>>;

impl UpdateState {
    /// The plugin updates Ctrl+U installs.
    fn installable(&self) -> impl Iterator<Item = &PendingUpdate> {
        self.plugins.iter().filter(|p| !p.needs_approval)
    }

    /// Whether Ctrl+U has something to do.
    pub fn pending(&self) -> bool {
        !self.installing && (self.app.app_update.is_some() || self.installable().next().is_some())
    }
}

fn lock(state: &SharedUpdates) -> std::sync::MutexGuard<'_, UpdateState> {
    state.lock().unwrap_or_else(|e| e.into_inner())
}

/// Whether the app looks for its own updates. Not in a desicompass session,
/// and not where Nix installed it: there an update is a new flake pin and a
/// rebuild, and a release page would only mislead.
pub fn app_updates_itself() -> bool {
    !sicompass_ui::session_mode::is_session_mode()
        && !std::env::current_exe().is_ok_and(|exe| exe.starts_with("/nix/store"))
}

/// The startup check: the app's update (when it updates itself), then the
/// plugins'. Each is written as soon as it is known. Downloads, so it runs on
/// its own thread.
pub fn check(state: &SharedUpdates, app: Option<sicompass_updater::UpdateChecker>) {
    if let Some(checker) = app {
        let status = checker.check_and_stage();
        for e in &status.errors {
            tracing::warn!("update check: {e}");
        }
        lock(state).app = status;
    }
    let plugins = plugin_updates::check();
    for p in &plugins {
        tracing::info!(
            "update check: {} {} -> {}{}",
            p.name,
            p.from,
            p.to,
            if p.needs_approval {
                " (asks for more access)"
            } else {
                ""
            }
        );
    }
    lock(state).plugins = plugins;
}

/// The banner for `state`, or `None` when there is nothing to say.
pub fn banner(state: &UpdateState) -> Option<String> {
    let installable = state.installable().count();
    banner_for(
        state.installing,
        state
            .app
            .app_update
            .as_ref()
            .map(|a| a.new_version.to_string()),
        installable,
        state.plugins.len() - installable,
    )
}

/// [`banner`], from what it says: whether plugins are being installed, the
/// app's new version, and how many plugin updates Ctrl+U installs and how
/// many wait for an approval in the Store.
fn banner_for(
    installing: bool,
    app_version: Option<String>,
    installable: usize,
    approval: usize,
) -> Option<String> {
    let count = |n: usize| {
        let mut args = localize::Args::new();
        args.set("count", n as i64);
        args
    };
    if installing {
        return Some(localize::t_args("updates-installing", &count(installable)));
    }
    let mut ready = Vec::new();
    if let Some(version) = app_version {
        let mut args = localize::Args::new();
        args.set("version", version);
        ready.push(localize::t_args("updates-app", &args));
    }
    if installable > 0 {
        ready.push(localize::t_args("updates-programs", &count(installable)));
    }
    let approval =
        (approval > 0).then(|| localize::t_args("updates-need-approval", &count(approval)));
    if ready.is_empty() {
        return approval;
    }
    let mut args = localize::Args::new();
    args.set("list", ready.join(", "));
    let mut msg = localize::t_args("updates-ready", &args);
    if let Some(approval) = approval {
        msg.push_str(". ");
        msg.push_str(&approval);
    }
    Some(msg)
}

/// How often [`forget_installed`] looks, while plugin updates are pending.
const INSTALLED_CHECK_INTERVAL: Duration = Duration::from_secs(2);

/// Forget the plugin updates that were installed some other way (the Store,
/// in this process or the superkey's): their plugin is no longer the version
/// the update was found for.
fn forget_installed(state: &mut UpdateState) {
    if state.plugins.is_empty() || state.installing {
        return;
    }
    if state
        .checked
        .is_some_and(|t| t.elapsed() < INSTALLED_CHECK_INTERVAL)
    {
        return;
    }
    state.checked = Some(Instant::now());
    let versions: std::collections::HashMap<String, Option<String>> =
        sicompass_sdk::installed_plugins::discover_all()
            .into_iter()
            .filter_map(|(_, _, m)| m.ok())
            .map(|m| (m.name, m.version))
            .collect();
    state.plugins.retain(|p| {
        versions
            .get(&p.name)
            .is_some_and(|v| v.as_deref().unwrap_or_default() == p.from)
    });
}

/// Refresh the banner from the update state. Called once per frame from the
/// main loop. Never panics.
///
/// FUTURE NOTIFICATION SYSTEM: when sicompass grows a real in-app
/// notification surface, replace the `error_message` writes below with
/// calls to the new notification API. The update state upstream and the
/// Ctrl+U keybind downstream stay unchanged, only this rendering site moves.
/// Grep for "FUTURE NOTIFICATION SYSTEM" to find all interim shims.
///
/// The state is passed in rather than read off the renderer: it holds
/// `sicompass-updater` and Store types, and the renderer is shared with a
/// greeter that must link neither.
pub fn process_update_events(renderer: &mut AppRenderer, update_state: Option<&SharedUpdates>) {
    let Some(state) = update_state else {
        renderer.app_update_pending = false;
        return;
    };
    let mut s = lock(state);
    forget_installed(&mut s);
    // What `shortcuts` reads to decide whether to advertise Ctrl+U.
    renderer.app_update_pending = s.pending();

    // The error slot is borrowed only when it is empty or holds our own
    // previous banner, so real provider errors never get clobbered.
    let can_write = renderer.error_message.is_empty() || renderer.update_message_active;

    // The plugins are in: the app's turn (on Windows this process ends here).
    if !s.installing && s.apply_app_after_plugins {
        s.apply_app_after_plugins = false;
        if let Some(app) = s.app.app_update.clone() {
            drop(s);
            let said = apply_app(&app);
            s = lock(state);
            s.results.extend(said);
        }
    }

    // Say how it went, once.
    if !s.installing && !s.results.is_empty() && can_write {
        // Shown until the user moves on, like any message: not the banner.
        renderer.error_message = std::mem::take(&mut s.results).join(" ");
        renderer.update_message_active = false;
        return;
    }

    let Some(msg) = banner(&s) else {
        if renderer.update_message_active {
            renderer.error_message.clear();
            renderer.update_message_active = false;
        }
        return;
    };
    if can_write {
        renderer.error_message = msg;
        renderer.update_message_active = true;
    }
}

/// Ctrl+U: install every plugin update that asks for nothing new, then the
/// app's update. Failures are said in the message slot.
pub fn handle_apply_updates(renderer: &mut AppRenderer, update_state: Option<&SharedUpdates>) {
    let Some(state) = update_state else {
        return;
    };
    let mut s = lock(state);
    if s.installing {
        return;
    }
    let todo: Vec<PendingUpdate> = s.installable().cloned().collect();
    let app = s.app.app_update.clone();
    if todo.is_empty() {
        drop(s);
        renderer.update_message_active = false;
        renderer.error_message = match app {
            Some(app) => apply_app(&app).unwrap_or_default(),
            None => localize::t("updates-none"),
        };
        return;
    }
    s.installing = true;
    s.apply_app_after_plugins = app.is_some();
    drop(s);

    let for_thread = Arc::clone(state);
    let spawned = std::thread::Builder::new()
        .name("sicompass-plugin-updates".into())
        .spawn(move || install_plugins(&for_thread, todo));
    if let Err(e) = spawned {
        let mut s = lock(state);
        s.installing = false;
        s.apply_app_after_plugins = false;
        let mut args = localize::Args::new();
        args.set("error", e.to_string());
        renderer.error_message = localize::t_args("updates-apply-failed", &args);
        renderer.update_message_active = false;
    }
}

/// Install `todo` one after another, each result a line in the state.
fn install_plugins(state: &SharedUpdates, todo: Vec<PendingUpdate>) {
    for update in todo {
        let result = plugin_updates::install(&update);
        let mut args = localize::Args::new();
        args.set("name", update.name.clone());
        let line = match result {
            Ok(version) => {
                tracing::info!("updated {} to {version}", update.name);
                args.set("version", version);
                localize::t_args("updates-installed", &args)
            }
            Err(e) => {
                tracing::warn!("could not update {}: {e}", update.name);
                args.set("error", e);
                localize::t_args("updates-failed", &args)
            }
        };
        let mut s = lock(state);
        s.plugins.retain(|p| p.name != update.name);
        s.results.push(line);
    }
    lock(state).installing = false;
}

/// Apply the app's update: on Windows the installer runs and this process
/// exits; elsewhere the release page opens. What to say about it, if anything.
fn apply_app(app_update: &sicompass_updater::AppUpdate) -> Option<String> {
    let checker = sicompass_updater::UpdateChecker::new(
        sicompass_updater::parse_version(env!("CARGO_PKG_VERSION"))
            .unwrap_or_else(|_| semver::Version::new(0, 0, 0)),
        "",
        "",
    );
    match checker.apply_app_update(app_update) {
        // Unreachable on Windows (it exits inside apply_app_update), and
        // elsewhere it returns Unsupported.
        Ok(()) => None,
        Err(e) if e.kind() == std::io::ErrorKind::Unsupported => {
            sicompass_sdk::platform::open_with_default(&app_update.release_url);
            Some(localize::t("updates-release-page-opened"))
        }
        Err(e) => {
            let mut args = localize::Args::new();
            args.set("error", e.to_string());
            Some(localize::t_args("updates-apply-failed", &args))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_update(version: &str) -> sicompass_updater::AppUpdate {
        sicompass_updater::AppUpdate {
            new_version: semver::Version::parse(version).unwrap(),
            staged_installer_path: None,
            release_url: String::new(),
        }
    }

    fn with_app(version: &str) -> UpdateState {
        let mut s = UpdateState::default();
        s.app.app_update = Some(app_update(version));
        s
    }

    fn en() {
        sicompass_settings::register_translations();
        localize::set_locale("en-US");
    }

    #[test]
    fn nothing_found_says_nothing() {
        en();
        assert_eq!(banner(&UpdateState::default()), None);
        assert!(!UpdateState::default().pending());
    }

    #[test]
    fn the_app_update_names_its_version() {
        en();
        let s = with_app("9.1.0");
        assert_eq!(
            banner(&s).as_deref(),
            Some("Updates ready: sicompass 9.1.0 (Ctrl+U to install)")
        );
        assert!(s.pending());
    }

    #[test]
    fn plugin_updates_are_counted_beside_the_app() {
        en();
        assert_eq!(
            banner_for(false, None, 1, 0).as_deref(),
            Some("Updates ready: 1 program (Ctrl+U to install)")
        );
        assert_eq!(
            banner_for(false, Some("9.1.0".into()), 3, 0).as_deref(),
            Some("Updates ready: sicompass 9.1.0, 3 programs (Ctrl+U to install)")
        );
    }

    #[test]
    fn an_update_asking_for_more_access_points_to_the_store() {
        en();
        assert_eq!(
            banner_for(false, None, 0, 1).as_deref(),
            Some("1 program update asks for more access: approve it in the store")
        );
        assert_eq!(
            banner_for(false, None, 2, 2).as_deref(),
            Some(
                "Updates ready: 2 programs (Ctrl+U to install). \
                 2 program updates ask for more access: approve them in the store"
            )
        );
    }

    #[test]
    fn while_installing_ctrl_u_has_nothing_to_do() {
        en();
        let mut s = with_app("9.1.0");
        s.installing = true;
        assert!(!s.pending());
        assert_eq!(
            banner_for(true, None, 2, 0).as_deref(),
            Some("Installing 2 program updates…")
        );
    }

    fn shared(s: UpdateState) -> SharedUpdates {
        Arc::new(Mutex::new(s))
    }

    #[test]
    fn the_banner_never_takes_the_place_of_a_real_error() {
        en();
        let state = shared(with_app("9.1.0"));
        let mut r = AppRenderer::new();
        r.error_message = "something broke".into();
        process_update_events(&mut r, Some(&state));
        assert_eq!(r.error_message, "something broke");
        assert!(r.app_update_pending, "Ctrl+U is still offered");

        // Once the error is gone, the banner shows, and goes when there is
        // nothing left to say.
        r.error_message.clear();
        process_update_events(&mut r, Some(&state));
        assert!(
            r.error_message.starts_with("Updates ready"),
            "{}",
            r.error_message
        );
        state.lock().unwrap().app.app_update = None;
        process_update_events(&mut r, Some(&state));
        assert_eq!(r.error_message, "");
        assert!(!r.app_update_pending);
    }

    #[test]
    fn the_results_of_an_install_are_said_once_after_it_finished() {
        en();
        let state = shared(UpdateState {
            installing: true,
            ..Default::default()
        });
        let mut r = AppRenderer::new();
        // A line from the install thread, while it runs: not yet.
        state
            .lock()
            .unwrap()
            .results
            .push("notes is updated to 1.1.0.".into());
        process_update_events(&mut r, Some(&state));
        assert!(
            r.error_message.starts_with("Installing"),
            "{}",
            r.error_message
        );

        state.lock().unwrap().installing = false;
        process_update_events(&mut r, Some(&state));
        assert_eq!(r.error_message, "notes is updated to 1.1.0.");
        // Said, and kept until the user moves on, not as the banner.
        assert!(!r.update_message_active);
        assert!(state.lock().unwrap().results.is_empty());
        process_update_events(&mut r, Some(&state));
        assert_eq!(r.error_message, "notes is updated to 1.1.0.");
    }

    #[test]
    fn ctrl_u_with_nothing_to_install_says_so() {
        en();
        let state = shared(UpdateState::default());
        let mut r = AppRenderer::new();
        handle_apply_updates(&mut r, Some(&state));
        assert_eq!(r.error_message, "No updates to install.");
    }

    #[test]
    fn every_update_message_is_in_every_locale() {
        let keys = |src: &str| -> Vec<String> {
            src.lines()
                .filter(|l| !l.starts_with(char::is_whitespace))
                .filter_map(|l| l.split_once('=').map(|(k, _)| k.trim().to_owned()))
                .filter(|k| k.starts_with("updates-"))
                .collect()
        };
        let en = keys(include_str!("../lib/lib_settings/locales/en-US.ftl"));
        assert!(en.len() >= 9, "{en:?}");
        for (lang, src) in [
            (
                "nl-BE",
                include_str!("../lib/lib_settings/locales/nl-BE.ftl"),
            ),
            (
                "fr-BE",
                include_str!("../lib/lib_settings/locales/fr-BE.ftl"),
            ),
            (
                "de-BE",
                include_str!("../lib/lib_settings/locales/de-BE.ftl"),
            ),
        ] {
            assert_eq!(keys(src), en, "{lang}");
        }
    }
}
