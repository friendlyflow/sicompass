// All modules are declared in lib.rs; the binary just re-uses them.
use sicompass::app_state;
use sicompass::render;
use sicompass::view;
use std::process;

fn main() {
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
