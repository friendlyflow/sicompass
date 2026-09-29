//! A desicompass session's accessibility settings, end to end through the
//! app's own startup: the settings provider from the factory, the settings
//! queue and `boot::ProgramsHooks`, with the shared object in a temporary
//! directory. A second `SharedAccessibility` on the same file plays the
//! superkey (or the login screen).
//!
//! In a session these settings are not sicompass's to change: its settings
//! page has no rows for them. It starts from the shared values and follows
//! every change made elsewhere.
//!
//! Its own test binary, because it points `XDG_CONFIG_HOME` at a temporary
//! directory for the whole process.

#![cfg(target_os = "linux")]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sicompass::programs;
use sicompass_sdk::ffon::FfonElement;
use sicompass_ui::accessibility::{AccessibilitySettings, POLL_INTERVAL, SharedAccessibility};
use sicompass_ui::app_state::{AppRenderer, PaletteTheme};

fn frame(r: &mut AppRenderer) {
    let hooks = std::mem::replace(&mut r.hooks, Box::new(sicompass_ui::registry::NoHooks));
    hooks.apply_pending_settings(r, false);
    r.hooks = hooks;
}

/// Every row text under `e`, recursively.
fn texts(e: &FfonElement, out: &mut Vec<String>) {
    match e {
        FfonElement::Str(s) => out.push(s.clone()),
        FfonElement::Obj(o) => {
            out.push(o.key.clone());
            for c in &o.children {
                texts(c, out);
            }
        }
    }
}

#[test]
fn sicompass_follows_the_shared_object_and_has_no_rows_for_it() {
    let config = tempfile::tempdir().unwrap();
    // SAFETY: this binary's only test, run before anything reads the variable.
    unsafe { std::env::set_var("XDG_CONFIG_HOME", config.path()) };
    sicompass_builtins::register_all();

    let path = config.path().join("accessibility.json");
    std::fs::write(&path, r#"{"colorScheme":"light"}"#).unwrap();
    let shared = Arc::new(Mutex::new(SharedAccessibility::new(
        Some(path.clone()),
        Vec::new(),
        AccessibilitySettings::builtin(),
    )));

    let mut r = AppRenderer::new();
    r.hooks = Box::new(sicompass::boot::ProgramsHooks {
        shared_accessibility: Some(Arc::clone(&shared)),
        ..Default::default()
    });
    let queue = programs::load_programs_with(&mut r, Some(&shared));
    programs::apply_pending_settings_with(&mut r, &queue, true, None);
    programs::apply_shared_accessibility(&mut r, &shared, None);
    r.settings_queue = Some(queue);

    // ---- it starts from the shared values ----
    assert_eq!(r.palette_theme, PaletteTheme::Light);

    // ---- its settings page has no accessibility rows ----
    let mut rows = Vec::new();
    texts(
        r.ffon.last().expect("settings is the last provider"),
        &mut rows,
    );
    for word in [
        "color scheme",
        "font scale",
        "screen reader",
        "shoulder-surfing",
    ] {
        assert!(
            !rows.iter().any(|t| t.to_lowercase().contains(word)),
            "a {word} row in a session: {rows:?}"
        );
    }

    // ---- a change made in the superkey is followed ----
    let mut superkey =
        SharedAccessibility::new(Some(path), Vec::new(), AccessibilitySettings::builtin());
    std::thread::sleep(POLL_INTERVAL);
    superkey.set("colorScheme", "dark").unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while r.palette_theme != PaletteTheme::Dark {
        assert!(
            Instant::now() < deadline,
            "sicompass never followed the superkey"
        );
        frame(&mut r);
        std::thread::sleep(Duration::from_millis(20));
    }
}
