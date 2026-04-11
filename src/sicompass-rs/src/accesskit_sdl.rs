//! AccessKit ↔ SDL3 bridge.
//!
//! Mirrors `accesskit_sdl.c` / `accesskit_sdl.h` from the C source.
//!
//! Platform dispatch:
//! * Linux   — [`accesskit_unix::Adapter`] (AT-SPI2)
//! * Windows — [`accesskit_windows::SubclassingAdapter`] (UI Automation)
//! * macOS   — [`accesskit_macos::SubclassingAdapter`] (NSAccessibility)
//!
//! Exposes two public operations:
//!
//! * [`AccessKitAdapter::new`] — create the adapter from the SDL3 window.
//! * [`AccessKitAdapter::update_if_active`] — rebuild the accessibility tree
//!   from the current [`AppRenderer`] state, but only when an assistive
//!   technology is actually listening (zero overhead otherwise).

use crate::app_state::AppRenderer;
use accesskit::{NodeBuilder, NodeId, Role, Tree, TreeUpdate};

// ---------------------------------------------------------------------------
// Node-ID convention
//
// 0 = root window node
// 1..=N = list items (1-based to avoid NodeId(0) where 0 is reserved)
// ---------------------------------------------------------------------------

const ROOT_ID: NodeId = NodeId(0);

// ---------------------------------------------------------------------------
// AccessKitAdapter
// ---------------------------------------------------------------------------

pub struct AccessKitAdapter {
    #[cfg(target_os = "linux")]
    adapter: accesskit_unix::Adapter,
    #[cfg(target_os = "windows")]
    adapter: accesskit_windows::SubclassingAdapter,
    #[cfg(target_os = "macos")]
    adapter: accesskit_macos::SubclassingAdapter,
}

impl AccessKitAdapter {
    /// Create the adapter.
    ///
    /// Returns `None` if the native window handle cannot be obtained or if the
    /// platform is not supported.  The caller should treat `None` as
    /// "accessibility disabled" and skip all subsequent calls.
    #[allow(unused_variables)]
    pub fn new(window: &sdl3::video::Window, renderer: &AppRenderer) -> Option<Self> {
        // ---- Linux (AT-SPI2) ------------------------------------------------
        #[cfg(target_os = "linux")]
        {
            let initial_tree = build_tree(renderer);
            let adapter = accesskit_unix::Adapter::new(
                ActivationHandlerImpl { initial_tree: Some(initial_tree) },
                NoopActionHandler,
                NoopDeactivationHandler,
            );
            return Some(AccessKitAdapter { adapter });
        }

        // ---- Windows (UI Automation) ----------------------------------------
        #[cfg(target_os = "windows")]
        {
            use sdl3::sys::properties::SDL_GetPointerProperty;
            use sdl3::sys::video::{SDL_GetWindowProperties, SDL_PROP_WINDOW_WIN32_HWND_POINTER};
            use windows::Win32::Foundation::HWND;

            let props = unsafe { SDL_GetWindowProperties(window.raw()) };
            let hwnd_ptr = unsafe {
                SDL_GetPointerProperty(
                    props,
                    SDL_PROP_WINDOW_WIN32_HWND_POINTER,
                    std::ptr::null_mut(),
                )
            };
            if hwnd_ptr.is_null() {
                return None;
            }
            let initial_tree = build_tree(renderer);
            let adapter = accesskit_windows::SubclassingAdapter::new(
                HWND(hwnd_ptr),
                ActivationHandlerImpl { initial_tree: Some(initial_tree) },
                NoopActionHandler,
            );
            return Some(AccessKitAdapter { adapter });
        }

        // ---- macOS (NSAccessibility) ----------------------------------------
        #[cfg(target_os = "macos")]
        {
            use sdl3::sys::properties::SDL_GetPointerProperty;
            use sdl3::sys::video::{
                SDL_GetWindowProperties, SDL_PROP_WINDOW_COCOA_WINDOW_POINTER,
            };

            let props = unsafe { SDL_GetWindowProperties(window.raw()) };
            // SDL3 exposes the NSWindow pointer here (not NSView); we pass it
            // to `for_window` which subclasses the content view automatically,
            // mirroring the C code's `is_view=false` path.
            let ns_window = unsafe {
                SDL_GetPointerProperty(
                    props,
                    SDL_PROP_WINDOW_COCOA_WINDOW_POINTER,
                    std::ptr::null_mut(),
                )
            };
            if ns_window.is_null() {
                return None;
            }
            let initial_tree = build_tree(renderer);
            let adapter = unsafe {
                accesskit_macos::SubclassingAdapter::for_window(
                    ns_window,
                    ActivationHandlerImpl { initial_tree: Some(initial_tree) },
                    NoopActionHandler,
                )
            };
            return Some(AccessKitAdapter { adapter });
        }

        // ---- Unsupported platform -------------------------------------------
        #[allow(unreachable_code)]
        None
    }

    /// Rebuild the accessibility tree from `renderer` and push it to the
    /// platform adapter — but only when an AT is actively listening.
    #[allow(unused_variables)]
    pub fn update_if_active(&mut self, renderer: &AppRenderer) {
        #[cfg(target_os = "linux")]
        self.adapter.update_if_active(|| build_tree(renderer));

        #[cfg(target_os = "windows")]
        if let Some(events) = self.adapter.update_if_active(|| build_tree(renderer)) {
            events.raise();
        }

        #[cfg(target_os = "macos")]
        if let Some(events) = self.adapter.update_if_active(|| build_tree(renderer)) {
            events.raise();
        }
    }

    /// Notify the adapter that the window gained or lost keyboard focus.
    #[allow(unused_variables)]
    pub fn update_window_focus(&mut self, focused: bool) {
        #[cfg(target_os = "linux")]
        self.adapter.update_window_focus_state(focused);

        // Windows: the subclassing adapter handles focus internally; no call
        // needed (same as the C source).
        #[cfg(target_os = "windows")]
        let _ = focused;

        #[cfg(target_os = "macos")]
        if let Some(events) = self.adapter.update_view_focus_state(focused) {
            events.raise();
        }
    }
}

// ---------------------------------------------------------------------------
// Label-to-speech helpers (mirrors listPrefixToWord / labelToSpeech in render.c)
// ---------------------------------------------------------------------------

fn list_prefix_to_word(prefix: &str) -> Option<&'static str> {
    match prefix {
        "-"   => Some("minus"),
        "-p"  => Some("minus p"),
        "-cc" => Some("minus cc"),
        "-c"  => Some("minus c"),
        "-rc" => Some("minus rc"),
        "-b"  => Some("minus b"),
        "-i"  => Some("minus i"),
        "-r"  => Some("minus r"),
        "+"   => Some("plus"),
        "+cc" => Some("plus cc"),
        "+c"  => Some("plus c"),
        "+l"  => Some("plus l"),
        "+R"  => Some("plus R"),
        "+i"  => Some("plus i"),
        _     => None,
    }
}

fn label_to_speech(label: &str) -> String {
    let Some((prefix, content)) = label.split_once(' ') else {
        return label.to_string();
    };
    match list_prefix_to_word(prefix) {
        Some(word) => format!("{word} {content}"),
        None       => content.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Build the accessibility tree
// ---------------------------------------------------------------------------

/// Convert the current AppRenderer visible list into a flat AccessKit tree.
///
/// Layout:
/// - Node 0 (ROOT_ID): `Role::Window` — the sicompass application window.
/// - Nodes 1..=N: `Role::ListItem` — one per item in `renderer.total_list`.
///
/// The focused node tracks `renderer.list_index`.
fn build_tree(renderer: &AppRenderer) -> TreeUpdate {
    let mut nodes: Vec<(NodeId, accesskit::Node)> = Vec::with_capacity(renderer.total_list.len() + 1);

    // ---- List items --------------------------------------------------------
    let mut child_ids: Vec<NodeId> = Vec::with_capacity(renderer.total_list.len());

    for (i, item) in renderer.total_list.iter().enumerate() {
        let id = NodeId(i as u64 + 1);
        let mut builder = NodeBuilder::new(Role::ListItem);
        builder.set_name(Box::<str>::from(label_to_speech(&item.label).as_str()));
        nodes.push((id, builder.build()));
        child_ids.push(id);
    }

    // ---- Root window node --------------------------------------------------
    let mut root_builder = NodeBuilder::new(Role::Window);
    root_builder.set_name(Box::<str>::from("sicompass"));
    root_builder.set_children(child_ids);
    // Insert root at position 0
    nodes.insert(0, (ROOT_ID, root_builder.build()));

    // Focus: the currently selected list item (1-based), or root if empty.
    let focus = if renderer.total_list.is_empty() {
        ROOT_ID
    } else {
        let idx = renderer.list_index.min(renderer.total_list.len() - 1);
        NodeId(idx as u64 + 1)
    };

    TreeUpdate {
        nodes,
        tree: Some(Tree::new(ROOT_ID)),
        focus,
    }
}

// ---------------------------------------------------------------------------
// Handler implementations
// ---------------------------------------------------------------------------

/// Provides the initial tree to the platform adapter when an AT connects.
struct ActivationHandlerImpl {
    initial_tree: Option<TreeUpdate>,
}

impl accesskit::ActivationHandler for ActivationHandlerImpl {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        self.initial_tree.take()
    }
}

/// No-op action handler: sicompass keyboard navigation is modal, so AT
/// "activate" actions are not needed.
struct NoopActionHandler;

impl accesskit::ActionHandler for NoopActionHandler {
    fn do_action(&mut self, _request: accesskit::ActionRequest) {}
}

/// No-op deactivation handler (AT-SPI2 / Unix only).
#[cfg(target_os = "linux")]
struct NoopDeactivationHandler;

#[cfg(target_os = "linux")]
impl accesskit::DeactivationHandler for NoopDeactivationHandler {
    fn deactivate_accessibility(&mut self) {}
}

// ---------------------------------------------------------------------------
// Mode-change announcement helpers
// ---------------------------------------------------------------------------

/// Format the accessibility announcement for a coordinate mode change.
///
/// Mirrors `accesskitSpeakModeChange` from the C source. When `context` is
/// non-empty the result is `"{mode} - {context}"`, otherwise just `"{mode}"`.
pub fn speak_mode_change_text(renderer: &AppRenderer, context: Option<&str>) -> String {
    let mode = renderer.coordinate.as_str();
    match context {
        Some(ctx) if !ctx.is_empty() => format!("{mode} - {ctx}"),
        _ => mode.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_state::{AppRenderer, RenderListItem};
    use sicompass_sdk::ffon::IdArray;

    fn make_renderer_with_list(labels: &[&str]) -> AppRenderer {
        let mut r = AppRenderer::new();
        for &label in labels {
            r.total_list.push(RenderListItem {
                id: IdArray::new(),
                label: label.to_string(),
                data: None,
                nav_path: None,
            });
        }
        r
    }

    #[test]
    fn build_tree_empty_list() {
        let r = AppRenderer::new();
        let tree = build_tree(&r);
        assert_eq!(tree.nodes.len(), 1, "only the root node");
        assert_eq!(tree.nodes[0].0, ROOT_ID);
        assert_eq!(tree.focus, ROOT_ID);
        assert!(tree.tree.is_some());
    }

    #[test]
    fn build_tree_with_items() {
        let r = make_renderer_with_list(&["Files", "Tutorial", "Settings"]);
        let tree = build_tree(&r);
        // root + 3 items
        assert_eq!(tree.nodes.len(), 4);
        assert_eq!(tree.nodes[0].0, ROOT_ID);
        assert_eq!(tree.nodes[1].0, NodeId(1));
        assert_eq!(tree.nodes[3].0, NodeId(3));
    }

    #[test]
    fn build_tree_focus_tracks_list_index() {
        let mut r = make_renderer_with_list(&["a", "b", "c"]);
        r.list_index = 2;
        let tree = build_tree(&r);
        assert_eq!(tree.focus, NodeId(3)); // 1-based
    }

    #[test]
    fn build_tree_focus_clamps_to_last_item() {
        let mut r = make_renderer_with_list(&["only"]);
        r.list_index = 99; // out of bounds
        let tree = build_tree(&r);
        assert_eq!(tree.focus, NodeId(1));
    }

    #[test]
    fn build_tree_item_role_is_list_item() {
        let r = make_renderer_with_list(&["x"]);
        let tree = build_tree(&r);
        let (_, item_node) = &tree.nodes[1];
        assert_eq!(item_node.role(), Role::ListItem);
    }

    #[test]
    fn build_tree_root_role_is_window() {
        let r = AppRenderer::new();
        let tree = build_tree(&r);
        let (_, root_node) = &tree.nodes[0];
        assert_eq!(root_node.role(), Role::Window);
    }

    // --- label_to_speech ---

    #[test]
    fn label_to_speech_no_space_returns_raw() {
        assert_eq!(label_to_speech("Files"), "Files");
    }

    #[test]
    fn label_to_speech_minus_i() {
        assert_eq!(label_to_speech("-i newfile.txt"), "minus i newfile.txt");
    }

    #[test]
    fn label_to_speech_bare_minus() {
        assert_eq!(label_to_speech("- something"), "minus something");
    }

    #[test]
    fn label_to_speech_plus_l() {
        assert_eq!(label_to_speech("+l foo"), "plus l foo");
    }

    #[test]
    fn label_to_speech_unknown_prefix_drops_prefix() {
        // Matches C render.c:220-221: unknown prefix → speak only the content.
        assert_eq!(label_to_speech("-z thing"), "thing");
    }

    #[test]
    fn build_tree_translates_list_item_names() {
        let r = make_renderer_with_list(&["-i newfile.txt", "+l dir"]);
        let tree = build_tree(&r);
        assert_eq!(tree.nodes[1].1.name().as_deref(), Some("minus i newfile.txt"));
        assert_eq!(tree.nodes[2].1.name().as_deref(), Some("plus l dir"));
    }

    #[test]
    fn build_tree_item_labels_match() {
        let r = make_renderer_with_list(&["Files", "Tutorial"]);
        let tree = build_tree(&r);
        assert_eq!(tree.nodes[1].1.name().as_deref(), Some("Files"));
        assert_eq!(tree.nodes[2].1.name().as_deref(), Some("Tutorial"));
    }

    #[test]
    fn build_tree_root_name_is_sicompass() {
        let r = AppRenderer::new();
        let tree = build_tree(&r);
        assert_eq!(tree.nodes[0].1.name().as_deref(), Some("sicompass"));
    }

    #[test]
    fn build_tree_has_correct_tree() {
        let r = AppRenderer::new();
        let tree = build_tree(&r);
        assert!(tree.tree.is_some());
        assert_eq!(tree.tree.unwrap().root, ROOT_ID);
    }

    // --- speak_mode_change_text ---

    #[test]
    fn speak_mode_change_simple_search_no_context() {
        let mut r = AppRenderer::new();
        r.coordinate = crate::app_state::Coordinate::SimpleSearch;
        assert_eq!(speak_mode_change_text(&r, None), "search");
    }

    #[test]
    fn speak_mode_change_with_context() {
        let mut r = AppRenderer::new();
        r.coordinate = crate::app_state::Coordinate::EditorInsert;
        assert_eq!(speak_mode_change_text(&r, Some("filename.txt")), "editor insert - filename.txt");
    }

    #[test]
    fn speak_mode_change_empty_context_gives_mode_only() {
        let mut r = AppRenderer::new();
        r.coordinate = crate::app_state::Coordinate::Command;
        assert_eq!(speak_mode_change_text(&r, Some("")), "command");
    }

    #[test]
    fn speak_mode_change_operator_general() {
        let r = AppRenderer::new(); // default is OperatorGeneral
        assert_eq!(speak_mode_change_text(&r, None), "operator");
    }

    #[test]
    fn speak_mode_change_operator_insert_with_context() {
        let mut r = AppRenderer::new();
        r.coordinate = crate::app_state::Coordinate::OperatorInsert;
        assert_eq!(speak_mode_change_text(&r, Some("Documents")), "operator insert - Documents");
    }

    #[test]
    fn speak_mode_change_editor_general() {
        let mut r = AppRenderer::new();
        r.coordinate = crate::app_state::Coordinate::EditorGeneral;
        assert_eq!(speak_mode_change_text(&r, None), "editor");
    }

    #[test]
    fn speak_mode_change_extended_search() {
        let mut r = AppRenderer::new();
        r.coordinate = crate::app_state::Coordinate::ExtendedSearch;
        assert_eq!(speak_mode_change_text(&r, None), "extended search");
    }

    #[test]
    fn speak_mode_change_scroll() {
        let mut r = AppRenderer::new();
        r.coordinate = crate::app_state::Coordinate::Scroll;
        assert_eq!(speak_mode_change_text(&r, None), "scroll");
    }

    #[test]
    fn speak_mode_change_dashboard() {
        let mut r = AppRenderer::new();
        r.coordinate = crate::app_state::Coordinate::Dashboard;
        assert_eq!(speak_mode_change_text(&r, None), "dashboard");
    }

    #[test]
    fn speak_mode_change_input_search() {
        let mut r = AppRenderer::new();
        r.coordinate = crate::app_state::Coordinate::InputSearch;
        assert_eq!(speak_mode_change_text(&r, None), "input search");
    }
}
