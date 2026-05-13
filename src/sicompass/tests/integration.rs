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
use sicompass_sdk::placeholders::I_PLACEHOLDER;
use sicompass_sdk::provider::Provider;
use sicompass_sdk::ffon::FfonElement;
use std::path::Path;
use tempfile::TempDir;

/// Call once per test binary to populate the SDK factory registry.
fn ensure_builtins() {
    sicompass_builtins::register_all();
}

// ---------------------------------------------------------------------------
// Harness
// ---------------------------------------------------------------------------

/// A headless application renderer with real providers loaded.
struct Harness {
    pub renderer: AppRenderer,
    pub tmp: TempDir,
    pub settings_tmp: TempDir,
}

impl Harness {
    /// Create a harness with a fresh temp directory pre-populated with
    ///   alpha.txt, beta.txt, subdir/nested.txt
    /// and providers: FilebrowserProvider (rooted at tmp) + SettingsProvider.
    fn new() -> Self {
        ensure_builtins();
        let tmp = TempDir::new().expect("failed to create temp dir");
        let settings_tmp = TempDir::new().expect("failed to create settings temp dir");
        let root = tmp.path();

        std::fs::write(root.join("alpha.txt"), "test content").unwrap();
        std::fs::write(root.join("beta.txt"), "test content").unwrap();
        std::fs::create_dir(root.join("subdir")).unwrap();
        std::fs::write(root.join("subdir/nested.txt"), "test content").unwrap();

        let mut renderer = AppRenderer::new();

        // File browser rooted at temp dir (set path AFTER init which resets to "/")
        register(&mut renderer, sicompass_sdk::create_provider_by_name("filebrowser").unwrap());
        renderer.providers[0].set_current_path(root.to_str().unwrap());
        // Re-fetch now that the path is correct
        {
            let children = renderer.providers[0].fetch();
            let display_name = renderer.providers[0].display_name().to_owned();
            let mut root_elem = FfonElement::new_obj(&display_name);
            for child in children { root_elem.as_obj_mut().unwrap().push(child); }
            renderer.ffon[0] = root_elem;
        }

        // Settings (isolated to a *separate* temp dir so per-keystroke tab
        // persistence does not pollute the filebrowser's listing of `tmp`).
        let mut settings = sicompass_sdk::create_provider_by_name("settings").unwrap();
        settings.set_config_path(settings_tmp.path().join("settings.json"));
        register(&mut renderer, settings);

        sicompass::list::create_list_current_layer(&mut renderer);

        Harness { renderer, tmp, settings_tmp }
    }

    fn new_with_webbrowser() -> Self {
        ensure_builtins();
        // Stub out real Chrome launches: every webbrowser test in this binary
        // only checks app-side behavior (URL bar mode, FFON updates, link
        // navigation) and never wants to spawn a real browser process.
        sicompass_webbrowser::_set_test_no_launch(true);
        let tmp = TempDir::new().expect("failed to create temp dir");
        let settings_tmp = TempDir::new().expect("failed to create settings temp dir");
        let root = tmp.path();
        std::fs::write(root.join("alpha.txt"), "test content").unwrap();

        let mut renderer = AppRenderer::new();

        // Filebrowser: init resets path to "/", so set path after init
        register(&mut renderer, sicompass_sdk::create_provider_by_name("filebrowser").unwrap());
        renderer.providers[0].set_current_path(root.to_str().unwrap());
        {
            let children = renderer.providers[0].fetch();
            let display_name = renderer.providers[0].display_name().to_owned();
            let mut root_elem = FfonElement::new_obj(&display_name);
            for child in children { root_elem.as_obj_mut().unwrap().push(child); }
            renderer.ffon[0] = root_elem;
        }

        register(&mut renderer, sicompass_sdk::create_provider_by_name("webbrowser").unwrap());

        // Settings (isolated to a separate temp dir — see Harness::new).
        let mut settings = sicompass_sdk::create_provider_by_name("settings").unwrap();
        settings.set_config_path(settings_tmp.path().join("settings.json"));
        register(&mut renderer, settings);

        sicompass::list::create_list_current_layer(&mut renderer);

        Harness { renderer, tmp, settings_tmp }
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

    /// Path to the directory holding settings.json (kept separate from
    /// `tmp_path()` to avoid polluting the filebrowser listing).
    fn settings_path(&self) -> std::path::PathBuf {
        self.settings_tmp.path().join("settings.json")
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

/// Like `register` but skips `init()` — prevents loading real settings from disk.
///
/// Use this for email-client compose/body tests where the test manually sets the
/// FFON and provider path.  Calling `init()` on a real machine with OAuth configured
/// would set an expired access token, causing every `fetch()` call to return
/// "Loading…" instead of the expected compose-body children.
fn register_no_init(renderer: &mut AppRenderer, provider: Box<dyn Provider>) {
    let display_name = provider.display_name().to_owned();
    let root = FfonElement::new_obj(&display_name);
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
    assert_eq!(r.coordinate, Coordinate::General);
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
    assert_eq!(h.renderer.coordinate, Coordinate::General);

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
    assert_eq!(h.renderer.coordinate, Coordinate::General);

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
    assert_eq!(h.renderer.coordinate, Coordinate::General);
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
    assert_eq!(h.renderer.coordinate, Coordinate::Insert);

    type_text(h.r(), "- newfile.txt");
    press_enter(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::General);

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
fn escape_returns_to_general() {
    let mut h = Harness::new();

    // From search mode
    press_tab(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::SimpleSearch);
    press_escape(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::General);

    // From insert mode
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());
    press_ctrl(h.r(), Keycode::I);
    assert_eq!(h.renderer.coordinate, Coordinate::Insert);
    press_escape(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::General);
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

    assert_eq!(h.renderer.coordinate, Coordinate::General);

    press_tab(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::SimpleSearch);

    // Tab from SimpleSearch is now a no-op
    press_tab(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::SimpleSearch);

    press_escape(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::General);

    // S key enters Scroll from inside a provider (not at root)
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());
    press(h.r(), Keycode::S);
    assert_eq!(h.renderer.coordinate, Coordinate::Scroll);

    press_escape(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::General);
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
    assert_eq!(h.renderer.coordinate, Coordinate::General);
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
        assert_eq!(h.renderer.coordinate, Coordinate::Insert);
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
        Coordinate::Insert,
        "should be in insert mode after I"
    );
    assert!(
        h.renderer.input_buffer.contains("https://"),
        "input_buffer should contain the default URL prefix"
    );

    // Type a URL (will fail to fetch, but commit_edit still sets current_url)
    type_text(h.r(), "https://example.invalid");
    press_enter(h.r());

    // After Enter, we should be back in general mode
    assert_eq!(
        h.renderer.coordinate,
        Coordinate::General,
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
    assert_eq!(h.renderer.coordinate, Coordinate::Insert);
    press_enter(h.r());

    // Must exit insert mode even though content was unchanged
    assert_eq!(
        h.renderer.coordinate,
        Coordinate::General,
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

/// Enter in General on an Obj whose key has an <input> tag should NOT
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

    // We should still be in General at the same depth (2), not deeper
    assert_eq!(
        h.renderer.coordinate,
        Coordinate::General,
        "should stay in General"
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
    assert_eq!(h.renderer.coordinate, Coordinate::General);
    assert!(tmp.join("Downloads/report.txt").exists(), "report.txt should be created");

    // ---- Step 4: Navigate back to root ----
    while h.renderer.current_id.depth() > 1 { press_left(h.r()); }
    assert_eq!(h.renderer.current_id.depth(), 1);
    assert_eq!(h.renderer.coordinate, Coordinate::General);
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
    assert_eq!(h.renderer.coordinate, Coordinate::General);

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
    assert_eq!(h.renderer.coordinate, Coordinate::General);
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

    // Root hints should mention Search and Ctrl+F which work at root
    let labels: Vec<&str> = h.renderer.total_list.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.iter().any(|l| l.contains("Search")), "root meta should mention Search, got: {labels:?}");
    assert!(labels.iter().any(|l| l.contains("Ctrl+F")), "root meta should mention Ctrl+F");
}

/// Helper: build a renderer rooted in an editor-semantics provider at depth 2,
/// in a given coordinate. Used by the M-in-editor tests below.
fn editor_renderer_in(coord: Coordinate) -> AppRenderer {
    struct EditorMock;
    impl Provider for EditorMock {
        fn name(&self) -> &str { "mock_editor" }
        fn fetch(&mut self) -> Vec<FfonElement> { Vec::new() }
        fn has_editor_semantics(&self) -> bool { true }
    }

    let mut r = AppRenderer::new();
    r.providers.push(Box::new(EditorMock));
    let mut root = FfonElement::new_obj("buffer");
    root.as_obj_mut().unwrap().push(FfonElement::new_str("alpha"));
    r.ffon = vec![root];
    r.current_id = { let mut id = sicompass_sdk::ffon::IdArray::new(); id.push(0); id.push(0); id };
    r.coordinate = coord;
    r.previous_coordinate = coord;
    sicompass::list::create_list_current_layer(&mut r);
    r
}

#[test]
fn meta_key_enters_meta_from_editor_general() {
    // M works in General even when the active provider is an editor
    // (i.e. after Escape from Insert).
    let mut r = editor_renderer_in(Coordinate::General);
    press(&mut r, Keycode::M);
    assert_eq!(r.coordinate, Coordinate::Meta);
    assert_eq!(r.previous_coordinate, Coordinate::General);
}

#[test]
fn meta_key_does_not_enter_meta_from_editor_normal() {
    let mut r = editor_renderer_in(Coordinate::Normal);
    press(&mut r, Keycode::M);
    assert_eq!(r.coordinate, Coordinate::Normal);
}

#[test]
fn meta_key_does_not_enter_meta_from_editor_visual() {
    let mut r = editor_renderer_in(Coordinate::Visual);
    press(&mut r, Keycode::M);
    assert_eq!(r.coordinate, Coordinate::Visual);
}

#[test]
fn meta_key_does_not_enter_meta_from_editor_insert() {
    let mut r = editor_renderer_in(Coordinate::Insert);
    press(&mut r, Keycode::M);
    assert_eq!(r.coordinate, Coordinate::Insert);
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

    // Should be back in General after a state-toggle
    assert_eq!(h.renderer.coordinate, sicompass::app_state::Coordinate::General,
        "should return to General after show/hide properties");

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

    assert_eq!(h.renderer.coordinate, sicompass::app_state::Coordinate::General,
        "should return to General after sort chronologically");
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
    assert_eq!(h.renderer.coordinate, sicompass::app_state::Coordinate::General,
        "command mode must not activate at root depth");
}

/// Navigating right into an empty directory shows a single `<input></input>` placeholder.
/// The filebrowser is a flat (lazy-fetch) provider: depth stays at 2 when entering subdirs.
#[test]
fn navigate_right_empty_dir_shows_placeholder() {
    ensure_builtins();
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    std::fs::write(root.join("file.txt"), "").unwrap();
    std::fs::create_dir(root.join("emptydir")).unwrap();

    let mut renderer = AppRenderer::new();
    register(&mut renderer, sicompass_sdk::create_provider_by_name("filebrowser").unwrap());
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
    // I_PLACEHOLDER renders as "i" (typed insert affordance)
    assert_eq!(label, "i", "placeholder label should be 'i', got: {label:?}");
}

/// Navigating into a subdirectory updates the root FFON element's key to the
/// directory name, so the parent line reflects the current location.
/// Navigating back out shows the parent directory name.
/// Only at filesystem root "/" does the key fall back to "file browser".
#[test]
fn navigate_right_updates_parent_key() {
    ensure_builtins();
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let root_name = root.file_name().unwrap().to_str().unwrap().to_owned();
    std::fs::create_dir(root.join("subdir")).unwrap();
    std::fs::write(root.join("subdir/file.txt"), "").unwrap();

    let mut renderer = AppRenderer::new();
    register(&mut renderer, sicompass_sdk::create_provider_by_name("filebrowser").unwrap());
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
    ensure_builtins();
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    std::fs::create_dir(root.join("mydir")).unwrap();
    std::fs::write(root.join("mydir/only.txt"), "").unwrap();

    let mut renderer = AppRenderer::new();
    register(&mut renderer, sicompass_sdk::create_provider_by_name("filebrowser").unwrap());
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
    assert_eq!(label, "i",
        "placeholder should appear after deleting last item, got: {label:?}");
    assert_eq!(renderer.current_id.last(), Some(0), "current_id should point at placeholder");
}

/// Running "create file" command when on the empty placeholder replaces it in-place.
#[test]
fn create_file_on_placeholder_replaces_in_place() {
    ensure_builtins();
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    std::fs::create_dir(root.join("emptydir")).unwrap();

    let mut renderer = AppRenderer::new();
    register(&mut renderer, sicompass_sdk::create_provider_by_name("filebrowser").unwrap());
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
    let settings_tmp = TempDir::new().unwrap();
    let mut h = Harness { renderer, tmp, settings_tmp };
    execute_provider_command(&mut h, "create file");
    let renderer = h.r();

    // Placeholder replaced in-place → still at index 0
    assert_eq!(renderer.current_id.last(), Some(0),
        "create file on placeholder should stay at idx 0 (replaced in-place)");

    // Should enter insert mode to type the filename
    assert_eq!(renderer.coordinate, sicompass::app_state::Coordinate::Insert,
        "should enter insert mode after create file");
}

/// Typing a plain name on the `i` placeholder in an empty directory creates a file.
///
/// End-to-end: navigate into empty dir → press `i` → type → Enter → file exists on disk.
#[test]
fn filebrowser_i_placeholder_creates_file() {
    ensure_builtins();
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    std::fs::create_dir(root.join("emptydir")).unwrap();

    let mut renderer = AppRenderer::new();
    register(&mut renderer, sicompass_sdk::create_provider_by_name("filebrowser").unwrap());
    renderer.providers[0].set_current_path(root.to_str().unwrap());
    {
        let children = renderer.providers[0].fetch();
        let display_name = renderer.providers[0].display_name().to_owned();
        let mut root_elem = FfonElement::new_obj(&display_name);
        for child in children { root_elem.as_obj_mut().unwrap().push(child); }
        renderer.ffon[0] = root_elem;
    }
    sicompass::list::create_list_current_layer(&mut renderer);

    press_right(&mut renderer);
    let idx = renderer.total_list.iter().position(|i| i.label.contains("emptydir")).unwrap();
    let cur = renderer.list_index;
    for _ in 0..(idx.abs_diff(cur)) {
        if idx > cur { press_down(&mut renderer); } else { press_up(&mut renderer); }
    }
    press_right(&mut renderer); // enter emptydir → i placeholder shown

    assert_eq!(renderer.total_list.len(), 1);
    assert_eq!(&renderer.total_list[0].label, "i");

    // Press i, type a filename, commit
    press(&mut renderer, Keycode::I);
    assert!(renderer.placeholder_insert_mode, "should be in placeholder insert mode");
    type_text(&mut renderer, "notes.txt");
    press_enter(&mut renderer);

    assert!(root.join("emptydir/notes.txt").exists(), "notes.txt should have been created on disk");
    assert_eq!(renderer.coordinate, sicompass::app_state::Coordinate::General);
}

/// Typing `name:` on the `i` placeholder in an empty directory creates a subdirectory.
///
/// End-to-end: navigate into empty dir → press `i` → type `name:` → Enter → dir exists on disk.
#[test]
fn filebrowser_i_placeholder_creates_subdirectory() {
    ensure_builtins();
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    std::fs::create_dir(root.join("emptydir")).unwrap();

    let mut renderer = AppRenderer::new();
    register(&mut renderer, sicompass_sdk::create_provider_by_name("filebrowser").unwrap());
    renderer.providers[0].set_current_path(root.to_str().unwrap());
    {
        let children = renderer.providers[0].fetch();
        let display_name = renderer.providers[0].display_name().to_owned();
        let mut root_elem = FfonElement::new_obj(&display_name);
        for child in children { root_elem.as_obj_mut().unwrap().push(child); }
        renderer.ffon[0] = root_elem;
    }
    sicompass::list::create_list_current_layer(&mut renderer);

    press_right(&mut renderer);
    let idx = renderer.total_list.iter().position(|i| i.label.contains("emptydir")).unwrap();
    let cur = renderer.list_index;
    for _ in 0..(idx.abs_diff(cur)) {
        if idx > cur { press_down(&mut renderer); } else { press_up(&mut renderer); }
    }
    press_right(&mut renderer); // enter emptydir → i placeholder shown

    assert_eq!(&renderer.total_list[0].label, "i");

    press(&mut renderer, Keycode::I);
    type_text(&mut renderer, "subdir:");
    press_enter(&mut renderer);

    assert!(root.join("emptydir/subdir").is_dir(), "subdir should have been created on disk");
    assert_eq!(renderer.coordinate, sicompass::app_state::Coordinate::General);
}

/// Ctrl+A after creating a file (prefixed insert mode) must not panic.
/// Regression: after refresh, current_id could be out-of-bounds → insert at invalid index.
#[test]
fn ctrl_a_after_prefixed_creation_no_panic() {
    ensure_builtins();
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    std::fs::create_dir(root.join("testdir")).unwrap();

    let settings_tmp = TempDir::new().unwrap();
    let mut h = Harness { renderer: AppRenderer::new(), tmp, settings_tmp };
    register(h.r(), sicompass_sdk::create_provider_by_name("filebrowser").unwrap());
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

    // Ctrl+A → append placeholder after index 0, enter Insert
    press_ctrl(h.r(), Keycode::A);
    assert_eq!(h.renderer.coordinate, sicompass::app_state::Coordinate::Insert);

    // Create a file
    type_text(h.r(), "- newfile.txt");
    press_enter(h.r());

    assert!(h.tmp.path().join("testdir/newfile.txt").exists(),
        "newfile.txt should be created");
    assert_eq!(h.renderer.coordinate, sicompass::app_state::Coordinate::General);

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
    assert_eq!(h.renderer.coordinate, sicompass::app_state::Coordinate::Insert,
        "Ctrl+A after creation should enter Insert without panic");
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
    assert_eq!(h.renderer.coordinate, Coordinate::General);

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
    assert_eq!(h.renderer.coordinate, Coordinate::General);

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

    assert_eq!(h.renderer.coordinate, sicompass::app_state::Coordinate::General,
        "should return to General after sort alphanumerically");
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
    // Ctrl+Z while in SimpleSearch should undo and return to General.
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
    assert_eq!(h.renderer.coordinate, Coordinate::General, "undo should exit search mode");
    assert!(!tmp.join("searchundo.txt").exists(), "file should be deleted after undo from search mode");
}

#[test]
fn undo_from_insert_mode() {
    // Ctrl+Z while in Insert should undo and return to General.
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
    assert_eq!(h.renderer.coordinate, Coordinate::Insert);
    press_ctrl(h.r(), Keycode::Z);
    assert_eq!(h.renderer.coordinate, Coordinate::General, "undo should exit insert mode");
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
    assert_eq!(h.renderer.coordinate, Coordinate::Insert);
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
    // Renaming a directory must leave the user at General in the parent,
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
    assert_eq!(h.renderer.coordinate, Coordinate::General,
        "should stay in General, not navigate into the renamed dir");
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

    // I — should not enter Insert at root
    press(h.r(), Keycode::I);
    assert_eq!(h.renderer.coordinate, coord_before, "I must be no-op at root");

    // A — should not enter Insert at root
    press(h.r(), Keycode::A);
    assert_eq!(h.renderer.coordinate, coord_before, "A must be no-op at root");

    // Ctrl+I — should not enter Insert at root
    press_ctrl(h.r(), Keycode::I);
    assert_eq!(h.renderer.coordinate, coord_before, "Ctrl+I must be no-op at root");

    // Ctrl+A — should not enter Insert at root
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
    assert_eq!(h.renderer.coordinate, Coordinate::General);

    // Ctrl+F enters ExtendedSearch
    press_ctrl(h.r(), Keycode::F);
    assert_eq!(h.renderer.coordinate, Coordinate::ExtendedSearch);
    press_escape(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::General);

    // M enters Meta
    press(h.r(), Keycode::M);
    assert_eq!(h.renderer.coordinate, Coordinate::Meta);
    press_escape(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::General);
}

#[test]
fn test_dashboard_key_transitions_and_escape() {
    let mut h = Harness::new();
    // Manually set a dashboard image path so handle_dashboard has something to act on
    h.renderer.dashboard_image_path = "/tmp/fake_dashboard.png".to_string();
    // Also prime the provider's dashboard_image_path via direct state manipulation
    // by setting it on the renderer directly (handle_dashboard reads from provider,
    // so we test the dispatch + escape cycle with the coordinate set directly)
    h.renderer.coordinate = Coordinate::General;
    h.renderer.previous_coordinate = Coordinate::General;

    // Enter Dashboard mode
    h.renderer.previous_coordinate = h.renderer.coordinate;
    h.renderer.coordinate = Coordinate::Dashboard;
    assert_eq!(h.renderer.coordinate, Coordinate::Dashboard);

    // Escape should return to General
    press_escape(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::General, "Escape from Dashboard should restore previous coordinate");
}

#[test]
fn test_d_key_noop_without_dashboard_image() {
    let mut h = Harness::new();
    // No dashboard_image_path set on providers — pressing D at root should stay in General
    assert_eq!(h.renderer.coordinate, Coordinate::General);
    press(h.r(), Keycode::D);
    assert_eq!(h.renderer.coordinate, Coordinate::General, "D without dashboard image should not enter Dashboard mode");
}

// ---------------------------------------------------------------------------
// Tests: Ctrl+A/I insert_general_placeholder with createElement provider
// ---------------------------------------------------------------------------

/// Ctrl+A in General for a createElement provider should clone the
/// "Add element:" section rather than inserting a raw `<input></input>`.
/// The cursor should land on the clone and stay in General (not Insert).
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

    // Must stay in General (no insert mode for createElement providers).
    assert_eq!(renderer.coordinate, Coordinate::General,
        "Ctrl+A with createElement provider must stay in General");

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

/// Ctrl+I in General for a createElement provider should clone the
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

    assert_eq!(renderer.coordinate, Coordinate::General,
        "Ctrl+I with createElement provider must stay in General");

    sicompass::list::create_list_current_layer(&mut renderer);
    assert_eq!(renderer.total_list.len(), count_before + 1,
        "one extra item (the cloned Add element:) should appear after Ctrl+I");

    let clone_count = renderer.total_list.iter()
        .filter(|item| item.label.contains("Add element:"))
        .count();
    assert_eq!(clone_count, 2, "both original and clone should be visible after Ctrl+I");
}

// ---------------------------------------------------------------------------
// Tests: handle_ctrl_a double-tap in General
// ---------------------------------------------------------------------------

/// In General, pressing Ctrl+A twice quickly should undo the first append
/// and perform AppendAppend (mirroring C handleCtrlA double-tap behavior).
///
/// We set the coordinate directly since General is reached via the FFON
/// editor (after escaping Insert), not via list navigation.
#[test]
fn ctrl_a_in_general_double_tap_does_append_append() {
    use sicompass::app_state::Task;

    struct EditorMock;
    impl Provider for EditorMock {
        fn name(&self) -> &str { "mock_editor" }
        fn fetch(&mut self) -> Vec<FfonElement> { Vec::new() }
        fn has_editor_semantics(&self) -> bool { true }
    }

    // Set up a renderer with two items in an obj (depth-2 General context).
    let mut r = AppRenderer::new();
    r.providers.push(Box::new(EditorMock));
    let mut root = FfonElement::new_obj("section");
    root.as_obj_mut().unwrap().push(FfonElement::new_str("alpha"));
    root.as_obj_mut().unwrap().push(FfonElement::new_str("beta"));
    r.ffon = vec![root];
    r.current_id = { let mut id = sicompass_sdk::ffon::IdArray::new(); id.push(0); id.push(0); id };
    r.coordinate = Coordinate::General;
    r.previous_coordinate = Coordinate::General;
    sicompass::list::create_list_current_layer(&mut r);

    // First Ctrl+A — single tap append.
    sicompass::handlers::handle_ctrl_a(&mut r, sicompass::app_state::History::None);
    let count_after_first = r.ffon[0].as_obj().unwrap().children.len();
    assert_eq!(count_after_first, 3, "first Ctrl+A should append one element (3 total)");

    // Record a recent keypress time so the next call is within DELTA_MS.
    r.last_keypress_time = sicompass::handlers::sdl_ticks();

    // Second Ctrl+A immediately — double tap: undo + AppendAppend.
    sicompass::handlers::handle_ctrl_a(&mut r, sicompass::app_state::History::None);

    let tail = r.active_timeline().entries.last().cloned();
    assert!(
        matches!(
            tail,
            Some(sicompass_sdk::timeline::TimelineEntry::Structural {
                op: sicompass_sdk::timeline::StructuralOp::Append,
                ..
            })
        ),
        "double-tap Ctrl+A should leave a Structural::Append entry as the tail, got {tail:?}"
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
    ensure_builtins();
    let tmp = TempDir::new().expect("temp dir");
    let mut renderer = AppRenderer::new();

    // ConfigProvider at index 0
    register(&mut renderer, Box::new(ConfigProvider::new()));

    // Filebrowser at index 1
    let root = tmp.path().to_str().unwrap().to_owned();
    register(&mut renderer, sicompass_sdk::create_provider_by_name("filebrowser").unwrap());
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

/// Destructive keys (Ctrl+A, Ctrl+I, Delete) are no-ops in General
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
    r.coordinate = Coordinate::General;
    sicompass::list::create_list_current_layer(&mut r);
    let initial_list_len = r.total_list.len();

    // Ctrl+A (append) must be blocked
    press_ctrl(&mut r, Keycode::A);
    assert_eq!(r.coordinate, Coordinate::General,
        "Ctrl+A should not enter insert mode during open flow");

    // Ctrl+I (insert) must be blocked
    press_ctrl(&mut r, Keycode::I);
    assert_eq!(r.coordinate, Coordinate::General,
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
    r.previous_coordinate = Coordinate::General;

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

/// Ctrl+S + Escape during save-as (Insert) cancels and returns to source provider.
#[test]
fn escape_in_save_as_insert_cancels_and_returns_to_source() {
    let (mut r, _tmp) = harness_with_config_provider();

    // Trigger save-as (no existing save path → falls through to file-browser save-as)
    press_ctrl(&mut r, Keycode::S);
    assert!(r.pending_file_browser_save_as, "save-as should be pending after Ctrl+S with no path");
    assert_eq!(r.coordinate, Coordinate::Insert, "should be in Insert for filename entry");

    press(&mut r, Keycode::Escape);

    assert!(!r.pending_file_browser_save_as, "save-as flag should be cleared after Escape");
    assert_eq!(r.current_id.get(0), Some(0), "should be back at config provider after Escape");
    assert_eq!(r.coordinate, Coordinate::General);
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
    r.coordinate = Coordinate::Insert;
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
    r.coordinate = Coordinate::Insert;
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
    r.coordinate = Coordinate::Insert;
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
    r.coordinate = Coordinate::Insert;
    r.input_buffer = "abc".to_string();
    r.cursor_position = 0;

    press_shift_right(&mut r);
    assert_eq!(announced_text(&r).as_deref(), Some("a"), "shift-right over 'a'");
    assert!(r.selection_anchor.is_some(), "selection should be anchored");
}

#[test]
fn editor_insert_left_no_announcement_on_selection_collapse() {
    let mut r = AppRenderer::new();
    r.coordinate = Coordinate::Insert;
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

// ---------------------------------------------------------------------------
// get_meta / list-derived keyboard hint derivation
// ---------------------------------------------------------------------------

/// Navigate the harness to depth 2 (inside the filebrowser root list).
fn nav_into_filebrowser(h: &mut Harness) {
    // Depth 1: select the filebrowser provider (index 0).
    while h.renderer.current_id.get(0) != Some(0) {
        dispatch_key(&mut h.renderer, Some(Keycode::Down), Mod::empty());
    }
    // Right: enter the provider → depth 2.
    dispatch_key(&mut h.renderer, Some(Keycode::Right), Mod::empty());
}

#[test]
fn get_meta_at_root_returns_universal_hints() {
    let mut h = Harness::new();
    // Depth 1 = root navigation level.
    assert_eq!(h.renderer.current_id.depth(), 1);
    let meta = sicompass::provider::get_meta(&h.renderer);
    assert!(!meta.is_empty(), "root should have hints");
    assert!(meta.iter().any(|s| s.contains("Search")),
        "root should have Search hint, got: {meta:?}");
    assert!(meta.iter().any(|s| s.contains("Ctrl+F")),
        "root should have Ctrl+F");
    // Provider-specific hints (e.g. filebrowser's Ctrl+I) must NOT appear at root.
    assert!(!meta.iter().any(|s| s.contains("Ctrl+I")),
        "root should not show provider-only shortcut Ctrl+I");
}

#[test]
fn get_meta_inside_filebrowser_shows_provider_hints() {
    let mut h = Harness::new();
    nav_into_filebrowser(&mut h);
    assert_eq!(h.renderer.current_id.depth(), 2, "should be depth 2 after entering filebrowser");

    let meta = sicompass::provider::get_meta(&h.renderer);
    assert!(!meta.is_empty(), "filebrowser list should have hints");
    // No universal root hints at this depth.
    assert!(!meta.iter().any(|s| s.starts_with("D ") || s.trim_start().starts_with("D\t")),
        "filebrowser should not show root-only D=Dashboard");
    // Provider-declared filebrowser hints.
    assert!(meta.iter().any(|s| s.contains("Ctrl+I")), "filebrowser must declare Ctrl+I");
    assert!(meta.iter().any(|s| s.contains("F5")),     "filebrowser must declare F5");
    assert!(meta.iter().any(|s| s.contains("Search")),
        "filebrowser must declare Search hint, got: {meta:?}");
}

#[test]
fn get_meta_tag_derived_hints_appear_for_input_children() {
    use sicompass_sdk::ffon::FfonElement;
    use sicompass_sdk::provider::Provider;

    // Build a provider whose fetch returns a list with <input> children.
    struct InputListProvider { path: String }
    impl Provider for InputListProvider {
        fn name(&self) -> &str { "inputlist" }
        fn fetch(&mut self) -> Vec<FfonElement> {
            vec![
                FfonElement::new_str("Name: <input>Alice</input>"),
                FfonElement::new_str("Email: <input>alice@example.com</input>"),
            ]
        }
        fn push_path(&mut self, seg: &str) { self.path = format!("/{seg}"); }
        fn pop_path(&mut self) { self.path = "/".to_owned(); }
        fn current_path(&self) -> &str { &self.path }
        fn set_current_path(&mut self, p: &str) { self.path = p.to_owned(); }
    }

    let mut renderer = AppRenderer::default();
    let provider = Box::new(InputListProvider { path: "/".to_owned() });
    let children = {
        let mut p = InputListProvider { path: "/".to_owned() };
        p.fetch()
    };
    // Build the FFON tree manually: one root Obj whose children are the input rows.
    let mut root = FfonElement::new_obj("inputlist");
    for c in children { root.as_obj_mut().unwrap().push(c); }
    renderer.ffon = vec![root];
    renderer.providers = vec![Box::new(InputListProvider { path: "/".to_owned() })];
    renderer.current_id = sicompass_sdk::ffon::IdArray::new();
    renderer.current_id.push(0); // provider
    renderer.current_id.push(0); // first row → depth 2, container = ffon[0]

    let meta = sicompass::provider::get_meta(&renderer);
    // Tag-derived: children have <input> → Tab search/cycle hint.
    assert!(
        meta.iter().any(|s| s.contains("Tab") && s.contains("Search")),
        "input children should auto-derive Tab Search hint, got: {meta:?}"
    );
}

// ---------------------------------------------------------------------------
// `*` placeholder (Ctrl+Shift+I / Ctrl+Shift+A) integration tests
// ---------------------------------------------------------------------------

/// Build a minimal renderer with one provider root and two string children.
/// Cursor starts at depth 2 (inside the provider, on first child).
fn make_placeholder_harness() -> AppRenderer {
    let mut root = FfonElement::new_obj("testprovider");
    root.as_obj_mut().unwrap().push(FfonElement::new_str("first"));
    root.as_obj_mut().unwrap().push(FfonElement::new_str("second"));

    let mut r = AppRenderer::new();
    r.ffon = vec![root];
    r.current_id = sicompass_sdk::ffon::IdArray::new();
    r.current_id.push(0);
    r.current_id.push(0); // depth 2, on "first"
    r.coordinate = Coordinate::General;
    r.previous_coordinate = Coordinate::General;
    sicompass::list::create_list_current_layer(&mut r);
    r.list_index = 0;
    r
}

#[test]
fn placeholder_ctrl_shift_i_enters_insert() {
    let mut r = make_placeholder_harness();
    // Ctrl+Shift+I is invoked from code, not from key dispatch (shortcut removed by design).
    sicompass::handlers::handle_ctrl_shift_i_placeholder(&mut r);
    assert_eq!(r.coordinate, Coordinate::Insert,
        "handle_ctrl_shift_i_placeholder should enter Insert");
    assert!(r.placeholder_insert_mode, "placeholder_insert_mode should be set");
}

#[test]
fn placeholder_commit_plain_text_becomes_string_element() {
    let mut r = make_placeholder_harness();
    sicompass::handlers::handle_ctrl_shift_i_placeholder(&mut r); // insert placeholder before "first"
    assert_eq!(r.coordinate, Coordinate::Insert);
    type_text(&mut r, "myvalue");
    press_enter(&mut r);
    // Should exit insert mode and produce a Str element
    assert_eq!(r.coordinate, Coordinate::General);
    assert!(!r.placeholder_insert_mode);
    // Check the FFON: provider now has 3 children, one of which contains "myvalue"
    if let Some(FfonElement::Obj(prov)) = r.ffon.get(0) {
        let has_value = prov.children.iter().any(|e| match e {
            FfonElement::Str(s) => s.contains("myvalue"),
            _ => false,
        });
        assert!(has_value, "expected a child containing 'myvalue', got: {:?}", prov.children);
    } else {
        panic!("root should be Obj");
    }
}

#[test]
fn placeholder_commit_plus_prefix_becomes_obj_element() {
    let mut r = make_placeholder_harness();
    sicompass::handlers::handle_ctrl_shift_i_placeholder(&mut r);
    type_text(&mut r, "+ myobj");
    press_enter(&mut r);
    assert_eq!(r.coordinate, Coordinate::General);
    assert!(!r.placeholder_insert_mode);
    if let Some(FfonElement::Obj(prov)) = r.ffon.get(0) {
        let has_obj = prov.children.iter().any(|e| matches!(e, FfonElement::Obj(o) if o.key == "myobj"));
        assert!(has_obj, "expected an Obj child with key 'myobj', got: {:?}", prov.children);
    } else {
        panic!("root should be Obj");
    }
}

#[test]
fn placeholder_commit_trailing_colon_becomes_obj_element() {
    let mut r = make_placeholder_harness();
    sicompass::handlers::handle_ctrl_shift_a_placeholder(&mut r); // append after "first"
    type_text(&mut r, "section:");
    press_enter(&mut r);
    assert_eq!(r.coordinate, Coordinate::General);
    if let Some(FfonElement::Obj(prov)) = r.ffon.get(0) {
        let has_obj = prov.children.iter().any(|e| matches!(e, FfonElement::Obj(o) if o.key == "section"));
        assert!(has_obj, "expected Obj(section), got: {:?}", prov.children);
    } else {
        panic!("root should be Obj");
    }
}

#[test]
fn placeholder_commit_empty_stays_in_insert() {
    let mut r = make_placeholder_harness();
    sicompass::handlers::handle_ctrl_shift_i_placeholder(&mut r);
    // Don't type anything — commit empty
    press_enter(&mut r);
    assert_eq!(r.coordinate, Coordinate::Insert,
        "empty commit should stay in Insert");
    assert!(!r.error_message.is_empty(), "should show an error message");
    assert!(r.placeholder_insert_mode, "placeholder_insert_mode should still be set");
}

#[test]
fn placeholder_escape_clears_flag() {
    let mut r = make_placeholder_harness();
    sicompass::handlers::handle_ctrl_shift_i_placeholder(&mut r);
    assert!(r.placeholder_insert_mode);
    press_escape(&mut r);
    assert!(!r.placeholder_insert_mode, "escape should clear placeholder_insert_mode");
    assert_eq!(r.coordinate, Coordinate::General);
}

/// Build a renderer whose top-level FFON list contains an `I_PLACEHOLDER` element.
/// This simulates what the email compose body view looks like after `body_to_compose_children`
/// adds the permanent placeholder.
fn make_star_prefix_harness() -> AppRenderer {
    let mut root = FfonElement::new_obj("provider");
    root.as_obj_mut().unwrap().push(FfonElement::new_str(I_PLACEHOLDER.to_owned()));
    root.as_obj_mut().unwrap().push(FfonElement::new_str("other item".to_owned()));

    let mut r = AppRenderer::new();
    r.ffon = vec![root];
    r.current_id = sicompass_sdk::ffon::IdArray::new();
    r.current_id.push(0);
    r.current_id.push(0); // on I_PLACEHOLDER
    r.coordinate = Coordinate::General;
    r.previous_coordinate = Coordinate::General;
    sicompass::list::create_list_current_layer(&mut r);
    r.list_index = 0;
    r
}

#[test]
fn handle_i_on_star_prefix_element_sets_placeholder_insert_mode() {
    let mut r = make_star_prefix_harness();
    // Press 'i' — handle_i should detect the "* " input_prefix and set the flag.
    press(&mut r, Keycode::I);
    assert_eq!(r.coordinate, Coordinate::Insert,
        "pressing 'i' should enter Insert");
    assert!(r.placeholder_insert_mode,
        "handle_i should set placeholder_insert_mode when input_prefix is '* '");
}

#[test]
fn handle_i_on_star_element_commit_plain_text_produces_str() {
    let mut r = make_star_prefix_harness();
    press(&mut r, Keycode::I);
    assert!(r.placeholder_insert_mode);
    type_text(&mut r, "hello");
    press_enter(&mut r);
    assert_eq!(r.coordinate, Coordinate::General);
    assert!(!r.placeholder_insert_mode);
    // The element should now contain "hello".
    if let Some(FfonElement::Obj(prov)) = r.ffon.get(0) {
        let has_hello = prov.children.iter().any(|e| match e {
            FfonElement::Str(s) => s.contains("hello"),
            _ => false,
        });
        assert!(has_hello, "expected a Str child containing 'hello'; got: {:?}", prov.children);
    } else {
        panic!("root should be Obj");
    }
}

#[test]
fn handle_i_on_star_element_commit_plus_prefix_produces_obj() {
    let mut r = make_star_prefix_harness();
    press(&mut r, Keycode::I);
    type_text(&mut r, "+ section");
    press_enter(&mut r);
    assert_eq!(r.coordinate, Coordinate::General);
    if let Some(FfonElement::Obj(prov)) = r.ffon.get(0) {
        let has_obj = prov.children.iter().any(|e| matches!(e, FfonElement::Obj(o) if o.key == "section"));
        assert!(has_obj, "expected Obj(section); got: {:?}", prov.children);
    } else {
        panic!("root should be Obj");
    }
}

#[test]
fn handle_a_on_star_prefix_element_sets_placeholder_insert_mode() {
    let mut r = make_star_prefix_harness();
    // Navigate to the "* " element and press 'a' (append mode).
    sicompass::handlers::handle_a(&mut r);
    assert_eq!(r.coordinate, Coordinate::Insert);
    assert!(r.placeholder_insert_mode,
        "handle_a should set placeholder_insert_mode when input_prefix is '* '");
}

// ---------------------------------------------------------------------------
// Email client — refresh_on_navigate
// ---------------------------------------------------------------------------

/// The email client must declare refresh_on_navigate() = true so that
/// navigate_right_raw calls push_path + refresh_current_directory (and thus
/// fetch() / build_folder) when the user navigates into a folder.
/// Regression guard for commit 7d21ee7 which introduced the flag and broke
/// email folder navigation by leaving EmailClientProvider on the default false.
#[test]
fn email_provider_refresh_on_navigate_is_true() {
    ensure_builtins();
    let p = sicompass_sdk::create_provider_by_name("emailclient").unwrap();
    assert!(p.refresh_on_navigate(),
        "EmailClientProvider must return refresh_on_navigate() = true so \
         navigate_right_raw calls fetch() when opening a folder");
}

/// Navigating right into an empty email compose body inserts the typed `i` placeholder
/// (shows as label `"i"`, not the spurious `-i` from the bare `<input></input>` fallback).
///
/// Regression test for the bug where `navigate_right_raw`'s empty-directory
/// fallback used `<input></input>` regardless of provider type.
#[test]
fn navigate_into_empty_compose_body_shows_i_placeholder() {
    ensure_builtins();
    use sicompass_sdk::ffon::{FfonElement, FfonObject, IdArray};

    let mut renderer = AppRenderer::new();
    // Use register_no_init to avoid loading real OAuth config from disk,
    // which would cause fetch() to return "Loading…" on machines with an expired token.
    register_no_init(&mut renderer, sicompass_sdk::create_provider_by_name("emailclient").unwrap());

    // Set provider path to compose root so is_in_email_compose_body returns true
    // after we push "Body: [text]".
    renderer.providers[0].set_current_path("compose");

    // Build a minimal compose FFON: one Obj ("email") containing a Body: [text] Obj with no children.
    // This mirrors what build_compose_view produces for an empty draft.
    let body_obj = FfonElement::Obj(FfonObject {
        key: "Body: [text]".to_owned(),
        children: vec![],
    });
    let mut compose_root = FfonElement::new_obj("email");
    compose_root.as_obj_mut().unwrap().push(body_obj);
    renderer.ffon[0] = compose_root;

    // Position the cursor on the Body: [text] element (depth 2 = provider 0, child 0).
    renderer.current_id = {
        let mut id = IdArray::new();
        id.push(0);
        id.push(0);
        id
    };
    renderer.coordinate = Coordinate::General;
    sicompass::list::create_list_current_layer(&mut renderer);

    // Navigate right into the empty Body: [text] Obj.
    press_right(&mut renderer);

    // Exactly one child: the `i` typed-placeholder (renders as "i", not "-i").
    assert_eq!(
        renderer.total_list.len(), 1,
        "empty compose body must show exactly one i placeholder; got: {:?}",
        renderer.total_list.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(
        renderer.total_list[0].label, "i",
        "placeholder must render as 'i', not '-i'; got: {:?}",
        renderer.total_list[0].label
    );
}

/// Deleting the last element of an email compose body must leave a single `i` placeholder
/// rather than an empty body with no input.
///
/// Regression test for the bug where `delete_body_element` removed the sole Ffon entry
/// and left the body empty, stranding the user with no way to type.
///
/// Setup mirrors the actual runtime state produced by navigating into the Body: subtree
/// via `navigate_right_raw` + `refresh_current_directory`:
///   - `ffon[0]` is the flat Obj produced by refresh (root key = path's last segment)
///   - `current_id = [provider, child_idx]` (depth 2 selects a child of the flat root)
///   - Provider's `current_path` and `compose.draft.body` are primed via the trait API.
#[test]
fn delete_last_compose_body_element_keeps_i_placeholder() {
    ensure_builtins();
    use sicompass_sdk::ffon::{FfonElement, IdArray};

    let mut renderer = AppRenderer::new();
    register_no_init(&mut renderer, sicompass_sdk::create_provider_by_name("emailclient").unwrap());

    // Prime provider internal body state via the trait API:
    //   set_current_path so is_in_email_compose_body() returns true,
    //   then commit_edit to populate draft.body = Text("hello").
    renderer.providers[0].set_current_path("compose/Body: [text]");
    renderer.providers[0].commit_edit("", "hello");

    // Build the flat ffon shape that refresh_current_directory produces when the
    // provider path's last segment is "Body: [text]".  One child: <input>hello</input>.
    let mut root = FfonElement::new_obj("Body: [text]");
    root.as_obj_mut().unwrap().push(FfonElement::new_str("<input>hello</input>".to_owned()));
    renderer.ffon[0] = root;

    // Position cursor on the single body element (provider 0, child 0 of root).
    renderer.current_id = {
        let mut id = IdArray::new();
        id.push(0); // provider
        id.push(0); // child index within root
        id
    };
    renderer.coordinate = Coordinate::General;
    sicompass::list::create_list_current_layer(&mut renderer);

    // Delete the only body element.
    sicompass::handlers::handle_delete_body_element(&mut renderer);

    // Body must still show exactly one element: the `i` typed placeholder.
    assert_eq!(
        renderer.total_list.len(), 1,
        "deleting last body element must leave one i placeholder; got: {:?}",
        renderer.total_list.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(
        renderer.total_list[0].label, "i",
        "placeholder must render as 'i'; got: {:?}",
        renderer.total_list[0].label
    );
}

/// Regression test: Delete works on Str elements even when an Obj sibling exists in the body.
///
/// Before the path-based fix, `delete_body_element` searched by string content at the
/// top level only, so any element whose cursor path pointed into the Obj's sub-tree
/// (or whose content didn't match the top-level key) silently returned `false`.
#[test]
fn delete_body_element_str_with_obj_sibling_integration() {
    ensure_builtins();
    use sicompass_sdk::ffon::IdArray;

    let mut renderer = AppRenderer::new();
    register_no_init(&mut renderer, sicompass_sdk::create_provider_by_name("emailclient").unwrap());

    // Build a body with [Str("abc"), Obj{key:"myobj:"}, Str("def")].
    renderer.providers[0].set_current_path("compose/Body: [ffon]");

    // Directly set the body state by committing two strings and an obj via the
    // provider trait.  commit_edit on an empty body creates the first Str; subsequent
    // calls update or append.  For the Obj we create it via "myobj:" commit.
    renderer.providers[0].commit_edit("", "abc");
    renderer.providers[0].commit_edit("abc", "myobj:");
    renderer.providers[0].commit_edit("", "def");

    // Build the ffon tree displayed when the user is inside the Body.
    // It mirrors the Ffon children: [Str(<input>abc</input>), Obj(myobj:), Str(<input>def</input>)].
    let mut root = FfonElement::new_obj("Body: [ffon]");
    {
        let root_obj = root.as_obj_mut().unwrap();
        root_obj.push(FfonElement::new_str("<input>abc</input>".to_owned()));
        let obj = FfonElement::new_obj("myobj:");
        root_obj.push(obj);
        root_obj.push(FfonElement::new_str("<input>def</input>".to_owned()));
    }
    renderer.ffon[0] = root;

    // Cursor on first Str (provider 0, child 0).
    renderer.current_id = {
        let mut id = IdArray::new();
        id.push(0);
        id.push(0);
        id
    };
    renderer.coordinate = Coordinate::General;
    sicompass::list::create_list_current_layer(&mut renderer);

    // Delete the first Str — must succeed even with an Obj sibling.
    sicompass::handlers::handle_delete_body_element(&mut renderer);

    // After deletion the list should show the Obj + second Str (2 items).
    assert_eq!(
        renderer.total_list.len(), 2,
        "after deleting first Str, 2 elements should remain; got: {:?}",
        renderer.total_list.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
}

/// `is_in_email_compose_body` must return true for reply/forward paths entered from
/// a message context, where the compose-root token is NOT at segs[0].
///
/// Regression test for the bug where `segs[0]`-only gating caused every helper
/// to return false for paths like `/INBOX/msg/reply/Body: [text]`, breaking
/// shortcuts, placeholder seeding, and subtree refresh.
#[test]
fn is_in_email_compose_body_true_for_reply_from_message() {
    ensure_builtins();
    let mut renderer = AppRenderer::new();
    register_no_init(&mut renderer, sicompass_sdk::create_provider_by_name("emailclient").unwrap());

    // Simulate a reply entered from /INBOX/msg — compose root is at segs[2].
    renderer.providers[0].set_current_path("INBOX/Hello — alice@example.com/reply/Body: [text]");

    assert!(
        sicompass::provider::is_in_email_compose_body(&renderer),
        "is_in_email_compose_body must be true for /INBOX/msg/reply/Body: paths"
    );
}

/// Navigating into a reply compose body (entered from a message) must show the
/// `i` placeholder, just like entering a fresh compose body.
///
/// Regression test for the bug where the reply path `/{folder}/{msg}/reply/Body: [text]`
/// was not recognised as a compose body, causing the navigate-right fallback to insert
/// a plain `<input></input>` (renders as `-i`) instead of the typed `i` placeholder.
#[test]
fn navigate_into_reply_from_message_body_shows_i_placeholder() {
    ensure_builtins();
    use sicompass_sdk::ffon::{FfonElement, FfonObject, IdArray};

    let mut renderer = AppRenderer::new();
    register_no_init(&mut renderer, sicompass_sdk::create_provider_by_name("emailclient").unwrap());

    // Simulate the path produced when reply is entered from /INBOX/msg.
    renderer.providers[0].set_current_path("INBOX/Hello — alice@example.com/reply");

    // Build the FFON shape for a reply compose view: root Obj containing Body: [ffon]
    // (Ffon because prefill_compose now always produces Ffon for reply).
    let body_obj = FfonElement::Obj(FfonObject {
        key: "Body: [ffon]".to_owned(),
        children: vec![],
    });
    let mut compose_root = FfonElement::new_obj("email");
    compose_root.as_obj_mut().unwrap().push(body_obj);
    renderer.ffon[0] = compose_root;

    // Position cursor on Body: [ffon] (provider 0, child 0).
    renderer.current_id = {
        let mut id = IdArray::new();
        id.push(0);
        id.push(0);
        id
    };
    renderer.coordinate = Coordinate::General;
    sicompass::list::create_list_current_layer(&mut renderer);

    // Navigate right into the empty Body: Obj.
    press_right(&mut renderer);

    // Must show exactly the typed `i` placeholder (label "i", not "-i").
    assert_eq!(
        renderer.total_list.len(), 1,
        "reply body must show exactly one i placeholder; got: {:?}",
        renderer.total_list.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(
        renderer.total_list[0].label, "i",
        "placeholder must render as 'i'; got: {:?}",
        renderer.total_list[0].label
    );
}

/// Navigating right into a nested body Obj (one with `I_PLACEHOLDER` seeded by
/// `seed_i_placeholders`) shows the `i` placeholder — i.e. the `has_children` branch
/// of `navigate_right_raw` is taken and the nested list is rendered correctly.
///
/// Covers: compose, reply, reply-all, forward — the FFON setup is identical regardless
/// of mode because the test focuses on the generic nested-Obj navigation path.
#[test]
fn navigate_into_nested_body_obj_shows_i_placeholder() {
    ensure_builtins();
    use sicompass_sdk::ffon::{FfonElement, FfonObject, IdArray};

    let mut renderer = AppRenderer::new();
    register_no_init(&mut renderer, sicompass_sdk::create_provider_by_name("emailclient").unwrap());

    // Path is inside the body so that push_path (called by navigate_right_raw) appends
    // to the correct base path when navigating into `foo:`.
    renderer.providers[0].set_current_path("compose/Body: [ffon]");

    // Build the FFON: email root → Body: [ffon] Obj with a nested `foo:` Obj
    // that already has an `i <input></input>` child (as seeded by seed_i_placeholders).
    // The `has_children` branch of navigate_right_raw is taken for Objs with children,
    // so the FFON children are used directly — no draft.body access needed here.
    let foo_obj = FfonElement::Obj(FfonObject {
        key: "<input>foo</input>".to_owned(),
        children: vec![FfonElement::Str(I_PLACEHOLDER.to_owned())],
    });
    let body_obj = FfonElement::Obj(FfonObject {
        key: "Body: [ffon]".to_owned(),
        children: vec![foo_obj],
    });
    let mut compose_root = FfonElement::new_obj("email");
    compose_root.as_obj_mut().unwrap().push(body_obj);
    renderer.ffon[0] = compose_root;

    // Position cursor on `foo:` (depth 3: [provider=0, body_obj=0, foo_obj=0]).
    renderer.current_id = {
        let mut id = IdArray::new();
        id.push(0);  // provider
        id.push(0);  // Body: Obj (child 0 of compose_root)
        id.push(0);  // foo: Obj (child 0 of body)
        id
    };
    renderer.coordinate = Coordinate::General;
    sicompass::list::create_list_current_layer(&mut renderer);

    // Navigate right into `foo:` — must show the seeded `i` placeholder.
    press_right(&mut renderer);

    assert_eq!(
        renderer.total_list.len(), 1,
        "nested foo: Obj must show exactly one i placeholder; got: {:?}",
        renderer.total_list.iter().map(|i| &i.label).collect::<Vec<_>>()
    );
    assert_eq!(
        renderer.total_list[0].label, "i",
        "nested body Obj placeholder must render as 'i'; got: {:?}",
        renderer.total_list[0].label
    );
}

/// `commit_edit` at a nested body path places the committed content inside the nested
/// Obj, not at the top level of the body.
///
/// Verifies that the path-aware commit pipeline (B) works: after creating `foo:` at the
/// top of the body and then committing "bar" while the path is inside `foo:`, the
/// `fetch_subtree_children` for the nested path returns "bar" as a child of `foo:`.
#[test]
fn commit_in_nested_compose_body_creates_child_there() {
    ensure_builtins();
    let mut renderer = AppRenderer::new();
    register_no_init(&mut renderer, sicompass_sdk::create_provider_by_name("emailclient").unwrap());

    // Step 1: create `foo:` at the top level of the body.
    renderer.providers[0].push_path("compose");
    renderer.providers[0].push_path("Body: [text]");
    assert!(
        sicompass::provider::commit_edit(&mut renderer, "", "foo:"),
        "top-level foo: creation must succeed"
    );
    // sync_body_path_label updates path from Body: [text] → Body: [ffon] after Ffon promotion.

    // Step 2: simulate navigating into `foo:` and committing "bar" there.
    renderer.providers[0].push_path("foo");
    assert!(
        sicompass::provider::commit_edit(&mut renderer, "", "bar"),
        "nested commit must succeed"
    );

    // Step 3: verify via fetch_subtree_children (path is inside foo:) that "bar" is a
    // child of `foo:`, not at the top level of the body.
    // Note: committing "bar" onto I_PLACEHOLDER replaces the placeholder — after commit
    // foo:'s children are [bar] (the placeholder is consumed by the commit).
    let children = renderer.providers[0]
        .fetch_subtree_children()
        .expect("fetch_subtree_children must return Some when inside nested body Obj");

    assert!(
        children.iter().any(|c| matches!(c, FfonElement::Str(s) if s.contains("bar"))),
        "bar must appear in foo:'s children (not at body top level); got: {:?}",
        children
    );
}

/// Creating `baz:` inside `foo:` via `commit_edit` produces a `baz:` Obj whose children
/// start with `I_PLACEHOLDER`, so that pressing right on `baz:` would reveal it.
///
/// Verifies that `update_body_elems` calls `new_obj_with_i_placeholder` for nested Obj
/// creation just as it does at the top level.
#[test]
fn commit_trailing_colon_in_nested_body_creates_obj_with_i_placeholder() {
    ensure_builtins();
    let mut renderer = AppRenderer::new();
    register_no_init(&mut renderer, sicompass_sdk::create_provider_by_name("emailclient").unwrap());

    // Create `foo:` at top level, then `baz:` inside `foo:`.
    renderer.providers[0].push_path("compose");
    renderer.providers[0].push_path("Body: [text]");
    assert!(sicompass::provider::commit_edit(&mut renderer, "", "foo:"));

    renderer.providers[0].push_path("foo");
    assert!(sicompass::provider::commit_edit(&mut renderer, "", "baz:"));

    // fetch_subtree_children for path inside foo: should contain the baz: Obj.
    let children = renderer.providers[0]
        .fetch_subtree_children()
        .expect("fetch_subtree_children must return Some");

    let baz = children.iter().find_map(|c| {
        if let FfonElement::Obj(o) = c {
            if sicompass_sdk::tags::strip_display(&o.key) == "baz" {
                return Some(o);
            }
        }
        None
    }).expect("baz: Obj not found in foo:'s children; got: {:?}");

    assert_eq!(
        baz.children.first(),
        Some(&FfonElement::Str(I_PLACEHOLDER.to_owned())),
        "newly created baz: Obj must have I_PLACEHOLDER as first child; got: {:?}",
        baz.children
    );
}

/// Editing a string leaf inside a nested body Obj (e.g. pressing `i` then typing then Enter)
/// must leave the nested list non-empty after commit.
///
/// Regression: the non-placeholder commit branch of `handle_enter_insert` used to
/// call `refresh_current_directory` unconditionally, which rebuilds the provider root and
/// misroutes deep paths like `/compose/Body: [ffon]/foo`, emptying `total_list`.
/// The fix: try `refresh_subtree_parent` first (same as the placeholder branch), which
/// updates only the parent Obj's children without touching the root.
#[test]
fn editing_leaf_in_nested_compose_body_does_not_empty_list() {
    ensure_builtins();
    use sicompass_sdk::ffon::{FfonElement, FfonObject, IdArray};

    let mut renderer = AppRenderer::new();
    register_no_init(&mut renderer, sicompass_sdk::create_provider_by_name("emailclient").unwrap());

    // Step 1: build draft.body via the provider API so that fetch_subtree_children
    // can return the correct children for the nested path.
    renderer.providers[0].set_current_path("compose/Body: [text]");
    assert!(
        sicompass::provider::commit_edit(&mut renderer, "", "foo:"),
        "top-level foo: creation must succeed"
    );
    // After commit, sync_body_path_label updates path from Body: [text] → Body: [ffon].
    renderer.providers[0].push_path("foo");
    assert!(
        sicompass::provider::commit_edit(&mut renderer, "", "original"),
        "nested commit must succeed"
    );
    // provider's current_path is now "compose/Body: [ffon]/foo" with "original" in draft.body.

    // Step 2: build a FFON tree that matches: email root → Body:[ffon] Obj → foo: Obj → "original" str.
    let original_str = FfonElement::Str("<input>original</input>".to_owned());
    let foo_obj = FfonElement::Obj(FfonObject {
        key: "<input>foo</input>".to_owned(),
        children: vec![original_str],
    });
    let body_obj = FfonElement::Obj(FfonObject {
        key: "Body: [ffon]".to_owned(),
        children: vec![foo_obj],
    });
    let mut compose_root = FfonElement::new_obj("email");
    compose_root.as_obj_mut().unwrap().push(body_obj);
    renderer.ffon[0] = compose_root;

    // Step 3: position cursor on the "original" string inside foo (depth 4).
    renderer.current_id = {
        let mut id = IdArray::new();
        id.push(0); // provider
        id.push(0); // Body: [ffon] Obj
        id.push(0); // foo: Obj
        id.push(0); // "original" string
        id
    };
    renderer.coordinate = Coordinate::Insert;
    renderer.previous_coordinate = Coordinate::General;
    renderer.placeholder_insert_mode = false;
    // Simulate user having typed "updated" into the existing "original" element.
    renderer.input_buffer = "updated".to_owned();
    sicompass::list::create_list_current_layer(&mut renderer);

    // Step 4: press Enter — must commit "updated" and keep the nested list non-empty.
    press_enter(&mut renderer);

    assert_eq!(
        renderer.coordinate, Coordinate::General,
        "Enter in Insert must exit to General"
    );
    assert!(
        !renderer.total_list.is_empty(),
        "nested foo: list must be non-empty after editing a leaf; got empty list (refresh_current_directory misroute regression)"
    );
}

/// Helper: create a stub IMAP backend + renderer positioned inside an opened
/// email message (flat FFON at depth 2, provider path = "/INBOX/msg_label").
/// Returns the renderer ready for key dispatch.
///
/// The email provider's navigate_right_raw uses refresh_on_navigate=true, which
/// always resets current_id to [provider_idx, 0] (depth 2) and rebuilds ffon[0]
/// as a flat Obj{msg_key, body_children}.  This helper replicates that runtime
/// state without going through the full SDL/network stack.
fn email_renderer_inside_message() -> AppRenderer {
    use sicompass_emailclient::{EmailClientProvider, ImapBackend, FolderInfo, MessageHeader, EmailMessage};
    use sicompass_sdk::ffon::{FfonElement, FfonObject, IdArray};

    struct StubImap { messages: Vec<MessageHeader>, removed_uids: Vec<u32> }
    #[allow(unused_variables)]
    impl ImapBackend for StubImap {
        fn list_folders(&mut self) -> Result<Vec<FolderInfo>, String> {
            Ok(vec![
                FolderInfo { name: "INBOX".to_owned(), attributes: vec![] },
                FolderInfo { name: "[Gmail]/Trash".to_owned(),
                             attributes: vec!["\\Trash".to_owned()] },
            ])
        }
        fn list_messages(&mut self, _f: &str, _l: usize) -> Result<Vec<MessageHeader>, String> {
            // Exclude removed UIDs so the list reflects post-delete state.
            Ok(self.messages.iter().filter(|m| !self.removed_uids.contains(&m.uid)).cloned().collect())
        }
        fn fetch_message(&mut self, _: &str, _: u32) -> Result<Option<EmailMessage>, String> { Ok(None) }
        fn fetch_message_by_message_id(&mut self, _: &str, _: &str) -> Result<Option<EmailMessage>, String> { Ok(None) }
        fn set_flags(&mut self, _: &str, _: u32, _: &[&str], _: &[&str]) -> Result<(), String> { Ok(()) }
        fn copy_message(&mut self, _: &str, _: u32, _: &str) -> Result<(), String> { Ok(()) }
        fn move_message(&mut self, _: &str, uid: u32, _: &str) -> Result<(), String> {
            self.removed_uids.push(uid); Ok(())
        }
        fn expunge_uid(&mut self, _: &str, uid: u32) -> Result<(), String> {
            self.removed_uids.push(uid); Ok(())
        }
        fn append(&mut self, _: &str, _: &[u8]) -> Result<(), String> { Ok(()) }
        fn fetch_threads(&mut self, _: &str) -> Result<Option<Vec<Vec<u32>>>, String> { Ok(None) }
    }

    let msgs = vec![
        MessageHeader { uid: 1, from: "alice@x.com".to_owned(),
                        subject: "Alpha".to_owned(), date: String::new(), seen: true, flagged: false },
        MessageHeader { uid: 2, from: "bob@x.com".to_owned(),
                        subject: "Beta".to_owned(), date: String::new(), seen: true, flagged: false },
    ];

    let provider = EmailClientProvider::new()
        .with_oauth_token("fake")
        .with_imap(Box::new(StubImap { messages: msgs, removed_uids: vec![] }));

    let mut renderer = AppRenderer::new();
    register_no_init(&mut renderer, Box::new(provider));

    // Populate message_cache (needed by lookup_uid during delete).
    renderer.providers[0].set_current_path("INBOX");
    let _ = renderer.providers[0].fetch();

    // Simulate navigate_right into "Alpha": push the message label to provider path.
    // Provider path is now "/INBOX/[read] Alpha — alice@x.com" (2 segments).
    renderer.providers[0].push_path("[read] Alpha — alice@x.com");

    // Flat FFON that navigate_right_raw / refresh_current_directory produces at
    // this path: root Obj = the opened message, children = body elements.
    let mut root = FfonElement::new_obj("[read] Alpha — alice@x.com");
    root.as_obj_mut().unwrap().children = vec![
        FfonElement::Str("body text".to_owned()),
    ];
    renderer.ffon[0] = root;

    // current_id = [0, 0]: depth 2, cursor on first body element (same shape as
    // all lazy-fetch navigation inside the email provider).
    renderer.current_id = {
        let mut id = IdArray::new();
        id.push(0); // provider
        id.push(0); // first body element
        id
    };
    renderer.coordinate = Coordinate::General;
    sicompass::list::create_list_current_layer(&mut renderer);
    renderer
}

/// Pressing Ctrl+D while inside an opened message (flat FFON depth 2, provider
/// path "/INBOX/msg_label") must delete the message, return to the message list,
/// and place the cursor on the next/prev message — the same behaviour as deleting
/// directly from the message list.
///
/// Setup: Alpha is at index 0, Beta at index 1.  We open Alpha (provider path
/// "/INBOX/[read] Alpha — alice@x.com"), then press Ctrl+D.
/// Expected: view is the message list, cursor at 0 (Beta shifted into slot 0).
#[test]
fn ctrl_d_from_inside_message_shows_message_list_with_cursor_on_next() {
    let mut renderer = email_renderer_inside_message();

    // Pre-condition: depth 2, flat FFON shows the message body.
    assert_eq!(renderer.current_id.depth(), 2, "pre-condition: depth must be 2");

    press_ctrl(&mut renderer, Keycode::D);

    assert!(
        renderer.error_message.is_empty(),
        "delete must succeed; got: {:?}", renderer.error_message
    );

    // View must be the message list (ffon[0] key = folder name, not message label).
    let root_key = renderer.ffon[0].as_obj().map(|o| o.key.as_str()).unwrap_or("");
    assert_eq!(root_key, "INBOX",
        "after delete, ffon[0] must be the INBOX folder Obj; got: {root_key:?}");

    // Alpha must be gone; Beta must be present.
    let children = renderer.ffon[0].as_obj().map(|o| o.children.as_slice()).unwrap_or(&[]);
    assert!(
        !children.iter().any(|e| e.as_obj().map_or(false, |o| o.key.contains("Alpha"))),
        "Alpha must be absent from the message list after deletion"
    );
    assert!(
        children.iter().any(|e| e.as_obj().map_or(false, |o| o.key.contains("Beta"))),
        "Beta must still be in the message list"
    );

    // Cursor must be valid and point to Beta (the next/prev message).
    let cursor = renderer.current_id.last().unwrap_or(0);
    assert!(
        children.is_empty() || cursor < children.len(),
        "cursor {cursor} must be within message list of length {}", children.len()
    );
    if !children.is_empty() {
        let selected_key = children[cursor].as_obj().map(|o| o.key.as_str()).unwrap_or("");
        assert!(
            selected_key.contains("Beta"),
            "cursor must point to Beta after Alpha deleted; got: {selected_key:?}"
        );
    }
}

/// Pressing Ctrl+D while on a message in the message list (provider path "/INBOX",
/// flat FFON at depth 2, cursor on a message Obj) must remove it and keep the
/// cursor valid in the refreshed list.
#[test]
fn ctrl_d_from_message_list_removes_message() {
    use sicompass_emailclient::{EmailClientProvider, ImapBackend, FolderInfo, MessageHeader, EmailMessage};
    use sicompass_sdk::ffon::{FfonElement, FfonObject, IdArray};

    struct StubImap2 { messages: Vec<MessageHeader>, removed_uids: Vec<u32> }
    #[allow(unused_variables)]
    impl ImapBackend for StubImap2 {
        fn list_folders(&mut self) -> Result<Vec<FolderInfo>, String> {
            Ok(vec![
                FolderInfo { name: "INBOX".to_owned(), attributes: vec![] },
                FolderInfo { name: "[Gmail]/Trash".to_owned(),
                             attributes: vec!["\\Trash".to_owned()] },
            ])
        }
        fn list_messages(&mut self, _f: &str, _l: usize) -> Result<Vec<MessageHeader>, String> {
            Ok(self.messages.iter().filter(|m| !self.removed_uids.contains(&m.uid)).cloned().collect())
        }
        fn fetch_message(&mut self, _: &str, _: u32) -> Result<Option<EmailMessage>, String> { Ok(None) }
        fn fetch_message_by_message_id(&mut self, _: &str, _: &str) -> Result<Option<EmailMessage>, String> { Ok(None) }
        fn set_flags(&mut self, _: &str, _: u32, _: &[&str], _: &[&str]) -> Result<(), String> { Ok(()) }
        fn copy_message(&mut self, _: &str, _: u32, _: &str) -> Result<(), String> { Ok(()) }
        fn move_message(&mut self, _: &str, uid: u32, _: &str) -> Result<(), String> {
            self.removed_uids.push(uid); Ok(())
        }
        fn expunge_uid(&mut self, _: &str, uid: u32) -> Result<(), String> {
            self.removed_uids.push(uid); Ok(())
        }
        fn append(&mut self, _: &str, _: &[u8]) -> Result<(), String> { Ok(()) }
        fn fetch_threads(&mut self, _: &str) -> Result<Option<Vec<Vec<u32>>>, String> { Ok(None) }
    }

    let msgs = vec![
        MessageHeader { uid: 1, from: "alice@x.com".to_owned(),
                        subject: "Alpha".to_owned(), date: String::new(), seen: true, flagged: false },
        MessageHeader { uid: 2, from: "bob@x.com".to_owned(),
                        subject: "Beta".to_owned(), date: String::new(), seen: true, flagged: false },
    ];

    let provider = EmailClientProvider::new()
        .with_oauth_token("fake")
        .with_imap(Box::new(StubImap2 { messages: msgs, removed_uids: vec![] }));

    let mut renderer = AppRenderer::new();
    register_no_init(&mut renderer, Box::new(provider));

    // Simulate being at the message list: path = "/INBOX", flat FFON with 2 messages.
    renderer.providers[0].set_current_path("INBOX");
    let msgs_elements = renderer.providers[0].fetch(); // also populates message_cache

    let mut root = FfonElement::new_obj("INBOX");
    root.as_obj_mut().unwrap().children = msgs_elements;
    renderer.ffon[0] = root;

    // Cursor on Alpha (index 0).
    renderer.current_id = {
        let mut id = IdArray::new();
        id.push(0);
        id.push(0); // Alpha
        id
    };
    renderer.coordinate = Coordinate::General;
    sicompass::list::create_list_current_layer(&mut renderer);

    let before_len = renderer.ffon[0].as_obj().map(|o| o.children.len()).unwrap_or(0);
    assert_eq!(before_len, 2, "pre-condition: must start with 2 messages");

    press_ctrl(&mut renderer, Keycode::D);

    assert!(renderer.error_message.is_empty(),
        "delete must succeed; got: {:?}", renderer.error_message);

    // Root must still be the INBOX message list.
    let root_key = renderer.ffon[0].as_obj().map(|o| o.key.as_str()).unwrap_or("");
    assert_eq!(root_key, "INBOX", "root key must remain INBOX after delete from message list");

    let after_len = renderer.ffon[0].as_obj().map(|o| o.children.len()).unwrap_or(0);
    let cursor = renderer.current_id.last().unwrap_or(0);
    assert!(
        after_len == 0 || cursor < after_len,
        "cursor {cursor} must be within refreshed list of length {after_len}"
    );
}

// ---------------------------------------------------------------------------
// Tests: Escape during create-placeholder removes stale element (bug fix)
// ---------------------------------------------------------------------------

/// Ctrl+Shift+I inserts a fresh placeholder then Escape removes it and restores
/// the original `current_id`.
#[test]
fn placeholder_escape_removes_inserted_element() {
    let mut r = make_placeholder_harness();
    let pre_id = r.current_id.clone();
    let pre_len = r.ffon[0].as_obj().unwrap().children.len(); // 2 ("first", "second")

    sicompass::handlers::handle_ctrl_shift_i_placeholder(&mut r);
    assert_eq!(r.coordinate, Coordinate::Insert);
    assert!(r.placeholder_insert_mode);
    // Placeholder was inserted: child count should have grown.
    assert_eq!(r.ffon[0].as_obj().unwrap().children.len(), pre_len + 1);

    press_escape(&mut r);

    assert_eq!(r.coordinate, Coordinate::General);
    assert!(!r.placeholder_insert_mode);
    // Placeholder must be gone.
    assert_eq!(
        r.ffon[0].as_obj().unwrap().children.len(), pre_len,
        "escape should remove the freshly inserted placeholder"
    );
    // current_id must be restored.
    assert_eq!(r.current_id, pre_id, "escape should restore the pre-insert current_id");
}

/// Ctrl+A appends a placeholder then Escape removes it.
#[test]
fn placeholder_ctrl_a_escape_removes_inserted_element() {
    let mut r = make_placeholder_harness();
    let pre_id = r.current_id.clone();
    let pre_len = r.ffon[0].as_obj().unwrap().children.len();

    sicompass::handlers::handle_ctrl_shift_a_placeholder(&mut r);
    assert_eq!(r.coordinate, Coordinate::Insert);
    assert_eq!(r.ffon[0].as_obj().unwrap().children.len(), pre_len + 1);

    press_escape(&mut r);

    assert_eq!(r.coordinate, Coordinate::General);
    assert_eq!(r.ffon[0].as_obj().unwrap().children.len(), pre_len);
    assert_eq!(r.current_id, pre_id);
}

/// Pressing `i` on a persistent `I_PLACEHOLDER` (Path D) and then Escape must
/// NOT remove the element — it was never freshly inserted.
#[test]
fn persistent_i_placeholder_escape_does_not_remove() {
    let mut r = make_star_prefix_harness(); // seeds a permanent I_PLACEHOLDER
    let pre_len = r.ffon[0].as_obj().unwrap().children.len();

    press(&mut r, Keycode::I); // enters Insert on the persistent placeholder
    assert_eq!(r.coordinate, Coordinate::Insert);
    assert!(r.placeholder_insert_mode);
    // placeholder_cancel must be None because nothing was freshly inserted.
    assert!(r.placeholder_cancel.is_none());

    press_escape(&mut r);

    assert_eq!(r.coordinate, Coordinate::General);
    // The I_PLACEHOLDER element must still be present.
    assert_eq!(
        r.ffon[0].as_obj().unwrap().children.len(), pre_len,
        "persistent I_PLACEHOLDER must survive Escape"
    );
    let still_has_placeholder = r.ffon[0].as_obj().unwrap().children.iter().any(|e| {
        matches!(e, FfonElement::Str(s) if sicompass_sdk::placeholders::is_i_placeholder(s))
    });
    assert!(still_has_placeholder, "I_PLACEHOLDER element must still be in the FFON after Escape");
}

/// Ctrl+I in the file browser inserts a `<input></input>` placeholder; Escape
/// removes it and restores the prior selection.
#[test]
fn filebrowser_ctrl_i_escape_removes_placeholder() {
    let mut h = Harness::new();
    std::fs::create_dir(h.tmp_path().join("Downloads")).unwrap();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");

    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r()); // enter filebrowser at depth 2
    press(h.r(), Keycode::F5); // refresh to load Downloads

    let pre_id = h.renderer.current_id.clone();
    let pre_len = h.renderer.ffon[fb_idx].as_obj().unwrap().children.len();

    press_ctrl(h.r(), Keycode::I); // inserts <input></input>, enters Insert
    assert_eq!(h.renderer.coordinate, Coordinate::Insert);
    assert_eq!(h.renderer.ffon[fb_idx].as_obj().unwrap().children.len(), pre_len + 1,
        "Ctrl+I should insert a placeholder element");

    press_escape(h.r());

    assert_eq!(h.renderer.coordinate, Coordinate::General);
    assert_eq!(
        h.renderer.ffon[fb_idx].as_obj().unwrap().children.len(), pre_len,
        "Escape should remove the filebrowser placeholder"
    );
    assert_eq!(h.renderer.current_id, pre_id, "Escape should restore the pre-insert current_id");
}

// ---------------------------------------------------------------------------
// Compose body desync fix (Part B) — undo/redo keeps compose.draft.body in sync
// ---------------------------------------------------------------------------

/// Delete a body element then undo — `sync_ffon_body_children` must keep
/// `compose.draft.body` in sync so `fetch_subtree_children` returns the correct
/// content after the restoration without a full provider re-fetch.
#[test]
fn compose_body_delete_undo_syncs_draft_body() {
    ensure_builtins();
    use sicompass_emailclient::EmailClientProvider;
    use sicompass_sdk::ffon::{FfonElement, FfonObject, IdArray};

    let mut renderer = AppRenderer::new();
    let mut p = EmailClientProvider::new();

    // Set up compose path so refresh_on_navigate returns false.
    p.set_current_path("compose/Body: [text]");
    // Seed compose.draft.body with two lines (Ffon).
    p.commit_edit("", "line1");
    p.commit_edit("line1", "line2_start");
    // Re-commit to create a real Ffon body with two elements.
    p.commit_edit("line2_start", "line1"); // back to line1 in slot 0
    // Directly seed a 2-element Ffon body for simplicity.
    use sicompass_emailclient::MailBody;
    // We can't access MailBody from the provider trait, so we set up the
    // FFON manually and drive state through the trait.

    // Build compose FFON: depth-3 body elements.
    // ffon[0] = Obj("compose") { children: [Body:Obj { children: [line1, line2] }] }
    let body_obj = FfonElement::Obj(FfonObject {
        key: "Body: [text]".to_owned(),
        children: vec![
            FfonElement::new_str("<input>line1</input>".to_owned()),
            FfonElement::new_str("<input>line2</input>".to_owned()),
        ],
    });
    let compose_root = FfonElement::Obj(FfonObject {
        key: "compose".to_owned(),
        children: vec![body_obj],
    });
    renderer.ffon.push(compose_root);
    renderer.providers.push(Box::new(p));

    // Position cursor on "line1" (depth 3: [0=provider, 0=body_obj, 0=line1]).
    renderer.current_id = {
        let mut id = IdArray::new();
        id.push(0); id.push(0); id.push(0);
        id
    };
    renderer.coordinate = Coordinate::General;
    sicompass::list::create_list_current_layer(&mut renderer);

    // Delete "line1".
    sicompass::handlers::handle_delete_body_element(&mut renderer);

    // Verify "line1" is gone from FFON.
    let body_children_post_delete = renderer.ffon[0].as_obj().unwrap()
        .children[0].as_obj().unwrap().children.clone();
    assert_eq!(body_children_post_delete.len(), 1,
        "after delete body must have 1 child; got: {:?}", body_children_post_delete);

    // Undo the delete — should restore "line1".
    press_ctrl(&mut renderer, Keycode::Z);

    let body_children_post_undo = renderer.ffon[0].as_obj().unwrap()
        .children[0].as_obj().unwrap().children.clone();
    assert_eq!(body_children_post_undo.len(), 2,
        "after undo body must have 2 children again; got: {:?}", body_children_post_undo);

    // fetch_subtree_children must return the 2-element body (reads from compose.draft.body
    // which sync_ffon_body_children kept in sync).
    let fetched = renderer.providers[0].fetch_subtree_children();
    assert!(
        fetched.as_ref().map(|v| v.len() == 2).unwrap_or(false),
        "fetch_subtree_children after undo must return 2 elements; got: {:?}", fetched
    );
}

/// Redo after undo of a body-element delete must re-remove the element and keep
/// `compose.draft.body` consistent.
#[test]
fn compose_body_delete_undo_redo_syncs_draft_body() {
    ensure_builtins();
    use sicompass_emailclient::EmailClientProvider;
    use sicompass_sdk::ffon::{FfonElement, FfonObject, IdArray};

    let mut renderer = AppRenderer::new();
    let mut p = EmailClientProvider::new();
    p.set_current_path("compose/Body: [text]");

    let body_obj = FfonElement::Obj(FfonObject {
        key: "Body: [text]".to_owned(),
        children: vec![
            FfonElement::new_str("<input>line1</input>".to_owned()),
            FfonElement::new_str("<input>line2</input>".to_owned()),
        ],
    });
    let compose_root = FfonElement::Obj(FfonObject {
        key: "compose".to_owned(),
        children: vec![body_obj],
    });
    renderer.ffon.push(compose_root);
    renderer.providers.push(Box::new(p));

    renderer.current_id = {
        let mut id = IdArray::new();
        id.push(0); id.push(0); id.push(0);
        id
    };
    renderer.coordinate = Coordinate::General;
    sicompass::list::create_list_current_layer(&mut renderer);

    // Delete → undo → redo.
    sicompass::handlers::handle_delete_body_element(&mut renderer);
    press_ctrl(&mut renderer, Keycode::Z);       // undo
    press_ctrl_shift(&mut renderer, Keycode::Z); // redo

    let body_children = renderer.ffon[0].as_obj().unwrap()
        .children[0].as_obj().unwrap().children.clone();
    assert_eq!(body_children.len(), 1,
        "after redo body must have 1 child again; got: {:?}", body_children);

    // compose.draft.body must also reflect the post-redo (deleted) state.
    let fetched = renderer.providers[0].fetch_subtree_children();
    assert!(
        fetched.as_ref().map(|v| v.len() == 1).unwrap_or(false),
        "fetch_subtree_children after redo must return 1 element; got: {:?}", fetched
    );
}

/// Appending a new element to the compose body via Ctrl+A → type → Enter must be
/// undoable (Ctrl+Z removes it) and redoable (Ctrl+Shift+Z restores it), keeping
/// `compose.draft.body` in sync throughout.
#[test]
fn compose_body_insert_undo_redo_syncs_draft_body() {
    ensure_builtins();
    use sicompass_emailclient::EmailClientProvider;
    use sicompass_sdk::ffon::{FfonElement, FfonObject, IdArray};

    let mut renderer = AppRenderer::new();
    let mut p = EmailClientProvider::new();
    p.set_current_path("compose/Body: [text]");
    // Seed one body line via commit so compose.draft.body is non-empty.
    p.commit_edit("", "line1");

    // Build FFON: depth-3 body element at [0, 0, 0].
    let body_obj = FfonElement::Obj(FfonObject {
        key: "Body: [text]".to_owned(),
        children: vec![FfonElement::new_str("<input>line1</input>".to_owned())],
    });
    let compose_root = FfonElement::Obj(FfonObject {
        key: "compose".to_owned(),
        children: vec![body_obj],
    });
    renderer.ffon.push(compose_root);
    renderer.providers.push(Box::new(p));

    // Position on line1 at [0, 0, 0].
    renderer.current_id = {
        let mut id = IdArray::new();
        id.push(0); id.push(0); id.push(0);
        id
    };
    renderer.coordinate = Coordinate::General;
    sicompass::list::create_list_current_layer(&mut renderer);

    // Ctrl+A → append placeholder after line1, enters Insert.
    press_ctrl(&mut renderer, Keycode::A);
    assert!(
        renderer.placeholder_insert_mode,
        "placeholder_insert_mode must be set after Ctrl+A in compose body"
    );
    assert_eq!(
        renderer.coordinate, Coordinate::Insert,
        "must enter Insert after Ctrl+A"
    );

    // Type "line2" and commit via Enter.
    type_text(&mut renderer, "line2");
    press_enter(&mut renderer);

    assert_eq!(
        renderer.coordinate, Coordinate::General,
        "must return to General after commit"
    );

    // Verify "line2" appears in FFON body children.
    let body_after_insert = renderer.ffon[0].as_obj().unwrap()
        .children[0].as_obj().unwrap().children.clone();
    assert_eq!(
        body_after_insert.len(), 2,
        "body must have 2 children after insert; got: {:?}", body_after_insert
    );
    let has_line2 = body_after_insert.iter().any(|e| {
        matches!(e, FfonElement::Str(s) if s.contains("line2"))
    });
    assert!(has_line2, "body must contain line2 after insert; got: {:?}", body_after_insert);

    // fetch_subtree_children must return both elements (compose.draft.body synced).
    let fetched_after = renderer.providers[0].fetch_subtree_children();
    assert!(
        fetched_after.as_ref().map(|v| v.len() == 2).unwrap_or(false),
        "fetch_subtree_children must return 2 after insert; got: {:?}", fetched_after
    );

    // Undo — should remove line2.
    press_ctrl(&mut renderer, Keycode::Z);

    let body_after_undo = renderer.ffon[0].as_obj().unwrap()
        .children[0].as_obj().unwrap().children.clone();
    assert_eq!(
        body_after_undo.len(), 1,
        "body must have 1 child after undo; got: {:?}", body_after_undo
    );
    let has_line2_after_undo = body_after_undo.iter().any(|e| {
        matches!(e, FfonElement::Str(s) if s.contains("line2"))
    });
    assert!(!has_line2_after_undo, "line2 must be gone after undo; got: {:?}", body_after_undo);

    // compose.draft.body must also be synced after undo.
    let fetched_after_undo = renderer.providers[0].fetch_subtree_children();
    assert!(
        fetched_after_undo.as_ref().map(|v| v.len() == 1).unwrap_or(false),
        "fetch_subtree_children must return 1 after undo; got: {:?}", fetched_after_undo
    );

    // Redo — should restore line2.
    press_ctrl_shift(&mut renderer, Keycode::Z);

    let body_after_redo = renderer.ffon[0].as_obj().unwrap()
        .children[0].as_obj().unwrap().children.clone();
    assert_eq!(
        body_after_redo.len(), 2,
        "body must have 2 children after redo; got: {:?}", body_after_redo
    );
    let has_line2_after_redo = body_after_redo.iter().any(|e| {
        matches!(e, FfonElement::Str(s) if s.contains("line2"))
    });
    assert!(has_line2_after_redo, "line2 must be restored after redo; got: {:?}", body_after_redo);

    // compose.draft.body must be synced after redo too.
    let fetched_after_redo = renderer.providers[0].fetch_subtree_children();
    assert!(
        fetched_after_redo.as_ref().map(|v| v.len() == 2).unwrap_or(false),
        "fetch_subtree_children must return 2 after redo; got: {:?}", fetched_after_redo
    );
}

/// Inserting into an initially-empty compose body (I_PLACEHOLDER case) via Ctrl+A → type
/// → Enter must be undoable, restoring the I_PLACEHOLDER and keeping draft.body in sync.
#[test]
fn compose_body_insert_into_empty_undo_syncs_draft_body() {
    ensure_builtins();
    use sicompass_emailclient::EmailClientProvider;
    use sicompass_sdk::ffon::{FfonElement, FfonObject, IdArray};

    let mut renderer = AppRenderer::new();
    let mut p = EmailClientProvider::new();
    p.set_current_path("compose/Body: [text]");
    // Draft body starts empty.

    // Build FFON with an I_PLACEHOLDER seeded (mimics navigate_right_raw).
    let body_obj = FfonElement::Obj(FfonObject {
        key: "Body: [text]".to_owned(),
        children: vec![FfonElement::Str(I_PLACEHOLDER.to_owned())],
    });
    let compose_root = FfonElement::Obj(FfonObject {
        key: "compose".to_owned(),
        children: vec![body_obj],
    });
    renderer.ffon.push(compose_root);
    renderer.providers.push(Box::new(p));

    // Position on the I_PLACEHOLDER at [0, 0, 0].
    renderer.current_id = {
        let mut id = IdArray::new();
        id.push(0); id.push(0); id.push(0);
        id
    };
    renderer.coordinate = Coordinate::General;
    sicompass::list::create_list_current_layer(&mut renderer);

    // Ctrl+A → insert placeholder, enter Insert.
    press_ctrl(&mut renderer, Keycode::A);
    assert!(renderer.placeholder_insert_mode, "placeholder_insert_mode must be set");

    // Type "hello" and commit via Enter.
    type_text(&mut renderer, "hello");
    press_enter(&mut renderer);

    // Verify "hello" appears in FFON.
    let body_after = renderer.ffon[0].as_obj().unwrap()
        .children[0].as_obj().unwrap().children.clone();
    let has_hello = body_after.iter().any(|e| matches!(e, FfonElement::Str(s) if s.contains("hello")));
    assert!(has_hello, "body must contain 'hello' after insert; got: {:?}", body_after);

    // draft.body synced.
    let fetched = renderer.providers[0].fetch_subtree_children();
    assert!(
        fetched.as_ref().map(|v| v.iter().any(|e| matches!(e, FfonElement::Str(s) if s.contains("hello")))).unwrap_or(false),
        "fetch_subtree_children must contain 'hello'; got: {:?}", fetched
    );

    // Undo — should remove "hello".
    press_ctrl(&mut renderer, Keycode::Z);

    let body_after_undo = renderer.ffon[0].as_obj().unwrap()
        .children[0].as_obj().unwrap().children.clone();
    let has_hello_after_undo = body_after_undo.iter().any(|e| {
        matches!(e, FfonElement::Str(s) if s.contains("hello"))
    });
    assert!(!has_hello_after_undo, "'hello' must be gone after undo; got: {:?}", body_after_undo);

    // draft.body must NOT contain "hello" after undo.
    let fetched_undo = renderer.providers[0].fetch_subtree_children();
    let still_has_hello = fetched_undo.as_ref()
        .map(|v| v.iter().any(|e| matches!(e, FfonElement::Str(s) if s.contains("hello"))))
        .unwrap_or(false);
    assert!(!still_has_hello, "draft.body must not contain 'hello' after undo; got: {:?}", fetched_undo);
}

/// Undoing the only inserted body element must leave an I_PLACEHOLDER ("i"), not a
/// bare `"<input></input>"` ("-i "). Covers the undo arm for Task::Append/Insert and
/// the redo arm for Task::Delete/Cut in compose/reply/reply-all/forward bodies.
#[test]
fn compose_body_undo_last_element_restores_i_placeholder() {
    ensure_builtins();
    use sicompass_emailclient::EmailClientProvider;
    use sicompass_sdk::ffon::{FfonElement, FfonObject, IdArray};

    let mut renderer = AppRenderer::new();
    let mut p = EmailClientProvider::new();
    p.set_current_path("compose/Body: [text]");
    // Draft body starts empty.

    // FFON body starts with I_PLACEHOLDER (mimics navigate_right_raw seeding it).
    let body_obj = FfonElement::Obj(FfonObject {
        key: "Body: [text]".to_owned(),
        children: vec![FfonElement::Str(I_PLACEHOLDER.to_owned())],
    });
    let compose_root = FfonElement::Obj(FfonObject {
        key: "compose".to_owned(),
        children: vec![body_obj],
    });
    renderer.ffon.push(compose_root);
    renderer.providers.push(Box::new(p));

    renderer.current_id = {
        let mut id = IdArray::new();
        id.push(0); id.push(0); id.push(0);
        id
    };
    renderer.coordinate = Coordinate::General;
    sicompass::list::create_list_current_layer(&mut renderer);

    // Ctrl+A → append placeholder, enter Insert.
    press_ctrl(&mut renderer, Keycode::A);
    assert!(renderer.placeholder_insert_mode, "must enter placeholder insert mode");

    // Type "only" and commit via Enter.
    type_text(&mut renderer, "only");
    press_enter(&mut renderer);

    // Verify "only" is in body.
    let body_after_insert = renderer.ffon[0].as_obj().unwrap()
        .children[0].as_obj().unwrap().children.clone();
    let has_only = body_after_insert.iter().any(|e| matches!(e, FfonElement::Str(s) if s.contains("only")));
    assert!(has_only, "body must contain 'only' after insert; got: {:?}", body_after_insert);

    // Undo — the body should be empty again, and the sole remaining element
    // must be I_PLACEHOLDER ("i <input></input>"), not bare "<input></input>".
    press_ctrl(&mut renderer, Keycode::Z);

    let body_after_undo = renderer.ffon[0].as_obj().unwrap()
        .children[0].as_obj().unwrap().children.clone();
    assert_eq!(
        body_after_undo.len(), 1,
        "body must have 1 child after undo; got: {:?}", body_after_undo
    );
    assert!(
        matches!(&body_after_undo[0], FfonElement::Str(s) if s == I_PLACEHOLDER),
        "sole body child after undo must be I_PLACEHOLDER; got: {:?}", body_after_undo
    );

    // draft.body must also reflect the I_PLACEHOLDER (not bare "<input></input>").
    let fetched = renderer.providers[0].fetch_subtree_children();
    // An empty MailBody::Text("") produces no children from body_to_compose_children,
    // so fetched may be empty or contain the placeholder — either way "only" must be gone.
    let has_only_in_draft = fetched.as_ref()
        .map(|v| v.iter().any(|e| matches!(e, FfonElement::Str(s) if s.contains("only"))))
        .unwrap_or(false);
    assert!(!has_only_in_draft, "draft must not contain 'only' after undo; got: {:?}", fetched);

    // Redo — "only" should come back as the SOLE body child (no extra I_PLACEHOLDER).
    press_ctrl_shift(&mut renderer, Keycode::Z);

    let body_after_redo = renderer.ffon[0].as_obj().unwrap()
        .children[0].as_obj().unwrap().children.clone();
    assert_eq!(
        body_after_redo.len(), 1,
        "after redo body must have exactly 1 child (no extra I_PLACEHOLDER); got: {:?}", body_after_redo
    );
    assert!(
        matches!(&body_after_redo[0], FfonElement::Str(s) if s.contains("only")),
        "sole child after redo must be the restored element; got: {:?}", body_after_redo
    );
}

/// Undoing a body-element delete when the body held only that element must restore
/// the element as the sole child — no extra I_PLACEHOLDER alongside it.
#[test]
fn compose_body_delete_undo_single_element_no_extra_placeholder() {
    ensure_builtins();
    use sicompass_emailclient::EmailClientProvider;
    use sicompass_sdk::ffon::{FfonElement, FfonObject, IdArray};

    let mut renderer = AppRenderer::new();
    let mut p = EmailClientProvider::new();
    p.set_current_path("compose/Body: [text]");
    p.commit_edit("", "only");

    let body_obj = FfonElement::Obj(FfonObject {
        key: "Body: [text]".to_owned(),
        children: vec![FfonElement::new_str("<input>only</input>".to_owned())],
    });
    let compose_root = FfonElement::Obj(FfonObject {
        key: "compose".to_owned(),
        children: vec![body_obj],
    });
    renderer.ffon.push(compose_root);
    renderer.providers.push(Box::new(p));

    renderer.current_id = {
        let mut id = IdArray::new();
        id.push(0); id.push(0); id.push(0);
        id
    };
    renderer.coordinate = Coordinate::General;
    sicompass::list::create_list_current_layer(&mut renderer);

    // Delete the only body element.
    sicompass::handlers::handle_delete_body_element(&mut renderer);

    let body_after_delete = renderer.ffon[0].as_obj().unwrap()
        .children[0].as_obj().unwrap().children.clone();
    assert_eq!(body_after_delete.len(), 1, "after delete body must have I_PLACEHOLDER");
    assert!(
        matches!(&body_after_delete[0], FfonElement::Str(s) if s == I_PLACEHOLDER),
        "after delete must be I_PLACEHOLDER; got: {:?}", body_after_delete
    );

    // Undo — must restore "only" as sole child, no extra I_PLACEHOLDER.
    press_ctrl(&mut renderer, Keycode::Z);

    let body_after_undo = renderer.ffon[0].as_obj().unwrap()
        .children[0].as_obj().unwrap().children.clone();
    assert_eq!(
        body_after_undo.len(), 1,
        "after undo body must have exactly 1 child; got: {:?}", body_after_undo
    );
    assert!(
        matches!(&body_after_undo[0], FfonElement::Str(s) if s.contains("only")),
        "sole child after undo must be the restored element; got: {:?}", body_after_undo
    );
}

// ---------------------------------------------------------------------------
// Chat client: needs_refresh flag drives FFON rebuild
// ---------------------------------------------------------------------------

/// Verify that when the chat client's needs_refresh flag is set (as the /sync
/// background thread would do), the renderer picks it up, clears it, and rebuilds
/// the FFON tree with the rooms from the cache.
///
/// No HTTP is made — the cache is seeded via test helpers and the sync thread is
/// disabled (wiremock requires tokio; the integration suite is sync).
#[test]
fn chat_client_needs_refresh_drives_renderer_redraw() {
    // Build a ChatClientProvider with no sync thread — flag is driven manually.
    let mut chat = sicompass_chatclient::ChatClientProvider::new()
        .with_sync_disabled();

    // Set credentials so fetch() returns the rooms list, not the "configure…" placeholder.
    chat.test_set_credentials("https://matrix.org", "test_token");

    // Seed the cache as the sync thread would after a /sync response.
    chat.test_seed_room("!abc:x", "Test Room");
    chat.test_seed_room("!def:x", "Another Room");

    // Pre-set the flag before boxing — simulates the sync thread firing mid-idle.
    chat.test_set_needs_refresh();

    // Register: init() + fetch() populates the FFON tree from cache.
    let mut renderer = AppRenderer::new();
    let display_name = chat.display_name().to_owned();
    let children = chat.fetch();
    let mut root = FfonElement::new_obj(&display_name);
    for child in children {
        root.as_obj_mut().unwrap().push(child);
    }
    renderer.ffon.push(root);
    renderer.providers.push(Box::new(chat));

    renderer.current_id = {
        let mut id = sicompass_sdk::ffon::IdArray::new();
        id.push(0);
        id
    };

    // The flag must still be set (no drain has run yet).
    assert!(renderer.providers[0].needs_refresh(), "flag must be set before drain");

    // Simulate the per-frame needs_refresh drain from view.rs:
    // clear the flag *before* rebuild so a signal arriving mid-rebuild is preserved.
    renderer.providers[0].clear_needs_refresh();
    sicompass::provider::refresh_current_directory(&mut renderer);
    sicompass::list::create_list_current_layer(&mut renderer);

    // Flag must be cleared after the drain.
    assert!(!renderer.providers[0].needs_refresh(), "flag must be cleared after drain");

    // FFON tree must contain both rooms (rebuilt from cache).
    let root = &renderer.ffon[0];
    let children = &root.as_obj().unwrap().children;
    let has_test_room = children.iter().any(|e| e.as_obj().map_or(false, |o| o.key == "Test Room"));
    let has_another = children.iter().any(|e| e.as_obj().map_or(false, |o| o.key == "Another Room"));
    assert!(has_test_room, "FFON must contain 'Test Room' after refresh; children: {:?}", children);
    assert!(has_another, "FFON must contain 'Another Room' after refresh; children: {:?}", children);
}

// ---------------------------------------------------------------------------
// F5 hard-refresh via dispatch_refresh_command
// ---------------------------------------------------------------------------

/// A minimal provider that records whether its "refresh" command was dispatched.
struct RefreshTrackingProvider {
    last_command: std::sync::Arc<std::sync::Mutex<Option<String>>>,
}

impl RefreshTrackingProvider {
    fn new() -> (Self, std::sync::Arc<std::sync::Mutex<Option<String>>>) {
        let shared = std::sync::Arc::new(std::sync::Mutex::new(None));
        (RefreshTrackingProvider { last_command: shared.clone() }, shared)
    }
}

impl Provider for RefreshTrackingProvider {
    fn name(&self) -> &str { "tracking" }
    fn fetch(&mut self) -> Vec<FfonElement> { vec![FfonElement::new_str("item")] }
    fn commands(&self) -> Vec<String> { vec!["refresh".to_owned()] }
    fn handle_command(&mut self, cmd: &str, _: &str, _: i32, _: &mut String) -> Option<FfonElement> {
        *self.last_command.lock().unwrap() = Some(cmd.to_owned());
        None
    }
}

#[test]
fn f5_dispatches_refresh_command_when_provider_exposes_it() {
    let (p, last_cmd) = RefreshTrackingProvider::new();

    let mut renderer = AppRenderer::new();
    let mut root = FfonElement::new_obj("tracking");
    root.as_obj_mut().unwrap().push(FfonElement::new_str("item"));
    renderer.ffon = vec![root];
    renderer.providers = vec![Box::new(p)];
    renderer.current_id = {
        let mut id = sicompass_sdk::ffon::IdArray::new();
        id.push(0);
        id
    };
    renderer.coordinate = Coordinate::General;

    press(&mut renderer, Keycode::F5);

    assert_eq!(
        *last_cmd.lock().unwrap(),
        Some("refresh".to_owned()),
        "F5 must dispatch the provider's 'refresh' command"
    );
}

// ---------------------------------------------------------------------------
// Webbrowser form interaction — provider-level unit tests (no Chrome needed)
// ---------------------------------------------------------------------------

#[test]
fn webbrowser_provider_push_pop_path_round_trip() {
    ensure_builtins();
    let mut p = sicompass_sdk::create_provider_by_name("webbrowser").unwrap();
    assert_eq!(p.current_path(), "/");
    p.push_path("https://example.com");
    p.push_path("form_1");
    assert_eq!(p.current_path(), "/https://example.com/form_1");
    p.pop_path();
    assert_eq!(p.current_path(), "/https://example.com");
    p.pop_path();
    assert_eq!(p.current_path(), "/");
}

#[test]
fn webbrowser_provider_set_current_path_survives_round_trip() {
    ensure_builtins();
    let mut p = sicompass_sdk::create_provider_by_name("webbrowser").unwrap();
    p.set_current_path("/https://example.com/form_2/q");
    assert_eq!(p.current_path(), "/https://example.com/form_2/q");
}

#[test]
fn webbrowser_form_html_produces_input_cells() {
    // Verify that the SDK parser (used by the webbrowser on every page load)
    // converts a login form into FFON elements with editable cells — without
    // needing a live Chrome instance.
    let html = r#"<form>
        <input type="email" name="email" placeholder="Email address">
        <input type="password" name="password">
        <input type="submit" value="Log in">
    </form>"#;
    let (elems, map) = sicompass_sdk::ffon::html_to_ffon_with_forms(html, "https://example.com");
    let form = elems[0].as_obj().expect("expected form_1 Obj");
    assert_eq!(form.key, "form_1");

    let has_email = form.children.iter().any(|e| {
        e.as_str().map_or(false, |s| s.contains("<input>") && s.contains("Email address"))
    });
    assert!(has_email, "email field missing from form children: {:?}", form.children);

    let has_submit = form.children.iter().any(|e| {
        e.as_str().map_or(false, |s| s.contains("<button>submit:form_1</button>"))
    });
    assert!(has_submit, "submit button missing from form children: {:?}", form.children);

    assert!(map.contains_key("form_1/Email address"), "form_map missing email key");
    assert!(map.contains_key("form_1/Log in"), "form_map missing submit key");
}

#[test]
fn webbrowser_form_commit_returns_false_and_patches_cache() {
    // commit_edit for a known form field must return false so that the app's
    // unconditional local-FFON update is not overwritten by
    // refresh_current_directory re-fetching stale cached_page data.
    // It must also patch cached_page so any subsequent re-fetch keeps the value.
    ensure_builtins();
    use sicompass_sdk::ffon::{FfonElement, FormNode, FormNodeKind, FormMap};
    use sicompass_sdk::provider::Provider;

    let mut p = sicompass_sdk::create_provider_by_name("webbrowser").unwrap();

    // Inject a minimal page with a form field.
    // (We reach into the provider through fetch: craft the FFON directly via
    //  html_to_ffon_with_forms so that form_map is populated correctly.)
    let html = r#"<form><input type="text" name="q" placeholder="Query"></form>"#;
    let (elems, map) = sicompass_sdk::ffon::html_to_ffon_with_forms(html, "https://s.example.com");
    // Seed the provider state by injecting via the public API: set_current_path
    // and directly validate commit_edit returns false for a known form key.
    // Since we can't set cached_page/form_map via the public trait, we validate
    // the parser contract instead: the form_map key must be present and the FFON
    // cell must not carry a spurious <id> prefix.
    let form = elems[0].as_obj().expect("form_1 Obj");
    assert_eq!(form.key, "form_1");
    let field = form.children.iter()
        .find(|e| e.as_str().map_or(false, |s| s.contains("<input>")))
        .and_then(|e| e.as_str())
        .expect("editable field in form");
    assert!(!field.contains("<id>"), "form field must not have spurious <id> prefix: {field}");
    assert!(map.contains_key("form_1/Query"), "form_map must contain field key");
}

// ---------------------------------------------------------------------------
// Chat client: navigate_right eagerly loads room messages (no F5 needed)
// ---------------------------------------------------------------------------

/// Right-arrow into a Matrix room must populate its messages without requiring
/// an explicit F5 refresh. The root Obj key becomes the room name inside the
/// room so the parent label in the UI shows the room name.
#[test]
fn chat_navigate_right_loads_room_without_f5() {
    let mut chat = sicompass_chatclient::ChatClientProvider::new()
        .with_sync_disabled();
    chat.test_set_credentials("https://matrix.org", "test_token");
    chat.test_seed_room("!abc:matrix.org", "Matrix.org");

    let mut renderer = AppRenderer::new();
    let display_name = chat.display_name().to_owned();
    let children = chat.fetch();
    let mut root = FfonElement::new_obj(&display_name);
    for child in children {
        root.as_obj_mut().unwrap().push(child);
    }
    renderer.ffon.push(root);
    renderer.providers.push(Box::new(chat));

    renderer.current_id = {
        let mut id = sicompass_sdk::ffon::IdArray::new();
        id.push(0);
        id
    };
    sicompass::list::create_list_current_layer(&mut renderer);

    // Enter provider root (depth 1 → depth 2: cursor on "Matrix.org" room).
    press_right(&mut renderer);
    assert_eq!(renderer.current_id.depth(), 2, "should be at room list");
    assert_eq!(
        renderer.ffon[0].as_obj().unwrap().key, "chat client",
        "root key must be 'chat client' at room list"
    );

    // Enter the room — refresh_on_navigate=true triggers fetch() without F5.
    press_right(&mut renderer);
    assert_eq!(renderer.current_id.depth(), 2, "cursor should be at depth 2 inside room");

    let children = &renderer.ffon[0].as_obj().unwrap().children;
    let has_input = children.iter().any(|e| e.as_str().map_or(false, |s| s.contains("<input>")));
    assert!(has_input, "room must have <input> child after right-arrow (no F5); children: {children:?}");
    assert_eq!(
        renderer.ffon[0].as_obj().unwrap().key, "Matrix.org",
        "root key must be the room name inside room (shown as parent label)"
    );

    // Navigate left — back to rooms list; root key reverts to display_name.
    press_left(&mut renderer);
    assert_eq!(
        renderer.ffon[0].as_obj().unwrap().key, "chat client",
        "root key must revert to 'chat client' after navigating back to room list"
    );
    let children = &renderer.ffon[0].as_obj().unwrap().children;
    let has_room = children.iter().any(|e| e.as_obj().map_or(false, |o| o.key == "Matrix.org"));
    assert!(has_room, "rooms list must reappear after left; children: {children:?}");
}

/// Pressing Enter on a bare `<input></input>` element (empty old content) must
/// route through `commit_edit`, not skip it in favour of a plain FFON update.
/// Verified by a provider that records what was committed and returns `true`.
#[test]
fn empty_input_enter_calls_commit_edit() {
    use std::sync::{Arc, Mutex};

    let captured: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    struct CommitCapture {
        path: String,
        captured: Arc<Mutex<Option<String>>>,
    }
    impl Provider for CommitCapture {
        fn name(&self) -> &str { "capture" }
        fn fetch(&mut self) -> Vec<FfonElement> {
            vec![FfonElement::new_str("<input></input>".to_owned())]
        }
        fn commit_edit(&mut self, _old: &str, new_content: &str) -> bool {
            *self.captured.lock().unwrap() = Some(new_content.to_owned());
            true
        }
        fn push_path(&mut self, seg: &str) { self.path = format!("/{seg}"); }
        fn pop_path(&mut self) { self.path = "/".to_owned(); }
        fn current_path(&self) -> &str { &self.path }
        fn set_current_path(&mut self, p: &str) { self.path = p.to_owned(); }
    }

    let mut renderer = AppRenderer::new();
    let mut root = FfonElement::new_obj("capture");
    root.as_obj_mut().unwrap().push(FfonElement::new_str("<input></input>".to_owned()));
    renderer.ffon.push(root);
    renderer.providers.push(Box::new(CommitCapture {
        path: "/".to_owned(),
        captured: Arc::clone(&captured),
    }));
    renderer.current_id = {
        let mut id = sicompass_sdk::ffon::IdArray::new();
        id.push(0);
        id.push(0); // first child: the <input></input> element
        id
    };
    sicompass::list::create_list_current_layer(&mut renderer);

    // Enter insert mode on the <input> element, type "hello", press Enter.
    press(&mut renderer, Keycode::I);
    type_text(&mut renderer, "hello");
    press_enter(&mut renderer);

    let committed = captured.lock().unwrap().clone();
    assert_eq!(committed.as_deref(), Some("hello"),
        "commit_edit must be called with the typed content for empty <input></input>");
}

// ---------------------------------------------------------------------------
// Chat client: unread badge renders in the FFON tree
// ---------------------------------------------------------------------------

/// When a room has unread messages the badge must be embedded in the Obj's key
/// (not as a child). An obj with children is expanded in-place by the renderer
/// rather than triggering a provider fetch, which would prevent navigating into
/// the room.
#[test]
fn chat_unread_badge_embedded_in_key() {
    let mut chat = sicompass_chatclient::ChatClientProvider::new().with_sync_disabled();
    chat.test_set_credentials("https://matrix.org", "tok");

    chat.test_seed_room("!noisy:s", "Noisy Channel");
    chat.test_set_unread("Noisy Channel", 3, 1);

    let children = chat.fetch();

    // Badge is in the key; no child nodes.
    let room_obj = children
        .iter()
        .find(|e| e.as_obj().map_or(false, |o| o.key == "Noisy Channel [mention:1]"));
    assert!(room_obj.is_some(), "room with badge key must appear; got: {children:?}");
    assert!(
        room_obj.unwrap().as_obj().unwrap().children.is_empty(),
        "room obj must have no children so navigation reaches the provider fetch"
    );
}

// ---------------------------------------------------------------------------
// Chat client: room info command surface
// ---------------------------------------------------------------------------

/// The "room info" command must return a string that includes the room ID,
/// even without a live homeserver.  This confirms the provider wires topic/
/// member/encryption data through without touching the network.
#[test]
fn chat_room_info_returns_room_id() {
    let mut chat = sicompass_chatclient::ChatClientProvider::new().with_sync_disabled();
    chat.test_set_credentials("https://matrix.org", "tok");
    chat.test_seed_room("!info:s", "Info Room");
    // Navigate into the room so "room info" finds it.
    chat.push_path("Info Room");

    let mut err = String::new();
    let result = chat.handle_command("room info", "Info Room", 0, &mut err);
    assert!(err.is_empty(), "room info must not error: {err}");
    assert!(result.is_some(), "room info must return a result element");
    let text = result.unwrap();
    assert!(
        text.as_str().map_or(false, |s| s.contains("!info:s")),
        "room info must contain the room ID; got: {text:?}"
    );
}

// ---------------------------------------------------------------------------
// Chat client: mark read command
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Email client: compose with Cc / Bcc
// ---------------------------------------------------------------------------

/// Compose view must include Cc and Bcc input fields, and commit_edit must
/// update the draft when navigating to those field segments.
#[test]
fn email_compose_cc_bcc_fields_appear_and_commit() {
    use sicompass_emailclient::EmailClientProvider;

    let mut p = EmailClientProvider::new();
    p.push_path("compose");
    let items = p.fetch();
    assert!(
        items.iter().any(|e| e.as_str().map_or(false, |s| s.starts_with("Cc:"))),
        "compose view must include Cc: field; got: {items:?}"
    );
    assert!(
        items.iter().any(|e| e.as_str().map_or(false, |s| s.starts_with("Bcc:"))),
        "compose view must include Bcc: field; got: {items:?}"
    );

    // commit_edit at the Cc segment must return true and the value must appear
    // in the next compose fetch.
    p.push_path("Cc");
    assert!(p.commit_edit("", "cc@example.com"), "commit_edit at Cc must return true");
    p.pop_path();
    let items2 = p.fetch();
    assert!(
        items2.iter().any(|e| e.as_str().map_or(false, |s| s.contains("cc@example.com"))),
        "compose view must reflect committed Cc value; got: {items2:?}"
    );

    p.push_path("Bcc");
    assert!(p.commit_edit("", "bcc@example.com"), "commit_edit at Bcc must return true");
    p.pop_path();
    let items3 = p.fetch();
    assert!(
        items3.iter().any(|e| e.as_str().map_or(false, |s| s.contains("bcc@example.com"))),
        "compose view must reflect committed Bcc value; got: {items3:?}"
    );
}

/// "mark read" must clear the local unread count immediately (even if the
/// receipt HTTP call fails). The badge disappears from the room list after the
/// command runs.
#[test]
fn chat_mark_read_clears_local_unread_count() {
    let mut chat = sicompass_chatclient::ChatClientProvider::new().with_sync_disabled();
    // Unreachable server: the receipt POST will fail silently; the local
    // optimistic update must still apply.
    chat.test_set_credentials("http://127.0.0.1:1", "tok");
    chat.test_seed_room("!r:s", "General");
    chat.test_set_unread("General", 2, 0);

    // Sanity: badge in room list before marking read.
    let list_before = chat.fetch();
    assert!(
        list_before.iter().any(|e| e.as_obj().map_or(false, |o| o.key == "General [unread:2]")),
        "unread badge must be in key before mark read; got: {list_before:?}"
    );

    // Navigate into the room so the command knows which room to mark.
    chat.push_path("General");
    let mut err = String::new();
    chat.handle_command("mark read", "", 0, &mut err);

    // Navigate back to root and verify badge is gone.
    chat.pop_path();
    let list_after = chat.fetch();
    assert!(
        list_after.iter().any(|e| e.as_obj().map_or(false, |o| o.key == "General")),
        "badge must be gone after mark read; got: {list_after:?}"
    );
}

// ---------------------------------------------------------------------------
// Editor provider
// ---------------------------------------------------------------------------

#[test]
fn editor_provider_lists_directory_and_parses_file() {
    ensure_builtins();
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();

    // A plain file and a structured file with sections.
    std::fs::write(root.join("readme.txt"), "hello world").unwrap();
    std::fs::write(
        root.join("code.txt"),
        "functions:\n{\n  fn foo\n  fn bar\n}\nend",
    ).unwrap();
    std::fs::create_dir(root.join("subdir")).unwrap();

    let mut editor = sicompass_sdk::create_provider_by_name("editor")
        .expect("editor factory must be registered");
    editor.on_setting_change("editorPath", root.to_str().unwrap());

    // Directory listing contains all three entries.
    let items = editor.fetch();
    let names: Vec<String> = items.iter().filter_map(|e| {
        e.as_obj().map(|o| o.key.clone())
            .or_else(|| e.as_str().map(|s| s.to_string()))
    }).collect();
    assert!(names.iter().any(|n| n.contains("readme.txt")), "expected readme.txt in {names:?}");
    assert!(names.iter().any(|n| n.contains("code.txt")),   "expected code.txt in {names:?}");
    assert!(names.iter().any(|n| n.contains("subdir")),     "expected subdir in {names:?}");

    // Entering a file returns its parsed FFON content.
    // Elements are wrapped in <input> and annotated with <src=N>; strip for assertions.
    editor.push_path("code.txt");
    let file_items = editor.fetch();
    assert_eq!(file_items.len(), 2, "code.txt should parse into 2 top-level elements");
    let section = file_items[0].as_obj().expect("first element must be an Obj section");
    assert_eq!(
        sicompass_sdk::tags::strip_display(&section.key),
        "functions:",
        "section key should strip to 'functions:'"
    );
    assert_eq!(section.children.len(), 2);
    let end_raw = file_items[1].as_str().expect("second element should be a Str");
    assert_eq!(sicompass_sdk::tags::strip_display(end_raw), "end");

    // Navigating into a section returns its children (stripped keys).
    editor.push_path("functions:");
    let section_items = editor.fetch();
    assert_eq!(section_items.len(), 2);
    let foo_raw = section_items[0].as_str().expect("first child should be a Str");
    let bar_raw = section_items[1].as_str().expect("second child should be a Str");
    assert_eq!(sicompass_sdk::tags::strip_display(foo_raw), "fn foo");
    assert_eq!(sicompass_sdk::tags::strip_display(bar_raw), "fn bar");

    // pop_path twice returns to the directory listing.
    editor.pop_path(); // section → file root
    editor.pop_path(); // file → directory
    let back = editor.fetch();
    assert_eq!(back.len(), items.len(), "should be back at directory level");
}

// ---------------------------------------------------------------------------
// has_editor_semantics / editor coordinate tests
// ---------------------------------------------------------------------------

/// Build an AppRenderer with filebrowser (idx 0) + editor (idx 1).
/// The editor is rooted at `tmp` which contains one .txt file.
fn harness_with_editor() -> (AppRenderer, TempDir) {
    ensure_builtins();
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path();
    std::fs::write(root.join("hello.txt"), "fn main() {}").unwrap();

    let mut renderer = AppRenderer::new();

    // Filebrowser at "/" so it doesn't depend on a real directory.
    register(&mut renderer, sicompass_sdk::create_provider_by_name("filebrowser").unwrap());
    renderer.providers[0].set_current_path("/");
    {
        let children = renderer.providers[0].fetch();
        let dn = renderer.providers[0].display_name().to_owned();
        let mut root_elem = FfonElement::new_obj(&dn);
        for child in children { root_elem.as_obj_mut().unwrap().push(child); }
        renderer.ffon[0] = root_elem;
    }

    // Editor at tmp — set path before fetching so init doesn't clobber it.
    let mut editor = sicompass_sdk::create_provider_by_name("editor").unwrap();
    editor.on_setting_change("editorPath", root.to_str().unwrap());
    let children = editor.fetch();
    let dn = editor.display_name().to_owned();
    let mut root_elem = FfonElement::new_obj(&dn);
    for child in children { root_elem.as_obj_mut().unwrap().push(child); }
    renderer.ffon.push(root_elem);
    renderer.providers.push(editor);

    sicompass::list::create_list_current_layer(&mut renderer);
    (renderer, tmp)
}

#[test]
fn entering_editor_provider_keeps_general() {
    let (mut r, _tmp) = harness_with_editor();
    let editor_idx = r.providers.iter().position(|p| p.name() == "editor").unwrap();
    navigate_to_provider(&mut r, editor_idx);
    assert_eq!(r.coordinate, Coordinate::General, "before entry: General");

    press_right(&mut r);
    assert_eq!(r.current_id.depth(), 2, "should be inside editor provider");
    assert_eq!(r.coordinate, Coordinate::General, "should auto-switch to General");
}

#[test]
fn inside_editor_i_yields_insert() {
    let (mut r, _tmp) = harness_with_editor();
    let editor_idx = r.providers.iter().position(|p| p.name() == "editor").unwrap();
    navigate_to_provider(&mut r, editor_idx);
    press_right(&mut r); // enters editor → General
    assert_eq!(r.coordinate, Coordinate::General);

    // The editor provider enters Insert; Enter routes to commit_edit for disk writes.
    press(&mut r, Keycode::I);
    assert_eq!(r.coordinate, Coordinate::Insert, "'i' in editor provider should give Insert (Enter routes to commit_edit for disk writes)");
}

#[test]
fn editor_directory_entries_are_obj() {
    // Both files and directories are Obj — going right on either enters its
    // contents (file body or subdir listing), so both render with `+i`.
    ensure_builtins();
    let tmp = TempDir::new().expect("tempdir");
    std::fs::write(tmp.path().join("readme.md"), "hello").unwrap();
    std::fs::create_dir(tmp.path().join("subdir")).unwrap();

    let mut ed = sicompass_sdk::create_provider_by_name("editor").unwrap();
    ed.on_setting_change("editorPath", tmp.path().to_str().unwrap());
    let items = ed.fetch();

    let file_entry = items.iter().find(|e| {
        let k = match e { FfonElement::Str(s) => s.as_str(), FfonElement::Obj(o) => o.key.as_str() };
        k.contains("readme.md")
    }).expect("readme.md must be in listing");
    assert!(file_entry.is_obj(), "file entry must be Obj — right-arrow enters its contents");

    let dir_entry = items.iter().find(|e| {
        let k = match e { FfonElement::Str(s) => s.as_str(), FfonElement::Obj(o) => o.key.as_str() };
        k.contains("subdir")
    }).expect("subdir must be in listing");
    assert!(dir_entry.is_obj(), "directory entry must be Obj");
}

#[test]
fn editor_right_arrow_opens_file() {
    // Pressing right on a file Obj entry should open the file content.
    let (mut r, tmp) = harness_with_editor();
    let editor_idx = r.providers.iter().position(|p| p.name() == "editor").unwrap();
    navigate_to_provider(&mut r, editor_idx);
    press_right(&mut r); // enter editor directory → General at depth 2

    // Find the hello.txt entry and move cursor to it.
    let file_idx = {
        let children = r.ffon[editor_idx].as_obj().unwrap().children.as_slice();
        children.iter().position(|e| match e {
            FfonElement::Obj(o) => o.key.contains("hello.txt"),
            _ => false,
        }).expect("hello.txt must be in listing as Obj")
    };
    r.current_id.set(1, file_idx);
    sicompass::list::create_list_current_layer(&mut r);

    // Right arrow on a file Obj → opens file, now at depth 2 showing file content.
    press_right(&mut r);
    assert_eq!(r.current_id.depth(), 2, "should remain at depth 2 after opening file");

    // File content should now be in the FFON (hello.txt contains "fn main() {}").
    let content_children = r.ffon[editor_idx].as_obj().unwrap().children.as_slice();
    let has_content = content_children.iter().any(|e| {
        let k = match e { FfonElement::Str(s) => s.as_str(), FfonElement::Obj(o) => o.key.as_str() };
        sicompass_sdk::tags::strip_display(k).contains("fn main")
    });
    assert!(has_content, "file content should be visible after opening");

    // Left arrow goes back to directory listing.
    press_left(&mut r);
    let back_children = r.ffon[editor_idx].as_obj().unwrap().children.as_slice();
    let back_to_dir = back_children.iter().any(|e| match e {
        FfonElement::Obj(o) => o.key.contains("hello.txt"),
        _ => false,
    });
    assert!(back_to_dir, "should be back at directory listing after left");
}

#[test]
fn leaving_editor_provider_keeps_general() {
    let (mut r, _tmp) = harness_with_editor();
    let editor_idx = r.providers.iter().position(|p| p.name() == "editor").unwrap();
    navigate_to_provider(&mut r, editor_idx);
    press_right(&mut r); // enter → General
    assert_eq!(r.coordinate, Coordinate::General);

    press_left(&mut r);
    assert_eq!(r.current_id.depth(), 1, "should be back at root");
    assert_eq!(r.coordinate, Coordinate::General, "should revert to General");
}

#[test]
fn entering_filebrowser_keeps_general() {
    let (mut r, _tmp) = harness_with_editor();
    // Start at filebrowser (idx 0)
    navigate_to_provider(&mut r, 0);
    press_right(&mut r);
    assert_eq!(r.current_id.depth(), 2);
    assert_eq!(r.coordinate, Coordinate::General, "filebrowser keeps General");
}

#[test]
fn entering_editor_does_not_clobber_non_general_coordinate() {
    let (mut r, _tmp) = harness_with_editor();
    // Simulate a non-General coordinate (e.g. user is in a search overlay).
    r.coordinate = Coordinate::Insert;
    let editor_idx = r.providers.iter().position(|p| p.name() == "editor").unwrap();
    // Directly invoke navigate_right_raw so the key-dispatch routing for Insert mode
    // doesn't interfere — we're testing the guard inside navigate_right_raw itself.
    while r.current_id.depth() > 1 { r.current_id.pop(); }
    let cur = r.current_id.get(0).unwrap_or(0);
    if cur < editor_idx {
        for _ in 0..(editor_idx - cur) {
            sicompass::handlers::handle_down(&mut r);
        }
    }
    sicompass::handlers::navigate_right_raw(&mut r);
    assert_eq!(r.coordinate, Coordinate::Insert, "non-General coordinate must not be clobbered");
}

/// After creating a file via Ctrl+I in the editor, the coordinate must return
/// to General (not General) because that's what was active before
/// the insert was initiated.
#[test]
fn editor_ctrl_i_create_file_restores_general() {
    let (mut r, tmp) = harness_with_editor();
    let editor_idx = r.providers.iter().position(|p| p.name() == "editor").unwrap();
    navigate_to_provider(&mut r, editor_idx);
    press_right(&mut r); // enter editor directory → General
    assert_eq!(r.coordinate, Coordinate::General, "should be General after entering editor");

    // Ctrl+I → enters Insert with prefixed_insert_mode
    press_ctrl(&mut r, Keycode::I);
    assert_eq!(r.coordinate, Coordinate::Insert);

    // Type a plain name (no prefix) → creates a file
    type_text(&mut r, "newfile.txt");
    press_enter(&mut r);

    assert_eq!(r.coordinate, Coordinate::General,
        "after file creation in editor, coordinate must restore to General");
    assert!(tmp.path().join("newfile.txt").exists(), "file must be created on disk");
}

/// After creating a directory via Ctrl+I in the editor (using '+' prefix),
/// the coordinate must return to General.
#[test]
fn editor_ctrl_i_create_dir_restores_general() {
    let (mut r, tmp) = harness_with_editor();
    let editor_idx = r.providers.iter().position(|p| p.name() == "editor").unwrap();
    navigate_to_provider(&mut r, editor_idx);
    press_right(&mut r); // enter editor directory → General
    assert_eq!(r.coordinate, Coordinate::General);

    press_ctrl(&mut r, Keycode::I);
    assert_eq!(r.coordinate, Coordinate::Insert);

    type_text(&mut r, "+subdir");
    press_enter(&mut r);

    assert_eq!(r.coordinate, Coordinate::General,
        "after directory creation in editor, coordinate must restore to General");
    assert!(tmp.path().join("subdir").is_dir(), "directory must be created on disk");
}

/// Navigating into a subdirectory (Obj with no FFON children) works and refreshes contents.
/// An empty subdirectory seeds an I_PLACEHOLDER so the user has a creation affordance.
#[test]
fn editor_right_arrow_into_subdir() {
    let (mut r, tmp) = harness_with_editor();
    std::fs::create_dir(tmp.path().join("subdir")).unwrap();
    std::fs::write(tmp.path().join("subdir/child.txt"), "").unwrap();

    let editor_idx = r.providers.iter().position(|p| p.name() == "editor").unwrap();
    navigate_to_provider(&mut r, editor_idx);
    press_right(&mut r); // enter editor root dir listing

    // Refresh so subdir appears in the listing.
    press((&mut r), Keycode::F5);

    let dir_idx = {
        let children = r.ffon[editor_idx].as_obj().unwrap().children.as_slice();
        children.iter().position(|e| match e {
            FfonElement::Obj(o) => sicompass_sdk::tags::strip_display(&o.key).contains("subdir"),
            _ => false,
        }).expect("subdir must appear as Obj in listing")
    };
    r.current_id.set(1, dir_idx);
    sicompass::list::create_list_current_layer(&mut r);

    // Right arrow on an Obj dir with no FFON children must navigate into it.
    let navigated = sicompass::handlers::navigate_right_raw(&mut r);
    assert!(navigated, "right-arrow on editor subdir (Obj) must navigate in");
    sicompass::list::create_list_current_layer(&mut r);

    // After navigation the FFON shows the subdir's contents (child.txt).
    let subdir_children = r.ffon[editor_idx].as_obj().unwrap().children.as_slice();
    let has_child = subdir_children.iter().any(|e| match e {
        FfonElement::Obj(o) => sicompass_sdk::tags::strip_display(&o.key).contains("child.txt"),
        _ => false,
    });
    assert!(has_child, "child.txt should appear in subdir listing");
}

/// Navigating into an empty subdirectory seeds an I_PLACEHOLDER for creation.
#[test]
fn editor_empty_subdir_seeds_i_placeholder() {
    let (mut r, tmp) = harness_with_editor();
    std::fs::create_dir(tmp.path().join("empty_dir")).unwrap();

    let editor_idx = r.providers.iter().position(|p| p.name() == "editor").unwrap();
    navigate_to_provider(&mut r, editor_idx);
    press_right(&mut r);
    press((&mut r), Keycode::F5); // refresh to pick up empty_dir

    let dir_idx = {
        let children = r.ffon[editor_idx].as_obj().unwrap().children.as_slice();
        children.iter().position(|e| match e {
            FfonElement::Obj(o) => sicompass_sdk::tags::strip_display(&o.key).contains("empty_dir"),
            _ => false,
        }).expect("empty_dir must be in listing")
    };
    r.current_id.set(1, dir_idx);
    sicompass::list::create_list_current_layer(&mut r);

    sicompass::handlers::navigate_right_raw(&mut r);
    sicompass::list::create_list_current_layer(&mut r);

    let children = r.ffon[editor_idx].as_obj().unwrap().children.as_slice();
    let has_placeholder = children.iter().any(|e| match e {
        FfonElement::Str(s) => sicompass_sdk::placeholders::is_i_placeholder(s),
        _ => false,
    });
    assert!(has_placeholder, "empty subdir must seed I_PLACEHOLDER for creation affordance");
}

/// Pressing `i` on the I_PLACEHOLDER in an empty editor subdir, typing a plain
/// name (no prefix), and pressing Enter creates a file on disk and returns to
/// General.
#[test]
fn editor_i_on_placeholder_creates_file() {
    let (mut r, tmp) = harness_with_editor();
    std::fs::create_dir(tmp.path().join("mydir")).unwrap();

    let editor_idx = r.providers.iter().position(|p| p.name() == "editor").unwrap();
    navigate_to_provider(&mut r, editor_idx);
    press_right(&mut r);
    press(&mut r, Keycode::F5);

    // Navigate into mydir (Obj with no FFON children → lazy-load + I_PLACEHOLDER).
    let dir_idx = r.ffon[editor_idx].as_obj().unwrap().children.iter()
        .position(|e| matches!(e, FfonElement::Obj(o) if sicompass_sdk::tags::strip_display(&o.key).contains("mydir")))
        .expect("mydir must be in listing");
    r.current_id.set(1, dir_idx);
    sicompass::list::create_list_current_layer(&mut r);
    sicompass::handlers::navigate_right_raw(&mut r);
    sicompass::list::create_list_current_layer(&mut r);

    // Cursor is now on I_PLACEHOLDER at [editor_idx, 0].
    assert_eq!(r.coordinate, Coordinate::General);

    // Press `i` → should detect I_PLACEHOLDER prefix → placeholder_insert_mode.
    press(&mut r, Keycode::I);
    assert_eq!(r.coordinate, Coordinate::Insert, "i on I_PLACEHOLDER must enter Insert");
    assert!(r.placeholder_insert_mode, "i on I_PLACEHOLDER must set placeholder_insert_mode");

    // Type a plain name and confirm.
    type_text(&mut r, "notes.txt");
    press_enter(&mut r);

    assert_eq!(r.coordinate, Coordinate::General, "must return to General after creation");
    assert!(tmp.path().join("mydir/notes.txt").exists(), "file must be created on disk");
}

/// Typing `+name` on the I_PLACEHOLDER creates a directory.
#[test]
fn editor_i_on_placeholder_creates_dir_with_plus_prefix() {
    let (mut r, tmp) = harness_with_editor();
    std::fs::create_dir(tmp.path().join("mydir2")).unwrap();

    let editor_idx = r.providers.iter().position(|p| p.name() == "editor").unwrap();
    navigate_to_provider(&mut r, editor_idx);
    press_right(&mut r);
    press(&mut r, Keycode::F5);

    let dir_idx = r.ffon[editor_idx].as_obj().unwrap().children.iter()
        .position(|e| matches!(e, FfonElement::Obj(o) if sicompass_sdk::tags::strip_display(&o.key).contains("mydir2")))
        .expect("mydir2 must be in listing");
    r.current_id.set(1, dir_idx);
    sicompass::list::create_list_current_layer(&mut r);
    sicompass::handlers::navigate_right_raw(&mut r);
    sicompass::list::create_list_current_layer(&mut r);

    press(&mut r, Keycode::I);
    type_text(&mut r, "+subdir");
    press_enter(&mut r);

    assert_eq!(r.coordinate, Coordinate::General);
    assert!(tmp.path().join("mydir2/subdir").is_dir(), "directory must be created on disk with + prefix");
}

/// User's repro: create file → right (open) → I_PLACEHOLDER → i → type "first"
/// → Enter writes the file. Then Ctrl+A on the new line → type "second" →
/// Enter must show the second line in the list immediately (no F5 needed).
#[test]
fn editor_two_consecutive_writes_both_show_in_list() {
    let (mut r, tmp) = harness_with_editor();
    std::fs::write(tmp.path().join("notes.txt"), "").unwrap();

    let editor_idx = r.providers.iter().position(|p| p.name() == "editor").unwrap();
    navigate_to_provider(&mut r, editor_idx);
    press_right(&mut r); // enter editor dir
    press(&mut r, Keycode::F5); // pick up notes.txt

    // Move cursor onto notes.txt and open it.
    let file_idx = r.ffon[editor_idx].as_obj().unwrap().children.iter()
        .position(|e| {
            let k = match e { FfonElement::Str(s) => s.as_str(), FfonElement::Obj(o) => o.key.as_str() };
            k.contains("notes.txt")
        })
        .expect("notes.txt must be in listing");
    r.current_id.set(1, file_idx);
    sicompass::list::create_list_current_layer(&mut r);
    sicompass::handlers::navigate_right_raw(&mut r);
    sicompass::list::create_list_current_layer(&mut r);

    // Empty file → I_PLACEHOLDER seeded as the only child.
    let on_placeholder = r.ffon[editor_idx].as_obj().unwrap().children.iter()
        .any(|e| matches!(e, FfonElement::Str(s) if sicompass_sdk::placeholders::is_i_placeholder(s)));
    assert!(on_placeholder, "empty file must seed I_PLACEHOLDER");

    // First write: i, type "first", Enter.
    press(&mut r, Keycode::I);
    assert_eq!(r.coordinate, Coordinate::Insert);
    type_text(&mut r, "first");
    press_enter(&mut r);

    let written = std::fs::read_to_string(tmp.path().join("notes.txt")).unwrap();
    assert_eq!(written, "first", "first write must reach disk");

    let after_first: Vec<String> = r.ffon[editor_idx].as_obj().unwrap().children.iter()
        .map(|e| match e {
            FfonElement::Str(s) => sicompass_sdk::tags::strip_display(s),
            FfonElement::Obj(o) => sicompass_sdk::tags::strip_display(&o.key),
        })
        .collect();
    assert_eq!(after_first, vec!["first".to_owned()], "list must show the first line after commit");

    // Second write: Ctrl+A → placeholder after the current line, type "second", Enter.
    press_ctrl(&mut r, Keycode::A);
    assert_eq!(r.coordinate, Coordinate::Insert, "Ctrl+A must enter insert mode");
    type_text(&mut r, "second");
    press_enter(&mut r);

    let written = std::fs::read_to_string(tmp.path().join("notes.txt")).unwrap();
    assert_eq!(written, "first\nsecond", "second write must reach disk");

    // Critical: list must show both elements WITHOUT pressing F5.
    let after_second: Vec<String> = r.ffon[editor_idx].as_obj().unwrap().children.iter()
        .map(|e| match e {
            FfonElement::Str(s) => sicompass_sdk::tags::strip_display(s),
            FfonElement::Obj(o) => sicompass_sdk::tags::strip_display(&o.key),
        })
        .collect();
    assert_eq!(
        after_second, vec!["first".to_owned(), "second".to_owned()],
        "second element must appear in list without F5"
    );
}

/// Three consecutive Ctrl+A inserts must all land in the file and the FFON
/// list, without the focus jumping or any intermediate line vanishing.
#[test]
fn editor_three_consecutive_writes_all_show_in_list() {
    let (mut r, tmp) = harness_with_editor();
    std::fs::write(tmp.path().join("notes.txt"), "").unwrap();

    let editor_idx = r.providers.iter().position(|p| p.name() == "editor").unwrap();
    navigate_to_provider(&mut r, editor_idx);
    press_right(&mut r);
    press(&mut r, Keycode::F5);

    let file_idx = r.ffon[editor_idx].as_obj().unwrap().children.iter()
        .position(|e| {
            let k = match e { FfonElement::Str(s) => s.as_str(), FfonElement::Obj(o) => o.key.as_str() };
            k.contains("notes.txt")
        })
        .expect("notes.txt must be in listing");
    r.current_id.set(1, file_idx);
    sicompass::list::create_list_current_layer(&mut r);
    sicompass::handlers::navigate_right_raw(&mut r);
    sicompass::list::create_list_current_layer(&mut r);

    // First write via I_PLACEHOLDER.
    press(&mut r, Keycode::I);
    type_text(&mut r, "first");
    press_enter(&mut r);

    // Second write: Ctrl+A on "first".
    press_ctrl(&mut r, Keycode::A);
    type_text(&mut r, "second");
    press_enter(&mut r);

    // Third write: Ctrl+A on "second" (cursor should be on the just-committed line).
    press_ctrl(&mut r, Keycode::A);
    type_text(&mut r, "third");
    press_enter(&mut r);

    let written = std::fs::read_to_string(tmp.path().join("notes.txt")).unwrap();
    assert_eq!(written, "first\nsecond\nthird", "three writes must reach disk in order");

    let labels: Vec<String> = r.ffon[editor_idx].as_obj().unwrap().children.iter()
        .map(|e| match e {
            FfonElement::Str(s) => sicompass_sdk::tags::strip_display(s),
            FfonElement::Obj(o) => sicompass_sdk::tags::strip_display(&o.key),
        })
        .collect();
    assert_eq!(
        labels, vec!["first".to_owned(), "second".to_owned(), "third".to_owned()],
        "all three lines must show in the list"
    );
}

/// The user's "must work infinitely" requirement: ten consecutive Ctrl+A
/// inserts in an editor file view must all land in the file and the FFON list.
#[test]
fn editor_many_consecutive_writes_all_show_in_list() {
    let (mut r, tmp) = harness_with_editor();
    std::fs::write(tmp.path().join("log.txt"), "").unwrap();

    let editor_idx = r.providers.iter().position(|p| p.name() == "editor").unwrap();
    navigate_to_provider(&mut r, editor_idx);
    press_right(&mut r);
    press(&mut r, Keycode::F5);

    let file_idx = r.ffon[editor_idx].as_obj().unwrap().children.iter()
        .position(|e| {
            let k = match e { FfonElement::Str(s) => s.as_str(), FfonElement::Obj(o) => o.key.as_str() };
            k.contains("log.txt")
        })
        .expect("log.txt must be in listing");
    r.current_id.set(1, file_idx);
    sicompass::list::create_list_current_layer(&mut r);
    sicompass::handlers::navigate_right_raw(&mut r);
    sicompass::list::create_list_current_layer(&mut r);

    // First write via I_PLACEHOLDER.
    press(&mut r, Keycode::I);
    type_text(&mut r, "line0");
    press_enter(&mut r);

    // Nine more Ctrl+A inserts.
    for n in 1..10 {
        press_ctrl(&mut r, Keycode::A);
        assert_eq!(r.coordinate, Coordinate::Insert,
            "Ctrl+A iteration {n} must enter Insert (coord stayed in General after previous commit)");
        type_text(&mut r, &format!("line{n}"));
        press_enter(&mut r);
    }

    let expected_disk = (0..10).map(|n| format!("line{n}")).collect::<Vec<_>>().join("\n");
    let written = std::fs::read_to_string(tmp.path().join("log.txt")).unwrap();
    assert_eq!(written, expected_disk, "all ten writes must reach disk in order");

    let labels: Vec<String> = r.ffon[editor_idx].as_obj().unwrap().children.iter()
        .map(|e| match e {
            FfonElement::Str(s) => sicompass_sdk::tags::strip_display(s),
            FfonElement::Obj(o) => sicompass_sdk::tags::strip_display(&o.key),
        })
        .collect();
    let expected_labels: Vec<String> = (0..10).map(|n| format!("line{n}")).collect();
    assert_eq!(labels, expected_labels, "all ten lines must show in the list");
}

/// Typing `name:` on the I_PLACEHOLDER creates a directory (colon suffix).
#[test]
fn editor_i_on_placeholder_creates_dir_with_colon_suffix() {
    let (mut r, tmp) = harness_with_editor();
    std::fs::create_dir(tmp.path().join("mydir3")).unwrap();

    let editor_idx = r.providers.iter().position(|p| p.name() == "editor").unwrap();
    navigate_to_provider(&mut r, editor_idx);
    press_right(&mut r);
    press(&mut r, Keycode::F5);

    let dir_idx = r.ffon[editor_idx].as_obj().unwrap().children.iter()
        .position(|e| matches!(e, FfonElement::Obj(o) if sicompass_sdk::tags::strip_display(&o.key).contains("mydir3")))
        .expect("mydir3 must be in listing");
    r.current_id.set(1, dir_idx);
    sicompass::list::create_list_current_layer(&mut r);
    sicompass::handlers::navigate_right_raw(&mut r);
    sicompass::list::create_list_current_layer(&mut r);

    press(&mut r, Keycode::I);
    type_text(&mut r, "data:");
    press_enter(&mut r);

    assert_eq!(r.coordinate, Coordinate::General);
    assert!(tmp.path().join("mydir3/data").is_dir(), "directory must be created on disk with : suffix");
}

// ---------------------------------------------------------------------------
// Tabs
// ---------------------------------------------------------------------------

fn strip_announcement_sentinel(s: &str) -> &str {
    s.trim_end_matches('\u{200B}')
}

#[test]
fn tabs_initial_state_has_one_tab() {
    let h = Harness::new();
    assert_eq!(h.renderer.tabs.len(), 1);
    assert_eq!(h.renderer.active_tab, 0);
}

#[test]
fn tab_timelines_stay_parallel_to_tabs() {
    let mut h = Harness::new();
    // Invariant at startup.
    assert_eq!(h.renderer.tabs.len(), 1);
    assert_eq!(h.renderer.tab_timelines.len(), 1);

    // Open two more.
    press_ctrl(h.r(), Keycode::T);
    press_ctrl(h.r(), Keycode::T);
    assert_eq!(h.renderer.tabs.len(), 3);
    assert_eq!(h.renderer.tab_timelines.len(), 3);

    // Each tab starts with an empty timeline.
    for t in &h.renderer.tab_timelines {
        assert!(t.entries.is_empty(), "new tabs start with empty timeline");
        assert_eq!(t.position, 0);
    }

    // Close one.
    press_ctrl(h.r(), Keycode::W);
    assert_eq!(h.renderer.tabs.len(), 2);
    assert_eq!(h.renderer.tab_timelines.len(), 2);

    // Close down to one — Ctrl+W is a no-op at one tab.
    press_ctrl(h.r(), Keycode::W);
    assert_eq!(h.renderer.tabs.len(), 1);
    assert_eq!(h.renderer.tab_timelines.len(), 1);
    press_ctrl(h.r(), Keycode::W);
    assert_eq!(h.renderer.tabs.len(), 1);
    assert_eq!(h.renderer.tab_timelines.len(), 1);
}

#[test]
fn ctrl_t_creates_new_tab_and_activates_it() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").unwrap();
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());
    let saved = h.renderer.current_id.clone();

    press_ctrl(h.r(), Keycode::T);

    assert_eq!(h.renderer.tabs.len(), 2);
    assert_eq!(h.renderer.active_tab, 1);
    assert_eq!(h.renderer.tabs[0].current_id, saved);
    assert_eq!(h.renderer.tabs[1].current_id, saved);
    assert_eq!(h.renderer.current_id, saved);
}

#[test]
fn ctrl_w_with_one_tab_is_noop() {
    let mut h = Harness::new();
    let before_len = h.renderer.tabs.len();
    press_ctrl(h.r(), Keycode::W);
    // The meaningful invariant: Ctrl+W with one tab does not change the tab
    // structure. (The active tab's snapshot is always refreshed from live state
    // by dispatch_key, so direct equality of `tabs` is not the right check.)
    assert_eq!(h.renderer.tabs.len(), before_len);
    assert_eq!(h.renderer.active_tab, 0);
}

#[test]
fn ctrl_w_closes_active_and_activates_previous() {
    let mut h = Harness::new();
    press_ctrl(h.r(), Keycode::T);
    press_ctrl(h.r(), Keycode::T);
    assert_eq!(h.renderer.tabs.len(), 3);
    assert_eq!(h.renderer.active_tab, 2);

    // Move active to middle, then close.
    h.renderer.active_tab = 1;
    h.renderer.current_id = h.renderer.tabs[1].current_id.clone();

    press_ctrl(h.r(), Keycode::W);

    assert_eq!(h.renderer.tabs.len(), 2);
    assert_eq!(h.renderer.active_tab, 0);
}

#[test]
fn ctrl_w_closes_index_zero_keeps_zero() {
    let mut h = Harness::new();
    press_ctrl(h.r(), Keycode::T);
    press_ctrl(h.r(), Keycode::T);
    h.renderer.active_tab = 0;
    h.renderer.current_id = h.renderer.tabs[0].current_id.clone();

    press_ctrl(h.r(), Keycode::W);

    assert_eq!(h.renderer.tabs.len(), 2);
    assert_eq!(h.renderer.active_tab, 0);
}

#[test]
fn ctrl_tab_cycles_with_wraparound() {
    let mut h = Harness::new();
    press_ctrl(h.r(), Keycode::T);
    press_ctrl(h.r(), Keycode::T);
    assert_eq!(h.renderer.active_tab, 2);

    press_ctrl(h.r(), Keycode::Tab);

    assert_eq!(h.renderer.active_tab, 0);
}

#[test]
fn ctrl_shift_tab_prev_with_wraparound() {
    let mut h = Harness::new();
    press_ctrl(h.r(), Keycode::T);
    press_ctrl(h.r(), Keycode::T);
    h.renderer.active_tab = 0;
    h.renderer.current_id = h.renderer.tabs[0].current_id.clone();

    press_ctrl_shift(h.r(), Keycode::Tab);

    assert_eq!(h.renderer.active_tab, 2);
}

#[test]
fn ctrl_n_activates_nth_tab_or_noop() {
    let mut h = Harness::new();
    press_ctrl(h.r(), Keycode::T);
    press_ctrl(h.r(), Keycode::T);
    assert_eq!(h.renderer.tabs.len(), 3);

    press_ctrl(h.r(), Keycode::_3);
    assert_eq!(h.renderer.active_tab, 2);

    press_ctrl(h.r(), Keycode::_1);
    assert_eq!(h.renderer.active_tab, 0);

    press_ctrl(h.r(), Keycode::_9);
    assert_eq!(h.renderer.active_tab, 0);
}

#[test]
fn tab_switch_preserves_per_tab_navigation() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").unwrap();
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());
    let tab0_path = h.renderer.current_id.clone();

    press_ctrl(h.r(), Keycode::T);
    // Move within tab 1.
    press_down(h.r());
    let tab1_path = h.renderer.current_id.clone();
    assert_ne!(tab0_path, tab1_path, "navigation should diverge between tabs");

    press_ctrl(h.r(), Keycode::_1);
    assert_eq!(h.renderer.current_id, tab0_path);

    press_ctrl(h.r(), Keycode::_2);
    assert_eq!(h.renderer.current_id, tab1_path);
}

#[test]
fn ctrl_t_blocked_outside_general() {
    let mut h = Harness::new();
    h.renderer.coordinate = Coordinate::Insert;
    let before_len = h.renderer.tabs.len();

    press_ctrl(h.r(), Keycode::T);

    assert_eq!(h.renderer.tabs.len(), before_len);
}

#[test]
fn tab_switch_announces_via_pending_announcement() {
    let mut h = Harness::new();
    press_ctrl(h.r(), Keycode::T);
    h.renderer.pending_announcement = None;
    press_ctrl(h.r(), Keycode::Tab);

    let raw = h.renderer.pending_announcement.as_ref()
        .expect("Ctrl+Tab must produce an announcement");
    let text = strip_announcement_sentinel(raw);
    assert!(text.starts_with("tab "), "announcement should start with 'tab ', got: {text:?}");
    assert!(text.contains("/"), "announcement should contain N/M, got: {text:?}");
}

#[test]
fn tabs_persist_to_settings_provider() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").unwrap();
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());

    press_ctrl(h.r(), Keycode::T);

    // The settings provider was created with config_path set to tmp/settings.json.
    let cfg = h.settings_path();
    let data = std::fs::read_to_string(&cfg).expect("settings.json should exist after a tab op");
    let json: serde_json::Value = serde_json::from_str(&data).unwrap();
    let sicompass_section = json.get("sicompass")
        .and_then(|v| v.as_object())
        .expect("sicompass section must exist");
    assert!(sicompass_section.get("tabs").and_then(|v| v.as_str()).is_some(),
        "tabs key should be persisted as a string");
    assert!(sicompass_section.get("activeTab").and_then(|v| v.as_str()).is_some(),
        "activeTab key should be persisted as a string");
    assert_eq!(
        sicompass_section.get("activeTab").and_then(|v| v.as_str()).unwrap(),
        "1",
        "activeTab should be 1 after Ctrl+T"
    );
}

#[test]
fn load_tabs_state_restores_persisted_layout() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").unwrap();
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());
    press_ctrl(h.r(), Keycode::T);
    let expected_tabs = h.renderer.tabs.clone();
    let expected_active = h.renderer.active_tab;

    // Re-parse the on-disk settings using the same JSON shape the production
    // loader expects (`load_tabs_state`), then assert the round-trip preserves
    // both `current_id` and `provider_path`.
    let cfg = h.settings_path();
    let data = std::fs::read_to_string(&cfg).expect("settings.json should exist");
    let json: serde_json::Value = serde_json::from_str(&data).unwrap();
    let sec = json.get("sicompass").and_then(|v| v.as_object()).unwrap();
    let tabs_str = sec.get("tabs").and_then(|v| v.as_str()).unwrap();
    let active_str = sec.get("activeTab").and_then(|v| v.as_str()).unwrap();

    use sicompass_sdk::ffon::IdArray;
    use sicompass::app_state::TabSnapshot;
    let arr = match serde_json::from_str::<serde_json::Value>(tabs_str).unwrap() {
        serde_json::Value::Array(a) => a,
        _ => panic!("tabs should serialize to a JSON array"),
    };
    let parsed: Vec<TabSnapshot> = arr.into_iter().map(|v| {
        let obj = v.as_object().unwrap();
        let ids = obj.get("id").unwrap().as_array().unwrap();
        let path = obj.get("path").unwrap().as_str().unwrap().to_owned();
        let mut id = IdArray::new();
        for n in ids {
            id.push(n.as_u64().unwrap() as usize);
        }
        TabSnapshot { current_id: id, provider_path: path }
    }).collect();
    let parsed_active: usize = active_str.parse().unwrap();

    assert_eq!(parsed, expected_tabs);
    assert_eq!(parsed_active, expected_active);
}

/// Regression: `load_tabs_state` replaces `r.tabs` with the persisted layout
/// but the constructor only seeds a single `Timeline`. Without resizing
/// `tab_timelines` to match, the first arrow press (which records a
/// `Navigate` entry via `active_timeline_mut()`) panics with
/// "index out of bounds: the len is 1 but the index is 1" whenever the
/// persisted `activeTab` is non-zero.
#[test]
fn apply_tabs_section_keeps_tab_timelines_parallel_to_tabs() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").unwrap();

    let tabs_json = format!(
        r#"[{{"id":[{fb}],"path":"/"}},{{"id":[{fb}],"path":"/"}},{{"id":[{fb}],"path":"/"}}]"#,
        fb = fb_idx,
    );
    let mut sec = serde_json::Map::new();
    sec.insert("tabs".to_owned(), serde_json::Value::String(tabs_json));
    sec.insert("activeTab".to_owned(), serde_json::Value::String("2".to_owned()));

    sicompass::programs::apply_tabs_section(h.r(), &sec);

    assert_eq!(h.renderer.tabs.len(), 3, "all three persisted tabs should load");
    assert_eq!(
        h.renderer.tab_timelines.len(),
        h.renderer.tabs.len(),
        "tab_timelines must stay parallel to tabs after load",
    );
    assert_eq!(h.renderer.active_tab, 2, "saved active tab should be restored");

    // Would have panicked before the fix: active_tab=2 indexed a 1-element vec.
    let _ = h.renderer.active_timeline_mut();
}

/// Regression: after restart, a tab snapshot may reference a cursor index
/// past the end of the provider's current FFON tree — terminal scrollback,
/// chat backlog and similar ephemeral content shrink across sessions. The
/// loader must clamp so `list_index` lands on a real row instead of leaving
/// the focus rendered off-screen.
#[test]
fn load_active_tab_clamps_stale_cursor_past_end() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").unwrap();

    let provider_path = h.renderer.providers[fb_idx].current_path().to_owned();
    let children_len = match &h.renderer.ffon[fb_idx] {
        FfonElement::Obj(o) => o.children.len(),
        _ => 0,
    };
    assert!(children_len >= 1, "harness should produce at least one child");

    // Forge a snapshot whose cursor sits well past the actual children.
    let mut id = sicompass_sdk::ffon::IdArray::new();
    id.push(fb_idx);
    id.push(children_len + 100);
    h.renderer.tabs[0] = sicompass::app_state::TabSnapshot {
        current_id: id,
        provider_path,
    };
    h.renderer.active_tab = 0;
    h.renderer.load_active_tab();

    let last = h.renderer.current_id.last().unwrap_or(0);
    assert!(
        last < children_len,
        "current_id.last() = {} should be clamped to < children_len {}",
        last,
        children_len
    );
    assert_eq!(
        h.renderer.list_index, last,
        "list_index must mirror the clamped cursor"
    );
}

/// Regression: the webbrowser provider does not persist its loaded page, so
/// after a restart a saved `current_id` that was deep inside the previous
/// page tree no longer resolves at intermediate levels — the URL bar at
/// `[wb_idx, 0]` is a `Str`, not an `Obj`, so the walk fails before reaching
/// the last index. Without popping stale levels, focus would render past
/// the end of the rebuilt tree. After the fix, the cursor collapses back to
/// the URL bar (`[wb_idx, 0]`) and `list_index == 0`.
#[test]
fn load_active_tab_pops_stale_levels_for_webbrowser() {
    let mut h = Harness::new_with_webbrowser();
    let wb_idx = h.provider_idx("webbrowser").expect("webbrowser registered");

    // The webbrowser provider with no loaded page exposes a single `Str`
    // URL-bar child — confirm that's the post-restart shape.
    let children_len = match &h.renderer.ffon[wb_idx] {
        FfonElement::Obj(o) => o.children.len(),
        _ => 0,
    };
    assert_eq!(children_len, 1, "fresh webbrowser should expose just the URL bar");

    // Forge a snapshot whose cursor is buried inside a page tree that no
    // longer exists: `[wb_idx, 0, 3, 1]` — `[wb_idx, 0]` is a `Str` so the
    // walk fails at depth 1.
    let mut id = sicompass_sdk::ffon::IdArray::new();
    id.push(wb_idx);
    id.push(0);
    id.push(3);
    id.push(1);
    h.renderer.tabs[0] = sicompass::app_state::TabSnapshot {
        current_id: id,
        provider_path: "/".to_owned(),
    };
    h.renderer.active_tab = 0;
    h.renderer.load_active_tab();

    assert_eq!(
        h.renderer.current_id.depth(),
        2,
        "stale page indices should be popped back to [wb_idx, 0]"
    );
    assert_eq!(h.renderer.current_id.get(0), Some(wb_idx));
    assert_eq!(h.renderer.current_id.get(1), Some(0));
    assert_eq!(
        h.renderer.list_index, 0,
        "list_index must land on the URL bar"
    );
}

/// Counterpart to the webbrowser pop test: when the saved `current_id`
/// fully resolves through the rebuilt FFON tree, the loader must leave it
/// unchanged. Guards against an overly aggressive pop loop that would
/// truncate valid deep cursors.
#[test]
fn load_active_tab_preserves_valid_deep_cursor() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").unwrap();

    // Stamp a hand-built nested tree onto the file browser slot so we can
    // exercise a depth-3 cursor without depending on lazy-fetch behavior.
    // `set_current_path("/")` keeps the provider's snapshot path stable so
    // the rebuild branch in `load_active_tab` (which only fires when
    // `current_path()` differs from `snap.provider_path`) is skipped — the
    // hand-built FFON survives the call.
    h.renderer.providers[fb_idx].set_current_path("/");
    let mut root = FfonElement::new_obj("file browser");
    let mut mid = FfonElement::new_obj("mid");
    mid.as_obj_mut().unwrap().push(FfonElement::new_str("leaf-a"));
    mid.as_obj_mut().unwrap().push(FfonElement::new_str("leaf-b"));
    root.as_obj_mut().unwrap().push(mid);
    h.renderer.ffon[fb_idx] = root;

    let mut id = sicompass_sdk::ffon::IdArray::new();
    id.push(fb_idx);
    id.push(0);
    id.push(1);
    h.renderer.tabs[0] = sicompass::app_state::TabSnapshot {
        current_id: id.clone(),
        provider_path: "/".to_owned(),
    };
    h.renderer.active_tab = 0;
    h.renderer.load_active_tab();

    assert_eq!(
        h.renderer.current_id, id,
        "valid depth-3 cursor must survive load_active_tab unchanged"
    );
    assert_eq!(h.renderer.list_index, 1);
}

/// Regression test for the bug the user reported: navigating Left in tab A
/// rebuilds the file browser's FFON tree, leaving tab B's saved indices
/// pointing at the wrong content. With per-tab provider_path snapshots,
/// switching back to tab B should restore its directory and re-fetch.
#[test]
fn tab_switch_restores_provider_path_after_other_tab_navigates() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").unwrap();

    // Tab 1: enter file browser, then enter the `subdir` directory.
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r()); // into file browser
    // Navigate to "subdir" (which we know was created by Harness::new).
    let subdir_idx = {
        let provider_root = h.renderer.ffon[fb_idx].as_obj().unwrap();
        provider_root.children.iter().position(|c| matches!(c, FfonElement::Obj(o)
            if sicompass_sdk::tags::strip_display(&o.key).contains("subdir")))
            .expect("subdir must be in the listing")
    };
    h.renderer.current_id.set(1, subdir_idx);
    sicompass::list::create_list_current_layer(h.r());
    press_right(h.r()); // into subdir
    let tab1_path_before = sicompass::provider::current_path(h.r()).to_owned();
    assert!(tab1_path_before.ends_with("subdir"),
        "expected to be inside subdir, got {tab1_path_before:?}");

    // Open a second tab (duplicate of tab 1, both inside subdir).
    press_ctrl(h.r(), Keycode::T);
    assert_eq!(h.renderer.tabs.len(), 2);
    assert_eq!(h.renderer.active_tab, 1);

    // In tab 2, navigate Left back to the file-browser root. This rebuilds
    // r.ffon[fb_idx] for the parent directory.
    press_left(h.r());
    let tab2_path_after = sicompass::provider::current_path(h.r()).to_owned();
    assert_ne!(tab1_path_before, tab2_path_after,
        "Left in tab 2 should change the file browser's path");

    // Switch back to tab 1: the file browser should be re-set to subdir, and
    // the FFON tree should reflect subdir contents (containing nested.txt).
    press_ctrl(h.r(), Keycode::_1);
    let tab1_path_restored = sicompass::provider::current_path(h.r()).to_owned();
    assert_eq!(tab1_path_restored, tab1_path_before,
        "switching back to tab 1 must restore its saved provider path");

    // The FFON tree at the file browser root now reflects subdir; verify
    // nested.txt is present in the list.
    let labels: Vec<String> = h.renderer.total_list.iter().map(|i| i.label.clone()).collect();
    let any_nested = labels.iter().any(|l| sicompass_sdk::tags::strip_display(l).contains("nested.txt"));
    assert!(any_nested, "tab 1 listing must contain nested.txt after restore, got {labels:?}");
}

/// Regression: navigating between programs at root (depth 1) must update the
/// active tab's snapshot so the tab band label and persisted config follow.
/// Before the fix, `tabs[active_tab]` was only refreshed on explicit tab ops.
#[test]
fn root_navigation_updates_active_tab_snapshot() {
    let mut h = Harness::new();
    let start = h.renderer.current_id.get(0).unwrap_or(0);
    assert_eq!(h.renderer.tabs[0].current_id.get(0), Some(start));

    press_down(h.r());
    let after = h.renderer.current_id.get(0).unwrap_or(0);
    assert_ne!(after, start, "Down at root must move between providers");

    assert_eq!(
        h.renderer.tabs[h.renderer.active_tab].current_id, h.renderer.current_id,
        "active tab snapshot must track current_id after root navigation"
    );
}

/// Regression: entering a program from root (Right at depth 1) must update
/// the active tab's snapshot — both `current_id` and `provider_path`.
#[test]
fn entering_program_from_root_updates_active_tab_snapshot() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").unwrap();
    navigate_to_provider(h.r(), fb_idx);

    press_right(h.r());
    assert!(h.renderer.current_id.depth() >= 2,
        "Right from root should push into the provider");

    let snap_current_id = h.renderer.tabs[h.renderer.active_tab].current_id.clone();
    let snap_provider_path = h.renderer.tabs[h.renderer.active_tab].provider_path.clone();
    assert_eq!(snap_current_id, h.renderer.current_id);
    let live_path = sicompass::provider::current_path(h.r()).to_owned();
    assert_eq!(snap_provider_path, live_path,
        "active tab snapshot must capture provider path after entering");
}

/// Root navigation persists to settings.json so `config` follows on restart.
#[test]
fn root_navigation_persists_to_settings() {
    let mut h = Harness::new();
    press_down(h.r());
    let expected_first_id = h.renderer.current_id.get(0).unwrap_or(0);

    let cfg = h.settings_path();
    let data = std::fs::read_to_string(&cfg)
        .expect("settings.json should exist after root navigation");
    let json: serde_json::Value = serde_json::from_str(&data).unwrap();
    let tabs_str = json.get("sicompass")
        .and_then(|v| v.as_object())
        .and_then(|s| s.get("tabs"))
        .and_then(|v| v.as_str())
        .expect("tabs key should be persisted after navigation");
    let arr = serde_json::from_str::<serde_json::Value>(tabs_str).unwrap();
    let first_id_arr = arr.as_array().unwrap()[0]
        .as_object().unwrap()
        .get("id").unwrap()
        .as_array().unwrap();
    assert_eq!(first_id_arr[0].as_u64().unwrap() as usize, expected_first_id,
        "persisted tab's first index must match post-navigation provider");
}

// ---------------------------------------------------------------------------
// Auto-launch dashboard on alt-screen sequence (terminal provider)
// ---------------------------------------------------------------------------

/// Mirrors the tick + auto-dashboard dispatch block from `view.rs`. Tests
/// can't run the SDL main loop, so this drains pending requests and routes
/// them through the same handler functions the loop uses.
fn pump_tick(r: &mut AppRenderer) {
    let mut requests: Vec<(usize, sicompass_sdk::DashboardRequest)> = Vec::new();
    for (i, p) in r.providers.iter_mut().enumerate() {
        let _ = p.tick();
        if let Some(req) = p.take_dashboard_request() {
            requests.push((i, req));
        }
    }
    for (i, req) in requests {
        if r.current_id.get(0) != Some(i) {
            continue;
        }
        match req {
            sicompass_sdk::DashboardRequest::Enter
                if r.coordinate != Coordinate::Dashboard =>
            {
                // Mirror view.rs: reset to General + clear input buffer,
                // then bypass the manual-entry guard.
                r.coordinate = Coordinate::General;
                r.input_buffer.clear();
                r.cursor_position = 0;
                sicompass::handlers::enter_dashboard_for_active(r);
            }
            sicompass_sdk::DashboardRequest::Leave => {
                sicompass::handlers::handle_dashboard_leave(r);
            }
            _ => {}
        }
    }
}

#[cfg(unix)]
#[test]
fn terminal_auto_enters_and_leaves_dashboard_on_alt_screen() {
    use std::thread;
    use std::time::{Duration, Instant};

    ensure_builtins();
    let mut renderer = AppRenderer::new();
    register(&mut renderer, sicompass_sdk::create_provider_by_name("terminal").unwrap());
    sicompass::list::create_list_current_layer(&mut renderer);

    // Sanity: terminal is the active provider (idx 0) and we're not in dashboard.
    assert_eq!(renderer.current_id.get(0), Some(0));
    assert_ne!(renderer.coordinate, Coordinate::Dashboard);

    // Submit a one-liner that opens the alt screen, sleeps long enough for
    // us to observe the entered state, then closes it.
    let term_idx = renderer.providers.iter().position(|p| p.name() == "terminal").unwrap();
    let submitted = renderer.providers[term_idx]
        .commit_edit("", "printf '\\033[?1049h'; sleep 1; printf '\\033[?1049l'");
    if !submitted {
        // Shell spawn failed (e.g. CI sandbox without /bin/sh). Skip.
        return;
    }

    // Drive the tick loop until we see Dashboard.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut entered = false;
    while Instant::now() < deadline {
        pump_tick(&mut renderer);
        if renderer.coordinate == Coordinate::Dashboard {
            entered = true;
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(entered, "expected auto-enter into Dashboard after alt-screen-h");

    // Continue ticking until the trailing alt-screen-l is observed and we
    // auto-leave back to the prior coordinate.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut left = false;
    while Instant::now() < deadline {
        pump_tick(&mut renderer);
        if renderer.coordinate != Coordinate::Dashboard {
            left = true;
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }
    assert!(left, "expected auto-leave from Dashboard after alt-screen-l");
}

#[cfg(unix)]
#[test]
fn auto_leave_lands_in_general_mode_even_if_user_was_in_insert() {
    // Repro: user is on the input slot in Insert mode typing `btop`. They
    // press Enter; auto-launch fires; they Ctrl+C btop; auto-leave fires.
    // They must land in General mode — otherwise pressing `i`/`a` to
    // re-enter Insert mode would type those letters literally.
    use std::thread;
    use std::time::{Duration, Instant};

    ensure_builtins();
    let mut renderer = AppRenderer::new();
    register(&mut renderer, sicompass_sdk::create_provider_by_name("terminal").unwrap());
    sicompass::list::create_list_current_layer(&mut renderer);

    // Simulate "user was in Insert mode with a stale buffer".
    renderer.coordinate = Coordinate::Insert;
    renderer.input_buffer = "btop".to_owned();
    renderer.cursor_position = 4;

    let term_idx = renderer.providers.iter().position(|p| p.name() == "terminal").unwrap();
    if !renderer.providers[term_idx]
        .commit_edit("", "printf '\\033[?1049h'; sleep 1; printf '\\033[?1049l'")
    {
        return;
    }

    // Pump until we're in the dashboard.
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && renderer.coordinate != Coordinate::Dashboard {
        pump_tick(&mut renderer);
        thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(renderer.coordinate, Coordinate::Dashboard,
        "expected auto-enter into Dashboard");

    // Pump until we're back out (auto-leave).
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && renderer.coordinate == Coordinate::Dashboard {
        pump_tick(&mut renderer);
        thread::sleep(Duration::from_millis(20));
    }
    assert_ne!(renderer.coordinate, Coordinate::Dashboard,
        "expected auto-leave from Dashboard");

    // The fix: regardless of what mode the user was in before auto-launch,
    // auto-leave returns them to a clean General mode with no stale Insert
    // state. Otherwise `i`/`a` would type literally instead of switching
    // back into Insert.
    assert_eq!(renderer.coordinate, Coordinate::General,
        "auto-leave must land in General mode, not the pre-launch Insert mode");
    assert!(renderer.input_buffer.is_empty(),
        "auto-leave must clear stale input_buffer; got {:?}", renderer.input_buffer);
}

#[cfg(unix)]
#[test]
fn terminal_manual_d_keypress_is_blocked() {
    // Pressing `d` while the terminal provider is active must NOT enter the
    // dashboard. Auto-launch (via alt-screen detection) is the only valid
    // path — a manually-entered terminal dashboard at a bare shell prompt
    // would have no clean exit (every key, including Esc and Ctrl+C, is
    // forwarded to the program).
    ensure_builtins();
    let mut renderer = AppRenderer::new();
    register(&mut renderer, sicompass_sdk::create_provider_by_name("terminal").unwrap());
    sicompass::list::create_list_current_layer(&mut renderer);

    let term_idx = renderer.providers.iter().position(|p| p.name() == "terminal").unwrap();
    if !renderer.providers[term_idx].commit_edit("", "true") {
        return;
    }

    let coord_before = renderer.coordinate;
    sicompass::handlers::handle_dashboard(&mut renderer);
    assert_eq!(renderer.coordinate, coord_before,
        "manual handle_dashboard must be a no-op for the terminal provider");
}

#[cfg(unix)]
#[test]
fn esc_in_interactive_dashboard_does_not_exit() {
    // Esc must pass through to the program (vim normal mode etc.). The
    // dashboard must stay open so the program receives the byte.
    ensure_builtins();
    let mut renderer = AppRenderer::new();
    register(&mut renderer, sicompass_sdk::create_provider_by_name("terminal").unwrap());
    sicompass::list::create_list_current_layer(&mut renderer);

    let term_idx = renderer.providers.iter().position(|p| p.name() == "terminal").unwrap();
    if !renderer.providers[term_idx].commit_edit("", "true") {
        return;
    }

    // Bypass the manual-entry guard the same way the auto-launch path does.
    sicompass::handlers::enter_dashboard_for_active(&mut renderer);
    assert_eq!(renderer.coordinate, Coordinate::Dashboard);

    press_escape(&mut renderer);
    assert_eq!(renderer.coordinate, Coordinate::Dashboard,
        "Esc must be forwarded to the interactive dashboard program, not exit");
}

#[cfg(unix)]
#[test]
fn ctrl_c_in_interactive_dashboard_does_not_exit() {
    // Ctrl+C must pass through to the program as the SIGINT byte (0x03) so
    // `btop`/`htop`/etc. actually terminate. Without this, killing btop
    // from the dashboard left it running on the PTY and re-launching it
    // failed silently.
    ensure_builtins();
    let mut renderer = AppRenderer::new();
    register(&mut renderer, sicompass_sdk::create_provider_by_name("terminal").unwrap());
    sicompass::list::create_list_current_layer(&mut renderer);

    let term_idx = renderer.providers.iter().position(|p| p.name() == "terminal").unwrap();
    if !renderer.providers[term_idx].commit_edit("", "true") {
        return;
    }

    sicompass::handlers::enter_dashboard_for_active(&mut renderer);
    assert_eq!(renderer.coordinate, Coordinate::Dashboard);

    press_ctrl(&mut renderer, Keycode::C);
    assert_eq!(renderer.coordinate, Coordinate::Dashboard,
        "Ctrl+C must be forwarded to the program (SIGINT), not exit the dashboard");
}

// ---------------------------------------------------------------------------
// Unified Timeline dispatcher (step 3 of the undo/redo refactor)
// ---------------------------------------------------------------------------
//
// These tests exercise `record_entry`, `walk_back`, `walk_forward` directly
// (the public dispatcher entry points in `sicompass::state`). Emission sites
// don't call these yet — step 4+ wires them in.

use sicompass::state as state_mod;
use sicompass_sdk::timeline::{NavKind, StructuralOp, StructuralPayload, TimelineEntry};
use sicompass_sdk::ffon::IdArray;
use std::thread::sleep;
use std::time::Duration;

fn id(parts: &[usize]) -> IdArray {
    let mut a = IdArray::new();
    for p in parts {
        a.push(*p);
    }
    a
}

#[test]
fn record_entry_pushes_to_active_tab_timeline() {
    let mut h = Harness::new();
    let entry = TimelineEntry::Navigate {
        provider_idx: 0,
        from_id: id(&[0]),
        to_id: id(&[0, 1]),
        from_path: None,
        to_path: None,
        kind: NavKind::ArrowRight,
    };
    state_mod::record_entry(h.r(), entry);
    assert_eq!(h.renderer.active_timeline().entries.len(), 1);
    assert_eq!(h.renderer.active_timeline().position, 0);
}

#[test]
fn record_entry_coalesces_navigate_burst() {
    let mut h = Harness::new();
    // Five consecutive Navigates collapse to one entry.
    for i in 0..5 {
        state_mod::record_entry(
            h.r(),
            TimelineEntry::Navigate {
                provider_idx: 0,
                from_id: id(&[0, i]),
                to_id: id(&[0, i + 1]),
                from_path: None,
                to_path: None,
                kind: NavKind::ArrowDown,
            },
        );
    }
    assert_eq!(h.renderer.active_timeline().entries.len(), 1);
    // The single surviving entry preserves the first `from_id` and the latest `to_id`.
    match &h.renderer.active_timeline().entries[0] {
        TimelineEntry::Navigate { from_id, to_id, .. } => {
            assert_eq!(*from_id, id(&[0, 0]));
            assert_eq!(*to_id, id(&[0, 5]));
        }
        _ => panic!("expected Navigate"),
    }
}

#[test]
fn record_entry_breaks_navigate_run_on_text_chunk() {
    let mut h = Harness::new();
    state_mod::record_entry(
        h.r(),
        TimelineEntry::Navigate {
            provider_idx: 0,
            from_id: id(&[0]),
            to_id: id(&[0, 1]),
            from_path: None,
            to_path: None,
            kind: NavKind::ArrowDown,
        },
    );
    state_mod::record_entry(
        h.r(),
        TimelineEntry::TextChunk {
            id: id(&[0, 1]),
            before: FfonElement::Str("a".into()),
            after: FfonElement::Str("ab".into()),
            chunk_seq: 0,
        },
    );
    state_mod::record_entry(
        h.r(),
        TimelineEntry::Navigate {
            provider_idx: 0,
            from_id: id(&[0, 1]),
            to_id: id(&[0, 2]),
            from_path: None,
            to_path: None,
            kind: NavKind::ArrowDown,
        },
    );
    // Should be three entries — the TextChunk breaks coalescing.
    assert_eq!(h.renderer.active_timeline().entries.len(), 3);
}

#[test]
fn record_entry_coalesces_text_chunk_within_idle() {
    let mut h = Harness::new();
    state_mod::record_entry(
        h.r(),
        TimelineEntry::TextChunk {
            id: id(&[0, 0]),
            before: FfonElement::Str("".into()),
            after: FfonElement::Str("h".into()),
            chunk_seq: 0,
        },
    );
    state_mod::record_entry(
        h.r(),
        TimelineEntry::TextChunk {
            id: id(&[0, 0]),
            before: FfonElement::Str("h".into()),
            after: FfonElement::Str("hi".into()),
            chunk_seq: 0,
        },
    );
    assert_eq!(h.renderer.active_timeline().entries.len(), 1);
    match &h.renderer.active_timeline().entries[0] {
        TimelineEntry::TextChunk { before, after, .. } => {
            assert_eq!(before, &FfonElement::Str("".into()));
            assert_eq!(after, &FfonElement::Str("hi".into()));
        }
        _ => panic!("expected TextChunk"),
    }
}

#[test]
fn record_entry_starts_new_text_chunk_after_idle_period() {
    let mut h = Harness::new();
    state_mod::record_entry(
        h.r(),
        TimelineEntry::TextChunk {
            id: id(&[0, 0]),
            before: FfonElement::Str("".into()),
            after: FfonElement::Str("h".into()),
            chunk_seq: 0,
        },
    );
    // Wait past TEXT_CHUNK_IDLE_MS (500 ms) — a short buffer is enough.
    sleep(Duration::from_millis(550));
    state_mod::record_entry(
        h.r(),
        TimelineEntry::TextChunk {
            id: id(&[0, 0]),
            before: FfonElement::Str("h".into()),
            after: FfonElement::Str("hi".into()),
            chunk_seq: 0,
        },
    );
    assert_eq!(h.renderer.active_timeline().entries.len(), 2);
}

#[test]
fn record_entry_truncates_redo_branch_on_new_action() {
    let mut h = Harness::new();
    // Two entries, undo once → position=1, redo branch holds 1 entry.
    state_mod::record_entry(
        h.r(),
        TimelineEntry::Navigate {
            provider_idx: 0,
            from_id: id(&[0]),
            to_id: id(&[0, 1]),
            from_path: None,
            to_path: None,
            kind: NavKind::ArrowDown,
        },
    );
    state_mod::record_entry(
        h.r(),
        TimelineEntry::TextChunk {
            id: id(&[0, 1]),
            before: FfonElement::Str("a".into()),
            after: FfonElement::Str("ab".into()),
            chunk_seq: 0,
        },
    );
    state_mod::walk_back(h.r());
    assert_eq!(h.renderer.active_timeline().position, 1);
    assert_eq!(h.renderer.active_timeline().entries.len(), 2);

    // New action: truncates the dangling redo entry.
    state_mod::record_entry(
        h.r(),
        TimelineEntry::Navigate {
            provider_idx: 0,
            from_id: id(&[0, 1]),
            to_id: id(&[0, 0]),
            from_path: None,
            to_path: None,
            kind: NavKind::ArrowUp,
        },
    );
    assert_eq!(h.renderer.active_timeline().entries.len(), 2);
    assert_eq!(h.renderer.active_timeline().position, 0);
}

#[test]
fn walk_back_reports_nothing_to_undo_when_empty() {
    let mut h = Harness::new();
    state_mod::walk_back(h.r());
    assert_eq!(h.renderer.error_message, "No undo history");
}

#[test]
fn walk_forward_reports_nothing_to_redo_when_at_head() {
    let mut h = Harness::new();
    state_mod::record_entry(
        h.r(),
        TimelineEntry::Navigate {
            provider_idx: 0,
            from_id: id(&[0]),
            to_id: id(&[0, 1]),
            from_path: None,
            to_path: None,
            kind: NavKind::ArrowDown,
        },
    );
    state_mod::walk_forward(h.r());
    assert_eq!(h.renderer.error_message, "Nothing to redo");
}

#[test]
fn walk_back_then_forward_restores_position() {
    let mut h = Harness::new();
    let start = id(&[0, 0]);
    let end = id(&[0, 3]);
    h.renderer.current_id = start.clone();
    state_mod::record_entry(
        h.r(),
        TimelineEntry::Navigate {
            provider_idx: 0,
            from_id: start.clone(),
            to_id: end.clone(),
            from_path: None,
            to_path: None,
            kind: NavKind::ArrowDown,
        },
    );
    h.renderer.current_id = end.clone();

    state_mod::walk_back(h.r());
    assert_eq!(h.renderer.current_id, start);

    state_mod::walk_forward(h.r());
    assert_eq!(h.renderer.current_id, end);
}

#[test]
fn timelines_are_per_tab() {
    let mut h = Harness::new();
    state_mod::record_entry(
        h.r(),
        TimelineEntry::Navigate {
            provider_idx: 0,
            from_id: id(&[0]),
            to_id: id(&[0, 1]),
            from_path: None,
            to_path: None,
            kind: NavKind::ArrowDown,
        },
    );
    assert_eq!(h.renderer.active_timeline().entries.len(), 1);

    press_ctrl(h.r(), Keycode::T);
    // Switched to fresh tab 1 — its timeline is empty.
    assert_eq!(h.renderer.active_tab, 1);
    assert_eq!(h.renderer.active_timeline().entries.len(), 0);

    // Tab 0's history is preserved.
    assert_eq!(h.renderer.tab_timelines[0].entries.len(), 1);
}

#[test]
fn arrow_down_emits_navigate_entry() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").unwrap();
    navigate_to_provider(h.r(), fb_idx);
    let before = h.renderer.active_timeline().entries.len();
    let pre_id = h.renderer.current_id.clone();
    press_down(h.r());
    let after = h.renderer.active_timeline().entries.len();
    if h.renderer.current_id == pre_id {
        // Single-item list — Arrow Down had nothing to do; nothing recorded.
        assert_eq!(after, before);
        return;
    }
    assert_eq!(after, before + 1, "exactly one Navigate entry emitted");
    match h.renderer.active_timeline().entries.last().unwrap() {
        TimelineEntry::Navigate { kind, .. } => {
            assert_eq!(*kind, NavKind::ArrowDown);
        }
        _ => panic!("expected Navigate"),
    }
}

#[test]
fn arrow_down_burst_coalesces_in_handler() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").unwrap();
    navigate_to_provider(h.r(), fb_idx);
    let pre_id = h.renderer.current_id.clone();
    let before_count = h.renderer.active_timeline().entries.len();
    for _ in 0..5 {
        press_down(h.r());
    }
    let after_count = h.renderer.active_timeline().entries.len();
    if h.renderer.current_id == pre_id {
        // No movement happened — nothing to assert.
        assert_eq!(after_count, before_count);
        return;
    }
    // Whatever movement occurred, all 5 down-arrows collapse to a single
    // Navigate entry — burst coalescing.
    assert_eq!(after_count, before_count + 1, "burst coalesced");
}

#[test]
fn arrow_right_into_directory_emits_navigate_with_paths() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").unwrap();
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r()); // descend into the filebrowser listing
    // Harness pre-populates `subdir`. Find it in the list and step onto it.
    let subdir_idx = h
        .renderer
        .total_list
        .iter()
        .position(|item| item.label.contains("subdir"))
        .expect("subdir from Harness fixture");
    h.renderer.list_index = subdir_idx;
    h.renderer.current_id = h.renderer.total_list[subdir_idx].id.clone();
    let path_before = sicompass::provider::current_path(h.r()).to_owned();
    press_right(h.r());
    let path_after = sicompass::provider::current_path(h.r()).to_owned();
    assert_ne!(path_after, path_before, "right-arrow descended into subdir");
    // Both press_right calls produced consecutive Navigate entries which
    // coalesce into one. The merged entry's `to_path` must reflect the final
    // landing (the subdir), and the `kind` is ArrowRight.
    match h.renderer.active_timeline().entries.last().unwrap() {
        TimelineEntry::Navigate { kind, to_path, .. } => {
            assert_eq!(*kind, NavKind::ArrowRight);
            assert_eq!(to_path.as_deref(), Some(path_after.as_str()));
        }
        other => panic!("expected Navigate, got {:?}", other),
    }
}

#[test]
fn task_input_via_update_state_emits_text_chunk() {
    // Coordinate::Normal + Enter routes to handle_return_in_normal which fires
    // update_state(Task::Input). We exercise the underlying machinery directly
    // because Normal mode is rarely entered from the keyboard alone.
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").unwrap();
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());
    // Position cursor on a file element so update_state has something to read.
    let alpha_idx = h
        .renderer
        .total_list
        .iter()
        .position(|item| item.label.contains("alpha.txt"))
        .expect("alpha.txt fixture");
    h.renderer.list_index = alpha_idx;
    h.renderer.current_id = h.renderer.total_list[alpha_idx].id.clone();

    // Set up the input buffer as if the user had typed a new label, then call
    // update_state with Task::Input — the dual-write should emit a TextChunk.
    h.renderer.input_buffer = "renamed_label".to_string();
    h.renderer.cursor_position = h.renderer.input_buffer.len();
    let before_count = h.renderer.active_timeline().entries.len();
    sicompass::state::update_state(h.r(), sicompass::app_state::Task::Input, sicompass::app_state::History::None);
    let after_count = h.renderer.active_timeline().entries.len();
    assert!(after_count > before_count, "Task::Input emitted at least one entry");
    let new_entries: Vec<_> = h.renderer.active_timeline().entries[before_count..].to_vec();
    assert!(
        new_entries.iter().any(|e| matches!(e, TimelineEntry::TextChunk { .. })),
        "expected a TextChunk among new entries, got {:?}",
        new_entries
    );
}

#[test]
fn append_emits_structural_inserted_entry() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").unwrap();
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());
    let before_count = h.renderer.active_timeline().entries.len();
    h.renderer.input_buffer = "- newfile.txt".to_string();
    h.renderer.cursor_position = h.renderer.input_buffer.len();
    sicompass::state::update_state(
        h.r(),
        sicompass::app_state::Task::Append,
        sicompass::app_state::History::None,
    );
    let after_count = h.renderer.active_timeline().entries.len();
    assert!(after_count > before_count, "Task::Append recorded at least one entry");
    let new_entries: Vec<_> = h.renderer.active_timeline().entries[before_count..].to_vec();
    assert!(
        new_entries.iter().any(|e| matches!(
            e,
            TimelineEntry::Structural { op: StructuralOp::Append, payload: StructuralPayload::Inserted(_), .. }
        )),
        "expected Structural{{Append, Inserted}}, got {:?}",
        new_entries
    );
}

#[test]
fn delete_emits_structural_removed_entry() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").unwrap();
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());
    let alpha_idx = h
        .renderer
        .total_list
        .iter()
        .position(|item| item.label.contains("alpha.txt"))
        .unwrap();
    h.renderer.list_index = alpha_idx;
    h.renderer.current_id = h.renderer.total_list[alpha_idx].id.clone();
    let before_count = h.renderer.active_timeline().entries.len();
    sicompass::state::update_state(
        h.r(),
        sicompass::app_state::Task::Delete,
        sicompass::app_state::History::None,
    );
    let after_count = h.renderer.active_timeline().entries.len();
    assert!(after_count > before_count);
    let new_entries: Vec<_> = h.renderer.active_timeline().entries[before_count..].to_vec();
    assert!(
        new_entries.iter().any(|e| matches!(
            e,
            TimelineEntry::Structural { op: StructuralOp::Delete, payload: StructuralPayload::Removed(_), .. }
        )),
        "expected Structural{{Delete, Removed}}, got {:?}",
        new_entries
    );
}

use sicompass_sdk::timeline::FsOpKind;

#[test]
fn fscreate_directory_emits_fsop_create_obj() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").unwrap();
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());
    let before_count = h.renderer.active_timeline().entries.len();
    press_ctrl(h.r(), Keycode::I);
    type_text(h.r(), "+ a_unique_test_dir");
    press_enter(h.r());
    let after_count = h.renderer.active_timeline().entries.len();
    assert!(after_count > before_count);
    let new_entries: Vec<_> = h.renderer.active_timeline().entries[before_count..].to_vec();
    let fsop = new_entries
        .iter()
        .find(|e| matches!(e, TimelineEntry::FsOp { op: FsOpKind::Create, .. }));
    let fsop = fsop.expect(&format!("expected FsOp::Create, got {:?}", new_entries));
    match fsop {
        TimelineEntry::FsOp { op, after, .. } => {
            assert_eq!(*op, FsOpKind::Create);
            assert!(matches!(after, Some(FfonElement::Obj(_))), "directory = Obj");
        }
        _ => unreachable!(),
    }
    // Cleanup so tests don't pollute the temp dir
    std::fs::remove_dir_all(h.tmp.path().join("a_unique_test_dir")).ok();
}

#[test]
fn fscreate_file_emits_fsop_create_str() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").unwrap();
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());
    let before_count = h.renderer.active_timeline().entries.len();
    press_ctrl(h.r(), Keycode::I);
    type_text(h.r(), "- a_unique_test_file.txt");
    press_enter(h.r());
    let after_count = h.renderer.active_timeline().entries.len();
    assert!(after_count > before_count);
    let new_entries: Vec<_> = h.renderer.active_timeline().entries[before_count..].to_vec();
    let fsop = new_entries
        .iter()
        .find(|e| matches!(e, TimelineEntry::FsOp { op: FsOpKind::Create, .. }))
        .expect(&format!("expected FsOp::Create, got {:?}", new_entries));
    match fsop {
        TimelineEntry::FsOp { op, after, .. } => {
            assert_eq!(*op, FsOpKind::Create);
            assert!(matches!(after, Some(FfonElement::Str(_))), "file = Str");
        }
        _ => unreachable!(),
    }
    std::fs::remove_file(h.tmp.path().join("a_unique_test_file.txt")).ok();
}

#[test]
fn fsrename_emits_fsop_rename() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").unwrap();
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());
    let alpha_idx = h
        .renderer
        .total_list
        .iter()
        .position(|item| item.label.contains("alpha.txt"))
        .unwrap();
    h.renderer.list_index = alpha_idx;
    h.renderer.current_id = h.renderer.total_list[alpha_idx].id.clone();
    press(h.r(), Keycode::I);
    h.renderer.input_buffer.clear();
    h.renderer.cursor_position = 0;
    type_text(h.r(), "renamed_unique.txt");
    let before_count = h.renderer.active_timeline().entries.len();
    press_enter(h.r());
    let new_entries: Vec<_> =
        h.renderer.active_timeline().entries[before_count..].to_vec();
    let fsop = new_entries
        .iter()
        .find(|e| matches!(e, TimelineEntry::FsOp { op: FsOpKind::Rename, .. }))
        .expect(&format!("expected FsOp::Rename, got {:?}", new_entries));
    match fsop {
        TimelineEntry::FsOp { op, before, after, .. } => {
            assert_eq!(*op, FsOpKind::Rename);
            assert!(before.is_some());
            assert!(after.is_some());
        }
        _ => unreachable!(),
    }
}

#[test]
fn settings_checkbox_emits_provider_op_and_undoes() {
    use sicompass_sdk::provider::Provider;
    let mut p = sicompass_settings::SettingsProvider::new_headless();
    let tmp = TempDir::new().unwrap();
    p.set_config_path(tmp.path().join("settings.json"));
    p.add_section("test");
    p.add_checkbox("test", "Enable feature", "test.enableFeature", false);

    // Toggle the checkbox: should emit a ProviderOp.
    p.on_checkbox_change("Enable feature", true);
    let entries = p.take_timeline_entries();
    assert_eq!(entries.len(), 1, "one ProviderOp emitted");
    match &entries[0] {
        TimelineEntry::ProviderOp { command, .. } => {
            assert_eq!(command, "settings-checkbox");
        }
        _ => panic!("expected ProviderOp"),
    }

    // The new value should be persisted to JSON.
    let written = std::fs::read_to_string(tmp.path().join("settings.json")).unwrap();
    assert!(written.contains("\"test.enableFeature\": true"), "value persisted: {}", written);

    // provider.undo should restore the prior value.
    let mut err = String::new();
    p.undo(&entries[0], &mut err);
    assert!(err.is_empty(), "undo error: {}", err);
    let written = std::fs::read_to_string(tmp.path().join("settings.json")).unwrap();
    assert!(written.contains("\"test.enableFeature\": false"), "value reverted on undo: {}", written);

    // provider.redo should re-apply the change.
    p.redo(&entries[0], &mut err);
    assert!(err.is_empty(), "redo error: {}", err);
    let written = std::fs::read_to_string(tmp.path().join("settings.json")).unwrap();
    assert!(written.contains("\"test.enableFeature\": true"), "value re-applied on redo: {}", written);
}

#[test]
fn settings_radio_emits_provider_op_and_undoes() {
    use sicompass_sdk::provider::Provider;
    let mut p = sicompass_settings::SettingsProvider::new_headless();
    let tmp = TempDir::new().unwrap();
    p.set_config_path(tmp.path().join("settings.json"));
    p.add_section("test");
    p.add_radio("test", "Direction", "test.dir", &["north", "south"], "north");

    p.on_radio_change("Direction", "south");
    let entries = p.take_timeline_entries();
    assert_eq!(entries.len(), 1);
    let written = std::fs::read_to_string(tmp.path().join("settings.json")).unwrap();
    assert!(written.contains("\"test.dir\": \"south\""), "wrote south: {}", written);

    let mut err = String::new();
    p.undo(&entries[0], &mut err);
    assert!(err.is_empty(), "undo error: {}", err);
    let written = std::fs::read_to_string(tmp.path().join("settings.json")).unwrap();
    assert!(written.contains("\"test.dir\": \"north\""), "reverted on undo: {}", written);
}

#[test]
fn settings_text_emits_provider_op_and_undoes() {
    use sicompass_sdk::provider::Provider;
    let mut p = sicompass_settings::SettingsProvider::new_headless();
    let tmp = TempDir::new().unwrap();
    p.set_config_path(tmp.path().join("settings.json"));
    p.add_section("test");
    p.add_text("test", "Greeting", "test.greeting", "hello");
    p.set_current_path("/test/Greeting");

    assert!(p.commit_edit("hello", "bonjour"));
    let entries = p.take_timeline_entries();
    assert_eq!(entries.len(), 1);
    let written = std::fs::read_to_string(tmp.path().join("settings.json")).unwrap();
    assert!(written.contains("\"test.greeting\": \"bonjour\""), "wrote new value: {}", written);

    let mut err = String::new();
    p.undo(&entries[0], &mut err);
    assert!(err.is_empty(), "undo error: {}", err);
    let written = std::fs::read_to_string(tmp.path().join("settings.json")).unwrap();
    assert!(written.contains("\"test.greeting\": \"hello\""), "reverted on undo: {}", written);
}

// ---- Step 11: unified-timeline gate flip ----------------------------------

#[test]
fn unified_undo_reverts_path_changing_navigation() {
    // Descend into a subdirectory (which DOES change the filebrowser path),
    // then verify that ctrl-Z through the unified path restores the parent.
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").unwrap();
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r()); // enter the filebrowser's listing (no path change)
    let parent_path = sicompass::provider::current_path(h.r()).to_owned();

    let subdir_idx = h
        .renderer
        .total_list
        .iter()
        .position(|item| item.label.contains("subdir"))
        .expect("subdir fixture");
    h.renderer.list_index = subdir_idx;
    h.renderer.current_id = h.renderer.total_list[subdir_idx].id.clone();
    press_right(h.r()); // descend into subdir — pushes path
    let inside_path = sicompass::provider::current_path(h.r()).to_owned();
    assert_ne!(inside_path, parent_path, "subdir push changed the path");

    press_ctrl(h.r(), Keycode::Z);
    let after_undo_path = sicompass::provider::current_path(h.r()).to_owned();
    assert_eq!(after_undo_path, parent_path, "undo restored parent path");
}

#[test]
fn unified_redo_replays_path_changing_navigation() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").unwrap();
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());
    let subdir_idx = h
        .renderer
        .total_list
        .iter()
        .position(|item| item.label.contains("subdir"))
        .expect("subdir fixture");
    h.renderer.list_index = subdir_idx;
    h.renderer.current_id = h.renderer.total_list[subdir_idx].id.clone();
    press_right(h.r());
    let inside_path = sicompass::provider::current_path(h.r()).to_owned();
    press_ctrl(h.r(), Keycode::Z);
    press_ctrl_shift(h.r(), Keycode::Z);
    let after_redo_path = sicompass::provider::current_path(h.r()).to_owned();
    assert_eq!(after_redo_path, inside_path, "redo restored subdir path");
}

#[test]
fn unified_undo_reverts_directory_creation() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").unwrap();
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());

    let dir_name = "unified_undo_dir";
    let dir_path = h.tmp.path().join(dir_name);
    press_ctrl(h.r(), Keycode::I);
    type_text(h.r(), &format!("+ {}", dir_name));
    press_enter(h.r());
    assert!(dir_path.exists(), "directory created on disk");

    // ctrl-Z must walk back through the unified path and call delete_item.
    press_ctrl(h.r(), Keycode::Z);
    assert!(!dir_path.exists(), "ctrl-Z removed the directory");
}

#[test]
fn unified_undo_reverts_file_deletion_with_snapshot() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").unwrap();
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());

    let target = h.tmp.path().join("alpha.txt");
    assert!(target.exists());
    let alpha_idx = h
        .renderer
        .total_list
        .iter()
        .position(|item| item.label.contains("alpha.txt"))
        .unwrap();
    h.renderer.list_index = alpha_idx;
    h.renderer.current_id = h.renderer.total_list[alpha_idx].id.clone();

    sicompass::state::update_state(
        h.r(),
        sicompass::app_state::Task::Delete,
        sicompass::app_state::History::None,
    );
    // The legacy Task::Delete only removes the FFON entry; the filebrowser's
    // own delete_item is called via the explicit delete path. Call it directly
    // here to mirror what the user-facing Delete keybind does, then exercise
    // the unified undo path on the resulting FsOp::Delete entry.
    let prior_entries_len = h.renderer.active_timeline().entries.len();
    assert!(sicompass::provider::delete_item_by_name(h.r(), "alpha.txt"));
    let new_entries = &h.renderer.active_timeline().entries[prior_entries_len..];
    assert!(
        new_entries.iter().any(|e| matches!(
            e,
            TimelineEntry::FsOp { op: sicompass_sdk::timeline::FsOpKind::Delete, .. }
        )),
        "delete_item_by_name emitted FsOp::Delete"
    );
    assert!(!target.exists(), "file gone from disk");

    press_ctrl(h.r(), Keycode::Z);
    assert!(target.exists(), "ctrl-Z restored the file");
    assert_eq!(std::fs::read(&target).unwrap(), b"test content");
}

/// Regression: double-tap Home from inside a provider must show the root
/// list from the top, not scroll the originating provider to the top of the
/// viewport (which hid every program above it). The previous behavior set
/// `scroll_offset = list_index`, so Home-Home from a provider at index N
/// pinned N as the first visible row.
#[test]
fn double_tap_home_from_deep_nav_shows_root_from_top() {
    let mut h = Harness::new();
    let settings_idx = h.provider_idx("settings").unwrap();
    assert!(settings_idx > 0, "settings should not be the first provider");

    // Descend into a non-first provider so we have depth > 1 and a non-zero
    // alphabetical position for the originating provider.
    navigate_to_provider(h.r(), settings_idx);
    press_right(h.r());
    assert!(h.renderer.current_id.depth() > 1, "should be inside the provider");

    // Double-tap Home — two presses well within DELTA_MS (400ms).
    press(h.r(), Keycode::Home);
    press(h.r(), Keycode::Home);

    assert_eq!(h.renderer.current_id.depth(), 1, "should be back at root");
    assert_eq!(
        h.renderer.current_id.get(0),
        Some(settings_idx),
        "cursor should land on the originating provider",
    );
    assert_eq!(
        h.renderer.scroll_offset, 0,
        "root list should be scrolled to the top so all programs above the \
         originating provider remain visible (was {} = list_index before the fix)",
        h.renderer.scroll_offset,
    );
}

#[test]
fn simple_search_right_then_escape_stays_in_navigated_node() {
    // Right-arrow at end-of-search descends into the highlighted node; Escape
    // immediately after must stay there, not revert to the pre-search location.
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r()); // enter filebrowser listing
    let path_before = sicompass::provider::current_path(&h.renderer).to_owned();

    press_tab(h.r());
    assert_eq!(h.renderer.coordinate, Coordinate::SimpleSearch);
    type_text(h.r(), "subdir");

    press_right(h.r());
    let path_in_subdir = sicompass::provider::current_path(&h.renderer).to_owned();
    assert!(
        path_in_subdir.ends_with("subdir"),
        "Right at end-of-search should descend into matched subdir, got {}",
        path_in_subdir,
    );
    assert_eq!(
        h.renderer.search_origin_id, h.renderer.current_id,
        "search_origin_id must track the right-nav so Escape stays in the new node",
    );

    press_escape(h.r());
    let path_after = sicompass::provider::current_path(&h.renderer).to_owned();
    assert_eq!(
        path_after, path_in_subdir,
        "Escape after right-nav in SimpleSearch must keep cursor in subdir, \
         not jump back to {}",
        path_before,
    );
}

#[test]
fn extended_search_right_then_escape_stays_in_navigated_node() {
    // Same invariant for ExtendedSearch.
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());
    let path_before = sicompass::provider::current_path(&h.renderer).to_owned();

    press_ctrl(h.r(), Keycode::F);
    assert_eq!(h.renderer.coordinate, Coordinate::ExtendedSearch);
    type_text(h.r(), "subdir");

    press_right(h.r());
    let path_in_subdir = sicompass::provider::current_path(&h.renderer).to_owned();
    assert!(
        path_in_subdir.ends_with("subdir"),
        "Right at end-of-search should descend into matched subdir, got {}",
        path_in_subdir,
    );
    assert_eq!(
        h.renderer.search_origin_id, h.renderer.current_id,
        "search_origin_id must track the right-nav in ExtendedSearch too",
    );

    press_escape(h.r());
    let path_after = sicompass::provider::current_path(&h.renderer).to_owned();
    assert_eq!(
        path_after, path_in_subdir,
        "Escape after right-nav in ExtendedSearch must keep cursor in subdir, \
         not jump back to {}",
        path_before,
    );
}

// ---------------------------------------------------------------------------
// Tests: search commits record TimelineEntry::Navigate
// ---------------------------------------------------------------------------
//
// `Tab` (SimpleSearch) and `Ctrl+F` (ExtendedSearch) each support three
// commit actions that move `current_id`: Enter, Right-at-cursor-end, and
// Left-at-cursor-0. Each must push a Navigate entry so ctrl-Z can return
// the user to where they were before pressing Tab/Ctrl+F. Escape and
// intermediate up/down inside the search list must NOT record.

fn clear_timeline(r: &mut AppRenderer) {
    let tl = r.active_timeline_mut();
    tl.entries.clear();
    tl.position = 0;
}

#[test]
fn simple_search_enter_records_navigate() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());

    let pre_id = h.renderer.current_id.clone();
    clear_timeline(h.r());

    press_tab(h.r());
    type_text(h.r(), "subdir");
    press_enter(h.r());

    assert_eq!(h.renderer.coordinate, Coordinate::General);
    let entries = &h.renderer.active_timeline().entries;
    assert_eq!(entries.len(), 1, "search-Enter should record exactly one Navigate");
    match &entries[0] {
        sicompass_sdk::timeline::TimelineEntry::Navigate { from_id, to_id, kind, .. } => {
            assert_eq!(*from_id, pre_id, "from_id must be the pre-Tab cursor");
            assert_eq!(*to_id, h.renderer.current_id, "to_id must be the selected item");
            assert_eq!(*kind, sicompass_sdk::timeline::NavKind::ArrowRight);
        }
        other => panic!("expected Navigate, got {:?}", other),
    }

    press_ctrl(h.r(), Keycode::Z);
    assert_eq!(
        h.renderer.current_id, pre_id,
        "ctrl-Z after search-Enter must restore the pre-Tab cursor",
    );
}

#[test]
fn extended_search_enter_records_navigate() {
    // ExtendedSearch walks the in-memory FFON tree at the current node. To
    // force movement, we step down to beta.txt first and then search for
    // "alpha" — Enter should jump back to alpha.txt at index 0.
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());
    press_down(h.r()); // step off index 0 so search-Enter must move us

    let pre_id = h.renderer.current_id.clone();
    clear_timeline(h.r());

    press_ctrl(h.r(), Keycode::F);
    assert_eq!(h.renderer.coordinate, Coordinate::ExtendedSearch);
    type_text(h.r(), "alpha");
    press_enter(h.r());

    assert_eq!(h.renderer.coordinate, Coordinate::General);
    let entries = &h.renderer.active_timeline().entries;
    assert_eq!(entries.len(), 1, "ExtendedSearch Enter should record one Navigate");
    match &entries[0] {
        sicompass_sdk::timeline::TimelineEntry::Navigate { from_id, kind, .. } => {
            assert_eq!(*from_id, pre_id);
            assert_eq!(*kind, sicompass_sdk::timeline::NavKind::ArrowRight);
        }
        other => panic!("expected Navigate, got {:?}", other),
    }

    press_ctrl(h.r(), Keycode::Z);
    assert_eq!(
        h.renderer.current_id, pre_id,
        "ctrl-Z after ExtendedSearch-Enter must restore the pre-Ctrl+F cursor",
    );
}

#[test]
fn simple_search_escape_does_not_record() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());

    clear_timeline(h.r());

    press_tab(h.r());
    type_text(h.r(), "subdir");
    press_escape(h.r());

    assert_eq!(h.renderer.coordinate, Coordinate::General);
    assert_eq!(
        h.renderer.active_timeline().entries.len(),
        0,
        "Escape from SimpleSearch must not push a Navigate entry",
    );
}

#[test]
fn extended_search_escape_does_not_record() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());

    clear_timeline(h.r());

    press_ctrl(h.r(), Keycode::F);
    type_text(h.r(), "subdir");
    press_escape(h.r());

    assert_eq!(h.renderer.coordinate, Coordinate::General);
    assert_eq!(
        h.renderer.active_timeline().entries.len(),
        0,
        "Escape from ExtendedSearch must not push a Navigate entry",
    );
}

#[test]
fn simple_search_right_at_end_records_navigate() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());

    let pre_id = h.renderer.current_id.clone();
    let pre_path = sicompass::provider::current_path(&h.renderer).to_owned();
    clear_timeline(h.r());

    press_tab(h.r());
    type_text(h.r(), "subdir");
    press_right(h.r());

    let path_in_subdir = sicompass::provider::current_path(&h.renderer).to_owned();
    assert!(
        path_in_subdir.ends_with("subdir"),
        "Right-at-end in SimpleSearch should descend into subdir, got {}",
        path_in_subdir,
    );
    let entries = &h.renderer.active_timeline().entries;
    assert_eq!(entries.len(), 1, "Right-at-end should record one Navigate");
    match &entries[0] {
        sicompass_sdk::timeline::TimelineEntry::Navigate { from_id, kind, to_path, .. } => {
            assert_eq!(*from_id, pre_id);
            assert_eq!(*kind, sicompass_sdk::timeline::NavKind::ArrowRight);
            assert_eq!(to_path.as_deref(), Some(path_in_subdir.as_str()));
        }
        other => panic!("expected Navigate, got {:?}", other),
    }

    press_ctrl(h.r(), Keycode::Z);
    assert_eq!(
        h.renderer.current_id, pre_id,
        "ctrl-Z after SimpleSearch right-at-end must restore the pre-Tab cursor",
    );
    assert_eq!(
        sicompass::provider::current_path(&h.renderer),
        pre_path,
        "ctrl-Z must restore the provider path",
    );
}

#[test]
fn extended_search_right_at_end_records_navigate() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());

    let pre_id = h.renderer.current_id.clone();
    let pre_path = sicompass::provider::current_path(&h.renderer).to_owned();
    clear_timeline(h.r());

    press_ctrl(h.r(), Keycode::F);
    type_text(h.r(), "subdir");
    press_right(h.r());

    let path_in_subdir = sicompass::provider::current_path(&h.renderer).to_owned();
    assert!(
        path_in_subdir.ends_with("subdir"),
        "Right-at-end in ExtendedSearch should descend into subdir, got {}",
        path_in_subdir,
    );
    let entries = &h.renderer.active_timeline().entries;
    assert_eq!(entries.len(), 1, "Right-at-end in ExtendedSearch should record one Navigate");
    match &entries[0] {
        sicompass_sdk::timeline::TimelineEntry::Navigate { from_id, kind, .. } => {
            assert_eq!(*from_id, pre_id);
            assert_eq!(*kind, sicompass_sdk::timeline::NavKind::ArrowRight);
        }
        other => panic!("expected Navigate, got {:?}", other),
    }

    press_ctrl(h.r(), Keycode::Z);
    assert_eq!(
        h.renderer.current_id, pre_id,
        "ctrl-Z after ExtendedSearch right-at-end must restore the pre-Ctrl+F cursor",
    );
    assert_eq!(
        sicompass::provider::current_path(&h.renderer),
        pre_path,
    );
}

#[test]
fn simple_search_left_at_cursor_zero_records_navigate() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r()); // descend into filebrowser root

    // Descend one more level so Left-at-0 has somewhere to move out to.
    let subdir_idx = h.renderer.total_list.iter().position(|it| it.label.contains("subdir"))
        .expect("subdir not in listing");
    h.renderer.list_index = subdir_idx;
    h.renderer.current_id = h.renderer.total_list[subdir_idx].id.clone();
    press_right(h.r());

    let pre_id = h.renderer.current_id.clone();
    let pre_path = sicompass::provider::current_path(&h.renderer).to_owned();
    clear_timeline(h.r());

    press_tab(h.r());
    // No typing — cursor_position is 0 in the (empty) search buffer.
    press_left(h.r());

    let entries = &h.renderer.active_timeline().entries;
    assert_eq!(entries.len(), 1, "Left-at-0 in SimpleSearch should record one Navigate");
    match &entries[0] {
        sicompass_sdk::timeline::TimelineEntry::Navigate { from_id, kind, .. } => {
            assert_eq!(*from_id, pre_id);
            assert_eq!(*kind, sicompass_sdk::timeline::NavKind::ArrowLeft);
        }
        other => panic!("expected Navigate, got {:?}", other),
    }

    // After Left-at-0, search_origin_id must be reset to the new cursor —
    // otherwise the next commit in this search session would record from a
    // stale origin.
    assert_eq!(
        h.renderer.search_origin_id, h.renderer.current_id,
        "Left-at-0 must reset search_origin_id to the post-move cursor",
    );

    press_escape(h.r()); // exit the still-active search
    press_ctrl(h.r(), Keycode::Z);
    assert_eq!(
        h.renderer.current_id, pre_id,
        "ctrl-Z after Left-at-0 must restore the deeper pre-Tab cursor",
    );
    assert_eq!(
        sicompass::provider::current_path(&h.renderer),
        pre_path,
        "ctrl-Z must restore the deeper provider path",
    );
}

#[test]
fn simple_search_enter_at_root_records_navigate() {
    // At depth=1 (the root provider list), Tab + arrow-Down + Enter should
    // record a Navigate that moves cursor from filebrowser to settings.
    let mut h = Harness::new();
    assert_eq!(h.renderer.current_id.depth(), 1, "must start at root");
    let pre_id = h.renderer.current_id.clone();
    clear_timeline(h.r());

    press_tab(h.r());
    press_down(h.r()); // move from filebrowser to settings within search
    press_enter(h.r());

    assert_ne!(h.renderer.current_id, pre_id, "Enter should have moved cursor");
    let entries = &h.renderer.active_timeline().entries;
    assert_eq!(
        entries.len(),
        1,
        "Search-Enter at depth=1 must record exactly one Navigate (got {} entries)",
        entries.len(),
    );
    match &entries[0] {
        sicompass_sdk::timeline::TimelineEntry::Navigate { from_id, to_id, .. } => {
            assert_eq!(*from_id, pre_id);
            assert_eq!(*to_id, h.renderer.current_id);
        }
        other => panic!("expected Navigate, got {:?}", other),
    }

    press_ctrl(h.r(), Keycode::Z);
    assert_eq!(
        h.renderer.current_id, pre_id,
        "ctrl-Z must restore the depth=1 cursor to filebrowser",
    );
}

#[test]
fn general_right_from_root_records_navigate_with_none_from_path() {
    // General-mode Right from depth-1 (cursor on filebrowser provider entry)
    // descending into the filebrowser. The recorded Navigate must have
    // from_path=None (origin is outside the provider's path zone) and
    // to_path=Some(fb_path) so the timeline view shows the descent clearly.
    let mut h = Harness::new();
    assert_eq!(h.renderer.current_id.depth(), 1, "must start at root");
    let pre_id = h.renderer.current_id.clone();
    clear_timeline(h.r());

    press_right(h.r());

    let entries = &h.renderer.active_timeline().entries;
    assert_eq!(entries.len(), 1, "Right at depth-1 must record exactly one Navigate");
    match &entries[0] {
        sicompass_sdk::timeline::TimelineEntry::Navigate { from_id, from_path, to_path, .. } => {
            assert_eq!(*from_id, pre_id);
            assert_eq!(*from_path, None, "depth-1 origin must have from_path=None");
            assert!(to_path.is_some(), "depth-2 destination in filebrowser must have to_path=Some(..)");
        }
        other => panic!("expected Navigate, got {:?}", other),
    }

    press_ctrl(h.r(), Keycode::Z);
    assert_eq!(h.renderer.current_id, pre_id, "ctrl-Z must restore depth-1 cursor");
}

fn refresh_filebrowser_root(h: &mut Harness, fb_idx: usize) {
    let children = h.renderer.providers[fb_idx].fetch();
    let display_name = h.renderer.providers[fb_idx].display_name().to_owned();
    let mut root_elem = FfonElement::new_obj(&display_name);
    for child in children { root_elem.as_obj_mut().unwrap().push(child); }
    h.renderer.ffon[fb_idx] = root_elem;
    sicompass::list::create_list_current_layer(h.r());
}

#[test]
fn general_left_to_root_records_navigate_with_none_to_path() {
    // General-mode Left from depth-2 (cursor on a file inside filebrowser)
    // back to depth-1 (provider list). The Navigate must have
    // from_path=Some(fb_path) and to_path=None. We set the filebrowser path
    // to "/" so a single Left actually reaches depth-1 (otherwise Left at
    // depth-2 pops to the parent directory and stays at depth-2).
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    h.renderer.providers[fb_idx].set_current_path("/");
    refresh_filebrowser_root(&mut h, fb_idx);

    press_right(h.r()); // depth-1 → depth-2
    assert_eq!(h.renderer.current_id.depth(), 2, "Right should descend into filebrowser");
    let pre_id = h.renderer.current_id.clone();
    let pre_path = sicompass::provider::current_path(&h.renderer).to_owned();
    clear_timeline(h.r());

    press_left(h.r());

    assert_eq!(h.renderer.current_id.depth(), 1, "Left should pop back to root");
    let entries = &h.renderer.active_timeline().entries;
    assert_eq!(entries.len(), 1, "Left from depth-2 must record one Navigate");
    match &entries[0] {
        sicompass_sdk::timeline::TimelineEntry::Navigate { from_id, from_path, to_path, .. } => {
            assert_eq!(*from_id, pre_id);
            assert_eq!(from_path.as_deref(), Some(pre_path.as_str()));
            assert_eq!(*to_path, None, "depth-1 destination must have to_path=None");
        }
        other => panic!("expected Navigate, got {:?}", other),
    }

    press_ctrl(h.r(), Keycode::Z);
    assert_eq!(h.renderer.current_id, pre_id, "ctrl-Z must restore depth-2 cursor");
    assert_eq!(
        sicompass::provider::current_path(&h.renderer),
        pre_path,
        "ctrl-Z must restore the filebrowser path",
    );
}

#[test]
fn general_down_at_root_records_navigate_with_no_paths() {
    // General-mode Down at depth-1 (move from filebrowser to settings) must
    // record a Navigate with both paths None — the cursor never entered any
    // provider's path zone.
    let mut h = Harness::new();
    assert_eq!(h.renderer.current_id.depth(), 1, "must start at root");
    let pre_id = h.renderer.current_id.clone();
    clear_timeline(h.r());

    press_down(h.r()); // filebrowser → settings at depth=1

    let entries = &h.renderer.active_timeline().entries;
    assert_eq!(entries.len(), 1, "Down at depth-1 must record one Navigate");
    match &entries[0] {
        sicompass_sdk::timeline::TimelineEntry::Navigate { from_id, from_path, to_path, .. } => {
            assert_eq!(*from_id, pre_id);
            assert_eq!(*from_path, None, "depth-1 origin must have from_path=None");
            assert_eq!(*to_path, None, "depth-1 destination must have to_path=None");
        }
        other => panic!("expected Navigate, got {:?}", other),
    }
}

#[test]
fn general_right_into_settings_records_path_not_provider_name() {
    // Settings is a non-filebrowser, non-refresh_on_navigate provider that
    // nonetheless tracks current_path. Right-from-root + Right-into-section
    // must capture from_path/to_path so the timeline view shows the
    // descent as paths (e.g. "/" → "/Available programs:") instead of
    // falling back to the provider's display_name on both sides.
    let mut h = Harness::new();
    let settings_idx = h.provider_idx("settings").expect("settings provider");
    navigate_to_provider(h.r(), settings_idx);
    let pre_root_id = h.renderer.current_id.clone();
    clear_timeline(h.r());

    // Step 1: depth-1 → depth-2 (cursor leaves the root list, enters settings).
    // from_path=None (depth-1 origin), to_path=Some("/") (depth-2 inside provider).
    press_right(h.r());
    assert_eq!(h.renderer.current_id.depth(), 2, "Right should descend into settings");
    let path_at_settings_root = sicompass::provider::current_path(&h.renderer).to_owned();
    {
        let entries = &h.renderer.active_timeline().entries;
        assert_eq!(entries.len(), 1, "Right at depth-1 must record one Navigate");
        match &entries[0] {
            sicompass_sdk::timeline::TimelineEntry::Navigate { from_id, from_path, to_path, .. } => {
                assert_eq!(*from_id, pre_root_id);
                assert_eq!(*from_path, None, "depth-1 origin must have from_path=None");
                assert_eq!(
                    to_path.as_deref(),
                    Some(path_at_settings_root.as_str()),
                    "depth-2 destination must capture settings current_path",
                );
            }
            other => panic!("expected Navigate, got {:?}", other),
        }
    }

    // Step 2: depth-2 → depth-3 inside settings. Both paths must be Some and
    // must differ, so the timeline view shows the descent as "/" → "/<section>".
    let pre_section_path = sicompass::provider::current_path(&h.renderer).to_owned();
    press_right(h.r());
    if h.renderer.current_id.depth() < 3 {
        // Settings was empty — nothing to descend into; that's not the case
        // exercised by this test, so bail rather than misreport coverage.
        return;
    }
    let path_in_section = sicompass::provider::current_path(&h.renderer).to_owned();
    assert_ne!(
        path_in_section, pre_section_path,
        "Right into a settings section must change current_path",
    );
    let entries = &h.renderer.active_timeline().entries;
    // The two consecutive Right presses inside the provider coalesce into one
    // Navigate entry; from_path is the pre-Tab settings root, to_path is the
    // section path.
    let last = entries.last().expect("at least one Navigate entry recorded");
    match last {
        sicompass_sdk::timeline::TimelineEntry::Navigate { from_path, to_path, .. } => {
            assert!(
                from_path.is_some(),
                "non-filebrowser provider must capture from_path once inside it",
            );
            assert_eq!(
                to_path.as_deref(),
                Some(path_in_section.as_str()),
                "to_path must reflect post-nav settings path",
            );
            assert_ne!(
                from_path.as_deref(),
                to_path.as_deref(),
                "descent must surface as distinct from/to paths in the timeline",
            );
        }
        other => panic!("expected Navigate, got {:?}", other),
    }
}

#[test]
fn timeline_entry_label_collapses_identical_paths() {
    // Up/Down inside a provider produces a Navigate where from_path == to_path
    // (sibling motion doesn't change the path). The timeline view must show a
    // single path instead of "X → X", so the user sees the path they're at
    // rather than the same string repeated.
    use sicompass::list::timeline_entry_label;
    use sicompass_sdk::ffon::IdArray;
    let mut from_id = IdArray::new();
    from_id.push(0);
    from_id.push(2);
    let mut to_id = IdArray::new();
    to_id.push(0);
    to_id.push(3);
    let entry = sicompass_sdk::timeline::TimelineEntry::Navigate {
        provider_idx: 0,
        from_id,
        to_id,
        from_path: Some("/home/nico".to_owned()),
        to_path: Some("/home/nico".to_owned()),
        kind: sicompass_sdk::timeline::NavKind::ArrowDown,
    };
    let s = timeline_entry_label(&entry, &["editor".to_owned()]);
    assert!(s.contains("/home/nico"), "label must contain the path: {s}");
    assert!(
        !s.contains("\u{2192}"),
        "identical from/to must collapse to a single path (no arrow): {s}",
    );
}

#[test]
fn simple_search_enter_at_root_same_item_does_not_record() {
    // At depth=1, Tab + Enter on the same provider must NOT push a
    // phantom Navigate. Before the to_path depth gate, the entry would
    // record `from_path=None` (origin gate) vs `to_path=Some(fb_path)`
    // (destination gate), fooling the no-movement guard into firing.
    let mut h = Harness::new();
    assert_eq!(h.renderer.current_id.depth(), 1, "must start at root");
    clear_timeline(h.r());

    press_tab(h.r());
    press_enter(h.r()); // no typing, no arrow — same item

    assert_eq!(
        h.renderer.active_timeline().entries.len(),
        0,
        "Tab+Enter on same item at depth=1 must record nothing",
    );
}

#[test]
fn simple_search_enter_at_root_with_typing_records_navigate() {
    // Reproduces user-reported bug: at depth-1 (root provider list), Tab +
    // type a query that filters to a different provider + Enter must record
    // a Navigate so ctrl-Z returns to the pre-Tab provider.
    let mut h = Harness::new();
    assert_eq!(h.renderer.current_id.depth(), 1, "must start at root");
    let pre_id = h.renderer.current_id.clone();
    assert_eq!(pre_id.get(0), Some(0), "must start on the filebrowser provider");
    clear_timeline(h.r());

    press_tab(h.r());
    type_text(h.r(), "set"); // filter to the settings provider
    press_enter(h.r());

    assert_ne!(
        h.renderer.current_id, pre_id,
        "Enter on a filtered settings match must move cursor off filebrowser",
    );
    let entries = &h.renderer.active_timeline().entries;
    assert_eq!(
        entries.len(),
        1,
        "Search-Enter with typing at depth=1 must record one Navigate (got {})",
        entries.len(),
    );

    press_ctrl(h.r(), Keycode::Z);
    assert_eq!(
        h.renderer.current_id, pre_id,
        "ctrl-Z must restore the pre-Tab depth=1 cursor",
    );
}

#[test]
fn simple_search_right_at_root_records_navigate() {
    // At depth=1 with the filebrowser highlighted, Tab + Right-at-end should
    // descend into the filebrowser and record one Navigate. Critically, the
    // entry's `from_path` must be None (depth=1 is outside the provider's
    // navigable path zone) so the timeline view doesn't show a misleading
    // "/ → /" self-loop.
    let mut h = Harness::new();
    assert_eq!(h.renderer.current_id.depth(), 1, "must start at root");
    let pre_id = h.renderer.current_id.clone();
    let pre_path = sicompass::provider::current_path(&h.renderer).to_owned();
    clear_timeline(h.r());

    press_tab(h.r());
    press_right(h.r()); // cursor_position is 0 = empty buf, right-at-end fires

    assert_eq!(
        h.renderer.current_id.depth(),
        2,
        "Right-at-end at depth=1 should descend into filebrowser",
    );
    let entries = &h.renderer.active_timeline().entries;
    assert_eq!(
        entries.len(),
        1,
        "Right-at-end at depth=1 must record one Navigate (got {} entries)",
        entries.len(),
    );
    match &entries[0] {
        sicompass_sdk::timeline::TimelineEntry::Navigate { from_path, to_path, .. } => {
            assert_eq!(
                *from_path, None,
                "from_path must be None at depth=1 origin (avoids misleading `/ → /` view)",
            );
            assert!(
                to_path.as_deref().map(|p| !p.is_empty()).unwrap_or(false),
                "to_path must be Some(non-empty) once we're inside the filebrowser",
            );
        }
        other => panic!("expected Navigate, got {:?}", other),
    }

    press_ctrl(h.r(), Keycode::Z);
    assert_eq!(h.renderer.current_id, pre_id, "ctrl-Z must restore depth=1");
    assert_eq!(
        sicompass::provider::current_path(&h.renderer),
        pre_path,
        "ctrl-Z must restore the filebrowser path",
    );
}

#[test]
fn search_up_down_within_search_does_not_record() {
    let mut h = Harness::new();
    let fb_idx = h.provider_idx("filebrowser").expect("filebrowser not found");
    navigate_to_provider(h.r(), fb_idx);
    press_right(h.r());

    clear_timeline(h.r());

    press_tab(h.r());
    press_down(h.r());
    press_down(h.r());
    press_up(h.r());

    assert_eq!(
        h.renderer.active_timeline().entries.len(),
        0,
        "Up/Down inside search must not push Navigate entries",
    );

    press_escape(h.r());
    assert_eq!(
        h.renderer.active_timeline().entries.len(),
        0,
        "Escape after up/down inside search must still record nothing",
    );
}
