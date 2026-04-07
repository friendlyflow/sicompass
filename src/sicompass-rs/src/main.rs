// All modules are declared in lib.rs; the binary just re-uses them.
use sicompass::app_state;
use sicompass::render;
use sicompass::view;
use std::process;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

fn main() {
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

    match app_state::AppState::new() {
        Ok(mut app) => app.run(),
        Err(e) => {
            eprintln!("sicompass: {e}");
            process::exit(1);
        }
    }
}
