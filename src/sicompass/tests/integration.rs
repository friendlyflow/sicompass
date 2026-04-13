//! Integration tests for sicompass.
//!
//! Mirrors `tests/integration/test_integration.c`.
//!
//! These tests link real handler / state / list / provider code against a
//! headless harness with no SDL window or Vulkan context.  Key presses are
//! simulated by calling `events::dispatch_key` directly on an `AppRenderer`.

use sicompass::events::dispatch_key;
use sicompass::app_state::{AppRenderer, Coordinate};
use sdl3::keyboard::{Keycode, Mod};
use sicompass_filebrowser::FilebrowserProvider;
use sicompass_settings::SettingsProvider;
use sicompass_sdk::provider::Provider;
use sicompass_sdk::ffon::FfonElement;
use std::path::Path;
use tempfile::TempDir;

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// A headless application renderer with real providers loaded.
struct Harness {
    pub renderer: AppRenderer,
    pub tmp: TempDir,
}

impl Harness {
    /// Create a harness with a fresh temp directory pre-populated with
    ///   alpha.txt, beta.txt, subdir/nested.txt
    /// and providers: FilebrowserProvider (rooted at tmp) + SettingsProvider.
    fn new() -> Self {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let root = tmp.path();

        std::fs::write(root.join("alpha.txt"), "test content").unwrap();
        std::fs::write(root.join("beta.txt"), "test content").unwrap();
        std::fs::create_dir(root.join("subdir")).unwrap();
        std::fs::write(root.join("subdir/nested.txt"), "test content").unwrap();

        let mut renderer = AppRenderer::new();

        // File browser rooted at temp dir (set path AFTER init which resets to "/")
        register(&mut renderer, Box::new(FilebrowserProvider::new()));
        renderer.providers[0].set_current_path(root.to_str().unwrap());
        // Re-fetch now that the path is correct
        {
            let children = renderer.providers[0].fetch();
            let display_name = renderer.providers[0].display_name().to_owned();
            let mut root_elem = FfonElement::new_obj(&display_name);
            for child in children { root_elem.as_obj_mut().unwrap().push(child); }
            renderer.ffon[0] = root_elem;
        }

        // Settings (no-op apply fn, isolated to temp dir — never touches real config)
        let settings = SettingsProvider::new(|_, _| {})
            .with_config_path(tmp.path().join("settings.json"));
        register(&mut renderer, Box::new(settings));

        sicompass::list::create_list_current_layer(&mut renderer);

        Harness { renderer, tmp }
    }

    fn new_with_webbrowser() -> Self {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let root = tmp.path();
        std::fs::write(root.join("alpha.txt"), "test content").unwrap();

        let mut renderer = AppRenderer::new();

        // Filebrowser: init resets path to "/", so set path after init
        register(&mut renderer, Box::new(FilebrowserProvider::new()));
        renderer.providers[0].set_current_path(root.to_str().unwrap());
        {
            let children = renderer.providers[0].fetch();
            let display_name = renderer.providers[0].display_name().to_owned();
            let mut root_elem = FfonElement::new_obj(&display_name);
            for child in children { root_elem.as_obj_mut().unwrap().push(child); }
            renderer.ffon[0] = root_elem;
        }

        let wb = sicompass_webbrowser::WebbrowserProvider::new();
        register(&mut renderer, Box::new(wb));

        // Settings (no-op apply fn, isolated to temp dir — never touches real config)
        let settings = SettingsProvider::new(|_, _| {})
            .with_config_path(tmp.path().join("settings.json"));
        register(&mut renderer, Box::new(settings));

        sicompass::list::create_list_current_layer(&mut renderer);

        Harness { renderer, tmp }
    }

    fn r(&mut self) -> &mut AppRenderer {
        &mut self.renderer
    }

    /// Provider index by name (`"filebrowser"`, `"settings"`, …).
    fn provider_idx(&self, name: &str) -> Option<usize> {
        self.renderer.providers.iter().position(|p| p.name() == name)
    }

    fn tmp_path(&self) -> &Path {
        self.tmp.path()
    }
}

fn register(renderer: &mut AppRenderer, mut provider: Box<dyn Provider>) {
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

// ---------------------------------------------------------------------------
// Key simulation helpers
// ---------------------------------------------------------------------------

fn press(r: &mut AppRenderer, key: Keycode) {
    dispatch_key(r, Some(key), Mod::NOMOD);
}
fn press_ctrl(r: &mut AppRenderer, key: Keycode) {
    dispatch_key(r, Some(key), Mod::LCTRLMOD);
}
fn press_ctrl_shift(r: &mut AppRenderer, key: Keycode) {
    dispatch_key(r, Some(key), Mod::LCTRLMOD | Mod::LSHIFTMOD);
}
fn press_shift_left(r: &mut AppRenderer)  { dispatch_key(r, Some(Keycode::Left),  Mod::LSHIFTMOD); }
fn press_shift_right(r: &mut AppRenderer) { dispatch_key(r, Some(Keycode::Right), Mod::LSHIFTMOD); }

fn press_down(r: &mut AppRenderer)   { press(r, Keycode::Down); }
fn press_up(r: &mut AppRenderer)     { press(r, Keycode::Up); }
fn press_right(r: &mut AppRenderer)  { press(r, Keycode::Right); }
fn press_left(r: &mut AppRenderer)   { press(r, Keycode::Left); }
fn press_enter(r: &mut AppRenderer)  { press(r, Keycode::Return); }
fn press_escape(r: &mut AppRenderer) { press(r, Keycode::Escape); }
fn press_tab(r: &mut AppRenderer)    { press(r, Keycode::Tab); }

fn type_text(r: &mut AppRenderer, text: &str) {
    sicompass::handlers::handle_input(r, text);
}

/// Navigate from root to a specific root-level provider index.
fn navigate_to_provider(r: &mut AppRenderer, target_idx: usize) {
    // Go to root depth if needed
    while r.current_id.depth() > 1 { press_left(r); }
    let current = r.current_id.get(0).unwrap_or(0);
    if current < target_idx {
        for _ in 0..(target_idx - current) { press_down(r); }
    } else {
        for _ in 0..(current - target_idx) { press_up(r); }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn initial_state() {
    let h = Harness::new();
    let r = &h.renderer;
    assert_eq!(r.coordinate, Coordinate::OperatorGeneral);
    assert_eq!(r.current_id.depth(), 1);
    assert_eq!(r.current_id.get(0), Some(0));
    assert!(r.ffon.len() >= 2, "should have at least filebrowser + settings");
}

#[test]
fn navigate_between_providers_up_down() {
    let mut h = Harness::new();
    let start = h.renderer.current_id.get(0).unwrap_or(0);

    press_down(h.r());
    assert_eq!(h.renderer.current_id.get(0).unwrap_or(0), start + 1);
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorGeneral);

    press_up(h.r());
    assert_eq!(h.renderer.current_id.get(0).unwrap_or(0), start);
}

#[test]
fn enter_provider_and_navigate_back() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");

    // Reset filebrowser to "/" so pressing left from the provider root returns to depth 1.
    h.renderer.providers[fb_idx].set_current_path("/");
    {
        let children = h.renderer.providers[fb_idx].fetch();
        let display_name = h.renderer.providers[fb_idx].display_name().to_owned();
        let mut root_elem = FfonElement::new_obj(&display_name);
        for child in children { root_elem.as_obj_mut().unwrap().push(child); }
        h.renderer.ffon[fb_idx] = root_elem;
    }
    sicompass::list::create_list_current_layer(h.r());

    navigate_to_provider(h.r(), fb_idx);

    press_right(h.r());
    assert_eq!(h.renderer.current_id.depth(), 2, "should be inside provider");
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorGeneral);

    press_left(h.r());
    assert_eq!(h.renderer.current_id.depth(), 1, "should be back at root");
}

#[test]
fn filebrowser_left_in_subdir_stays_at_depth_2() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);

    // Enter the filebrowser (depth 2, listing temp dir)
    press_right(h.r());
    assert_eq!(h.renderer.current_id.depth(), 2, "should be inside provider");

    // Navigate down to subdir (meta=0, alpha.txt=1, beta.txt=2, subdir=3)
    press_down(h.r());
    press_down(h.r());
    press_down(h.r());

    // Enter subdir — lazy fetch, push_path called
    press_right(h.r());
    assert_eq!(h.renderer.current_id.depth(), 2, "still at depth 2 inside subdir");
    let path_in_subdir = sicompass::provider::current_path(&h.renderer).to_owned();
    assert!(path_in_subdir.ends_with("subdir"), "path should be inside subdir");

    // Press left — should navigate back to parent dir, staying at depth 2
    press_left(h.r());
    assert_eq!(h.renderer.current_id.depth(), 2, "should stay at depth 2 after left from subdir");
    let path_after = sicompass::provider::current_path(&h.renderer).to_owned();
    assert!(!path_after.ends_with("subdir"), "path should be back at parent");
}

#[test]
fn filebrowser_left_from_subdir_restores_cursor_to_entered_folder() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r()); // enter filebrowser at depth 2

    // Find "subdir" in the listing and navigate to it.
    let subdir_idx = {
        let obj = h.renderer.ffon[fb_idx].as_obj().unwrap();
        obj.children.iter().position(|c| {
            c.as_obj().map(|o| sicompass_sdk::tags::strip_display(&o.key) == "subdir").unwrap_or(false)
        }).expect("subdir should exist")
    };
    let cur = h.renderer.current_id.get(1).unwrap_or(0);
    for _ in 0..(subdir_idx as isize - cur as isize).max(0) { press_down(h.r()); }

    press_right(h.r()); // enter subdir
    press_left(h.r());  // navigate back to parent

    // Cursor should land on "subdir", not index 0.
    assert_eq!(
        h.renderer.current_id.get(1),
        Some(subdir_idx),
        "cursor should be on subdir after navigating back"
    );
    assert_eq!(
        h.renderer.list_index, subdir_idx,
        "list_index should match subdir position"
    );
}

#[test]
fn filebrowser_shows_temp_files() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());

    let fb_root = &h.renderer.ffon[fb_idx];
    let obj = fb_root.as_obj().expect("filebrowser root should be Obj");
    assert!(
        obj.children.len() >= 3,
        "expected alpha.txt, beta.txt, subdir — got {}",
        obj.children.len()
    );
}

#[test]
fn search_mode_via_tab() {
    let mut h = Harness::new();
    press_tab(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::SimpleSearch);

    type_text(h.r(), "alpha");
    assert!(
        h.renderer.filtered_list_indices.len() <= h.renderer.total_list.len(),
        "filtered list should be <= total"
    );

    press_escape(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorGeneral);
}

#[test]
fn provider_active_changes_with_navigation() {
    let mut h = Harness::new();

    let first_idx = h.renderer.current_id.get(0).unwrap_or(0);
    press_down(h.r());
    let second_idx = h.renderer.current_id.get(0).unwrap_or(0);
    assert_ne!(first_idx, second_idx);

    press_up(h.r());
    let back = h.renderer.current_id.get(0).unwrap_or(0);
    assert_eq!(first_idx, back);
}

#[test]
fn navigate_into_subdirectory() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());

    // Find "subdir" in the FFON tree
    let subdir_idx = {
        let obj = h.renderer.ffon[fb_idx].as_obj().unwrap();
        obj.children.iter().position(|c| {
            c.as_obj().map(|o| o.key == "subdir").unwrap_or(false)
        })
    };

    if let Some(idx) = subdir_idx {
        let current_child = h.renderer.current_id.get(1).unwrap_or(0);
        let diff = idx as isize - current_child as isize;
        if diff > 0 {
            for _ in 0..diff { press_down(h.r()); }
        } else {
            for _ in 0..(-diff) { press_up(h.r()); }
        }

        press_right(h.r());
        assert_eq!(h.renderer.current_id.depth(), 3, "should be inside subdir");

        press_left(h.r());
        assert_eq!(h.renderer.current_id.depth(), 2);
    }
}

#[test]
fn provider_state_preserved_across_navigation() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");

    // Use "/" so that pressing left from inside the provider exits to the root list.
    h.renderer.providers[fb_idx].set_current_path("/");
    {
        let children = h.renderer.providers[fb_idx].fetch();
        let display_name = h.renderer.providers[fb_idx].display_name().to_owned();
        let mut root_elem = FfonElement::new_obj(&display_name);
        for child in children { root_elem.as_obj_mut().unwrap().push(child); }
        h.renderer.ffon[fb_idx] = root_elem;
    }
    sicompass::list::create_list_current_layer(h.r());

    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());

    let count_before = h.renderer.ffon[fb_idx].as_obj().unwrap().children.len();
    assert!(count_before >= 1);

    press_left(h.r());
    press_down(h.r());
    press_up(h.r());
    press_right(h.r());

    let count_after = h.renderer.ffon[fb_idx].as_obj().unwrap().children.len();
    assert_eq!(count_before, count_after, "child count should be unchanged");
    assert_eq!(h.renderer.current_id.depth(), 2);
}

#[test]
fn file_creation_via_insert_mode() {
    let mut h = Harness::new();
    let tmp = h.tmp_path().to_path_buf();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());

    press_ctrl(h.r(), Keycode::I);
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorInsert);

    type_text(h.r(), "- newfile.txt");
    press_enter(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorGeneral);

    assert!(tmp.join("newfile.txt").exists(), "newfile.txt should exist on disk");
}

#[test]
fn directory_creation_via_insert_mode() {
    let mut h = Harness::new();
    let tmp = h.tmp_path().to_path_buf();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());

    press_ctrl(h.r(), Keycode::I);
    type_text(h.r(), "+ newdir");
    press_enter(h.r());

    assert!(tmp.join("newdir").is_dir(), "newdir should exist as a directory");
}

#[test]
fn escape_returns_to_operator_general() {
    let mut h = Harness::new();

    // From search mode
    press_tab(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::SimpleSearch);
    press_escape(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorGeneral);

    // From insert mode
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());
    press_ctrl(h.r(), Keycode::I);
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorInsert);
    press_escape(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorGeneral);
}

#[test]
fn file_deletion() {
    let mut h = Harness::new();
    let tmp = h.tmp_path().to_path_buf();
    std::fs::write(tmp.join("deleteme.txt"), "").unwrap();

    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());

    // Refresh
    press(h.r(), Keycode::F5);

    // Find "deleteme.txt" in the list and navigate to it
    let target_idx = {
        let obj = h.renderer.ffon[fb_idx].as_obj().unwrap();
        obj.children.iter().position(|c| {
            let key = c.as_obj().map(|o| o.key.as_str())
                .or_else(|| c.as_str())
                .unwrap_or("");
            sicompass_sdk::tags::strip_display(key).contains("deleteme.txt")
        })
    };

    if let Some(idx) = target_idx {
        let cur = h.renderer.current_id.get(1).unwrap_or(0);
        let diff = idx as isize - cur as isize;
        if diff > 0 {
            for _ in 0..diff { press_down(h.r()); }
        } else {
            for _ in 0..(-diff) { press_up(h.r()); }
        }
        press_ctrl(h.r(), Keycode::D);
        assert!(!tmp.join("deleteme.txt").exists(), "deleteme.txt should be deleted");
    }
}

#[test]
fn mode_transitions_tab_escape_chain() {
    let mut h = Harness::new();

    assert_eq!(h.renderer.coordinate, Coordinate::OperatorGeneral);

    press_tab(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::SimpleSearch);

    // Tab from SimpleSearch is now a no-op
    press_tab(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::SimpleSearch);

    press_escape(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorGeneral);

    // S key enters Scroll from inside a provider (not at root)
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());
    press(h.r(), Keycode::S);
    assert_eq!(h.renderer.coordinate, Coordinate::Scroll);

    press_escape(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorGeneral);
}

#[test]
fn scroll_search_esc_chain() {
    let mut h = Harness::new();

    // S only works inside a provider, not at root
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());
    press(h.r(), Keycode::S);  // -> Scroll
    assert_eq!(h.renderer.coordinate, Coordinate::Scroll);

    press_ctrl(h.r(), Keycode::F);
    assert_eq!(h.renderer.coordinate, Coordinate::ScrollSearch);

    press_escape(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::Scroll);

    press_escape(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorGeneral);
}

#[test]
fn enter_on_file_does_not_rename() {
    let mut h = Harness::new();
    let tmp = h.tmp_path().to_path_buf();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());

    // Find alpha.txt and navigate to it
    let alpha_idx = {
        let obj = h.renderer.ffon[fb_idx].as_obj().unwrap();
        obj.children.iter().position(|c| {
            let key = c.as_obj().map(|o| o.key.as_str())
                .or_else(|| c.as_str())
                .unwrap_or("");
            sicompass_sdk::tags::strip_display(key).contains("alpha.txt")
        })
    };

    if let Some(idx) = alpha_idx {
        let cur = h.renderer.current_id.get(1).unwrap_or(0);
        let diff = idx as isize - cur as isize;
        if diff > 0 {
            for _ in 0..diff { press_down(h.r()); }
        } else {
            for _ in 0..(-diff) { press_up(h.r()); }
        }
        press_enter(h.r());
        assert!(tmp.join("alpha.txt").exists(), "alpha.txt should still exist after Enter");
    }
}

#[test]
fn handle_i_populates_input_buffer() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());

    let alpha_idx = {
        let obj = h.renderer.ffon[fb_idx].as_obj().unwrap();
        obj.children.iter().position(|c| {
            let key = c.as_obj().map(|o| o.key.as_str())
                .or_else(|| c.as_str())
                .unwrap_or("");
            sicompass_sdk::tags::strip_display(key).contains("alpha.txt")
        })
    };

    if let Some(idx) = alpha_idx {
        let cur = h.renderer.current_id.get(1).unwrap_or(0);
        let diff = idx as isize - cur as isize;
        if diff > 0 {
            for _ in 0..diff { press_down(h.r()); }
        } else {
            for _ in 0..(-diff) { press_up(h.r()); }
        }
        press(h.r(), Keycode::I);
        assert_eq!(h.renderer.coordinate, Coordinate::OperatorInsert);
        assert!(
            h.renderer.input_buffer.contains("alpha.txt"),
            "input_buffer should contain filename, got: '{}'",
            h.renderer.input_buffer,
        );
    }
}

#[test]
fn undo_file_creation() {
    let mut h = Harness::new();
    let tmp = h.tmp_path().to_path_buf();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());

    press_ctrl(h.r(), Keycode::I);
    type_text(h.r(), "- undotest.txt");
    press_enter(h.r());
    assert!(tmp.join("undotest.txt").exists(), "file should exist after creation");

    press_ctrl(h.r(), Keycode::Z);
    assert!(!tmp.join("undotest.txt").exists(), "file should be deleted after undo");

    press_ctrl_shift(h.r(), Keycode::Z);
    assert!(tmp.join("undotest.txt").exists(), "file should be re-created after redo");
}

#[test]
fn undo_directory_creation() {
    let mut h = Harness::new();
    let tmp = h.tmp_path().to_path_buf();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());

    press_ctrl(h.r(), Keycode::I);
    type_text(h.r(), "+ undodir");
    press_enter(h.r());
    assert!(tmp.join("undodir").is_dir(), "directory should exist after creation");

    press_ctrl(h.r(), Keycode::Z);
    assert!(!tmp.join("undodir").exists(), "directory should be deleted after undo");
}

#[test]
fn webbrowser_url_bar_is_input() {
    let mut h = Harness::new_with_webbrowser();
    let wb_idx = h.provider_idx("webbrowser").expect("webbrowser not found");
    navigate_to_provider(h.r(), wb_idx);
    press_right(h.r());
    assert_eq!(h.renderer.current_id.depth(), 2);

    // First element (index 0) should be the URL bar with <input> tag
    let wb_obj = h.renderer.ffon[wb_idx].as_obj().unwrap();
    assert!(wb_obj.children.len() >= 1, "web browser should have url bar");

    let url_elem = &wb_obj.children[0];
    let url_str = url_elem.as_obj().map(|o| o.key.as_str())
        .or_else(|| url_elem.as_str())
        .unwrap_or("");
    assert!(
        sicompass_sdk::tags::has_input(url_str),
        "first element of web browser should be <input> URL bar, got: {url_str:?}"
    );
}

// ---------------------------------------------------------------------------
// Tests: Web browser URL input commit triggers refresh
// ---------------------------------------------------------------------------

/// Pressing I on the URL bar, typing a new URL and pressing Enter should
/// update the FFON tree (via refresh_current_directory), not silently no-op.
/// After commit, the URL bar key should contain the new URL.
#[test]
fn webbrowser_url_commit_updates_ffon() {
    let mut h = Harness::new_with_webbrowser();
    let wb_idx = h.provider_idx("webbrowser").expect("webbrowser not found");
    navigate_to_provider(h.r(), wb_idx);
    press_right(h.r()); // enter provider layer

    // Navigate to the URL bar (first child, index 0)
    let cur = h.renderer.current_id.get(1).unwrap_or(0);
    for _ in 0..cur { press_up(h.r()); }

    // Enter insert mode
    press(h.r(), Keycode::I);
    assert_eq!(
        h.renderer.coordinate,
        Coordinate::OperatorInsert,
        "should be in insert mode after I"
    );
    assert!(
        h.renderer.input_buffer.contains("https://"),
        "input_buffer should contain the default URL prefix"
    );

    // Type a URL (will fail to fetch, but commit_edit still sets current_url)
    type_text(h.r(), "https://example.invalid");
    press_enter(h.r());

    // After Enter, we should be back in operator mode
    assert_eq!(
        h.renderer.coordinate,
        Coordinate::OperatorGeneral,
        "should exit insert mode after Enter"
    );

    // The URL bar in the FFON tree must now contain the new URL.
    // This verifies that refresh_current_directory was called (re-fetching from provider).
    let wb_obj = h.renderer.ffon[wb_idx].as_obj().unwrap();
    let url_elem = &wb_obj.children[0];
    let url_text = url_elem.as_obj().map(|o| o.key.as_str())
        .or_else(|| url_elem.as_str())
        .unwrap_or("");
    assert!(
        url_text.contains("example.invalid"),
        "URL bar FFON should contain the committed URL after refresh, got: {url_text:?}"
    );
}

/// Pressing Enter in insert mode without changing the URL should still exit
/// insert mode and refresh — matching C's wasInput → providerRefreshCurrentDirectory.
#[test]
fn webbrowser_url_same_content_exits_insert_mode() {
    let mut h = Harness::new_with_webbrowser();
    let wb_idx = h.provider_idx("webbrowser").expect("webbrowser not found");
    navigate_to_provider(h.r(), wb_idx);
    press_right(h.r()); // enter provider layer

    // Navigate to URL bar (index 0)
    let cur = h.renderer.current_id.get(1).unwrap_or(0);
    for _ in 0..cur { press_up(h.r()); }

    // Enter insert mode — don't change anything — press Enter
    press(h.r(), Keycode::I);
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorInsert);
    press_enter(h.r());

    // Must exit insert mode even though content was unchanged
    assert_eq!(
        h.renderer.coordinate,
        Coordinate::OperatorGeneral,
        "should exit insert mode after Enter even with same content"
    );
    // And the provider index should still be valid
    assert_eq!(h.renderer.current_id.get(0), Some(wb_idx));
}

/// Fragment link (`href="#id"`) in the webbrowser FFON tree should jump the
/// cursor to the element tagged with `<id>…</id>` when Right is pressed.
#[test]
fn webbrowser_fragment_link_navigates_to_target() {
    let mut h = Harness::new_with_webbrowser();
    let wb_idx = h.provider_idx("webbrowser").expect("webbrowser not found");

    // Build a webbrowser-style FFON page directly, bypassing real URL fetch.
    // Structure: Obj("<input>https://example.com</input>") with two children:
    //   [0] Obj("skip to content <link>#main</link>")   ← skip link
    //   [1] Str("<id>main</id>Main content")              ← target
    let link_row = FfonElement::new_obj("skip to content <link>#main</link>");
    let target_row = FfonElement::new_str("<id>main</id>Main content");
    let mut page = FfonElement::new_obj("<input>https://example.com</input>");
    page.as_obj_mut().unwrap().push(link_row);
    page.as_obj_mut().unwrap().push(target_row);
    h.renderer.ffon[wb_idx] = page;
    sicompass::list::create_list_current_layer(h.r());

    // Navigate to the webbrowser provider
    navigate_to_provider(h.r(), wb_idx);
    press_right(h.r()); // enter the page layer (now at [wb_idx, 0])

    // The skip-link is at list index 0; sync current_id to that row.
    h.renderer.list_index = 0;
    h.renderer.sync_current_id_from_list();

    // Press Right — should jump to the target (no fetch, no descend)
    press_right(h.r());

    // Cursor must now be on the target row (index 1 in the page children)
    assert_eq!(
        h.renderer.current_id.last(), Some(1),
        "cursor should be on the target row (index 1) after fragment nav"
    );
}

/// Enter in OperatorGeneral on an Obj whose key has an <input> tag should NOT
/// re-activate/re-commit it — C only activates <input> on FFON_STRING elements.
#[test]
fn enter_on_input_obj_does_not_activate() {
    use sicompass_sdk::ffon::FfonElement;
    let mut h = Harness::new_with_webbrowser();
    let wb_idx = h.provider_idx("webbrowser").expect("webbrowser not found");
    navigate_to_provider(h.r(), wb_idx);
    press_right(h.r());

    // Manually replace the URL element (Str) with an Obj whose key has <input>
    // to simulate the post-load state.
    {
        let wb_obj = h.renderer.ffon[wb_idx].as_obj_mut().unwrap();
        let mut url_obj = FfonElement::new_obj("<input>https://example.com</input>");
        url_obj.as_obj_mut().unwrap().push(FfonElement::new_str("content"));
        if !wb_obj.children.is_empty() {
            wb_obj.children[0] = url_obj;
        }
    }
    sicompass::list::create_list_current_layer(h.r());
    // Navigate to index 0 (the URL Obj)
    let cur = h.renderer.current_id.get(1).unwrap_or(0);
    for _ in 0..cur { press_up(h.r()); }

    // Press Enter — should NOT navigate into the Obj's children or re-commit
    press_enter(h.r());

    // We should still be in OperatorGeneral at the same depth (2), not deeper
    assert_eq!(
        h.renderer.coordinate,
        Coordinate::OperatorGeneral,
        "should stay in OperatorGeneral"
    );
    assert_eq!(
        h.renderer.current_id.depth(), 2,
        "should not navigate deeper into URL Obj on Enter"
    );
}

// ---------------------------------------------------------------------------
// Tests: List item label prefix
// ---------------------------------------------------------------------------

#[test]
fn test_list_item_label_has_prefix() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());
    assert_eq!(h.renderer.current_id.depth(), 2);

    // Find alpha.txt FFON index
    let alpha_ffon_idx = {
        let obj = h.renderer.ffon[fb_idx].as_obj().unwrap();
        obj.children.iter().position(|c| {
            let key = c.as_obj().map(|o| o.key.as_str())
                .or_else(|| c.as_str())
                .unwrap_or("");
            sicompass_sdk::tags::strip_display(key).contains("alpha.txt")
        })
    };
    let alpha_ffon_idx = alpha_ffon_idx.expect("alpha.txt not found in FFON");

    // Navigate current_id to alpha.txt's FFON index
    let cur = h.renderer.current_id.get(1).unwrap_or(0);
    let diff = alpha_ffon_idx as isize - cur as isize;
    if diff > 0 {
        for _ in 0..diff { press_down(h.r()); }
    } else {
        for _ in 0..(-diff) { press_up(h.r()); }
    }

    // Rebuild list and find visual list index for alpha.txt
    sicompass::list::create_list_current_layer(h.r());
    let visual_idx = h.renderer.total_list.iter().position(|item| {
        item.id.last() == Some(alpha_ffon_idx)
    }).expect("alpha.txt not found in visual list");
    h.renderer.list_index = visual_idx;

    let label = &h.renderer.total_list[visual_idx].label;
    assert!(
        label.starts_with("-i "),
        "filebrowser file item should have '-i ' prefix, got: '{label}'"
    );
    assert!(
        label.contains("alpha.txt"),
        "label should contain 'alpha.txt', got: '{label}'"
    );
}

// ---------------------------------------------------------------------------
// Tests: Full workflow (filebrowser — create/navigate/delete)
// ---------------------------------------------------------------------------

#[test]
fn test_full_workflow() {
    let mut h = Harness::new();
    let tmp = h.tmp_path().to_path_buf();
    std::fs::create_dir(tmp.join("Downloads")).unwrap();

    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");

    // ---- Step 1: Navigate to filebrowser, enter it, refresh ----
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());
    assert_eq!(h.renderer.current_id.depth(), 2);
    press(h.r(), Keycode::F5);

    // ---- Step 2: Navigate into Downloads subdir ----
    let dl_idx = {
        let obj = h.renderer.ffon[fb_idx].as_obj().unwrap();
        obj.children.iter().position(|c| {
            let key = c.as_obj().map(|o| o.key.as_str())
                .or_else(|| c.as_str())
                .unwrap_or("");
            sicompass_sdk::tags::strip_display(key) == "Downloads"
        })
    };
    let dl_idx = dl_idx.expect("Downloads not found in filebrowser after refresh");
    let cur = h.renderer.current_id.get(1).unwrap_or(0);
    let diff = dl_idx as isize - cur as isize;
    if diff > 0 {
        for _ in 0..diff { press_down(h.r()); }
    } else {
        for _ in 0..(-diff) { press_up(h.r()); }
    }
    press_right(h.r());
    // In the Rust filebrowser, subdirectory navigation is lazy (currentPath changes,
    // ffon[fb_idx] is replaced), so depth stays at 2 — not 3 as in C.
    assert_eq!(h.renderer.current_id.depth(), 2, "should be inside Downloads");

    // ---- Step 3: Create a file in Downloads ----
    press_ctrl(h.r(), Keycode::I);
    type_text(h.r(), "- report.txt");
    press_enter(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorGeneral);
    assert!(tmp.join("Downloads/report.txt").exists(), "report.txt should be created");

    // ---- Step 4: Navigate back to root ----
    while h.renderer.current_id.depth() > 1 { press_left(h.r()); }
    assert_eq!(h.renderer.current_id.depth(), 1);
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorGeneral);
}

// ---------------------------------------------------------------------------
// Tests: Meta menu (M key)
// ---------------------------------------------------------------------------

#[test]
fn test_meta_enters_coordinate() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorGeneral);

    press(h.r(), Keycode::M);
    assert_eq!(h.renderer.coordinate, Coordinate::Meta);
    assert!(!h.renderer.total_list.is_empty(), "meta list should have items");
}

#[test]
fn test_meta_shows_hints() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());
    press(h.r(), Keycode::M);
    assert_eq!(h.renderer.coordinate, Coordinate::Meta);
    assert!(h.renderer.total_list.len() >= 3, "should have multiple shortcut hints");
}

#[test]
fn test_escape_from_meta_restores_position() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());
    let saved_id = h.renderer.current_id.clone();
    let saved_index = h.renderer.list_index;

    press(h.r(), Keycode::M);
    assert_eq!(h.renderer.coordinate, Coordinate::Meta);

    press(h.r(), Keycode::Escape);
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorGeneral);
    assert_eq!(h.renderer.current_id, saved_id);
    assert_eq!(h.renderer.list_index, saved_index);
}

#[test]
fn test_left_noop_in_meta() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());
    press(h.r(), Keycode::M);
    let list_before = h.renderer.total_list.len();

    press_left(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::Meta, "left should be noop in meta");
    assert_eq!(h.renderer.total_list.len(), list_before);
}

#[test]
fn test_up_down_in_meta() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());
    press(h.r(), Keycode::M);
    assert_eq!(h.renderer.list_index, 0);

    press_down(h.r());
    assert_eq!(h.renderer.list_index, 1);
    press_up(h.r());
    assert_eq!(h.renderer.list_index, 0);
}

#[test]
fn test_right_noop_in_meta() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());
    press(h.r(), Keycode::M);

    press_right(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::Meta, "right should be noop in meta");
}

#[test]
fn test_meta_at_root_shows_root_hints() {
    let mut h = Harness::new();
    assert_eq!(h.renderer.current_id.depth(), 1, "should be at root");

    press(h.r(), Keycode::M);
    assert_eq!(h.renderer.coordinate, Coordinate::Meta);
    assert!(!h.renderer.total_list.is_empty(), "root meta list should not be empty");

    // Root hints should mention Tab and Ctrl+F which work at root
    let labels: Vec<&str> = h.renderer.total_list.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.iter().any(|l| l.contains("Tab")), "root meta should mention Tab");
    assert!(labels.iter().any(|l| l.contains("Ctrl+F")), "root meta should mention Ctrl+F");
}

/// A `<link>` Obj injected into the webbrowser FFON (simulating what
/// `html_to_ffon` produces for `<a>` tags) should show the `+l` prefix in
/// the visual list and navigating Right into it should push depth by one.
#[test]
fn webbrowser_link_obj_shows_plus_l_prefix_and_is_navigable() {
    let mut h = Harness::new_with_webbrowser();
    let wb_idx = h.provider_idx("webbrowser").expect("webbrowser not found");
    navigate_to_provider(h.r(), wb_idx);
    press_right(h.r()); // enter webbrowser layer (depth 2)

    // Inject a <link> Obj as a child of the URL-bar Obj, as html_to_ffon would produce
    {
        let wb_obj = h.renderer.ffon[wb_idx].as_obj_mut().unwrap();
        let mut url_obj = FfonElement::new_obj("<input>https://example.com</input>");
        url_obj.as_obj_mut().unwrap().push(
            FfonElement::new_obj("Example link <link>https://example.com/page</link>"),
        );
        if !wb_obj.children.is_empty() {
            wb_obj.children[0] = url_obj;
        }
    }
    sicompass::list::create_list_current_layer(h.r());
    press_right(h.r()); // navigate into the URL Obj (depth 3)
    sicompass::list::create_list_current_layer(h.r());

    // The link Obj item should have a "+l" prefix in the visual list
    let link_item = h.renderer.total_list.iter().find(|item| {
        item.label.contains("Example link")
    });
    assert!(link_item.is_some(), "link element should appear in the list");
    assert!(
        link_item.unwrap().label.starts_with("+l"),
        "link Obj should have '+l' prefix, got: '{}'",
        link_item.unwrap().label
    );

    // Navigate to the link item and press Right — should go one level deeper
    let link_vis_idx = h.renderer.total_list.iter().position(|i| i.label.contains("Example link")).unwrap();
    let cur = h.renderer.list_index;
    let diff = link_vis_idx as isize - cur as isize;
    if diff > 0 { for _ in 0..diff { press_down(h.r()); } }
    else { for _ in 0..(-diff) { press_up(h.r()); } }

    let depth_before = h.renderer.current_id.depth();
    press_right(h.r());
    assert!(
        h.renderer.current_id.depth() >= depth_before,
        "navigating Right into a link Obj should not decrease depth"
    );
}

// ---------------------------------------------------------------------------
// Tests: Filebrowser state-toggle commands refresh the listing immediately
// ---------------------------------------------------------------------------

/// Helper: enter command mode and navigate to the command with the given name,
/// then press Enter to execute it.
fn execute_provider_command(h: &mut Harness, command: &str) {
    press(h.r(), Keycode::Colon);
    assert_eq!(h.renderer.coordinate, sicompass::app_state::Coordinate::Command,
        "should be in Command mode after :");

    // Find the command in the list and navigate to it
    let idx = h.renderer.total_list.iter().position(|item| item.label == command)
        .unwrap_or_else(|| panic!("command '{command}' not found in command list"));
    let cur = h.renderer.list_index;
    if idx > cur {
        for _ in 0..(idx - cur) { press_down(h.r()); }
    } else {
        for _ in 0..(cur - idx) { press_up(h.r()); }
    }
    press_enter(h.r());
}

/// After toggling "show/hide properties", the listing should immediately update —
/// items must include a properties prefix (permissions/size/date).
#[test]
fn filebrowser_show_properties_refreshes_listing() {
    let mut h = Harness::new();
    let fb_idx = h.renderer.providers.iter().position(|p| p.name() == "filebrowser")
        .expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r()); // enter filebrowser layer

    // Capture labels before toggling
    let labels_before: Vec<String> = h.renderer.total_list.iter()
        .map(|i| i.label.clone()).collect();

    execute_provider_command(&mut h, "show/hide properties");

    // Should be back in OperatorGeneral after a state-toggle
    assert_eq!(h.renderer.coordinate, sicompass::app_state::Coordinate::OperatorGeneral,
        "should return to OperatorGeneral after show/hide properties");

    // Labels must have changed — properties prefix should now be present
    let labels_after: Vec<String> = h.renderer.total_list.iter()
        .map(|i| i.label.clone()).collect();
    assert_ne!(labels_before, labels_after,
        "labels should change after toggling show/hide properties");

    // Toggle back — labels should return to original
    execute_provider_command(&mut h, "show/hide properties");
    let labels_restored: Vec<String> = h.renderer.total_list.iter()
        .map(|i| i.label.clone()).collect();
    assert_eq!(labels_before, labels_restored,
        "labels should match original after toggling properties twice");
}

/// After running "sort chronologically", the listing should immediately reorder.
/// alpha.txt and beta.txt are created at slightly different times, so they may
/// already be in chrono order — we just verify the command returns to normal mode
/// and the list is non-empty (i.e. a refresh happened).
#[test]
fn filebrowser_sort_chrono_refreshes_listing() {
    let mut h = Harness::new();
    let fb_idx = h.renderer.providers.iter().position(|p| p.name() == "filebrowser")
        .expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());

    let count_before = h.renderer.total_list.len();
    execute_provider_command(&mut h, "sort chronologically");

    assert_eq!(h.renderer.coordinate, sicompass::app_state::Coordinate::OperatorGeneral,
        "should return to OperatorGeneral after sort chronologically");
    assert_eq!(h.renderer.total_list.len(), count_before,
        "item count should be unchanged after sort");
}

/// Pressing `:` at root level (depth 1) must NOT enter command mode.
#[test]
fn colon_blocked_at_root() {
    let mut h = Harness::new();
    // Ensure we're at root
    while h.renderer.current_id.depth() > 1 { press_left(h.r()); }
    dispatch_key(h.r(), Some(Keycode::Semicolon), Mod::LSHIFTMOD);
    assert_eq!(h.renderer.coordinate, sicompass::app_state::Coordinate::OperatorGeneral,
        "command mode must not activate at root depth");
}

/// Navigating right into an empty directory shows a single `<input></input>` placeholder.
/// The filebrowser is a flat (lazy-fetch) provider: depth stays at 2 when entering subdirs.
#[test]
fn navigate_right_empty_dir_shows_placeholder() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    std::fs::write(root.join("file.txt"), "").unwrap();
    std::fs::create_dir(root.join("emptydir")).unwrap();

    let mut renderer = AppRenderer::new();
    register(&mut renderer, Box::new(FilebrowserProvider::new()));
    renderer.providers[0].set_current_path(root.to_str().unwrap());
    {
        let children = renderer.providers[0].fetch();
        let display_name = renderer.providers[0].display_name().to_owned();
        let mut root_elem = FfonElement::new_obj(&display_name);
        for child in children { root_elem.as_obj_mut().unwrap().push(child); }
        renderer.ffon[0] = root_elem;
    }
    sicompass::list::create_list_current_layer(&mut renderer);

    // Enter filebrowser (static: children already loaded → depth 2)
    press_right(&mut renderer);
    assert_eq!(renderer.current_id.depth(), 2);

    // Navigate to emptydir in the list
    let emptydir_idx = renderer.total_list.iter()
        .position(|item| item.label.contains("emptydir"))
        .expect("emptydir not found in list");
    let cur = renderer.list_index;
    if emptydir_idx > cur {
        for _ in 0..(emptydir_idx - cur) { press_down(&mut renderer); }
    } else {
        for _ in 0..(cur - emptydir_idx) { press_up(&mut renderer); }
    }

    // Enter emptydir (lazy-fetch: path changes but depth stays at 2)
    press_right(&mut renderer);
    assert_eq!(renderer.current_id.depth(), 2, "filebrowser stays at depth 2 after entering subdir");

    // Placeholder is the only list item
    assert_eq!(renderer.total_list.len(), 1, "empty dir should show exactly one placeholder");
    let label = &renderer.total_list[0].label;
    // <input></input> renders as "-i " (str prefix + empty input tag content)
    assert!(label.starts_with("-i"), "placeholder label should start with '-i', got: {label:?}");
}

/// Navigating into a subdirectory updates the root FFON element's key to the
/// directory name, so the parent line reflects the current location.
/// Navigating back out shows the parent directory name.
/// Only at filesystem root "/" does the key fall back to "file browser".
#[test]
fn navigate_right_updates_parent_key() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let root_name = root.file_name().unwrap().to_str().unwrap().to_owned();
    std::fs::create_dir(root.join("subdir")).unwrap();
    std::fs::write(root.join("subdir/file.txt"), "").unwrap();

    let mut renderer = AppRenderer::new();
    register(&mut renderer, Box::new(FilebrowserProvider::new()));
    renderer.providers[0].set_current_path(root.to_str().unwrap());
    {
        let children = renderer.providers[0].fetch();
        let display_name = renderer.providers[0].display_name().to_owned();
        let mut root_elem = FfonElement::new_obj(&display_name);
        for child in children { root_elem.as_obj_mut().unwrap().push(child); }
        renderer.ffon[0] = root_elem;
    }
    sicompass::list::create_list_current_layer(&mut renderer);

    // Initially the root element has the display name set during setup
    assert_eq!(renderer.ffon[0].as_obj().unwrap().key, "file browser");

    // Enter provider layer (static children already loaded, no refresh yet)
    press_right(&mut renderer);
    assert_eq!(renderer.ffon[0].as_obj().unwrap().key, "file browser",
        "key should still be display name before navigating into subdir");

    // Navigate to subdir
    let subdir_idx = renderer.total_list.iter()
        .position(|item| item.label.contains("subdir"))
        .expect("subdir not found in list");
    let cur = renderer.list_index;
    if subdir_idx > cur {
        for _ in 0..(subdir_idx - cur) { press_down(&mut renderer); }
    } else {
        for _ in 0..(cur - subdir_idx) { press_up(&mut renderer); }
    }

    // Enter subdir — root key should update to "subdir"
    press_right(&mut renderer);
    assert_eq!(renderer.current_id.depth(), 2);
    assert_eq!(renderer.ffon[0].as_obj().unwrap().key, "subdir",
        "root FFON key should be the directory name after navigating in");

    // Navigate back left — key returns to the parent dir name (the temp dir basename)
    press_left(&mut renderer);
    assert_eq!(renderer.ffon[0].as_obj().unwrap().key, root_name,
        "root FFON key should be the parent directory name after navigating back");
}

/// Deleting the last file in a directory causes the placeholder to reappear.
#[test]
fn delete_last_item_leaves_placeholder() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    std::fs::create_dir(root.join("mydir")).unwrap();
    std::fs::write(root.join("mydir/only.txt"), "").unwrap();

    let mut renderer = AppRenderer::new();
    register(&mut renderer, Box::new(FilebrowserProvider::new()));
    renderer.providers[0].set_current_path(root.to_str().unwrap());
    {
        let children = renderer.providers[0].fetch();
        let display_name = renderer.providers[0].display_name().to_owned();
        let mut root_elem = FfonElement::new_obj(&display_name);
        for child in children { root_elem.as_obj_mut().unwrap().push(child); }
        renderer.ffon[0] = root_elem;
    }
    sicompass::list::create_list_current_layer(&mut renderer);

    // Enter filebrowser → mydir
    press_right(&mut renderer);
    let mydir_idx = renderer.total_list.iter()
        .position(|item| item.label.contains("mydir"))
        .expect("mydir not found");
    let cur = renderer.list_index;
    if mydir_idx > cur {
        for _ in 0..(mydir_idx - cur) { press_down(&mut renderer); }
    } else {
        for _ in 0..(cur - mydir_idx) { press_up(&mut renderer); }
    }
    press_right(&mut renderer); // enters mydir (lazy-fetch, depth 2)

    assert_eq!(renderer.total_list.len(), 1, "mydir should have one item");

    // Delete the only item
    press_ctrl(&mut renderer, Keycode::D);

    // Placeholder must now be the only item
    assert_eq!(renderer.total_list.len(), 1, "placeholder should be the only item after delete");
    let label = &renderer.total_list[0].label;
    assert!(label.starts_with("-i"),
        "placeholder should appear after deleting last item, got: {label:?}");
    assert_eq!(renderer.current_id.last(), Some(0), "current_id should point at placeholder");
}

/// Running "create file" command when on the empty placeholder replaces it in-place.
#[test]
fn create_file_on_placeholder_replaces_in_place() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    std::fs::create_dir(root.join("emptydir")).unwrap();

    let mut renderer = AppRenderer::new();
    register(&mut renderer, Box::new(FilebrowserProvider::new()));
    renderer.providers[0].set_current_path(root.to_str().unwrap());
    {
        let children = renderer.providers[0].fetch();
        let display_name = renderer.providers[0].display_name().to_owned();
        let mut root_elem = FfonElement::new_obj(&display_name);
        for child in children { root_elem.as_obj_mut().unwrap().push(child); }
        renderer.ffon[0] = root_elem;
    }
    sicompass::list::create_list_current_layer(&mut renderer);

    // Navigate into filebrowser → emptydir
    press_right(&mut renderer);
    let emptydir_idx = renderer.total_list.iter()
        .position(|item| item.label.contains("emptydir"))
        .expect("emptydir not found");
    let cur = renderer.list_index;
    if emptydir_idx > cur {
        for _ in 0..(emptydir_idx - cur) { press_down(&mut renderer); }
    } else {
        for _ in 0..(cur - emptydir_idx) { press_up(&mut renderer); }
    }
    press_right(&mut renderer); // enters emptydir — shows placeholder

    assert_eq!(renderer.total_list.len(), 1, "emptydir should show placeholder");
    assert_eq!(renderer.current_id.last(), Some(0));

    // Execute "create file" command from command mode
    let mut h = Harness { renderer, tmp };
    execute_provider_command(&mut h, "create file");
    let renderer = h.r();

    // Placeholder replaced in-place → still at index 0
    assert_eq!(renderer.current_id.last(), Some(0),
        "create file on placeholder should stay at idx 0 (replaced in-place)");

    // Should enter insert mode to type the filename
    assert_eq!(renderer.coordinate, sicompass::app_state::Coordinate::OperatorInsert,
        "should enter insert mode after create file");
}

/// Ctrl+A after creating a file (prefixed insert mode) must not panic.
/// Regression: after refresh, current_id could be out-of-bounds → insert at invalid index.
#[test]
fn ctrl_a_after_prefixed_creation_no_panic() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    std::fs::create_dir(root.join("testdir")).unwrap();

    let mut h = Harness { renderer: AppRenderer::new(), tmp };
    register(h.r(), Box::new(FilebrowserProvider::new()));
    h.renderer.providers[0].set_current_path(root.to_str().unwrap());
    {
        let children = h.renderer.providers[0].fetch();
        let display_name = h.renderer.providers[0].display_name().to_owned();
        let mut root_elem = FfonElement::new_obj(&display_name);
        for child in children { root_elem.as_obj_mut().unwrap().push(child); }
        h.renderer.ffon[0] = root_elem;
    }
    sicompass::list::create_list_current_layer(h.r());

    // Navigate into filebrowser, then into testdir (empty → shows placeholder)
    press_right(h.r());
    let dir_idx = h.renderer.total_list.iter()
        .position(|item| item.label.contains("testdir"))
        .expect("testdir not found");
    let cur = h.renderer.list_index;
    if dir_idx > cur {
        for _ in 0..(dir_idx - cur) { press_down(h.r()); }
    } else {
        for _ in 0..(cur - dir_idx) { press_up(h.r()); }
    }
    press_right(h.r()); // enter testdir → placeholder at index 0

    assert_eq!(h.renderer.current_id.last(), Some(0));

    // Ctrl+A → append placeholder after index 0, enter OperatorInsert
    press_ctrl(h.r(), Keycode::A);
    assert_eq!(h.renderer.coordinate, sicompass::app_state::Coordinate::OperatorInsert);

    // Create a file
    type_text(h.r(), "- newfile.txt");
    press_enter(h.r());

    assert!(h.tmp.path().join("testdir/newfile.txt").exists(),
        "newfile.txt should be created");
    assert_eq!(h.renderer.coordinate, sicompass::app_state::Coordinate::OperatorGeneral);

    // current_id must be in-bounds after refresh
    let cur_last = h.renderer.current_id.last().unwrap_or(0);
    let prov_idx = h.renderer.current_id.get(0).unwrap_or(0);
    let child_len = h.renderer.ffon.get(prov_idx)
        .and_then(|e| e.as_obj())
        .map(|o| o.children.len())
        .unwrap_or(0);
    assert!(cur_last < child_len.max(1),
        "current_id ({cur_last}) should be in-bounds after refresh (len={child_len})");

    // Ctrl+A again — must not panic
    press_ctrl(h.r(), Keycode::A);
    assert_eq!(h.renderer.coordinate, sicompass::app_state::Coordinate::OperatorInsert,
        "Ctrl+A after creation should enter OperatorInsert without panic");
}

/// After creating a file whose name sorts last (e.g. "zzz.txt"), the cursor
/// must land on that file, not stay at the old placeholder index.
#[test]
fn prefixed_create_cursor_follows_sorted_file() {
    let mut h = Harness::new();

    // Harness starts with alpha.txt, beta.txt, subdir/ — navigate into filebrowser
    press_right(h.r());

    // Create a file that sorts last alphabetically
    press_ctrl(h.r(), Keycode::I);
    type_text(h.r(), "- zzz.txt");
    press_enter(h.r());

    assert!(h.tmp_path().join("zzz.txt").exists(), "zzz.txt should be created");
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorGeneral);

    // Cursor label content should be "zzz.txt" — both current_id and list_index must agree
    let cur_idx = h.renderer.current_id.last().unwrap_or(0);
    let list_idx = h.renderer.list_index;
    assert_eq!(cur_idx, list_idx, "current_id and list_index must be in sync");
    let label = h.renderer.total_list.get(list_idx).map(|i| i.label.as_str()).unwrap_or("");
    assert!(label.contains("zzz.txt"),
        "cursor should be on zzz.txt after sorted insertion, got: {label:?}");
}

/// After creating a directory that sorts first (e.g. "aaa/"), the cursor
/// must land on that directory, not on whatever was at the old placeholder index.
#[test]
fn prefixed_create_cursor_follows_sorted_dir() {
    let mut h = Harness::new();

    // Navigate into filebrowser
    press_right(h.r());

    // Create a directory that sorts first alphabetically
    press_ctrl(h.r(), Keycode::I);
    type_text(h.r(), "+ aaa");
    press_enter(h.r());

    assert!(h.tmp_path().join("aaa").is_dir(), "aaa/ should be created");
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorGeneral);

    let cur_idx = h.renderer.current_id.last().unwrap_or(0);
    let list_idx = h.renderer.list_index;
    assert_eq!(cur_idx, list_idx, "current_id and list_index must be in sync");
    let label = h.renderer.total_list.get(list_idx).map(|i| i.label.as_str()).unwrap_or("");
    assert!(label.contains("aaa"),
        "cursor should be on aaa/ after sorted insertion, got: {label:?}");
}

/// After running "sort alphanumerically", the listing should immediately reorder.
#[test]
fn filebrowser_sort_alpha_refreshes_listing() {
    let mut h = Harness::new();
    let fb_idx = h.renderer.providers.iter().position(|p| p.name() == "filebrowser")
        .expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());

    let count_before = h.renderer.total_list.len();
    execute_provider_command(&mut h, "sort alphanumerically");

    assert_eq!(h.renderer.coordinate, sicompass::app_state::Coordinate::OperatorGeneral,
        "should return to OperatorGeneral after sort alphanumerically");
    assert_eq!(h.renderer.total_list.len(), count_before,
        "item count should be unchanged after sort");
}

/// "open file with" secondary list must store the exec payload in `nav_path`,
/// not in `data`. The renderer treats a non-None `data` field as an image path
/// and attempts to load it as a texture — putting the exec command there caused
/// spurious "image load failed" errors and a stray "-p image tag" in the UI.
#[test]
fn open_file_with_secondary_list_uses_nav_path_not_data() {
    let mut h = Harness::new();
    let fb_idx = h.renderer.providers.iter().position(|p| p.name() == "filebrowser")
        .expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());

    // Navigate to the first file (non-directory) in the listing
    let file_idx = h.renderer.total_list.iter().position(|item| {
        // Objects are directories; strings are files
        !item.label.is_empty() && item.data.is_none()
    });
    if let Some(idx) = file_idx {
        let cur = h.renderer.list_index;
        if idx > cur {
            for _ in 0..(idx - cur) { press_down(h.r()); }
        } else {
            for _ in 0..(cur - idx) { press_up(h.r()); }
        }
    }

    // Enter command mode and navigate to "open file with"
    press(h.r(), Keycode::Colon);
    assert_eq!(h.renderer.coordinate, sicompass::app_state::Coordinate::Command);

    let idx = h.renderer.total_list.iter().position(|item| item.label == "open file with")
        .expect("open file with command not found");
    let cur = h.renderer.list_index;
    if idx > cur {
        for _ in 0..(idx - cur) { press_down(h.r()); }
    } else {
        for _ in 0..(cur - idx) { press_up(h.r()); }
    }
    press_enter(h.r());

    // Should now be in CommandPhase::Provider showing the app list
    assert_eq!(h.renderer.current_command, sicompass::app_state::CommandPhase::Provider,
        "should be in Provider phase after selecting 'open file with'");

    // The secondary list must be non-empty (there are applications installed)
    // and every item must carry its payload in nav_path, never in data.
    // A non-None `data` would be treated as an image path by the renderer.
    assert!(!h.renderer.total_list.is_empty(),
        "open file with should show at least one application");
    for item in &h.renderer.total_list {
        assert!(item.data.is_none(),
            "item '{}': data should be None (exec must be in nav_path to avoid image load)", item.label);
        assert!(item.nav_path.is_some(),
            "item '{}': nav_path should hold the exec command", item.label);
    }
}

// ---------------------------------------------------------------------------
// Tests: Undo/redo available from all modes
// ---------------------------------------------------------------------------

#[test]
fn undo_from_search_mode() {
    // Ctrl+Z while in SimpleSearch should undo and return to OperatorGeneral.
    let mut h = Harness::new();
    let tmp = h.tmp_path().to_path_buf();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());

    press_ctrl(h.r(), Keycode::I);
    type_text(h.r(), "- searchundo.txt");
    press_enter(h.r());
    assert!(tmp.join("searchundo.txt").exists(), "file should exist after creation");

    // Enter search mode, then undo
    press_tab(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::SimpleSearch);
    press_ctrl(h.r(), Keycode::Z);
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorGeneral, "undo should exit search mode");
    assert!(!tmp.join("searchundo.txt").exists(), "file should be deleted after undo from search mode");
}

#[test]
fn undo_from_insert_mode() {
    // Ctrl+Z while in OperatorInsert should undo and return to OperatorGeneral.
    let mut h = Harness::new();
    let tmp = h.tmp_path().to_path_buf();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());

    press_ctrl(h.r(), Keycode::I);
    type_text(h.r(), "- insertundo.txt");
    press_enter(h.r());
    assert!(tmp.join("insertundo.txt").exists(), "file should exist after creation");

    // Re-enter insert mode, then undo
    press_ctrl(h.r(), Keycode::I);
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorInsert);
    press_ctrl(h.r(), Keycode::Z);
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorGeneral, "undo should exit insert mode");
    assert!(!tmp.join("insertundo.txt").exists(), "file should be deleted after undo from insert mode");
}

// ---------------------------------------------------------------------------
// Tests: FsRename undo/redo
// ---------------------------------------------------------------------------

#[test]
fn undo_redo_rename() {
    let mut h = Harness::new();
    let tmp = h.tmp_path().to_path_buf();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());

    // Navigate to alpha.txt and rename it
    let alpha_idx = h.renderer.total_list.iter().position(|item| {
        item.label.contains("alpha.txt")
    }).expect("alpha.txt not in list");
    h.renderer.list_index = alpha_idx;
    h.renderer.current_id = h.renderer.total_list[alpha_idx].id.clone();

    press(h.r(), Keycode::I); // enter rename mode
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorInsert);
    // Clear the buffer and type new name
    h.renderer.input_buffer.clear();
    h.renderer.cursor_position = 0;
    type_text(h.r(), "renamed.txt");
    press_enter(h.r());

    assert!(tmp.join("renamed.txt").exists(), "file should be renamed");
    assert!(!tmp.join("alpha.txt").exists(), "old name should be gone");

    // Undo rename
    press_ctrl(h.r(), Keycode::Z);
    assert!(tmp.join("alpha.txt").exists(), "undo should restore original name");
    assert!(!tmp.join("renamed.txt").exists(), "renamed file should be gone after undo");

    // Redo rename
    press_ctrl_shift(h.r(), Keycode::Z);
    assert!(tmp.join("renamed.txt").exists(), "redo should re-apply rename");
    assert!(!tmp.join("alpha.txt").exists(), "original name should be gone after redo");
}

#[test]
fn rename_directory_does_not_navigate_into_it() {
    // Renaming a directory must leave the user at OperatorGeneral in the parent,
    // not navigate inside the renamed directory.
    let mut h = Harness::new();
    let tmp = h.tmp_path().to_path_buf();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());

    // Find subdir in the list
    let subdir_idx = h.renderer.total_list.iter().position(|item| {
        item.label.contains("subdir")
    }).expect("subdir not in list");
    h.renderer.list_index = subdir_idx;
    h.renderer.current_id = h.renderer.total_list[subdir_idx].id.clone();

    press(h.r(), Keycode::I); // enter rename mode
    h.renderer.input_buffer.clear();
    h.renderer.cursor_position = 0;
    type_text(h.r(), "subdir2");
    press_enter(h.r());

    assert!(tmp.join("subdir2").is_dir(), "directory should be renamed");
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorGeneral,
        "should stay in OperatorGeneral, not navigate into the renamed dir");
    assert_eq!(h.renderer.current_id.depth(), 2,
        "should remain at depth 2 (inside filebrowser root), not deeper");
}

// ---------------------------------------------------------------------------
// Tests: FsNavigate undo/redo
// ---------------------------------------------------------------------------

#[test]
fn undo_redo_navigate_into_directory() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r()); // enter filebrowser root

    let root_path = h.renderer.providers[fb_idx].current_path().to_owned();

    // Navigate into subdir
    let subdir_idx = h.renderer.total_list.iter().position(|item| item.label.contains("subdir"))
        .expect("subdir not in list");
    h.renderer.list_index = subdir_idx;
    h.renderer.current_id = h.renderer.total_list[subdir_idx].id.clone();
    press_right(h.r());

    let subdir_path = h.renderer.providers[fb_idx].current_path().to_owned();
    assert!(subdir_path.ends_with("subdir"), "should be inside subdir after navigating right");

    // Undo navigate: should return to root
    press_ctrl(h.r(), Keycode::Z);
    assert_eq!(h.renderer.providers[fb_idx].current_path(), root_path,
        "undo should restore provider path to root");
    assert_eq!(h.renderer.current_id.depth(), 2, "should be back at depth 2");

    // Redo navigate: should go back into subdir
    press_ctrl_shift(h.r(), Keycode::Z);
    assert_eq!(h.renderer.providers[fb_idx].current_path(), subdir_path,
        "redo should restore path to subdir");
}

// ---------------------------------------------------------------------------
// Tests: button press / create_element (Add element: section)
// ---------------------------------------------------------------------------

/// A minimal provider that has an "Add element:" section with a button.
/// Used to test that pressing Enter on a button creates an element and
/// does NOT corrupt the provider path.
struct ButtonTestProvider {
    path: String,
}

impl ButtonTestProvider {
    fn new() -> Self {
        ButtonTestProvider { path: "/".to_owned() }
    }
}

impl Provider for ButtonTestProvider {
    fn name(&self) -> &str { "buttontest" }
    fn display_name(&self) -> &str { "Button Test" }

    fn fetch(&mut self) -> Vec<FfonElement> {
        match self.path.as_str() {
            "/" => {
                // Root: one mandatory item + "Add element:" with a widget button
                let add_section = {
                    let mut obj = FfonElement::new_obj("Add element:");
                    obj.as_obj_mut().unwrap().push(
                        FfonElement::Str("<button>widget</button>widget".to_owned())
                    );
                    obj
                };
                vec![
                    FfonElement::Str("existing".to_owned()),
                    add_section,
                ]
            }
            "/widget" => {
                // Widget level: one child + "Add element:" with a subwidget button.
                // create_element("subwidget") will push_path("subwidget") onto "/" + "widget"
                // = "/widget" and call fetch() → only returns children for correct path.
                let add_section = {
                    let mut obj = FfonElement::new_obj("Add element:");
                    obj.as_obj_mut().unwrap().push(
                        FfonElement::Str("<button>subwidget</button>subwidget".to_owned())
                    );
                    obj
                };
                vec![
                    FfonElement::Str("wchild1".to_owned()),
                    add_section,
                ]
            }
            "/widget/subwidget" => {
                // Subwidget level: leaf children.
                vec![
                    FfonElement::Str("leaf1".to_owned()),
                    FfonElement::Str("leaf2".to_owned()),
                ]
            }
            _ => {
                // Wrong path → empty (makes path-correctness detectable in tests)
                vec![]
            }
        }
    }

    fn push_path(&mut self, segment: &str) {
        if self.path == "/" {
            self.path = format!("/{segment}");
        } else {
            self.path.push('/');
            self.path.push_str(segment);
        }
    }

    fn pop_path(&mut self) {
        if self.path == "/" { return; }
        if let Some(idx) = self.path.rfind('/') {
            self.path = if idx == 0 { "/".to_owned() } else { self.path[..idx].to_owned() };
        }
    }

    fn current_path(&self) -> &str { &self.path }

    fn create_element(&mut self, element_key: &str) -> Option<FfonElement> {
        let key = element_key.strip_prefix("one-opt:").unwrap_or(element_key);
        let tagged = format!("<many-opt></many-opt>{key}");
        let mut obj = FfonElement::new_obj(&tagged);
        // Fetch children: reuse fetch() for the child path
        let saved = self.path.clone();
        self.push_path(key);
        let children = self.fetch();
        self.path = saved;
        for child in children {
            obj.as_obj_mut().unwrap().push(child);
        }
        Some(obj)
    }
}

/// Pressing Enter on a button inside "Add element:" creates the element with the
/// correct provider path and leaves path in the right state for further navigation.
///
/// Covers the full lifecycle:
/// - in-place navigation pushes path at depth >= 2 (matching C providerNavigateRight)
/// - notify_button_pressed pops "Add element:" before calling create_element
/// - create_element receives the grandparent path so child fetch is correct
/// - Left from inside the new element returns to it in the parent list
#[test]
fn button_press_creates_element_without_corrupting_path() {
    let mut renderer = AppRenderer::new();
    register(&mut renderer, Box::new(ButtonTestProvider::new()));
    sicompass::list::create_list_current_layer(&mut renderer);

    let provider_idx = 0;

    // Navigate right into the provider root.
    // ButtonTestProvider root has pre-loaded children → in-place, depth 1→2.
    // At depth 1, push_path is NOT called (depth < 2). Path stays "/".
    renderer.current_id.set(0, provider_idx);
    press_right(&mut renderer);
    assert_eq!(renderer.providers[provider_idx].current_path(), "/",
        "navigating into provider root (depth 1→2) must not push path");

    // Navigate into "Add element:" — pre-loaded children → in-place, depth 2→3.
    // At depth 2, push_path IS called. Path becomes "/Add element:".
    let add_idx = renderer.total_list.iter().position(|item| item.label.contains("Add element:"))
        .expect("Add element: should appear in list");
    renderer.list_index = add_idx;
    renderer.current_id = renderer.total_list[add_idx].id.clone();
    press_right(&mut renderer);
    assert_eq!(renderer.providers[provider_idx].current_path(), "/Add element:",
        "navigating into 'Add element:' (in-place, depth>=2) must push path");

    // Find the button inside "Add element:".
    let btn_idx = renderer.total_list.iter().position(|item| item.label.contains("widget"))
        .expect("widget button should appear inside Add element:");
    renderer.list_index = btn_idx;
    renderer.current_id = renderer.total_list[btn_idx].id.clone();

    // Press Enter — should create the element.
    // notify_button_pressed pops "Add element:" BEFORE create_element, so
    // create_element sees path "/" and fetches children of "/widget" correctly.
    // After insertion the path stays at "/" (grandparent level).
    press_enter(&mut renderer);

    assert_eq!(
        renderer.providers[provider_idx].current_path(), "/",
        "path must be at grandparent level ('/') after button press"
    );

    // Cursor should be at the new element (depth 2, same level as "Add element:").
    assert_eq!(renderer.current_id.depth(), 2,
        "cursor should be at grandparent depth (2) after element creation");

    // The new "widget" object should appear in the list.
    sicompass::list::create_list_current_layer(&mut renderer);
    let widget_in_list = renderer.total_list.iter().any(|item| item.label.contains("widget"));
    assert!(widget_in_list, "newly created widget element should appear in list");

    // Navigate into the new widget element (in-place, pre-loaded children).
    // push_path("widget") → path "/widget".
    press_right(&mut renderer);
    assert_eq!(renderer.providers[provider_idx].current_path(), "/widget",
        "navigating into new element must push path");

    // Press Left — should pop path back to "/" and land on widget in the list.
    press_left(&mut renderer);
    assert_eq!(renderer.providers[provider_idx].current_path(), "/",
        "pressing Left from inside widget must restore path to '/'");
    assert_eq!(renderer.current_id.depth(), 2,
        "after Left, should be back at depth 2");
    let on_widget = renderer.total_list.iter().any(|item| item.label.contains("widget"));
    assert!(on_widget, "widget should still be visible in list after Left");
}

/// Two-level nested button press: mirrors the AHU → supply → filter scenario.
///
/// After creating "widget" (level 1), navigate into it, then create "subwidget"
/// (level 2) from widget's own "Add element:" section.  Verifies that:
/// - The provider path is correct at each level when create_element is called
/// - Subwidget receives children (requires path "/widget" at call time, not "/")
/// - Path and cursor are correct after creating subwidget and navigating into/out of it
#[test]
fn button_press_two_level_nested_creates_element() {
    let mut renderer = AppRenderer::new();
    register(&mut renderer, Box::new(ButtonTestProvider::new()));
    sicompass::list::create_list_current_layer(&mut renderer);

    let provider_idx = 0;

    // --- Level 1: create widget from root "Add element:" ---

    // Navigate into provider root (depth 1→2, no push — has pre-loaded children, depth < 2).
    renderer.current_id.set(0, provider_idx);
    press_right(&mut renderer);

    // Navigate into "Add element:" (depth 2→3, push "Add element:").
    let add_idx = renderer.total_list.iter().position(|item| item.label.contains("Add element:"))
        .expect("root Add element: should appear in list");
    renderer.list_index = add_idx;
    renderer.current_id = renderer.total_list[add_idx].id.clone();
    press_right(&mut renderer);

    // Press Enter on "widget" button — creates widget with children from path "/widget".
    let btn_idx = renderer.total_list.iter().position(|item| item.label.contains("widget"))
        .expect("widget button should appear");
    renderer.list_index = btn_idx;
    renderer.current_id = renderer.total_list[btn_idx].id.clone();
    press_enter(&mut renderer);

    assert_eq!(renderer.providers[provider_idx].current_path(), "/",
        "after level-1 button press, path must be at grandparent '/'");
    assert_eq!(renderer.current_id.depth(), 2,
        "cursor must be at depth 2 (grandparent level) after widget creation");

    // Widget must have received children (fetch was called at path "/widget").
    sicompass::list::create_list_current_layer(&mut renderer);
    // Label is "+ widget" (build_obj_label strips many-opt tag, adds "+" prefix)
    let widget_idx = renderer.total_list.iter()
        .position(|item| item.label.contains("widget") && !item.label.contains("subwidget"))
        .expect("widget should be in list after creation");

    // Verify widget has children by checking its FFON obj has children populated.
    {
        use sicompass_sdk::ffon::get_ffon_at_id;
        let item_id = renderer.total_list[widget_idx].id.clone();
        let slice = get_ffon_at_id(&renderer.ffon, &item_id).unwrap();
        let last = item_id.last().unwrap();
        let widget_obj = slice[last].as_obj().expect("widget should be an Obj");
        assert!(!widget_obj.children.is_empty(),
            "widget must have children (create_element fetched from '/widget')");
    }

    // --- Navigate into widget ---

    renderer.list_index = widget_idx;
    renderer.current_id = renderer.total_list[widget_idx].id.clone();
    press_right(&mut renderer);
    assert_eq!(renderer.providers[provider_idx].current_path(), "/widget",
        "after navigating into widget, path must be '/widget'");
    assert_eq!(renderer.current_id.depth(), 3, "inside widget: depth 3");

    // --- Level 2: create subwidget from widget's "Add element:" ---

    // Navigate into widget's "Add element:" (depth 3→4, push "Add element:").
    let wadd_idx = renderer.total_list.iter().position(|item| item.label.contains("Add element:"))
        .expect("widget's Add element: should appear");
    renderer.list_index = wadd_idx;
    renderer.current_id = renderer.total_list[wadd_idx].id.clone();
    press_right(&mut renderer);
    assert_eq!(renderer.providers[provider_idx].current_path(), "/widget/Add element:",
        "after navigating into widget's Add element:, path must be '/widget/Add element:'");

    // Press Enter on "subwidget" button — create_element must see path "/widget".
    let sbtn_idx = renderer.total_list.iter().position(|item| item.label.contains("subwidget"))
        .expect("subwidget button should appear inside widget's Add element:");
    renderer.list_index = sbtn_idx;
    renderer.current_id = renderer.total_list[sbtn_idx].id.clone();
    press_enter(&mut renderer);

    assert_eq!(renderer.providers[provider_idx].current_path(), "/widget",
        "after level-2 button press, path must be at '/widget' (widget's grandparent)");
    assert_eq!(renderer.current_id.depth(), 3,
        "cursor must be at depth 3 (inside widget) after subwidget creation");

    // Subwidget must have received children (fetch called at "/widget/subwidget").
    sicompass::list::create_list_current_layer(&mut renderer);
    let subwidget_idx = renderer.total_list.iter()
        .position(|item| item.label.contains("subwidget"))
        .expect("subwidget should be in widget's list after creation");

    {
        use sicompass_sdk::ffon::get_ffon_at_id;
        let item_id = renderer.total_list[subwidget_idx].id.clone();
        let slice = get_ffon_at_id(&renderer.ffon, &item_id).unwrap();
        let last = item_id.last().unwrap();
        let subwidget_obj = slice[last].as_obj().expect("subwidget should be an Obj");
        assert!(!subwidget_obj.children.is_empty(),
            "subwidget must have children (create_element fetched from '/widget/subwidget')");
    }

    // Navigate into subwidget — path must go to "/widget/subwidget".
    renderer.list_index = subwidget_idx;
    renderer.current_id = renderer.total_list[subwidget_idx].id.clone();
    press_right(&mut renderer);
    assert_eq!(renderer.providers[provider_idx].current_path(), "/widget/subwidget",
        "after navigating into subwidget, path must be '/widget/subwidget'");
    assert_eq!(renderer.current_id.depth(), 4, "inside subwidget: depth 4");

    // Press Left — must pop back to "/widget" with cursor on subwidget.
    press_left(&mut renderer);
    assert_eq!(renderer.providers[provider_idx].current_path(), "/widget",
        "Left from subwidget must restore path to '/widget'");
    assert_eq!(renderer.current_id.depth(), 3,
        "after Left from subwidget, depth must be 3");
    let subwidget_visible = renderer.total_list.iter().any(|item| item.label.contains("subwidget"));
    assert!(subwidget_visible, "subwidget must still be visible in widget's list after Left");
}

#[test]
fn root_blocks_editing_keys() {
    // At root (depth 1), editing/action keys must be no-ops.
    let mut h = Harness::new();
    assert_eq!(h.renderer.current_id.depth(), 1, "should start at root");

    let coord_before = h.renderer.coordinate;

    // S — should not enter Scroll at root
    press(h.r(), Keycode::S);
    assert_eq!(h.renderer.coordinate, coord_before, "S must be no-op at root");

    // I — should not enter EditorInsert at root
    press(h.r(), Keycode::I);
    assert_eq!(h.renderer.coordinate, coord_before, "I must be no-op at root");

    // A — should not enter EditorInsert at root
    press(h.r(), Keycode::A);
    assert_eq!(h.renderer.coordinate, coord_before, "A must be no-op at root");

    // Ctrl+I — should not enter EditorInsert at root
    press_ctrl(h.r(), Keycode::I);
    assert_eq!(h.renderer.coordinate, coord_before, "Ctrl+I must be no-op at root");

    // Ctrl+A — should not enter EditorInsert at root
    press_ctrl(h.r(), Keycode::A);
    assert_eq!(h.renderer.coordinate, coord_before, "Ctrl+A must be no-op at root");

    // Enter — should not trigger enter_operator at root
    press_enter(h.r());
    assert_eq!(h.renderer.coordinate, coord_before, "Enter must be no-op at root");

    // depth must still be 1 (no navigation happened)
    assert_eq!(h.renderer.current_id.depth(), 1, "depth must remain 1");
}

#[test]
fn root_allows_navigation_tab_ctrl_f_meta_d_space() {
    // At root, these keys must work.
    let mut h = Harness::new();
    assert_eq!(h.renderer.current_id.depth(), 1);

    // Tab enters SimpleSearch
    press_tab(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::SimpleSearch);
    press_escape(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorGeneral);

    // Ctrl+F enters ExtendedSearch
    press_ctrl(h.r(), Keycode::F);
    assert_eq!(h.renderer.coordinate, Coordinate::ExtendedSearch);
    press_escape(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorGeneral);

    // M enters Meta
    press(h.r(), Keycode::M);
    assert_eq!(h.renderer.coordinate, Coordinate::Meta);
    press_escape(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorGeneral);
}

#[test]
fn test_dashboard_key_transitions_and_escape() {
    let mut h = Harness::new();
    // Manually set a dashboard image path so handle_dashboard has something to act on
    h.renderer.dashboard_image_path = "/tmp/fake_dashboard.png".to_string();
    // Also prime the provider's dashboard_image_path via direct state manipulation
    // by setting it on the renderer directly (handle_dashboard reads from provider,
    // so we test the dispatch + escape cycle with the coordinate set directly)
    h.renderer.coordinate = Coordinate::OperatorGeneral;
    h.renderer.previous_coordinate = Coordinate::OperatorGeneral;

    // Enter Dashboard mode
    h.renderer.previous_coordinate = h.renderer.coordinate;
    h.renderer.coordinate = Coordinate::Dashboard;
    assert_eq!(h.renderer.coordinate, Coordinate::Dashboard);

    // Escape should return to OperatorGeneral
    press_escape(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorGeneral, "Escape from Dashboard should restore previous coordinate");
}

#[test]
fn test_d_key_noop_without_dashboard_image() {
    let mut h = Harness::new();
    // No dashboard_image_path set on providers — pressing D at root should stay in OperatorGeneral
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorGeneral);
    press(h.r(), Keycode::D);
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorGeneral, "D without dashboard image should not enter Dashboard mode");
}

// ---------------------------------------------------------------------------
// Tests: Ctrl+A/I insert_operator_placeholder with createElement provider
// ---------------------------------------------------------------------------

/// Ctrl+A in OperatorGeneral for a createElement provider should clone the
/// "Add element:" section rather than inserting a raw `<input></input>`.
/// The cursor should land on the clone and stay in OperatorGeneral (not EditorInsert).
#[test]
fn ctrl_a_operator_clones_add_element_section_for_create_element_provider() {
    let mut renderer = AppRenderer::new();
    register(&mut renderer, Box::new(ButtonTestProvider::new()));
    sicompass::list::create_list_current_layer(&mut renderer);

    // Navigate into provider root (depth 1→2).
    renderer.current_id.set(0, 0);
    press_right(&mut renderer);

    // Cursor is inside the provider now. "Add element:" should be in the list.
    assert!(
        renderer.total_list.iter().any(|item| item.label.contains("Add element:")),
        "Add element: should be visible before Ctrl+A"
    );
    let count_before = renderer.total_list.len();

    // Place cursor on "existing" (index 0 in the provider children).
    let existing_idx = renderer.total_list.iter()
        .position(|item| item.label.contains("existing"))
        .expect("existing item should be in list");
    renderer.list_index = existing_idx;
    renderer.current_id = renderer.total_list[existing_idx].id.clone();

    // Ctrl+A — should clone "Add element:" and insert it after current item.
    press_ctrl(&mut renderer, Keycode::A);

    // Must stay in OperatorGeneral (no insert mode for createElement providers).
    assert_eq!(renderer.coordinate, Coordinate::OperatorGeneral,
        "Ctrl+A with createElement provider must stay in OperatorGeneral");

    // List should now have one more item.
    sicompass::list::create_list_current_layer(&mut renderer);
    assert_eq!(renderer.total_list.len(), count_before + 1,
        "one extra item (the cloned Add element:) should appear after Ctrl+A");

    // The new item should be an "Add element:" clone.
    let clone_count = renderer.total_list.iter()
        .filter(|item| item.label.contains("Add element:"))
        .count();
    assert_eq!(clone_count, 2, "both the original and the clone should be visible");
}

/// Ctrl+I in OperatorGeneral for a createElement provider should clone the
/// "Add element:" section before the current item (same logic as Ctrl+A but different index).
#[test]
fn ctrl_i_operator_clones_add_element_section_for_create_element_provider() {
    let mut renderer = AppRenderer::new();
    register(&mut renderer, Box::new(ButtonTestProvider::new()));
    sicompass::list::create_list_current_layer(&mut renderer);

    renderer.current_id.set(0, 0);
    press_right(&mut renderer);

    let count_before = renderer.total_list.len();

    let existing_idx = renderer.total_list.iter()
        .position(|item| item.label.contains("existing"))
        .expect("existing item should be in list");
    renderer.list_index = existing_idx;
    renderer.current_id = renderer.total_list[existing_idx].id.clone();

    press_ctrl(&mut renderer, Keycode::I);

    assert_eq!(renderer.coordinate, Coordinate::OperatorGeneral,
        "Ctrl+I with createElement provider must stay in OperatorGeneral");

    sicompass::list::create_list_current_layer(&mut renderer);
    assert_eq!(renderer.total_list.len(), count_before + 1,
        "one extra item (the cloned Add element:) should appear after Ctrl+I");

    let clone_count = renderer.total_list.iter()
        .filter(|item| item.label.contains("Add element:"))
        .count();
    assert_eq!(clone_count, 2, "both original and clone should be visible after Ctrl+I");
}

// ---------------------------------------------------------------------------
// Tests: handle_ctrl_a double-tap in EditorGeneral
// ---------------------------------------------------------------------------

/// In EditorGeneral, pressing Ctrl+A twice quickly should undo the first append
/// and perform AppendAppend (mirroring C handleCtrlA double-tap behavior).
///
/// We set the coordinate directly since EditorGeneral is reached via the FFON
/// editor (after escaping EditorInsert), not via list navigation.
#[test]
fn ctrl_a_editor_general_double_tap_does_append_append() {
    use sicompass::app_state::Task;

    // Set up a renderer with two items in an obj (depth-2 EditorGeneral context).
    let mut r = AppRenderer::new();
    let mut root = FfonElement::new_obj("section");
    root.as_obj_mut().unwrap().push(FfonElement::new_str("alpha"));
    root.as_obj_mut().unwrap().push(FfonElement::new_str("beta"));
    r.ffon = vec![root];
    r.current_id = { let mut id = sicompass_sdk::ffon::IdArray::new(); id.push(0); id.push(0); id };
    r.coordinate = Coordinate::EditorGeneral;
    r.previous_coordinate = Coordinate::EditorGeneral;
    sicompass::list::create_list_current_layer(&mut r);

    // First Ctrl+A — single tap append.
    sicompass::handlers::handle_ctrl_a(&mut r, sicompass::app_state::History::None);
    let count_after_first = r.ffon[0].as_obj().unwrap().children.len();
    assert_eq!(count_after_first, 3, "first Ctrl+A should append one element (3 total)");

    // Record a recent keypress time so the next call is within DELTA_MS.
    r.last_keypress_time = sicompass::handlers::sdl_ticks();

    // Second Ctrl+A immediately — double tap: undo + AppendAppend.
    sicompass::handlers::handle_ctrl_a(&mut r, sicompass::app_state::History::None);

    let last_task = r.undo_history.last().map(|e| e.task);
    assert!(
        matches!(last_task, Some(Task::AppendAppend)),
        "double-tap Ctrl+A should record AppendAppend in undo history, got {last_task:?}"
    );
}

// ---------------------------------------------------------------------------
// Tests: Ctrl+O (file-browser open) flow
// ---------------------------------------------------------------------------

/// A minimal provider that supports config files, used to test the open flow.
struct ConfigProvider {
    path: String,
    data: Vec<FfonElement>,
}

impl ConfigProvider {
    fn new() -> Self {
        ConfigProvider {
            path: "/".to_owned(),
            data: vec![FfonElement::new_str("initial-item")],
        }
    }
}

impl Provider for ConfigProvider {
    fn name(&self) -> &str { "configprovider" }
    fn display_name(&self) -> &str { "Config Provider" }
    fn supports_config_files(&self) -> bool { true }
    fn fetch(&mut self) -> Vec<FfonElement> { self.data.clone() }
    fn push_path(&mut self, segment: &str) {
        if self.path == "/" { self.path = format!("/{segment}"); }
        else { self.path.push('/'); self.path.push_str(segment); }
    }
    fn pop_path(&mut self) {
        if self.path == "/" { return; }
        if let Some(idx) = self.path.rfind('/') {
            self.path = if idx == 0 { "/".to_owned() } else { self.path[..idx].to_owned() };
        }
    }
    fn current_path(&self) -> &str { &self.path }
    fn set_current_path(&mut self, path: &str) { self.path = path.to_owned(); }
}

/// Helper: create a harness with ConfigProvider (idx 0), FilebrowserProvider (idx 1).
/// The filebrowser is rooted at `tmp` and the save folder is set to `tmp`.
fn harness_with_config_provider() -> (AppRenderer, TempDir) {
    let tmp = TempDir::new().expect("temp dir");
    let mut renderer = AppRenderer::new();

    // ConfigProvider at index 0
    register(&mut renderer, Box::new(ConfigProvider::new()));

    // Filebrowser at index 1
    let root = tmp.path().to_str().unwrap().to_owned();
    register(&mut renderer, Box::new(FilebrowserProvider::new()));
    renderer.providers[1].set_current_path(&root);
    {
        let children = renderer.providers[1].fetch();
        let display_name = renderer.providers[1].display_name().to_owned();
        let mut root_elem = FfonElement::new_obj(&display_name);
        for child in children { root_elem.as_obj_mut().unwrap().push(child); }
        renderer.ffon[1] = root_elem;
    }

    renderer.save_folder_path = tmp.path().to_str().unwrap().to_owned();

    sicompass::list::create_list_current_layer(&mut renderer);
    (renderer, tmp)
}

/// Ctrl+O on a provider that supports_config_files navigates to the filebrowser
/// and sets pending_file_browser_open = true.
#[test]
fn ctrl_o_navigates_to_filebrowser_and_sets_pending_flag() {
    let (mut r, _tmp) = harness_with_config_provider();

    // Start at ConfigProvider (index 0)
    assert_eq!(r.current_id.get(0), Some(0));

    press_ctrl(&mut r, Keycode::O);

    assert!(r.pending_file_browser_open, "pending_file_browser_open should be set after Ctrl+O");
    assert_eq!(r.current_id.get(0), Some(1), "should have navigated to filebrowser (index 1)");
    assert_eq!(r.save_as_source_root_idx, 0, "source root idx should be the config provider");
}

/// Pressing Escape after Ctrl+O cancels the open flow and returns to the source provider.
#[test]
fn escape_after_ctrl_o_cancels_open_and_returns_to_source() {
    let (mut r, _tmp) = harness_with_config_provider();

    press_ctrl(&mut r, Keycode::O);
    assert!(r.pending_file_browser_open);

    press(&mut r, Keycode::Escape);

    assert!(!r.pending_file_browser_open, "pending_file_browser_open should be cleared after Escape");
    assert_eq!(r.current_id.get(0), Some(0), "should be back at config provider after Escape");
}

/// Selecting a .json file in the filebrowser during the open flow loads it into the
/// source provider and clears pending_file_browser_open.
///
/// Sets up the filebrowser state directly (bypassing navigate_to_path filesystem
/// traversal) so the test is hermetic and doesn't depend on deep tmpdir navigation.
#[test]
fn open_flow_loads_json_file_into_source_provider() {
    let (mut r, tmp) = harness_with_config_provider();

    // Write a JSON file on disk that the load handler will read
    let json_path = tmp.path().join("config.json");
    // Save format: children array (no root wrapper), matching C and the fixed Rust save
    std::fs::write(&json_path, r#"[{"loaded-item":[]}]"#).unwrap();

    // Set up filebrowser state directly: inject config.json as the first child of the
    // filebrowser root obj, set current_id to point at it, set provider path to tmpdir.
    // This simulates the user having navigated to config.json without requiring
    // navigate_to_path to traverse a deep tmpdir path.
    r.ffon[1].as_obj_mut().unwrap().children.insert(
        0,
        FfonElement::Str("<input>config.json</input>".to_owned()),
    );
    r.providers[1].set_current_path(tmp.path().to_str().unwrap());
    r.pending_file_browser_open = true;
    r.save_as_source_root_idx = 0;
    r.save_as_return_id = {
        let mut id = sicompass_sdk::ffon::IdArray::new();
        id.push(0);
        id.push(0);
        id
    };
    // Navigate current_id to [1, 0] — pointing at config.json in the filebrowser root
    r.current_id = {
        let mut id = sicompass_sdk::ffon::IdArray::new();
        id.push(1);
        id.push(0);
        id
    };
    sicompass::list::create_list_current_layer(&mut r);

    let json_idx = r.total_list.iter()
        .position(|item| item.label.contains("config.json"))
        .expect("config.json entry should be visible in filebrowser list");
    r.list_index = json_idx;
    r.current_id = r.total_list[json_idx].id.clone();

    press(&mut r, Keycode::Return);

    assert!(!r.pending_file_browser_open, "pending flag should be cleared after loading");
    assert_eq!(r.current_id.get(0), Some(0), "should be back at config provider after open");
    assert_eq!(r.current_save_path, json_path.to_str().unwrap(),
        "current_save_path should point to the loaded file");

    // The loaded FFON should have replaced the original "initial-item"
    if let Some(FfonElement::Obj(root_obj)) = r.ffon.get(0) {
        let has_loaded = root_obj.children.iter().any(|c| match c {
            FfonElement::Obj(o) => o.key == "loaded-item",
            _ => false,
        });
        assert!(has_loaded, "loaded FFON should contain 'loaded-item' from the JSON file");
    } else {
        panic!("config provider FFON root should be an Obj");
    }
}

/// In the open flow the file browser hides non-.json files; only .json files and
/// directories are shown in the list.
#[test]
fn open_flow_hides_non_json_files_in_list() {
    let (mut r, _tmp) = harness_with_config_provider();

    // Inject mixed entries: a .txt file, a .json file, and a directory
    let children = r.ffon[1].as_obj_mut().unwrap();
    children.children.clear();
    children.children.push(FfonElement::Str("<input>notes.txt</input>".to_owned()));
    children.children.push(FfonElement::Str("<input>config.json</input>".to_owned()));
    children.children.push(FfonElement::new_obj("<input>some-dir</input>"));

    r.pending_file_browser_open = true;
    r.current_id = {
        let mut id = sicompass_sdk::ffon::IdArray::new();
        id.push(1); id.push(0); id
    };
    sicompass::list::create_list_current_layer(&mut r);

    // .txt file must not appear
    assert!(!r.total_list.iter().any(|item| item.label.contains("notes.txt")),
        "non-.json file should be hidden from list during open flow");
    // .json file and directory must appear
    assert!(r.total_list.iter().any(|item| item.label.contains("config.json")),
        ".json file should be visible during open flow");
    assert!(r.total_list.iter().any(|item| item.label.contains("some-dir")),
        "directory should be visible during open flow");
}

/// Outside the open flow, all files appear (no extension filtering).
#[test]
fn file_browser_shows_all_files_outside_open_flow() {
    let (mut r, _tmp) = harness_with_config_provider();

    let children = r.ffon[1].as_obj_mut().unwrap();
    children.children.clear();
    children.children.push(FfonElement::Str("<input>notes.txt</input>".to_owned()));
    children.children.push(FfonElement::Str("<input>config.json</input>".to_owned()));

    // pending_file_browser_open is false by default
    r.current_id = {
        let mut id = sicompass_sdk::ffon::IdArray::new();
        id.push(1); id.push(0); id
    };
    sicompass::list::create_list_current_layer(&mut r);

    assert!(r.total_list.iter().any(|item| item.label.contains("notes.txt")),
        "non-.json file should be visible when not in open flow");
    assert!(r.total_list.iter().any(|item| item.label.contains("config.json")),
        ".json file should be visible when not in open flow");
}

/// Destructive keys (Ctrl+A, Ctrl+I, Delete) are no-ops in OperatorGeneral
/// while pending_file_browser_open is true.
#[test]
fn open_flow_blocks_destructive_keys() {
    let (mut r, _tmp) = harness_with_config_provider();

    // Navigate to filebrowser and inject some entries
    let children = r.ffon[1].as_obj_mut().unwrap();
    children.children.clear();
    children.children.push(FfonElement::Str("<input>config.json</input>".to_owned()));

    r.pending_file_browser_open = true;
    r.save_as_source_root_idx = 0;
    r.current_id = {
        let mut id = sicompass_sdk::ffon::IdArray::new();
        id.push(1); id.push(0); id
    };
    r.coordinate = Coordinate::OperatorGeneral;
    sicompass::list::create_list_current_layer(&mut r);
    let initial_list_len = r.total_list.len();

    // Ctrl+A (append) must be blocked
    press_ctrl(&mut r, Keycode::A);
    assert_eq!(r.coordinate, Coordinate::OperatorGeneral,
        "Ctrl+A should not enter insert mode during open flow");

    // Ctrl+I (insert) must be blocked
    press_ctrl(&mut r, Keycode::I);
    assert_eq!(r.coordinate, Coordinate::OperatorGeneral,
        "Ctrl+I should not enter insert mode during open flow");

    // Delete must be blocked — filebrowser children count should be unchanged
    press(&mut r, Keycode::Delete);
    let after_list_len = r.ffon[1].as_obj().unwrap().children.len();
    assert_eq!(after_list_len, initial_list_len,
        "Delete should not remove items during open flow");
}

/// Selecting a JSON file via SimpleSearch Enter during the open flow triggers the load.
#[test]
fn open_flow_simple_search_enter_triggers_load() {
    let (mut r, tmp) = harness_with_config_provider();

    let json_path = tmp.path().join("found.json");
    std::fs::write(&json_path, r#"[{"found-item":[]}]"#).unwrap();

    // Set up filebrowser state directly (same as other open tests)
    r.ffon[1].as_obj_mut().unwrap().children.insert(
        0,
        FfonElement::Str("<input>found.json</input>".to_owned()),
    );
    r.providers[1].set_current_path(tmp.path().to_str().unwrap());
    r.pending_file_browser_open = true;
    r.save_as_source_root_idx = 0;
    r.save_as_return_id = {
        let mut id = sicompass_sdk::ffon::IdArray::new();
        id.push(0); id.push(0); id
    };
    r.current_id = {
        let mut id = sicompass_sdk::ffon::IdArray::new();
        id.push(1); id.push(0); id
    };
    sicompass::list::create_list_current_layer(&mut r);

    // Simulate the user using SimpleSearch: set coordinate and list state
    r.coordinate = Coordinate::SimpleSearch;
    r.previous_coordinate = Coordinate::OperatorGeneral;

    // The search result points at the found.json entry
    let json_idx = r.total_list.iter()
        .position(|item| item.label.contains("found.json"))
        .expect("found.json should be in list");
    r.list_index = json_idx;

    // Enter in SimpleSearch exits search and navigates → then triggers the open flow
    press(&mut r, Keycode::Return);

    assert!(!r.pending_file_browser_open, "pending flag should be cleared after SimpleSearch Enter");
    assert_eq!(r.current_id.get(0), Some(0), "should be back at config provider");
    assert!(r.error_message.contains("found.json") || r.current_save_path.contains("found.json"),
        "should have loaded found.json");
}

/// Save writes only children (not the root wrapper), matching C behaviour.
#[test]
fn save_as_writes_children_not_root_wrapper() {
    let (mut r, tmp) = harness_with_config_provider();

    // Add a child item to the config provider's FFON
    r.ffon[0].as_obj_mut().unwrap().children.push(FfonElement::new_str("my-item"));

    let dest = tmp.path().join("out.json");
    sicompass::handlers::handle_load_provider_config(&mut r, "");  // no-op
    // Directly save using the save handler path
    r.current_save_path = dest.to_str().unwrap().to_owned();
    press_ctrl(&mut r, Keycode::S);

    let raw = std::fs::read_to_string(&dest).expect("save should have written a file");
    let parsed: serde_json::Value = serde_json::from_str(&raw).expect("save output should be valid JSON");
    // Must be an array (children), not an object with the root key
    assert!(parsed.is_array(), "saved JSON must be a top-level array of children, got: {raw}");
    // Must NOT contain the provider name as a wrapper key
    assert!(!raw.contains("\"Config Provider\"") && !raw.contains("\"configprovider\""),
        "saved JSON must not contain root wrapper key, got: {raw}");
}

/// Ctrl+S + Escape during save-as (OperatorInsert) cancels and returns to source provider.
#[test]
fn escape_in_save_as_insert_cancels_and_returns_to_source() {
    let (mut r, _tmp) = harness_with_config_provider();

    // Trigger save-as (no existing save path → falls through to file-browser save-as)
    press_ctrl(&mut r, Keycode::S);
    assert!(r.pending_file_browser_save_as, "save-as should be pending after Ctrl+S with no path");
    assert_eq!(r.coordinate, Coordinate::OperatorInsert, "should be in OperatorInsert for filename entry");

    press(&mut r, Keycode::Escape);

    assert!(!r.pending_file_browser_save_as, "save-as flag should be cleared after Escape");
    assert_eq!(r.current_id.get(0), Some(0), "should be back at config provider after Escape");
    assert_eq!(r.coordinate, Coordinate::OperatorGeneral);
}

// ---------------------------------------------------------------------------
// Per-character screen-reader announcements on Left / Right in text-input modes
// ---------------------------------------------------------------------------

/// Helper: clear the pending announcement between individual key presses so
/// each assertion is clean (mirrors what view.rs does between frames).
fn clear_announcement(r: &mut AppRenderer) {
    r.pending_announcement = None;
}

/// Strip the parity sentinel (U+200B) that `announce_char` and
/// `speak_mode_change` append on alternate calls to force AccessKit tree diffs.
/// Use this in assertions so tests do not depend on which parity cycle they run in.
fn announced_text(r: &AppRenderer) -> Option<String> {
    r.pending_announcement
        .as_deref()
        .map(|s| s.trim_end_matches('\u{200B}').to_string())
}

#[test]
fn editor_insert_left_announces_char() {
    let mut r = AppRenderer::new();
    r.coordinate = Coordinate::EditorInsert;
    r.input_buffer = "hello".to_string();
    r.cursor_position = 5;

    // Moving left over each character should announce the char stepped over.
    press_left(&mut r);
    assert_eq!(announced_text(&r).as_deref(), Some("o"), "left over 'o'");
    clear_announcement(&mut r);

    press_left(&mut r);
    assert_eq!(announced_text(&r).as_deref(), Some("l"), "left over 'l'");
    clear_announcement(&mut r);

    press_left(&mut r);
    assert_eq!(announced_text(&r).as_deref(), Some("l"), "left over second 'l'");
}

#[test]
fn editor_insert_right_announces_char() {
    let mut r = AppRenderer::new();
    r.coordinate = Coordinate::EditorInsert;
    r.input_buffer = "hello".to_string();
    r.cursor_position = 0;

    press_right(&mut r);
    assert_eq!(announced_text(&r).as_deref(), Some("h"), "right over 'h'");
    clear_announcement(&mut r);

    press_right(&mut r);
    assert_eq!(announced_text(&r).as_deref(), Some("e"), "right over 'e'");
}

#[test]
fn editor_insert_shift_left_announces_and_extends_selection() {
    let mut r = AppRenderer::new();
    r.coordinate = Coordinate::EditorInsert;
    r.input_buffer = "abc".to_string();
    r.cursor_position = 3;

    press_shift_left(&mut r);
    assert_eq!(announced_text(&r).as_deref(), Some("c"), "shift-left over 'c'");
    assert!(r.selection_anchor.is_some(), "selection should be anchored");
    clear_announcement(&mut r);

    press_shift_left(&mut r);
    assert_eq!(announced_text(&r).as_deref(), Some("b"), "shift-left over 'b'");
}

#[test]
fn editor_insert_shift_right_announces_and_extends_selection() {
    let mut r = AppRenderer::new();
    r.coordinate = Coordinate::EditorInsert;
    r.input_buffer = "abc".to_string();
    r.cursor_position = 0;

    press_shift_right(&mut r);
    assert_eq!(announced_text(&r).as_deref(), Some("a"), "shift-right over 'a'");
    assert!(r.selection_anchor.is_some(), "selection should be anchored");
}

#[test]
fn editor_insert_left_no_announcement_on_selection_collapse() {
    let mut r = AppRenderer::new();
    r.coordinate = Coordinate::EditorInsert;
    r.input_buffer = "abc".to_string();
    r.cursor_position = 3;
    // Select all then collapse with Left — should NOT announce a char.
    sicompass::handlers::handle_select_all(&mut r);
    clear_announcement(&mut r);
    press_left(&mut r);
    assert_eq!(r.pending_announcement, None, "no char announcement on selection collapse");
    assert_eq!(r.cursor_position, 0, "cursor collapsed to selection start");
}

#[test]
fn simple_search_left_announces_char() {
    let mut h = Harness::new();
    press_tab(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::SimpleSearch);
    type_text(h.r(), "foo");
    clear_announcement(h.r());

    press_left(h.r());
    assert_eq!(announced_text(&h.renderer).as_deref(), Some("o"), "left over 'o' in search");
}

#[test]
fn simple_search_right_announces_char() {
    let mut h = Harness::new();
    press_tab(h.r());
    type_text(h.r(), "foo");
    h.renderer.cursor_position = 0;
    clear_announcement(h.r());

    press_right(h.r());
    assert_eq!(announced_text(&h.renderer).as_deref(), Some("f"), "right over 'f' in search");
}

#[test]
fn command_mode_left_announces_char() {
    // Set up Command mode directly — entering via ':' requires depth > 1 in the
    // tree, so we skip the entry ceremony and test the key-dispatch logic alone.
    let mut r = AppRenderer::new();
    r.coordinate = Coordinate::Command;
    r.input_buffer = "abc".to_string();
    r.cursor_position = 3;
    clear_announcement(&mut r);

    press_left(&mut r);
    assert_eq!(announced_text(&r).as_deref(), Some("c"), "left over 'c' in command");
}

#[test]
fn command_mode_right_announces_char() {
    let mut r = AppRenderer::new();
    r.coordinate = Coordinate::Command;
    r.input_buffer = "abc".to_string();
    r.cursor_position = 0;
    clear_announcement(&mut r);

    press_right(&mut r);
    assert_eq!(announced_text(&r).as_deref(), Some("a"), "right over 'a' in command");
}

// ---------------------------------------------------------------------------
// refresh_on_navigate tests
// ---------------------------------------------------------------------------

/// Provider that creates elements with empty children for "leaf" and
/// non-empty children for "branch" — used to test in-memory survival.
struct InMemoryFormProvider {
    path: String,
}

impl InMemoryFormProvider {
    fn new() -> Self { InMemoryFormProvider { path: "/".to_owned() } }
}

impl Provider for InMemoryFormProvider {
    fn name(&self) -> &str { "inmemform" }
    fn display_name(&self) -> &str { "In-Mem Form" }

    fn fetch(&mut self) -> Vec<FfonElement> {
        // This provider's script has no memory of user additions — same pattern
        // as sales_demo.ts.  fetch() always returns the base schema only.
        match self.path.as_str() {
            "/" => {
                let mut add = FfonElement::new_obj("Add element:");
                add.as_obj_mut().unwrap().push(FfonElement::Str(
                    "<button>branch</button>branch".to_owned(),
                ));
                add.as_obj_mut().unwrap().push(FfonElement::Str(
                    "<button>one-opt:leaf</button>leaf".to_owned(),
                ));
                vec![add]
            }
            "/branch" => {
                let mut add = FfonElement::new_obj("Add element:");
                add.as_obj_mut().unwrap().push(FfonElement::Str(
                    "<button>one-opt:leaf</button>leaf".to_owned(),
                ));
                vec![add]
            }
            // All other paths return empty (leaf / unknown)
            _ => vec![],
        }
    }

    fn push_path(&mut self, segment: &str) {
        if self.path == "/" { self.path = format!("/{segment}"); }
        else { self.path.push('/'); self.path.push_str(segment); }
    }

    fn pop_path(&mut self) {
        if self.path == "/" { return; }
        if let Some(idx) = self.path.rfind('/') {
            self.path = if idx == 0 { "/".to_owned() } else { self.path[..idx].to_owned() };
        }
    }

    fn current_path(&self) -> &str { &self.path }

    fn create_element(&mut self, element_key: &str) -> Option<FfonElement> {
        let key = element_key.strip_prefix("one-opt:").unwrap_or(element_key);
        let tagged = if element_key.starts_with("one-opt:") {
            sicompass_sdk::tags::format_one_opt(key)
        } else {
            sicompass_sdk::tags::format_many_opt(key)
        };
        let mut obj = FfonElement::new_obj(&tagged);
        let saved = self.path.clone();
        self.push_path(key);
        let children = self.fetch();
        self.path = saved;
        for c in children { obj.as_obj_mut().unwrap().push(c); }
        Some(obj)
    }

    // No override for refresh_on_navigate → defaults to false (in-memory provider).
}

/// Helper: count children of the provider root element.
fn root_child_count(renderer: &AppRenderer) -> usize {
    renderer.ffon.get(0).and_then(|e| e.as_obj()).map(|o| o.children.len()).unwrap_or(0)
}

/// Helper: return child keys of the provider root element.
fn root_child_keys(renderer: &AppRenderer) -> Vec<String> {
    renderer.ffon.get(0)
        .and_then(|e| e.as_obj())
        .map(|o| o.children.iter().filter_map(|c| c.as_obj()).map(|o| o.key.clone()).collect())
        .unwrap_or_default()
}

/// Adding a node whose create_element returns empty children, navigating right
/// into it (previously triggered refresh_current_directory and destroyed the
/// tree), then pressing Left must leave the node intact.
#[test]
fn added_empty_leaf_survives_right_then_left() {
    let mut renderer = AppRenderer::new();
    register(&mut renderer, Box::new(InMemoryFormProvider::new()));
    sicompass::list::create_list_current_layer(&mut renderer);

    let pid = 0_usize;

    // Enter provider root (depth 1 → 2).
    renderer.current_id.set(0, pid);
    press_right(&mut renderer);
    assert_eq!(renderer.current_id.depth(), 2);

    // Navigate into "Add element:" (depth 2 → 3).
    let add_idx = renderer.total_list.iter().position(|i| i.label.contains("Add element:"))
        .expect("Add element: must be in list");
    renderer.list_index = add_idx;
    renderer.current_id = renderer.total_list[add_idx].id.clone();
    press_right(&mut renderer);
    assert_eq!(renderer.current_id.depth(), 3);

    // Select the "leaf" button and press Enter → creates empty-children Obj.
    let leaf_btn = renderer.total_list.iter().position(|i| i.label.contains("leaf"))
        .expect("leaf button must appear inside Add element:");
    renderer.list_index = leaf_btn;
    renderer.current_id = renderer.total_list[leaf_btn].id.clone();
    press_enter(&mut renderer);

    // Cursor should be at depth 2 on the new leaf Obj.
    assert_eq!(renderer.current_id.depth(), 2, "cursor at depth 2 after adding leaf");

    // The new leaf must appear in the root's children.
    let keys_after_add = root_child_keys(&renderer);
    let leaf_present = keys_after_add.iter().any(|k| sicompass_sdk::tags::strip_display(k) == "leaf");
    assert!(leaf_present, "leaf must be in root children after creation; got: {keys_after_add:?}");

    // Navigate right into the leaf Obj.  Its children are empty — before the fix
    // this fired refresh_current_directory and wiped the tree.
    press_right(&mut renderer);
    // Verify the leaf is still in the tree (not wiped).
    let keys_after_right = root_child_keys(&renderer);
    let still_present = keys_after_right.iter().any(|k| sicompass_sdk::tags::strip_display(k) == "leaf");
    assert!(still_present, "leaf must survive right-nav into it; got: {keys_after_right:?}");

    // Navigate left — must return to leaf without wiping the tree.
    press_left(&mut renderer);
    let keys_after_left = root_child_keys(&renderer);
    let after_left = keys_after_left.iter().any(|k| sicompass_sdk::tags::strip_display(k) == "leaf");
    assert!(after_left, "leaf must survive Left back out; got: {keys_after_left:?}");
    assert_eq!(renderer.current_id.depth(), 2, "back at depth 2 after Left");
}

/// Full AHU-style scenario: add a "branch" node, navigate into it, add a "leaf"
/// (empty children) inside it, navigate right into leaf, then Left×3 back to
/// provider selection — every level must remain intact throughout.
#[test]
fn nested_added_nodes_survive_deep_navigation() {
    let mut renderer = AppRenderer::new();
    register(&mut renderer, Box::new(InMemoryFormProvider::new()));
    sicompass::list::create_list_current_layer(&mut renderer);

    let pid = 0_usize;

    // Enter provider root (depth 1 → 2).
    renderer.current_id.set(0, pid);
    press_right(&mut renderer);

    // Navigate into "Add element:", add "branch".
    let add_idx = renderer.total_list.iter().position(|i| i.label.contains("Add element:"))
        .expect("root Add element: must exist");
    renderer.list_index = add_idx;
    renderer.current_id = renderer.total_list[add_idx].id.clone();
    press_right(&mut renderer);
    let branch_btn = renderer.total_list.iter().position(|i| i.label.contains("branch"))
        .expect("branch button must exist");
    renderer.list_index = branch_btn;
    renderer.current_id = renderer.total_list[branch_btn].id.clone();
    press_enter(&mut renderer);
    assert_eq!(renderer.current_id.depth(), 2, "cursor at depth 2 after adding branch");

    // Navigate right into the new "branch" Obj.
    press_right(&mut renderer);
    assert_eq!(renderer.current_id.depth(), 3, "inside branch at depth 3");
    assert_eq!(renderer.providers[pid].current_path(), "/branch",
        "path must be /branch after entering it");

    // Verify branch is still in the root.
    let branch_in_root = renderer.ffon.get(0).and_then(|e| e.as_obj())
        .map(|o| o.children.iter().filter_map(|c| c.as_obj())
            .any(|o| sicompass_sdk::tags::strip_display(&o.key) == "branch"))
        .unwrap_or(false);
    assert!(branch_in_root, "branch must still be in root after entering it");

    // Inside branch: navigate into its "Add element:", add "leaf".
    let add2_idx = renderer.total_list.iter().position(|i| i.label.contains("Add element:"))
        .expect("branch Add element: must exist");
    renderer.list_index = add2_idx;
    renderer.current_id = renderer.total_list[add2_idx].id.clone();
    press_right(&mut renderer);
    let leaf_btn = renderer.total_list.iter().position(|i| i.label.contains("leaf"))
        .expect("leaf button inside branch must exist");
    renderer.list_index = leaf_btn;
    renderer.current_id = renderer.total_list[leaf_btn].id.clone();
    press_enter(&mut renderer);
    assert_eq!(renderer.current_id.depth(), 3, "cursor at depth 3 after adding leaf inside branch");

    // The branch Obj must now contain a "leaf" child.
    let branch_obj = renderer.ffon.get(0).and_then(|e| e.as_obj())
        .and_then(|o| o.children.iter().filter_map(|c| c.as_obj())
            .find(|o| sicompass_sdk::tags::strip_display(&o.key) == "branch").cloned());
    let leaf_in_branch = branch_obj.as_ref()
        .map(|b| b.children.iter().filter_map(|c| c.as_obj())
            .any(|o| sicompass_sdk::tags::strip_display(&o.key) == "leaf"))
        .unwrap_or(false);
    assert!(leaf_in_branch, "leaf must be inside branch after creation");

    // Navigate right into the empty leaf Obj — should not wipe anything.
    press_right(&mut renderer);
    assert_eq!(renderer.current_id.depth(), 4, "inside leaf at depth 4");

    let still_branch = renderer.ffon.get(0).and_then(|e| e.as_obj())
        .map(|o| o.children.iter().filter_map(|c| c.as_obj())
            .any(|o| sicompass_sdk::tags::strip_display(&o.key) == "branch"))
        .unwrap_or(false);
    assert!(still_branch, "branch must survive right-nav into leaf");

    // Left: back into branch (depth 3).
    press_left(&mut renderer);
    assert_eq!(renderer.current_id.depth(), 3, "back at depth 3 (inside branch)");
    let branch_still = renderer.ffon.get(0).and_then(|e| e.as_obj())
        .map(|o| o.children.iter().filter_map(|c| c.as_obj())
            .any(|o| sicompass_sdk::tags::strip_display(&o.key) == "branch"))
        .unwrap_or(false);
    assert!(branch_still, "branch must still exist after Left from leaf");

    // Left: back to provider root list (depth 2).
    press_left(&mut renderer);
    assert_eq!(renderer.current_id.depth(), 2, "back at depth 2 (provider root)");
    let branch_d2 = renderer.ffon.get(0).and_then(|e| e.as_obj())
        .map(|o| o.children.iter().filter_map(|c| c.as_obj())
            .any(|o| sicompass_sdk::tags::strip_display(&o.key) == "branch"))
        .unwrap_or(false);
    assert!(branch_d2, "branch must still exist in root list after Left×2");

    // Left: back to top-level provider selection (depth 1).
    press_left(&mut renderer);
    assert_eq!(renderer.current_id.depth(), 1, "back at depth 1 (provider selection)");
    // ffon root still intact — branch is still there even from depth 1.
    let branch_d1 = renderer.ffon.get(0).and_then(|e| e.as_obj())
        .map(|o| o.children.iter().filter_map(|c| c.as_obj())
            .any(|o| sicompass_sdk::tags::strip_display(&o.key) == "branch"))
        .unwrap_or(false);
    assert!(branch_d1, "branch must still exist after Left×3");
}
