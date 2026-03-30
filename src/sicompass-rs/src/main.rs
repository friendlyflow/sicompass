#![allow(dead_code, unused_imports)]
//! sicompass — Rust port of the Vulkan/SDL3 modal UI application.
//!
//! Module organisation mirrors the C source files in `src/sicompass/`:
//!
//! | Rust module          | C source              | Responsibility                          |
//! |----------------------|-----------------------|-----------------------------------------|
//! | `app_state`          | `app_state.h`         | `AppRenderer`, `Coordinate`, enums      |
//! | `render`             | `view.c` (render)     | Vulkan frame orchestration, AccessKit   |
//! | `view`               | `view.c` (view)       | `updateView`, mode-based render dispatch|
//! | `text`               | `text.c`              | FreeType + rustybuzz glyph pipeline     |
//! | `rectangle`          | `rectangle.c`         | Vulkan rectangle pipeline               |
//! | `image`              | `image.c`             | Texture cache, image pipeline           |
//! | `events`             | `events.c`            | SDL event → handler dispatch            |
//! | `handlers`           | `handlers.c`          | Vim-like key handlers                   |
//! | `list`               | `list.c`              | Right-panel list building / filtering   |
//! | `provider`           | `provider.c`          | Provider registry, navigation           |
//! | `programs`           | `programs.c`          | Plugin manifests, dlopen, scripts       |
//! | `state`              | `state.c`             | FFON mutations, undo history            |
//! | `unicode_search`     | `unicode_search.c`    | NFC + case-folded search                |
//! | `caret`              | `caret.c`             | Cursor blink state                      |
//! | `checkmark`          | `checkmark.c`         | Checkmark geometry                      |
//! | `accesskit_sdl`      | `accesskit_sdl.c`     | Native AccessKit ↔ SDL3 bridge          |

use app_state::AppState;

fn main() {
    let mut app = AppState::new().expect("failed to initialise application");
    app.run();
}

// ---------------------------------------------------------------------------
// Module stubs (Phase 3+ — filled in as the port progresses)
// ---------------------------------------------------------------------------

mod app_state {
    //! Application state: Vulkan context, SDL3 window, `AppRenderer`.
    //!
    //! Equivalent to `SiCompassApplication` + `AppRenderer` in `app_state.h` / `main.h`.

    use sicompass_sdk::Provider;

    pub const MAX_FRAMES_IN_FLIGHT: usize = 2;
    pub const WINDOW_TITLE: &str = "sicompass";

    // ---- Enums ----------------------------------------------------------------

    /// Navigation / edit mode — mirrors the C `Coordinate` enum.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub enum Coordinate {
        #[default]
        OperatorGeneral,
        OperatorInsert,
        EditorGeneral,
        EditorInsert,
        EditorNormal,
        EditorVisual,
        SimpleSearch,
        ExtendedSearch,
        Command,
        Scroll,
        ScrollSearch,
        InputSearch,
        Dashboard,
    }

    /// Pending edit task — mirrors the C `Task` enum.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub enum Task {
        #[default]
        None,
        Input,
        Append,
        AppendAppend,
        Insert,
        InsertInsert,
        Delete,
        ArrowUp,
        ArrowDown,
        ArrowLeft,
        ArrowRight,
        Cut,
        Copy,
        Paste,
        FsCreate,
    }

    /// Undo/redo direction.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub enum History {
        #[default]
        None,
        Undo,
        Redo,
    }

    // ---- AppState (top-level shell) ------------------------------------------

    /// Owns the SDL3 window, Vulkan context, and the `AppRenderer`.
    ///
    /// This is the Rust equivalent of `SiCompassApplication`.
    pub struct AppState {
        // TODO: SDL3 window handle (sdl3::video::Window)
        // TODO: Vulkan instance, device, swap-chain (ash types)
        // TODO: AppRenderer
    }

    impl AppState {
        /// Initialise SDL3, create a Vulkan window, set up renderers.
        pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
            // TODO Phase 3: SDL3 init, Vulkan setup
            Ok(AppState {})
        }

        /// Run the main event loop until the window is closed.
        pub fn run(&mut self) {
            // TODO Phase 4: SDL event loop → events::dispatch()
        }
    }
}

// ---------------------------------------------------------------------------

mod render {
    //! Vulkan frame orchestration and AccessKit update pump.
    //!
    //! Equivalent to the rendering half of `view.c` and `render.c`.

    /// Draw one frame: acquire swap-chain image → record command buffer →
    /// submit → present.
    pub fn draw_frame() {
        // TODO Phase 3: ash swap-chain acquire, render-pass, present
    }
}

// ---------------------------------------------------------------------------

mod view {
    //! Mode-aware render dispatch — equivalent to `updateView` in `view.c`.

    use crate::app_state::Coordinate;

    /// Re-render the current frame according to the active `Coordinate` mode.
    pub fn update_view(_coord: Coordinate) {
        // TODO Phase 4: dispatch to render_interaction / render_scroll / etc.
    }
}

// ---------------------------------------------------------------------------

mod text {
    //! FreeType + rustybuzz text rendering pipeline.
    //!
    //! Equivalent to `text.c` / `text.h`.  Pure geometry helpers are testable
    //! without a GPU; Vulkan upload is gated behind a feature flag.

    // ---- Pure geometry (no GPU required, fully testable) --------------------

    /// Measure how many pixels wide `text` renders at the given `font_size`.
    /// Returns `0` when the font renderer is not yet initialised.
    pub fn measure_text_width(_text: &str, _font_size_pt: f32) -> i32 {
        // TODO Phase 3: FreeType advance-width sum
        0
    }

    /// Count the number of UTF-8 characters (grapheme clusters) in `s` that
    /// fit within `max_px` pixels.
    pub fn chars_fitting(_s: &str, _font_size_pt: f32, _max_px: i32) -> usize {
        // TODO Phase 3
        0
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        // Port of tests/sicompass/test_text_math.c (14 tests)
        // These are pure geometry tests — no GPU or font file needed.

        #[test]
        fn measure_empty_string_is_zero() {
            assert_eq!(measure_text_width("", 12.0), 0);
        }

        #[test]
        fn chars_fitting_empty_string_is_zero() {
            assert_eq!(chars_fitting("", 12.0, 100), 0);
        }
    }
}

// ---------------------------------------------------------------------------

mod rectangle {
    //! Vulkan rectangle (filled quad) pipeline.
    //!
    //! Equivalent to `rectangle.c` / `rectangle.h`.

    /// Record draw commands for a filled rectangle.
    pub fn draw_rect(_x: i32, _y: i32, _w: i32, _h: i32, _color: u32) {
        // TODO Phase 3: ash vertex buffer, pipeline
    }
}

// ---------------------------------------------------------------------------

mod image {
    //! Image loading and Vulkan texture cache.
    //!
    //! Equivalent to `image.c` / `image.h`.

    /// Load an image from `path` into the GPU texture cache and return its
    /// cache handle.  Returns `None` if the file cannot be decoded.
    pub fn load_image(_path: &str) -> Option<usize> {
        // TODO Phase 3: image crate decode → ash VkImage upload
        None
    }
}

// ---------------------------------------------------------------------------

mod events {
    //! SDL3 event → handler dispatch.
    //!
    //! Equivalent to the event-routing logic in `view.c` / `events.c`.

    /// Dispatch a single SDL event to the appropriate handler.
    pub fn dispatch(_raw_event: ()) {
        // TODO Phase 4: match SDL_EventType → handlers::*
    }

    #[cfg(test)]
    mod tests {
        // Port of tests/sicompass/test_events.c (42 tests)
        // TODO Phase 4: mock AppRenderer + SDL event construction helpers
    }
}

// ---------------------------------------------------------------------------

mod handlers {
    //! Vim-like key handlers.
    //!
    //! Equivalent to `handlers.c`.

    #[cfg(test)]
    mod tests {
        // Port of tests/sicompass/test_handlers.c (39 tests)
        // Port of tests/sicompass/test_handlers_advanced.c (57 tests)
        // Port of tests/sicompass/test_clipboard.c (29 tests)
        // TODO Phase 4: headless AppRenderer + mockall for caret/provider/list
    }
}

// ---------------------------------------------------------------------------

mod list {
    //! Right-panel list building and filtering.
    //!
    //! Equivalent to `list.c`.

    /// A single entry in the right-panel list.
    #[derive(Debug, Clone, PartialEq)]
    pub struct ListItem {
        /// Display label (may include tag markup).
        pub label: String,
        /// Breadcrumb shown in extended-search mode.
        pub breadcrumb: Option<String>,
        /// Non-null → path-based navigation (deep-search items not in FFON tree).
        pub nav_path: Option<String>,
    }

    #[cfg(test)]
    mod tests {
        // Port of tests/sicompass/test_list.c (13 tests)
        // TODO Phase 4
    }
}

// ---------------------------------------------------------------------------

mod provider {
    //! App-level provider registry and navigation.
    //!
    //! Wraps `sicompass_sdk::Provider` with navigation context and cross-provider
    //! link resolution.  Equivalent to `provider.c`.

    #[cfg(test)]
    mod tests {
        // Port of tests/sicompass/test_provider.c (45 tests)
        // TODO Phase 4: mock providers via trait objects
    }
}

// ---------------------------------------------------------------------------

mod programs {
    //! Plugin manifest loading, `libloading`-based dlopen, script providers.
    //!
    //! Equivalent to `programs.c`.

    /// Register all providers enabled in the current settings.
    pub fn load_programs() {
        // TODO Phase 4/6: read settings, dlopen plugins, register providers
    }
}

// ---------------------------------------------------------------------------

mod state {
    //! FFON mutations and undo/redo history.
    //!
    //! Equivalent to `state.c`.

    #[cfg(test)]
    mod tests {
        // Port of tests/sicompass/test_state.c (41 tests)
        // Port of tests/sicompass/test_update.c (16 tests)
        // TODO Phase 4: pure logic, no mocking needed
    }
}

// ---------------------------------------------------------------------------

mod unicode_search {
    //! NFC-normalised, case-folded search.
    //!
    //! Equivalent to `unicode_search.c`.

    use unicode_normalization::UnicodeNormalization;

    /// Return `true` if `haystack` contains `needle` after NFC normalisation
    /// and Unicode case-folding.
    pub fn contains_normalised(haystack: &str, needle: &str) -> bool {
        if needle.is_empty() {
            return true;
        }
        let h: String = haystack.nfc().collect::<String>().to_lowercase();
        let n: String = needle.nfc().collect::<String>().to_lowercase();
        h.contains(&n)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        // Port of tests/sicompass/test_unicode_search.c (21 tests)

        #[test]
        fn empty_needle_always_matches() {
            assert!(contains_normalised("anything", ""));
        }

        #[test]
        fn exact_ascii_match() {
            assert!(contains_normalised("hello world", "world"));
        }

        #[test]
        fn case_insensitive_match() {
            assert!(contains_normalised("Hello World", "hello"));
        }

        #[test]
        fn no_match() {
            assert!(!contains_normalised("hello", "xyz"));
        }

        #[test]
        fn nfc_normalisation_match() {
            // "é" can be U+00E9 (precomposed) or U+0065 U+0301 (decomposed)
            let precomposed = "\u{00e9}";
            let decomposed = "e\u{0301}";
            assert!(contains_normalised(precomposed, decomposed));
            assert!(contains_normalised(decomposed, precomposed));
        }
    }
}

// ---------------------------------------------------------------------------

mod caret {
    //! Cursor blink state.
    //!
    //! Equivalent to `caret.c`.

    /// Caret visibility state driven by `update()`.
    #[derive(Debug, Default)]
    pub struct CaretState {
        pub visible: bool,
        last_toggle_ms: u64,
    }

    impl CaretState {
        pub const BLINK_INTERVAL_MS: u64 = 400;

        pub fn new() -> Self {
            CaretState::default()
        }

        /// Advance blink state given the current time in milliseconds.
        pub fn update(&mut self, now_ms: u64) {
            if now_ms.saturating_sub(self.last_toggle_ms) >= Self::BLINK_INTERVAL_MS {
                self.visible = !self.visible;
                self.last_toggle_ms = now_ms;
            }
        }

        /// Reset to visible, anchoring the blink timer at `now_ms`.
        pub fn reset(&mut self, now_ms: u64) {
            self.visible = true;
            self.last_toggle_ms = now_ms;
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        // Port of tests/sicompass/test_caret.c (17 tests)

        #[test]
        fn new_caret_not_visible() {
            let c = CaretState::new();
            assert!(!c.visible);
        }

        #[test]
        fn reset_makes_visible() {
            let mut c = CaretState::new();
            c.reset(0);
            assert!(c.visible);
        }

        #[test]
        fn no_toggle_before_interval() {
            let mut c = CaretState::new();
            c.reset(0);
            c.update(CaretState::BLINK_INTERVAL_MS - 1);
            assert!(c.visible);
        }

        #[test]
        fn toggles_at_interval() {
            let mut c = CaretState::new();
            c.reset(0);
            c.update(CaretState::BLINK_INTERVAL_MS);
            assert!(!c.visible);
        }

        #[test]
        fn toggles_again_at_second_interval() {
            let mut c = CaretState::new();
            c.reset(0);
            c.update(CaretState::BLINK_INTERVAL_MS);
            c.update(CaretState::BLINK_INTERVAL_MS * 2);
            assert!(c.visible);
        }
    }
}

// ---------------------------------------------------------------------------

mod checkmark {
    //! Checkmark geometry for checkbox rendering.
    //!
    //! Equivalent to `checkmark.c`.  Pure math — no GPU dependency.

    /// A line segment in 2-D space (pixel coordinates).
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct Segment {
        pub x0: f32,
        pub y0: f32,
        pub x1: f32,
        pub y1: f32,
    }

    /// Generate the two line segments that form a checkmark inside a box of
    /// `size × size` pixels, centred at `(cx, cy)`.
    ///
    /// Returns `[short_stroke, long_stroke]`.
    pub fn checkmark_segments(cx: f32, cy: f32, size: f32) -> [Segment; 2] {
        let half = size * 0.5;
        let third = size / 3.0;
        // Short stroke: bottom-left corner of the tick
        let short = Segment {
            x0: cx - half,
            y0: cy,
            x1: cx - third,
            y1: cy + third,
        };
        // Long stroke: up to the top-right
        let long = Segment {
            x0: cx - third,
            y0: cy + third,
            x1: cx + half,
            y1: cy - half + third,
        };
        [short, long]
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        // Port of tests/sicompass/test_checkmark.c (10 tests)

        #[test]
        fn checkmark_returns_two_segments() {
            let segs = checkmark_segments(10.0, 10.0, 12.0);
            assert_eq!(segs.len(), 2);
        }

        #[test]
        fn short_stroke_ends_where_long_begins() {
            let segs = checkmark_segments(10.0, 10.0, 12.0);
            assert!((segs[0].x1 - segs[1].x0).abs() < f32::EPSILON);
            assert!((segs[0].y1 - segs[1].y0).abs() < f32::EPSILON);
        }

        #[test]
        fn zero_size_collapses_to_point() {
            let segs = checkmark_segments(5.0, 5.0, 0.0);
            for seg in &segs {
                assert!((seg.x0 - seg.x1).abs() < f32::EPSILON);
                assert!((seg.y0 - seg.y1).abs() < f32::EPSILON);
            }
        }
    }
}

// ---------------------------------------------------------------------------

mod accesskit_sdl {
    //! Native AccessKit ↔ SDL3 bridge.
    //!
    //! In Rust, AccessKit is a first-class crate — no C FFI layer needed.
    //! Equivalent to `accesskit_sdl.c`.

    #[cfg(test)]
    mod tests {
        // Port of tests/sicompass/test_accesskit.c (19 tests)
        // TODO Phase 4: use real accesskit Rust API (no FFI to mock)
    }
}

// ---------------------------------------------------------------------------
// Integration tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod integration {
    //! Headless integration harness.
    //!
    //! Port of `tests/integration/test_integration.c` (27 tests).
    //! Uses a headless `AppRenderer` (no Vulkan/SDL3/AccessKit) with real
    //! providers (filebrowser, settings) and simulated key presses.
    //!
    //! TODO Phase 4: implement `HarnessAppRenderer` + `press_key()` helpers.
}
