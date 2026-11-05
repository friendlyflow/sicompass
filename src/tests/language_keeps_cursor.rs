//! Picking a language moves the language group: the rows of the sicompass section are
//! sorted by what they say, and now they say something else. The cursor stays in the
//! group, on the language just picked, with the group's own name above the list.
//!
//! Its own test binary, because it changes the process's language and points
//! `XDG_CONFIG_HOME` at a temporary directory.

use sdl3::keyboard::{Keycode, Mod};
use sicompass::programs;
use sicompass_sdk::ffon::{FfonElement, IdArray};
use sicompass_sdk::{localize, tags};
use sicompass_ui::app_state::AppRenderer;
use sicompass_ui::list;

/// The rows the cursor is among.
fn rows(r: &AppRenderer) -> &[FfonElement] {
    sicompass_sdk::ffon::get_ffon_at_id(&r.ffon, &r.current_id).expect("the cursor is in the tree")
}

fn text(e: &FfonElement) -> &str {
    match e {
        FfonElement::Str(s) => s,
        FfonElement::Obj(o) => &o.key,
    }
}

/// Into the language group of the settings' sicompass section, as Right would.
fn into_language_group(r: &mut AppRenderer) {
    let settings = r
        .providers
        .iter()
        .position(|p| p.name() == "settings")
        .unwrap();
    let mut id = IdArray::new();
    id.push(settings);
    id.push(0); // the sicompass section
    id.push(0); // among its rows
    r.current_id = id;
    let group = localize::t("settings-radio-language");
    let at = rows(r)
        .iter()
        .position(|e| text(e) == format!("<radio>{group}"))
        .expect("the language group");
    r.current_id.set_last(at);
    r.current_id.push(0);
    list::create_list_current_layer(r);
    r.coordinate = sicompass_ui::app_state::Coordinate::General;
}

#[test]
fn picking_a_language_keeps_the_cursor_in_the_language_group() {
    let config = tempfile::tempdir().unwrap();
    // SAFETY: this binary's only test, run before anything reads the variable.
    unsafe { std::env::set_var("XDG_CONFIG_HOME", config.path()) };
    sicompass_builtins::register_all();

    let mut r = AppRenderer::new();
    let queue = programs::load_programs(&mut r);
    programs::apply_pending_settings(&mut r, &queue, true);
    into_language_group(&mut r);

    for (locale, native) in [
        ("nl-BE", "Nederlands (België)"),
        ("de-BE", "Deutsch (Belgien)"),
        ("fr-BE", "Français (Belgique)"),
        ("en-US", "English"),
    ] {
        let at = rows(&r)
            .iter()
            .position(|e| tags::strip_display(text(e)) == native)
            .unwrap_or_else(|| panic!("{native} in {:?}", rows(&r)));
        r.current_id.set_last(at);
        list::create_list_current_layer(&mut r);
        r.list_index = at;

        sicompass_ui::handlers::handle_enter_general(&mut r);
        programs::apply_pending_settings(&mut r, &queue, false);

        assert_eq!(localize::current_locale(), locale);
        assert_eq!(
            list::parent_line_text(&r),
            localize::t("settings-radio-language"),
            "{locale}: the group's name above the list, at {:?}",
            r.current_id
        );
        assert_eq!(
            r.current_id.last(),
            Some(at),
            "{locale}: on the same choice"
        );
        let item = &r.total_list[r.list_index];
        assert!(item.label.starts_with("-rc "), "{locale}: {}", item.label);
        assert!(item.label.ends_with(native), "{locale}: {}", item.label);
        assert!(
            r.providers[r.current_id.get(0).unwrap()]
                .current_path()
                .ends_with(&format!("/{}", localize::t("settings-radio-language"))),
            "{locale}: the provider's path follows, {}",
            r.providers[r.current_id.get(0).unwrap()].current_path()
        );
    }

    // Undo puts the last language back, and the cursor stays in the group for that too.
    sicompass::shortcuts::dispatch_key(&mut r, Some(Keycode::Z), Mod::LCTRLMOD);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut changed = false;
    while !r.pending_provider_ops.is_empty() {
        changed |= sicompass::events::run_provider_ticks(&mut r).0;
        assert!(
            std::time::Instant::now() < deadline,
            "undo did not complete"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    if changed {
        sicompass::provider::refresh_current_directory(&mut r);
        list::create_list_current_layer(&mut r);
    }
    programs::apply_pending_settings(&mut r, &queue, false);
    assert_eq!(localize::current_locale(), "fr-BE");
    assert_eq!(
        list::parent_line_text(&r),
        localize::t("settings-radio-language"),
        "after undo, at {:?}",
        r.current_id
    );
}
