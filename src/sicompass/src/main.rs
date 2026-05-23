// All modules are declared in lib.rs; the binary just re-uses them.
use sicompass::app_state;
use sicompass::render;
use std::process;
use std::sync::{Arc, Mutex};
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

/// GitHub owner/repo for the self-update check. Derived from the
/// `repository = "https://github.com/<owner>/<repo>"` URL in
/// `src/sicompass/Cargo.toml` (kept here as a literal so the updater
/// thread doesn't need to parse Cargo metadata at runtime).
const GITHUB_OWNER: &str = "friendlyflow";
const GITHUB_REPO: &str = "sicompass";

fn main() {
    // Register all built-in providers with the SDK factory and manifest registries.
    // Must happen before load_programs() so create_provider_by_name() resolves them.
    sicompass_builtins::register_all();

    // Set up dual logging: always write debug to a log file, optionally stderr via RUST_LOG.
    let log_dir = sicompass_sdk::platform::log_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    sicompass_sdk::platform::make_dirs(&log_dir);
    let file_appender = tracing_appender::rolling::daily(&log_dir, "sicompass.log");
    let file_layer = fmt::layer()
        .with_writer(file_appender)
        .with_ansi(false)
        .with_filter(EnvFilter::new("debug"));
    let stderr_layer = fmt::layer()
        .with_filter(EnvFilter::from_default_env());
    tracing_subscriber::registry()
        .with(file_layer)
        .with(stderr_layer)
        .init();

    if std::env::args().any(|a| a == "--check") {
        process::exit(render::check_runtime_files());
    }

    // ---- Background self-update check ------------------------------------
    // Spawn before AppState::new() so the check runs concurrently with
    // window/Vulkan setup. The status Arc is wired into AppRenderer below.
    // Anything that fails (no network, no plugins dir, malformed manifest)
    // is logged and swallowed — startup never blocks.
    let auto_update_enabled = read_auto_update_check_setting();
    let (update_state, update_rx) = if auto_update_enabled {
        let state = Arc::new(Mutex::new(sicompass_updater::UpdateStatus::default()));
        let (tx, rx) = std::sync::mpsc::channel();
        let state_for_thread = Arc::clone(&state);
        let plugins_dir = sicompass_sdk::platform::plugins_dir().unwrap_or_default();
        let app_version = sicompass_updater::parse_version(env!("CARGO_PKG_VERSION"))
            .unwrap_or_else(|_| semver::Version::new(0, 0, 0));
        let _ = std::thread::Builder::new()
            .name("sicompass-updater".into())
            .spawn(move || {
                let checker = sicompass_updater::UpdateChecker::new(
                    app_version,
                    plugins_dir,
                    GITHUB_OWNER,
                    GITHUB_REPO,
                )
                .with_event_sender(tx);
                let result = checker.check_and_stage();
                *state_for_thread.lock().unwrap() = result;
            });
        (Some(state), Some(rx))
    } else {
        (None, None)
    };

    match app_state::AppState::new() {
        Ok(mut app) => {
            app.renderer.update_state = update_state;
            app.renderer.update_event_rx = update_rx;
            app.run()
        }
        Err(e) => {
            eprintln!("sicompass: {e}");
            process::exit(1);
        }
    }
}

/// Read `sicompass.autoUpdateCheck` from settings.json. Defaults to `true`
/// when the file or key is absent so updates work out of the box; users
/// can disable via the settings UI checkbox.
fn read_auto_update_check_setting() -> bool {
    let Some(path) = sicompass_sdk::platform::main_config_path() else {
        return true;
    };
    let Ok(data) = std::fs::read_to_string(&path) else {
        return true;
    };
    let Ok(root) = serde_json::from_str::<serde_json::Value>(&data) else {
        return true;
    };
    root.get("sicompass")
        .and_then(|v| v.as_object())
        .and_then(|sc| sc.get("autoUpdateCheck"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}
