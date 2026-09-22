//! The greeter as the user meets it: real renderer state, real key handling.
//!
//! These drive `sicompass-ui` headlessly — no SDL window, no Vulkan — the same
//! way `src/sicompass/tests/integration.rs` drives the application. That is
//! what makes it possible to assert on the thing that actually matters for a
//! login screen: the list rows a screen reader will read out, and the fact that
//! the password never appears among them.
//!
//! The prefixes below are the app's own list vocabulary (`list.rs`):
//! `+R <group> [<state>]` a radio group, `-rc`/`-r` a chosen/unchosen radio
//! option, `-i` an editable field, `-b` a button, `-` a plain line. A screen
//! reader speaks them as "plus R", "dash i" and so on, so asserting on them is
//! asserting on what is spoken.

#![cfg(target_os = "linux")]

use sdl3::keyboard::{Keycode, Mod};
use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::tags;
use sicompass_ui::app_state::AppRenderer;
use sicompass_ui::events::dispatch_key;
use sicompass_ui::list::create_list_current_layer;
use sicompass_ui::registry::register_provider;

/// The page `LoginProvider::fetch` produces.
///
/// Rebuilt here rather than imported, because the provider lives in a binary
/// crate. What is under test is the *renderer's* treatment of this page — the
/// masking, the prefixes, the insert mode — which is the half a greeter must
/// not get wrong, and which the provider's own unit tests cannot see.
fn greeter_page() -> Vec<FfonElement> {
    let mut user = FfonElement::new_obj("<radio>User".to_owned());
    let u = user.as_obj_mut().unwrap();
    u.push(FfonElement::Str(tags::format_checked("nico")));
    u.push(FfonElement::Str("guest".to_owned()));

    let mut session = FfonElement::new_obj("<radio>Session".to_owned());
    let s = session.as_obj_mut().unwrap();
    s.push(FfonElement::Str(tags::format_checked("Desicompass")));
    s.push(FfonElement::Str("COSMIC".to_owned()));

    vec![
        user,
        session,
        FfonElement::Str(format!("Password: {}", tags::format_password(""))),
        FfonElement::Str("<button>suspend</button>Suspend".to_owned()),
        FfonElement::Str("<button>reboot</button>Restart".to_owned()),
        FfonElement::Str("<button>poweroff</button>Shut down".to_owned()),
        FfonElement::Str("Tuesday 22 September, 15:04".to_owned()),
    ]
}

struct PageProvider(Vec<FfonElement>);

impl sicompass_sdk::provider::Provider for PageProvider {
    fn name(&self) -> &str {
        "login"
    }
    fn display_name(&self) -> String {
        "Sign in".to_owned()
    }
    fn fetch(&mut self) -> Vec<FfonElement> {
        self.0.clone()
    }
}

fn renderer() -> AppRenderer {
    let mut r = AppRenderer::new();
    register_provider(&mut r, Box::new(PageProvider(greeter_page())));
    // Descend into the provider: the greeter opens straight onto its own page,
    // not onto a root list with one entry in it.
    r.current_id.push(0);
    create_list_current_layer(&mut r);
    r
}

fn rows(r: &AppRenderer) -> Vec<String> {
    r.total_list.iter().map(|i| i.label.clone()).collect()
}

#[test]
fn the_page_reads_as_two_radio_groups_a_field_and_three_buttons() {
    let r = renderer();
    let l = rows(&r);

    // A radio group renders with its chosen option spliced into the label, so
    // the group line alone tells a screen-reader user what is selected.
    assert!(l[0].starts_with("+R User"), "got {:?}", l[0]);
    assert!(
        l[0].contains("nico"),
        "the group must name its selection: {:?}",
        l[0]
    );
    assert!(l[1].starts_with("+R Session"), "got {:?}", l[1]);
    assert!(l[1].contains("Desicompass"), "got {:?}", l[1]);

    // The password is an editable field, with the same `-i` prefix as any
    // other `<input>` so the edit flow and the spoken word match.
    assert!(l[2].starts_with("-i Password:"), "got {:?}", l[2]);

    assert_eq!(l[3], "-b Suspend");
    assert_eq!(l[4], "-b Restart");
    assert_eq!(l[5], "-b Shut down");
    assert!(l[6].starts_with("- Tuesday"), "got {:?}", l[6]);
    assert_eq!(l.len(), 7, "no extra rows: {l:?}");
}

#[test]
fn entering_the_user_group_lists_its_options_with_the_checked_one_marked() {
    let mut r = renderer();
    dispatch_key(&mut r, Some(Keycode::Right), Mod::NOMOD);
    let l = rows(&r);
    assert_eq!(l, vec!["-rc nico", "-r guest"], "got {l:?}");
}

/// The whole reason `<password>` exists. Typed characters must never reach the
/// rendered list, which is also the text AccessKit publishes.
#[test]
fn a_typed_password_is_masked_in_the_list_and_never_appears_in_full() {
    const SECRET: &str = "hunter2";
    let mut r = renderer();

    dispatch_key(&mut r, Some(Keycode::Down), Mod::NOMOD);
    dispatch_key(&mut r, Some(Keycode::Down), Mod::NOMOD);
    assert!(rows(&r)[2].starts_with("-i Password:"));

    dispatch_key(&mut r, Some(Keycode::I), Mod::NOMOD);
    assert!(r.input_is_password, "editing must enter password mode");

    // Text arrives as a text-input event, not as key presses: that is the path
    // SDL uses for anything with a keyboard layout behind it.
    sicompass_ui::handlers::handle_input(&mut r, SECRET);

    // The buffer holds the real value; nothing rendered does.
    assert_eq!(r.input_buffer, SECRET);
    for row in rows(&r) {
        assert!(!row.contains(SECRET), "the list leaked the password: {row}");
    }
}

#[test]
fn the_field_starts_empty_so_a_failed_attempt_leaves_nothing_behind() {
    let r = renderer();
    let row = rows(&r)[2].clone();
    assert_eq!(row.trim_end(), "-i Password:", "got {row:?}");
}
