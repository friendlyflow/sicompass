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

        // Settings (no-op apply fn)
        let settings = SettingsProvider::new(|_, _| {});
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

        let settings = SettingsProvider::new(|_, _| {});
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
    navigate_to_provider(h.r(), fb_idx);

    press_right(h.r());
    assert_eq!(h.renderer.current_id.depth(), 2, "should be inside provider");
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorGeneral);

    press_left(h.r());
    assert_eq!(h.renderer.current_id.depth(), 1, "should be back at root");
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
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());

    let count_before = h.renderer.ffon[fb_idx].as_obj().unwrap().children.len();
    assert!(count_before >= 3);

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

    // Second Tab from search -> Scroll
    press_tab(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::Scroll);

    press_escape(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::SimpleSearch);

    press_escape(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::OperatorGeneral);
}

#[test]
fn scroll_search_esc_chain() {
    let mut h = Harness::new();

    press_tab(h.r());  // -> SimpleSearch
    press_tab(h.r());  // -> Scroll
    assert_eq!(h.renderer.coordinate, Coordinate::Scroll);

    press_ctrl(h.r(), Keycode::F);
    assert_eq!(h.renderer.coordinate, Coordinate::ScrollSearch);

    press_escape(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::Scroll);

    press_escape(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::SimpleSearch);

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

    // Second element (index 1) should be the URL bar with <input> tag
    let wb_obj = h.renderer.ffon[wb_idx].as_obj().unwrap();
    assert!(wb_obj.children.len() >= 2, "web browser should have meta + url bar");

    let url_elem = &wb_obj.children[1];
    let url_str = url_elem.as_obj().map(|o| o.key.as_str())
        .or_else(|| url_elem.as_str())
        .unwrap_or("");
    assert!(
        sicompass_sdk::tags::has_input(url_str),
        "second element of web browser should be <input> URL bar, got: {url_str:?}"
    );
}
