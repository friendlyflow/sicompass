//! Provider loading — equivalent to `programs.c`.
//!
//! Reads the settings config, determines which providers are enabled, and
//! registers them in the `AppRenderer`.

use crate::app_state::AppRenderer;
use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::provider::Provider;
use sicompass_tutorial::TutorialProvider;

// ---------------------------------------------------------------------------
// Built-in provider names
// ---------------------------------------------------------------------------

/// Programs enabled by default when no config exists.
const DEFAULT_PROGRAMS: &[&str] = &["tutorial"];

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Register a `Box<dyn Provider>` into the renderer.
///
/// Calls `init()`, fetches the initial tree, and creates the root
/// `FfonElement::Obj` used by the navigation system.
pub fn register_provider(renderer: &mut AppRenderer, mut provider: Box<dyn Provider>) {
    provider.init();
    let children = provider.fetch();
    let display_name = provider.display_name().to_owned();

    let mut root = FfonElement::new_obj(&display_name);
    for child in children {
        root.as_obj_mut().unwrap().push(child);
    }

    renderer.ffon.push(root);
    renderer.providers.push(provider);
}

/// Load providers according to the settings config.
pub fn load_programs(renderer: &mut AppRenderer) {
    let enabled = enabled_programs();

    for name in &enabled {
        match name.as_str() {
            "tutorial" => {
                register_provider(renderer, Box::new(TutorialProvider::new_headless()));
            }
            other => {
                eprintln!("sicompass: unknown program '{other}' — skipping");
            }
        }
    }
}

/// Enable a provider by name at runtime (hot-load).
pub fn enable_provider(renderer: &mut AppRenderer, name: &str) {
    match name {
        "tutorial" => {
            register_provider(renderer, Box::new(TutorialProvider::new_headless()));
        }
        other => {
            eprintln!("sicompass: cannot enable unknown provider '{other}'");
        }
    }
}

/// Disable and remove a provider by name.
pub fn disable_provider(renderer: &mut AppRenderer, name: &str) {
    let Some(idx) = renderer.providers.iter().position(|p| p.name() == name) else {
        return;
    };

    renderer.ffon.remove(idx);
    renderer.providers.remove(idx);

    // Clamp root navigation index
    let max_root = renderer.ffon.len().saturating_sub(1);
    if let Some(root_idx) = renderer.current_id.get(0) {
        if root_idx > max_root {
            renderer.current_id.set_last(max_root);
        }
    }
}

// ---------------------------------------------------------------------------
// Config helpers
// ---------------------------------------------------------------------------

fn enabled_programs() -> Vec<String> {
    if let Some(path) = sicompass_sdk::platform::main_config_path() {
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(root) = serde_json::from_str::<serde_json::Value>(&data) {
                if let Some(section) = root.get("Available programs:") {
                    let all_names = &[
                        "tutorial", "sales demo", "chat client",
                        "email client", "web browser",
                    ];
                    let mut result = Vec::new();
                    for &name in all_names {
                        let key = format!("enable_{name}");
                        if let Some(val) = section.get(&key) {
                            if val.as_bool().unwrap_or(false) {
                                result.push(name.to_owned());
                            }
                        } else if DEFAULT_PROGRAMS.contains(&name) {
                            result.push(name.to_owned());
                        }
                    }
                    if !result.is_empty() {
                        return result;
                    }
                }
            }
        }
    }

    DEFAULT_PROGRAMS.iter().map(|s| s.to_string()).collect()
}
