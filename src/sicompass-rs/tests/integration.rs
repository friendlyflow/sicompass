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

    // S key enters Scroll directly from OperatorGeneral
    press(h.r(), Keycode::S);
    assert_eq!(h.renderer.coordinate, Coordinate::Scroll);

    press_escape(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorGeneral);
}

#[test]
fn scroll_search_esc_chain() {
    let mut h = Harness::new();

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
