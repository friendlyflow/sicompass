//! AccessKit ↔ SDL3 bridge.
//!
//! Mirrors `accesskit_sdl.c` / `accesskit_sdl.h` from the C source.
//!
//! Platform dispatch:
//! * Linux   — [`accesskit_unix::Adapter`] (AT-SPI2)
//! * Windows — [`accesskit_windows::SubclassingAdapter`] (UI Automation)
//! * macOS   — [`accesskit_macos::SubclassingAdapter`] (NSAccessibility)
//!
//! Linux and Windows share one tree ([`build_tree`]); macOS needs a different
//! one ([`build_tree_macos`]) because NSAccessibility ignores the mechanisms
//! the shared tree speaks through.  See the comment above `build_tree_macos`.
//!
//! Exposes two public operations:
//!
//! * [`AccessKitAdapter::new`] — create the adapter from the SDL3 window.
//! * [`AccessKitAdapter::update_if_active`] — rebuild the accessibility tree
//!   from the current [`AppRenderer`] state, but only when an assistive
//!   technology is actually listening (zero overhead otherwise).

use crate::app_state::AppRenderer;
use accesskit::{Live, Node, NodeId, Role, TreeId, TreeInfo, TreeUpdate};

// ---------------------------------------------------------------------------
// Node-ID convention
//
// 0 = root window node
// 1..=N = list items (1-based to avoid NodeId(0) where 0 is reserved)
// ---------------------------------------------------------------------------

const ROOT_ID: NodeId = NodeId(0);
/// Single placeholder list-item node.  Its label is updated in place on every
/// navigation step; Orca therefore only ever speaks the current item (mirrors
/// `ELEMENT_ID` in the C `render.c`).
const ELEMENT_ID: NodeId = NodeId(1);
/// Reserved ID for the polite live-region node used for mode-change announcements.
const ANNOUNCEMENT_ID: NodeId = NodeId(u64::MAX);

// ---------------------------------------------------------------------------
// AccessKitAdapter
// ---------------------------------------------------------------------------

pub struct AccessKitAdapter {
    #[cfg(target_os = "linux")]
    adapter: accesskit_unix::Adapter,
    /// Shared with `ActivationHandlerImpl`; set to `true` once the AT-SPI
    /// background thread calls `request_initial_tree` (tree is registered).
    #[cfg(target_os = "linux")]
    registered: std::sync::Arc<std::sync::atomic::AtomicBool>,
    #[cfg(target_os = "windows")]
    adapter: accesskit_windows::SubclassingAdapter,
    #[cfg(target_os = "macos")]
    adapter: accesskit_macos::SubclassingAdapter,
    /// Tracks what has already been spoken, so the single macOS live region
    /// announces each item or message exactly once.  See [`SpeechChannel`].
    #[cfg(target_os = "macos")]
    channel: SpeechChannel,
}

/// The parity sentinel the `speak_*` producers append so that two identical
/// consecutive announcements still differ (see `AppRenderer::speak_mode_change`).
/// Screen readers ignore it; the comparisons here have to.
#[cfg(target_os = "macos")]
const PARITY_SENTINEL: char = '\u{200B}';

/// What was last published to the macOS live region.
#[cfg(target_os = "macos")]
#[derive(Clone, Default, PartialEq, Eq, Debug)]
struct Spoken {
    /// Exactly the value put on the node, parity sentinel included.  Republished
    /// verbatim when there is nothing new, so no event fires.
    value: String,
    /// BCP-47 tag for `value`.  Empty means "use the UI locale".
    lang: String,
}

#[cfg(target_os = "macos")]
impl Spoken {
    /// `value` without the parity sentinel — what was actually *said*.
    fn text(&self) -> &str {
        self.value.trim_end_matches(PARITY_SENTINEL)
    }
}

/// Picks what the single macOS live region should carry each frame.
///
/// Two sources compete.  `speak_current_element` and friends push through
/// `pending_announcement`, but only in the filter modes (search / command /
/// extended search); in normal mode a selection change is visible *only* as a
/// new element label.  Publishing both through separate live regions would
/// speak the item twice in filter modes, so one channel picks between them:
///
/// 1. a `pending_announcement` we have not published yet wins;
/// 2. otherwise a changed element label is published;
/// 3. otherwise the previous value is republished verbatim, so nothing fires.
///
/// `pending_announcement` is never cleared by its producers, which is why it is
/// compared against the last one published rather than merely tested for
/// presence.
#[cfg(target_os = "macos")]
#[derive(Clone, Default, Debug)]
struct SpeechChannel {
    last_announcement: Option<String>,
    /// The element label as of the last frame.  Tracked separately from
    /// `published` because the two sources interleave: after an announcement is
    /// published, comparing the element label against `published` would make an
    /// *unchanged* label look new and re-read the whole list item.  In Insert
    /// mode that fires on every keystroke, burying the typed-character echo.
    last_element: String,
    published: Spoken,
}

#[cfg(target_os = "macos")]
impl SpeechChannel {
    /// The state that *would* follow from `renderer`.  Pure: the caller commits
    /// it only once the adapter has accepted the tree, so nothing is consumed
    /// while the adapter is still inactive.
    fn next(&self, renderer: &AppRenderer) -> Self {
        let announcement = renderer.pending_announcement.clone();
        let (element_label, element_content) = current_element(renderer);
        let ui_locale = sicompass_sdk::localize::current_locale();

        // 1. A new announcement. App-generated, so tagged with the UI locale.
        if let Some(text) = &announcement
            && !text.is_empty()
            && announcement != self.last_announcement
        {
            return Self {
                last_announcement: announcement.clone(),
                last_element: element_label,
                published: Spoken {
                    value: text.clone(),
                    lang: ui_locale,
                },
            };
        }

        // 2. A genuinely changed element label. Compared against the previous
        //    label rather than against what was last published: an announcement
        //    published in between must not make an unchanged label look new.
        if !element_label.is_empty() && element_label != self.last_element {
            return Self {
                last_announcement: announcement,
                published: Spoken {
                    lang: detect_language(&element_content).unwrap_or(ui_locale),
                    value: element_label.clone(),
                },
                last_element: element_label,
            };
        }

        // 3. Nothing new to say.
        Self {
            last_announcement: announcement,
            last_element: element_label,
            published: self.published.clone(),
        }
    }
}

/// Whether `SICOMPASS_A11Y_DEBUG` is set, for tracing the macOS adapter.
#[cfg(target_os = "macos")]
fn a11y_debug() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("SICOMPASS_A11Y_DEBUG").is_some())
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
            let registered = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let initial_tree = build_tree(renderer);
            let adapter = accesskit_unix::Adapter::new(
                ActivationHandlerImpl {
                    initial_tree: Some(initial_tree),
                    registered: std::sync::Arc::clone(&registered),
                },
                NoopActionHandler,
                NoopDeactivationHandler,
            );
            return Some(AccessKitAdapter {
                adapter,
                registered,
            });
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
                ActivationHandlerImpl {
                    initial_tree: Some(initial_tree),
                },
                NoopActionHandler,
            );
            return Some(AccessKitAdapter { adapter });
        }

        // ---- macOS (NSAccessibility) ----------------------------------------
        #[cfg(target_os = "macos")]
        {
            use sdl3::sys::properties::SDL_GetPointerProperty;
            use sdl3::sys::video::{SDL_GetWindowProperties, SDL_PROP_WINDOW_COCOA_WINDOW_POINTER};

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
                if a11y_debug() {
                    eprintln!("[a11y] macos: no NSWindow pointer; accessibility disabled");
                }
                return None;
            }
            // Nothing to say yet, so the live region starts without a value.
            let channel = SpeechChannel::default();
            let initial_tree = build_tree_macos(renderer, "", "");
            let adapter = unsafe {
                accesskit_macos::SubclassingAdapter::for_window(
                    ns_window,
                    ActivationHandlerImpl {
                        initial_tree: Some(initial_tree),
                    },
                    NoopActionHandler,
                )
            };
            if a11y_debug() {
                eprintln!("[a11y] macos: subclassed content view of NSWindow {ns_window:?}");
            }
            return Some(AccessKitAdapter { adapter, channel });
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

        // macOS: choose the live-region text *before* borrowing `self.adapter`
        // mutably (the update closure could not borrow `self` at the same
        // time), and commit the channel state only if the adapter took the
        // update — while it is still inactive the closure is never called, and
        // consuming an announcement there would lose it.
        #[cfg(target_os = "macos")]
        {
            let next = self.channel.next(renderer);
            let spoken = next.published.clone();
            let result = self
                .adapter
                .update_if_active(|| build_tree_macos(renderer, &spoken.value, &spoken.lang));
            if let Some(events) = result {
                let changed = next.published != self.channel.published;
                self.channel = next;
                if a11y_debug() && changed {
                    eprintln!(
                        "[a11y] macos: announce {:?} (lang {})",
                        self.channel.published.text(),
                        self.channel.published.lang
                    );
                }
                events.raise();
            } else if a11y_debug() {
                // Sampled: this is the steady state until VoiceOver first
                // queries the view, and would otherwise print every frame.
                static WARNED: std::sync::atomic::AtomicBool =
                    std::sync::atomic::AtomicBool::new(false);
                if !WARNED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    eprintln!(
                        "[a11y] macos: adapter still inactive (VoiceOver has not queried the view)"
                    );
                }
            }
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

    /// Block (with a timeout) until the AT-SPI background thread has called
    /// `request_initial_tree`, meaning AT-SPI is registered and the
    /// accessibility tree is live.  Call this before `window.show()` so that
    /// the window becomes visible only after Orca already knows about it —
    /// eliminating the gap where Orca would otherwise keep reading the terminal.
    ///
    /// On non-Linux platforms this is a no-op (Windows/macOS adapters register
    /// synchronously via window subclassing).
    #[allow(unused_variables)]
    pub fn wait_for_registration(&self, timeout: std::time::Duration) {
        #[cfg(target_os = "linux")]
        {
            let deadline = std::time::Instant::now() + timeout;
            while !self.registered.load(std::sync::atomic::Ordering::Acquire) {
                if std::time::Instant::now() >= deadline {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Label-to-speech helpers (mirrors listPrefixToWord / labelToSpeech in render.c)
// ---------------------------------------------------------------------------

fn list_prefix_to_word(prefix: &str) -> Option<&'static str> {
    match prefix {
        "-" => Some("minus"),
        "-p" => Some("minus p"),
        "-cc" => Some("minus cc"),
        "-c" => Some("minus c"),
        "-rc" => Some("minus rc"),
        "-b" => Some("minus b"),
        "-i" => Some("minus i"),
        "-r" => Some("minus r"),
        "+" => Some("plus"),
        "+cc" => Some("plus cc"),
        "+c" => Some("plus c"),
        "+l" => Some("plus l"),
        "+R" => Some("plus R"),
        "+i" => Some("plus i"),
        // Timeline-view positioners, which follow the `-` list prefix
        // (e.g. "- > x" → "minus current x"). HEAD is the next ctrl-Z target;
        // redo-branch entries have already been undone. Without these mappings
        // the marker is silently stripped, leaving screenreader users with no
        // way to distinguish current / undone / older entries in the history.
        ">" => Some("current"),
        "\u{00B7}" => Some("undone"), // ·
        _ => None,
    }
}

pub(crate) fn label_to_speech(label: &str) -> String {
    let Some((prefix, content)) = label.split_once(' ') else {
        return label.to_string();
    };
    match list_prefix_to_word(prefix) {
        Some(word) => {
            // A list prefix may be followed by a timeline positioner marker
            // ("- > x" / "- · x"); announce that second marker too.
            if let Some((marker, rest)) = content.split_once(' ') {
                if let Some(marker_word) = list_prefix_to_word(marker) {
                    return format!("{word} {marker_word} {rest}");
                }
            }
            format!("{word} {content}")
        }
        None => content.to_string(),
    }
}

// ---------------------------------------------------------------------------
// Per-item language detection (so the screen reader speaks each item in its
// own language instead of always the default voice)
// ---------------------------------------------------------------------------

/// The spoken *content* of a list label with the FFON list prefix stripped.
/// Language detection runs on this so the English prefix words ("minus i",
/// "current", …) don't bias the result.
fn speech_content(label: &str) -> &str {
    // Peel every leading known prefix, including a timeline positioner that
    // follows the list prefix ("- > x" → "x"), so detection sees only content.
    let mut s = label;
    while let Some((prefix, content)) = s.split_once(' ') {
        if list_prefix_to_word(prefix).is_some() {
            s = content;
        } else {
            break;
        }
    }
    s
}

/// Best-effort BCP-47 language tag for `content`, or `None` when detection is
/// not trustworthy (too short, low confidence, or a language we don't map).
/// The caller falls back to the active UI locale in that case.
///
/// `whatlang` is statistical and unreliable on short fragments (file names,
/// single words, numeric lists), so we require a minimum amount of signal and
/// a confidence floor before trusting it. This means short items keep the UI
/// locale's voice by design.
///
/// The floor is deliberately well below `Info::is_reliable()`'s 0.9 (which
/// rejects most ordinary one-line sentences); the length guard already filters
/// out the genuinely ambiguous fragments.
fn detect_language(content: &str) -> Option<String> {
    const MIN_CONFIDENCE: f64 = 0.5;
    let text = content.trim_end_matches('\u{200B}').trim();
    if text.chars().count() < 12 || text.split_whitespace().count() < 3 {
        return None;
    }
    let info = whatlang::detect(text)?;
    if info.confidence() < MIN_CONFIDENCE {
        return None;
    }
    // Map the languages we expect to the ISO 639-1 subtags screen readers use.
    // Unmapped languages return None so the caller falls back to the UI locale.
    let tag = match info.lang() {
        whatlang::Lang::Eng => "en",
        whatlang::Lang::Nld => "nl",
        whatlang::Lang::Fra => "fr",
        whatlang::Lang::Deu => "de",
        whatlang::Lang::Spa => "es",
        whatlang::Lang::Ita => "it",
        whatlang::Lang::Por => "pt",
        _ => return None,
    };
    Some(tag.to_owned())
}

// ---------------------------------------------------------------------------
// Build the accessibility tree
// ---------------------------------------------------------------------------

/// The currently selected list item as `(spoken label, raw content)`.
///
/// The spoken label has the FFON list prefix expanded to words
/// ([`label_to_speech`]); the raw content has it stripped entirely
/// ([`speech_content`]) and is what language detection runs on.
///
/// Both are empty when the list is empty.
fn current_element(renderer: &AppRenderer) -> (String, String) {
    if renderer.total_list.is_empty() {
        return (String::new(), String::new());
    }
    let raw_idx = if renderer.filtered_list_indices.is_empty() {
        renderer.list_index.min(renderer.total_list.len() - 1)
    } else {
        renderer
            .filtered_list_indices
            .get(renderer.list_index)
            .copied()
            .unwrap_or(0)
            .min(renderer.total_list.len() - 1)
    };
    let raw_label = &renderer.total_list[raw_idx].label;
    (
        label_to_speech(raw_label),
        speech_content(raw_label).to_owned(),
    )
}

/// Convert the current AppRenderer visible list into a flat AccessKit tree.
///
/// Build the accessibility tree from current renderer state.
///
/// Layout (mirrors the C `render.c` single-element pattern):
/// - `ROOT_ID` (`Role::Window`): the sicompass application window.
/// - `ELEMENT_ID` (`Role::ListItem`): **one** placeholder whose label is the
///   currently selected item.  Updated in place on every navigation step so
///   Orca only ever reads the current item, not an enumeration of all items.
/// - `ANNOUNCEMENT_ID` (`Role::ListItem`, `Live::Polite`): always-present
///   live-region.  Its name is `pending_announcement` when an announcement is
///   queued, `""` otherwise.  Keeping the node permanently in the tree (rather
///   than adding/removing it) ensures AccessKit fires `LiveRegionChanged` on
///   every content change — which NVDA and Orca monitor — rather than the less
///   reliable `NodeAdded` event.
///
/// Focus is `ELEMENT_ID` when `total_list` is non-empty, `ROOT_ID` otherwise.
///
/// macOS builds its own tree ([`build_tree_macos`]) because NSAccessibility
/// honours neither the label-mutation nor the label-only live region above;
/// this stays compiled under `cfg(test)` there so the tests keep pinning the
/// AT-SPI / UIA shape.
#[cfg(any(not(target_os = "macos"), test))]
fn build_tree(renderer: &AppRenderer) -> TreeUpdate {
    let mut nodes: Vec<(NodeId, accesskit::Node)> = Vec::with_capacity(3);

    // Active UI locale (e.g. "nl-BE"); used as the language fallback for any
    // node whose content can't be reliably auto-detected, and directly for the
    // app-generated announcement / root nodes.
    let ui_locale = sicompass_sdk::localize::current_locale();

    // ---- Single focused element node (mirrors C's ELEMENT_ID) --------------
    let (element_label, element_content) = current_element(renderer);
    let mut elem = Node::new(Role::ListItem);
    elem.set_label(Box::<str>::from(element_label.as_str()));
    // Speak each item in its own language: auto-detect from the content, fall
    // back to the UI locale when detection isn't reliable. The screen reader
    // only honours this when its automatic language switching is enabled and a
    // voice for the language is installed.
    let elem_lang = detect_language(&element_content).unwrap_or_else(|| ui_locale.clone());
    elem.set_language(elem_lang);
    nodes.push((ELEMENT_ID, elem));

    // ---- Announcement live-region node (always present) --------------------
    // Announcements (mode changes, errors, tab switches) are app-generated in
    // the active locale and usually too short to detect, so tag them with the
    // UI locale directly.
    let ann_text = renderer.pending_announcement.as_deref().unwrap_or("");
    let mut ann = Node::new(Role::ListItem);
    ann.set_label(Box::<str>::from(ann_text));
    ann.set_language(ui_locale.clone());
    ann.set_live(Live::Polite);
    nodes.push((ANNOUNCEMENT_ID, ann));

    // ---- Root window node --------------------------------------------------
    let mut root_builder = Node::new(Role::Window);
    root_builder.set_label(Box::<str>::from("sicompass"));
    root_builder.set_language(ui_locale.clone());
    root_builder.set_children(vec![ELEMENT_ID, ANNOUNCEMENT_ID]);
    nodes.insert(0, (ROOT_ID, root_builder));

    let focus = if renderer.total_list.is_empty() {
        ROOT_ID
    } else {
        ELEMENT_ID
    };

    TreeUpdate {
        nodes,
        tree: Some(TreeInfo::new(ROOT_ID)),
        tree_id: TreeId::ROOT,
        focus,
    }
}

// ---------------------------------------------------------------------------
// macOS (NSAccessibility) tree
// ---------------------------------------------------------------------------
//
// sicompass exposes no accessibility tree: it tells the screen reader what to
// say rather than publishing a navigable structure.  On AT-SPI and UIA that
// works by mutating one pinned node's *label*, which Orca and NVDA speak.
// NSAccessibility honours neither half of that:
//
// * A label-only change posts `NSAccessibilityTitleChanged`, which VoiceOver
//   does not re-speak, and `focus_moved` only fires when the focused node *id*
//   changes — which it never does here.
// * Live-region announcements are gated on the node's `value`, never its label
//   (`accesskit_macos::event::EventGenerator`), so a label-only live region is
//   silent.
//
// So macOS gets its own tree: everything spoken goes through a single
// live-region `value`, which becomes `NSAccessibilityAnnouncementRequested`.
// `ELEMENT_ID` remains as a focusable anchor — `accesskit_macos` stays in
// `State::Inactive`, generating no events at all, until VoiceOver first queries
// the subclassed view, so there has to be something focusable for it to find.
// The anchor deliberately carries no `value`: that would post an extra
// `NSAccessibilityValueChanged` for the focused node on top of the
// announcement, and VoiceOver may speak both.

/// Build the macOS accessibility tree.
///
/// `spoken` is the text the live region should carry this frame, chosen by
/// [`AccessKitAdapter::next_spoken`].  An empty `spoken` leaves the node's
/// value unset, so `Node::value()` stays `None` and no announcement is posted.
#[cfg(target_os = "macos")]
fn build_tree_macos(renderer: &AppRenderer, spoken: &str, spoken_lang: &str) -> TreeUpdate {
    let ui_locale = sicompass_sdk::localize::current_locale();
    let mut nodes: Vec<(NodeId, accesskit::Node)> = Vec::with_capacity(3);

    // ---- Root window node --------------------------------------------------
    let mut root = Node::new(Role::Window);
    root.set_label(Box::<str>::from("sicompass"));
    root.set_language(ui_locale.clone());
    root.set_children(vec![ELEMENT_ID, ANNOUNCEMENT_ID]);
    nodes.push((ROOT_ID, root));

    // ---- Focusable anchor --------------------------------------------------
    // Label only, so exploring the app with the VoiceOver cursor still reads
    // the current item. Speech itself comes from the live region below.
    let (element_label, element_content) = current_element(renderer);
    let mut elem = Node::new(Role::ListItem);
    elem.set_label(Box::<str>::from(element_label.as_str()));
    elem.set_language(detect_language(&element_content).unwrap_or_else(|| ui_locale.clone()));
    nodes.push((ELEMENT_ID, elem));

    // ---- The one announcement channel --------------------------------------
    let mut ann = Node::new(Role::ListItem);
    ann.set_live(Live::Polite);
    ann.set_language(if spoken_lang.is_empty() {
        ui_locale
    } else {
        spoken_lang.to_owned()
    });
    if !spoken.is_empty() {
        ann.set_value(Box::<str>::from(spoken));
    }
    nodes.push((ANNOUNCEMENT_ID, ann));

    // Focus the anchor unconditionally: `Role::Window` is excluded by
    // `accesskit_macos::node::can_be_focused`, so focusing the root instead
    // would leave `accessibilityFocusedUIElement` nil and give VoiceOver
    // nothing to engage with when the list is empty.
    TreeUpdate {
        nodes,
        tree: Some(TreeInfo::new(ROOT_ID)),
        tree_id: TreeId::ROOT,
        focus: ELEMENT_ID,
    }
}

// ---------------------------------------------------------------------------
// Handler implementations
// ---------------------------------------------------------------------------

/// Provides the initial tree to the platform adapter when an AT connects.
struct ActivationHandlerImpl {
    initial_tree: Option<TreeUpdate>,
    /// Shared flag set to `true` when AT-SPI calls `request_initial_tree`,
    /// signalling the main thread that D-Bus registration is complete.
    #[cfg(target_os = "linux")]
    registered: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl accesskit::ActivationHandler for ActivationHandlerImpl {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        #[cfg(target_os = "linux")]
        self.registered
            .store(true, std::sync::atomic::Ordering::Release);
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
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_state::{AppRenderer, RenderListItem};
    use sicompass_sdk::ffon::IdArray;

    /// Strip the parity sentinel (U+200B) appended by `speak_mode_change` and
    /// `announce_char` to force AccessKit tree diffs on consecutive identical
    /// announcements. Tests use this to assert the logical text without caring
    /// about parity cycle state.
    fn announced_text(r: &AppRenderer) -> Option<String> {
        r.pending_announcement
            .as_deref()
            .map(|s| s.trim_end_matches('\u{200B}').to_string())
    }

    fn make_renderer_with_list(labels: &[&str]) -> AppRenderer {
        let mut r = AppRenderer::new();
        for &label in labels {
            r.total_list.push(RenderListItem {
                id: IdArray::new(),
                label: label.to_string(),
                data: None,
                nav_path: None,
                ext_prefix: None,
            });
        }
        r
    }

    #[test]
    fn build_tree_empty_list() {
        let r = AppRenderer::new();
        let tree = build_tree(&r);
        // root + single element placeholder + announcement node
        assert_eq!(tree.nodes.len(), 3, "root + element + announcement node");
        assert_eq!(tree.nodes[0].0, ROOT_ID);
        assert_eq!(tree.focus, ROOT_ID);
        assert!(tree.tree.is_some());
    }

    #[test]
    fn build_tree_with_items() {
        let r = make_renderer_with_list(&["Files", "Tutorial", "Settings"]);
        let tree = build_tree(&r);
        // always: root + single element + announcement (regardless of list size)
        assert_eq!(tree.nodes.len(), 3);
        assert_eq!(tree.nodes[0].0, ROOT_ID);
        assert_eq!(tree.nodes[1].0, ELEMENT_ID);
        assert_eq!(tree.nodes[2].0, ANNOUNCEMENT_ID);
    }

    #[test]
    fn build_tree_focus_tracks_list_index() {
        let mut r = make_renderer_with_list(&["a", "b", "c"]);
        r.list_index = 2;
        let tree = build_tree(&r);
        // Focus always lands on ELEMENT_ID; the label reflects the selected item.
        assert_eq!(tree.focus, ELEMENT_ID);
        assert_eq!(tree.nodes[1].1.label().as_deref(), Some("c"));
    }

    #[test]
    fn build_tree_focus_clamps_to_last_item() {
        let mut r = make_renderer_with_list(&["only"]);
        r.list_index = 99; // out of bounds
        let tree = build_tree(&r);
        assert_eq!(tree.focus, ELEMENT_ID);
        assert_eq!(tree.nodes[1].1.label().as_deref(), Some("only"));
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

    // --- detect_language ---
    //
    // whatlang only classifies Latin-script text confidently once there's a
    // sentence or two of signal (short fragments get a low score and fall back
    // by design), so these use realistic message-length passages.

    const NL_PASSAGE: &str = "Goedemorgen, dit is een belangrijk bericht over de vergadering van volgende week. Laat ons alstublieft weten of u aanwezig kunt zijn want wij moeten de zaal op tijd reserveren.";
    const EN_PASSAGE: &str = "Good morning, this is an important message about next week's meeting. Please let us know whether you will be able to attend because we need to reserve the room in time.";
    const FR_PASSAGE: &str =
        "Bonjour, ceci est un message important concernant la réunion de la semaine prochaine.";
    const DE_PASSAGE: &str =
        "Guten Morgen, dies ist eine wichtige Nachricht über das Treffen der nächsten Woche.";

    #[test]
    fn detect_language_dutch_passage() {
        assert_eq!(detect_language(NL_PASSAGE).as_deref(), Some("nl"));
    }

    #[test]
    fn detect_language_french_passage() {
        assert_eq!(detect_language(FR_PASSAGE).as_deref(), Some("fr"));
    }

    #[test]
    fn detect_language_german_passage() {
        assert_eq!(detect_language(DE_PASSAGE).as_deref(), Some("de"));
    }

    #[test]
    fn detect_language_english_passage() {
        assert_eq!(detect_language(EN_PASSAGE).as_deref(), Some("en"));
    }

    #[test]
    fn detect_language_short_single_word_is_none() {
        // Too few words to classify — fall back to the UI locale.
        assert_eq!(detect_language("newfile.txt"), None);
    }

    #[test]
    fn detect_language_too_short_is_none() {
        // Under the minimum character threshold.
        assert_eq!(detect_language("a b c"), None);
    }

    #[test]
    fn detect_language_ignores_parity_sentinel() {
        // The trailing U+200B announcement-parity marker must not break detection.
        let with_sentinel = format!("{FR_PASSAGE}\u{200B}");
        assert_eq!(detect_language(&with_sentinel).as_deref(), Some("fr"));
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
    fn label_to_speech_timeline_head_arrow_says_current() {
        assert_eq!(
            label_to_speech("> nav ArrowRight /home/nico"),
            "current nav ArrowRight /home/nico",
        );
    }

    #[test]
    fn label_to_speech_timeline_redo_branch_dot_says_undone() {
        assert_eq!(
            label_to_speech("\u{00B7} nav ArrowRight /home/nico"),
            "undone nav ArrowRight /home/nico",
        );
    }

    #[test]
    fn label_to_speech_timeline_head_minus_arrow_says_minus_current() {
        assert_eq!(
            label_to_speech("- > nav ArrowRight /home/nico"),
            "minus current nav ArrowRight /home/nico",
        );
    }

    #[test]
    fn label_to_speech_timeline_redo_minus_dot_says_minus_undone() {
        assert_eq!(
            label_to_speech("- \u{00B7} nav ArrowRight /home/nico"),
            "minus undone nav ArrowRight /home/nico",
        );
    }

    #[test]
    fn build_tree_translates_list_item_names() {
        // First item (index 0)
        let r = make_renderer_with_list(&["-i newfile.txt", "+l dir"]);
        let tree = build_tree(&r);
        assert_eq!(
            tree.nodes[1].1.label().as_deref(),
            Some("minus i newfile.txt")
        );
        // Second item (index 1)
        let mut r2 = make_renderer_with_list(&["-i newfile.txt", "+l dir"]);
        r2.list_index = 1;
        let tree2 = build_tree(&r2);
        assert_eq!(tree2.nodes[1].1.label().as_deref(), Some("plus l dir"));
    }

    #[test]
    fn build_tree_item_labels_match() {
        // First item (index 0)
        let r = make_renderer_with_list(&["Files", "Tutorial"]);
        let tree = build_tree(&r);
        assert_eq!(tree.nodes[1].1.label().as_deref(), Some("Files"));
        // Second item (index 1)
        let mut r2 = make_renderer_with_list(&["Files", "Tutorial"]);
        r2.list_index = 1;
        let tree2 = build_tree(&r2);
        assert_eq!(tree2.nodes[1].1.label().as_deref(), Some("Tutorial"));
    }

    // --- per-item language tagging ---

    #[test]
    fn build_tree_element_language_detected_from_content() {
        // A Dutch item is tagged "nl" regardless of the UI locale. The "-"
        // FFON prefix is stripped before detection by `speech_content`.
        let label = format!("- {NL_PASSAGE}");
        let r = make_renderer_with_list(&[label.as_str()]);
        let tree = build_tree(&r);
        assert_eq!(tree.nodes[1].1.language(), Some("nl"));
    }

    #[test]
    fn build_tree_element_language_falls_back_to_ui_locale() {
        // Short, undetectable content keeps the active UI locale's voice.
        let r = make_renderer_with_list(&["-i newfile.txt"]);
        let tree = build_tree(&r);
        let ui = sicompass_sdk::localize::current_locale();
        assert_eq!(tree.nodes[1].1.language(), Some(ui.as_str()));
    }

    #[test]
    fn build_tree_announcement_language_is_ui_locale() {
        let mut r = AppRenderer::new();
        r.pending_announcement = Some("search".to_string());
        let tree = build_tree(&r);
        let ui = sicompass_sdk::localize::current_locale();
        let ann = tree
            .nodes
            .iter()
            .find(|(id, _)| *id == ANNOUNCEMENT_ID)
            .unwrap();
        assert_eq!(ann.1.language(), Some(ui.as_str()));
    }

    #[test]
    fn build_tree_root_name_is_sicompass() {
        let r = AppRenderer::new();
        let tree = build_tree(&r);
        assert_eq!(tree.nodes[0].1.label().as_deref(), Some("sicompass"));
    }

    #[test]
    fn build_tree_has_correct_tree() {
        let r = AppRenderer::new();
        let tree = build_tree(&r);
        assert!(tree.tree.is_some());
        assert_eq!(tree.tree.unwrap().root, ROOT_ID);
    }

    // --- announcement live-region ---

    #[test]
    fn build_tree_includes_announcement_node_always() {
        // The announcement node is always present; when pending it carries the text.
        let mut r = AppRenderer::new();
        r.pending_announcement = Some("search".to_string());
        let tree = build_tree(&r);
        let ann = tree.nodes.iter().find(|(id, _)| *id == ANNOUNCEMENT_ID);
        assert!(ann.is_some(), "announcement node should always be present");
        let (_, node) = ann.unwrap();
        assert_eq!(node.label().unwrap(), "search");
        assert_eq!(node.live(), Some(accesskit::Live::Polite));
    }

    #[test]
    fn build_tree_announcement_node_empty_when_no_pending() {
        // The announcement node is still in the tree but with empty name when idle.
        let r = AppRenderer::new();
        let tree = build_tree(&r);
        let ann = tree.nodes.iter().find(|(id, _)| *id == ANNOUNCEMENT_ID);
        assert!(ann.is_some(), "announcement node should always be present");
        let (_, node) = ann.unwrap();
        assert_eq!(node.label().unwrap_or(""), "");
        assert_eq!(node.live(), Some(accesskit::Live::Polite));
    }

    // --- AppRenderer::speak_mode_change ---

    #[test]
    fn speak_mode_change_simple_search_no_context() {
        let mut r = AppRenderer::new();
        r.coordinate = crate::app_state::Coordinate::SimpleSearch;
        r.speak_mode_change(None);
        assert_eq!(announced_text(&r).as_deref(), Some("search mode"));
    }

    #[test]
    fn speak_mode_change_with_context() {
        let mut r = AppRenderer::new();
        r.coordinate = crate::app_state::Coordinate::Insert;
        r.speak_mode_change(Some("filename.txt".to_string()));
        assert_eq!(
            announced_text(&r).as_deref(),
            Some("insert mode - filename.txt")
        );
    }

    #[test]
    fn speak_mode_change_masks_password_context() {
        // A password field's value must never be spoken: the context is masked
        // to one asterisk per character.
        let mut r = AppRenderer::new();
        r.coordinate = crate::app_state::Coordinate::Insert;
        r.input_is_password = true;
        r.speak_mode_change(Some("s3cr3t".to_string()));
        assert_eq!(announced_text(&r).as_deref(), Some("insert mode - ******"));
    }

    #[test]
    fn announce_char_masks_password() {
        // Each typed/cursored character is echoed as `*` while editing a
        // password, not the real character.
        let mut r = AppRenderer::new();
        r.input_is_password = true;
        crate::handlers::announce_char(&mut r, 'k');
        assert_eq!(announced_text(&r).as_deref(), Some("*"));
        // Non-password edits announce the real character.
        r.input_is_password = false;
        crate::handlers::announce_char(&mut r, 'k');
        assert_eq!(announced_text(&r).as_deref(), Some("k"));
    }

    #[test]
    fn speak_mode_change_empty_context_gives_mode_only() {
        let mut r = AppRenderer::new();
        r.coordinate = crate::app_state::Coordinate::Command;
        r.speak_mode_change(Some(String::new()));
        assert_eq!(announced_text(&r).as_deref(), Some("command mode"));
    }

    #[test]
    fn speak_mode_change_controls_palette_reads_controls() {
        // The `c` window-controls palette reuses Coordinate::Command but is
        // distinguished by CommandPhase::Controls; it must announce "controls",
        // not "command".
        let mut r = AppRenderer::new();
        r.coordinate = crate::app_state::Coordinate::Command;
        r.current_command = crate::app_state::CommandPhase::Controls;
        r.speak_mode_change(Some(String::new()));
        assert_eq!(announced_text(&r).as_deref(), Some("controls mode"));
    }

    #[test]
    fn speak_mode_change_names_each_colon_layer() {
        // One name per colon layer, so the screen reader says which one answered.
        // The terminal's shell and claude's session are the same *state* and
        // differ only in whether a further layer is on offer.
        for (coord, want) in [
            (crate::app_state::Coordinate::SessionCommand, "command mode"),
            (
                crate::app_state::Coordinate::SessionFirstCommand,
                "first command mode",
            ),
            (
                crate::app_state::Coordinate::SecondCommand,
                "second command mode",
            ),
        ] {
            let mut r = AppRenderer::new();
            r.coordinate = coord;
            r.speak_mode_change(Some(String::new()));
            assert_eq!(announced_text(&r).as_deref(), Some(want), "{coord:?}");
        }
    }

    #[test]
    fn the_second_layer_is_no_longer_a_homophone_of_insert_mode() {
        // It used to announce through `mode-insert-palette`, whose value was
        // byte-identical to `mode-insert` in every bundle — so by ear the skills
        // palette and real Insert mode were the same mode.
        let mut r = AppRenderer::new();
        r.coordinate = crate::app_state::Coordinate::SecondCommand;
        r.speak_mode_change(None);
        let second = announced_text(&r);

        let mut r = AppRenderer::new();
        r.coordinate = crate::app_state::Coordinate::Insert;
        r.speak_mode_change(None);
        assert_ne!(second, announced_text(&r));
    }

    #[test]
    fn speak_mode_change_general() {
        let mut r = AppRenderer::new(); // default is General
        r.speak_mode_change(None);
        assert_eq!(announced_text(&r).as_deref(), Some("general mode"));
    }

    #[test]
    fn speak_mode_change_insert_with_context() {
        let mut r = AppRenderer::new();
        r.coordinate = crate::app_state::Coordinate::Insert;
        r.speak_mode_change(Some("Documents".to_string()));
        assert_eq!(
            announced_text(&r).as_deref(),
            Some("insert mode - Documents")
        );
    }

    #[test]
    fn speak_mode_change_normal() {
        let mut r = AppRenderer::new();
        r.coordinate = crate::app_state::Coordinate::Normal;
        r.speak_mode_change(None);
        assert_eq!(announced_text(&r).as_deref(), Some("normal mode"));
    }

    #[test]
    fn speak_mode_change_visual() {
        let mut r = AppRenderer::new();
        r.coordinate = crate::app_state::Coordinate::Visual;
        r.speak_mode_change(None);
        assert_eq!(announced_text(&r).as_deref(), Some("visual mode"));
    }

    #[test]
    fn speak_mode_change_extended_search() {
        let mut r = AppRenderer::new();
        r.coordinate = crate::app_state::Coordinate::ExtendedSearch;
        r.speak_mode_change(None);
        assert_eq!(announced_text(&r).as_deref(), Some("extended search mode"));
    }

    #[test]
    fn speak_mode_change_scroll() {
        let mut r = AppRenderer::new();
        r.coordinate = crate::app_state::Coordinate::Scroll;
        r.speak_mode_change(None);
        assert_eq!(announced_text(&r).as_deref(), Some("scroll mode"));
    }

    #[test]
    fn speak_mode_change_dashboard() {
        let mut r = AppRenderer::new();
        r.coordinate = crate::app_state::Coordinate::Dashboard;
        r.speak_mode_change(None);
        assert_eq!(announced_text(&r).as_deref(), Some("dashboard mode"));
    }

    #[test]
    fn speak_mode_change_input_search() {
        let mut r = AppRenderer::new();
        r.coordinate = crate::app_state::Coordinate::InputSearch;
        r.speak_mode_change(None);
        assert_eq!(announced_text(&r).as_deref(), Some("input search mode"));
    }

    // -----------------------------------------------------------------------
    // macOS: the single announcement channel
    // -----------------------------------------------------------------------

    #[cfg(target_os = "macos")]
    mod macos {
        use super::*;

        /// Advance the channel the way `update_if_active` does, and return what
        /// the live region would carry.
        fn publish(channel: &mut SpeechChannel, r: &AppRenderer) -> String {
            *channel = channel.next(r);
            channel.published.value.clone()
        }

        fn announcement_value(tree: &TreeUpdate) -> Option<String> {
            let (id, node) = tree.nodes.last().expect("announcement node");
            assert_eq!(*id, ANNOUNCEMENT_ID);
            node.value().map(|v| v.to_string())
        }

        #[test]
        fn tree_shape_is_root_anchor_announcement() {
            let r = make_renderer_with_list(&["a"]);
            let tree = build_tree_macos(&r, "hello", "en");
            assert_eq!(tree.nodes.len(), 3);
            assert_eq!(tree.nodes[0].0, ROOT_ID);
            assert_eq!(tree.nodes[1].0, ELEMENT_ID);
            assert_eq!(tree.nodes[2].0, ANNOUNCEMENT_ID);
        }

        #[test]
        fn announcement_node_carries_value_and_anchor_does_not() {
            let r = make_renderer_with_list(&["a"]);
            let tree = build_tree_macos(&r, "hello", "en");
            // The value is what accesskit_macos turns into
            // NSAccessibilityAnnouncementRequested; a label alone is silent.
            assert_eq!(announcement_value(&tree).as_deref(), Some("hello"));
            assert_eq!(tree.nodes[2].1.live(), Some(Live::Polite));
            // The anchor stays label-only: a value there would post an extra
            // NSAccessibilityValueChanged for the focused node.
            assert_eq!(tree.nodes[1].1.label(), Some("a"));
            assert!(tree.nodes[1].1.value().is_none());
        }

        #[test]
        fn empty_spoken_leaves_value_unset() {
            let r = make_renderer_with_list(&["a"]);
            let tree = build_tree_macos(&r, "", "");
            // None, not Some(""), so no announcement is posted at startup.
            assert!(announcement_value(&tree).is_none());
        }

        #[test]
        fn focus_is_the_anchor_even_when_list_is_empty() {
            // Role::Window is excluded by can_be_focused, so focusing the root
            // would leave accessibilityFocusedUIElement nil.
            let tree = build_tree_macos(&AppRenderer::new(), "", "");
            assert_eq!(tree.focus, ELEMENT_ID);
            let tree = build_tree_macos(&make_renderer_with_list(&["a"]), "", "");
            assert_eq!(tree.focus, ELEMENT_ID);
        }

        #[test]
        fn announcement_wins_over_element_label() {
            let mut r = make_renderer_with_list(&["item"]);
            r.speak_mode_change(None);
            let expected = r.pending_announcement.clone().unwrap();
            let mut channel = SpeechChannel::default();
            assert_eq!(publish(&mut channel, &r), expected);
        }

        #[test]
        fn changed_element_label_is_published_without_an_announcement() {
            let mut r = make_renderer_with_list(&["first", "second"]);
            let mut channel = SpeechChannel::default();
            assert_eq!(publish(&mut channel, &r), "first");
            r.list_index = 1;
            assert_eq!(publish(&mut channel, &r), "second");
        }

        #[test]
        fn unchanged_state_republishes_verbatim() {
            let r = make_renderer_with_list(&["only"]);
            let mut channel = SpeechChannel::default();
            let first = publish(&mut channel, &r);
            // Identical value => no tree diff => no repeated announcement.
            assert_eq!(publish(&mut channel, &r), first);
            assert_eq!(publish(&mut channel, &r), first);
        }

        #[test]
        fn repeated_identical_announcement_still_changes_the_value() {
            let mut r = AppRenderer::new();
            r.speak_mode_change(None);
            let mut channel = SpeechChannel::default();
            let first = publish(&mut channel, &r);
            // Same text, opposite parity sentinel: the value must differ or
            // accesskit_macos suppresses the second announcement.
            r.speak_mode_change(None);
            let second = publish(&mut channel, &r);
            assert_ne!(first, second);
            assert_eq!(
                first.trim_end_matches(PARITY_SENTINEL),
                second.trim_end_matches(PARITY_SENTINEL)
            );
        }

        #[test]
        fn filter_mode_item_is_not_spoken_twice() {
            // speak_current_element pushes the selected item through
            // pending_announcement *with* a parity sentinel. The next frame
            // must not re-publish the same item as a bare element label.
            let mut r = make_renderer_with_list(&["alpha", "beta"]);
            r.list_index = 1;
            r.speak_current_element();
            let mut channel = SpeechChannel::default();
            let announced = publish(&mut channel, &r);
            assert_eq!(announced.trim_end_matches(PARITY_SENTINEL), "beta");
            assert_eq!(publish(&mut channel, &r), announced, "spoken twice");
        }

        /// An announcement must not be followed by a re-read of the unchanged
        /// element label.  VoiceOver replaces a queued medium-priority
        /// announcement with the next one, so a spurious follow-up one frame
        /// later (~16 ms) swallows whatever was just said.
        #[test]
        fn announcement_is_not_followed_by_an_element_re_read() {
            // Single token: `label_to_speech` treats a leading word as a list
            // prefix and drops it, which would obscure what this test checks.
            let mut r = make_renderer_with_list(&["alpha"]);
            let mut channel = SpeechChannel::default();
            // Settle on the element label first, as normal-mode navigation does.
            assert_eq!(publish(&mut channel, &r), "alpha");

            r.speak_mode_change(None);
            let spoken = publish(&mut channel, &r);
            assert_ne!(spoken, "alpha");
            // The next frames must republish it verbatim, not the list item.
            assert_eq!(publish(&mut channel, &r), spoken);
            assert_eq!(publish(&mut channel, &r), spoken);
        }

        /// `w` (whereami) in General mode: the position must survive, not be
        /// replaced by the list item one frame later.
        #[test]
        fn whereami_survives_the_next_frame() {
            let mut r = make_renderer_with_list(&["alpha", "beta"]);
            let mut channel = SpeechChannel::default();
            publish(&mut channel, &r);
            r.speak_focus_position();
            let position = publish(&mut channel, &r);
            assert_eq!(publish(&mut channel, &r), position, "clobbered by re-read");
        }

        /// Typing in Insert mode echoes one character per keystroke; each echo
        /// must be the last thing published until the next keystroke.
        #[test]
        fn typed_characters_are_echoed_one_per_keystroke() {
            let mut r = make_renderer_with_list(&["alpha"]);
            let mut channel = SpeechChannel::default();
            publish(&mut channel, &r);

            for ch in ['a', 'b', 'c'] {
                crate::handlers::announce_char(&mut r, ch);
                let echoed = publish(&mut channel, &r);
                assert_eq!(
                    echoed.trim_end_matches(PARITY_SENTINEL),
                    ch.to_string(),
                    "keystroke not echoed"
                );
                // Idle frame between keystrokes: nothing new may be published.
                assert_eq!(publish(&mut channel, &r), echoed);
            }
        }

        /// Entering Insert mode speaks the mode plus the existing text, and that
        /// must not be clobbered either.
        #[test]
        fn insert_mode_entry_announces_the_existing_text() {
            let mut r = make_renderer_with_list(&["alpha"]);
            r.coordinate = crate::app_state::Coordinate::Insert;
            let mut channel = SpeechChannel::default();
            publish(&mut channel, &r);

            r.speak_mode_change(Some("existing".to_string()));
            let spoken = publish(&mut channel, &r);
            assert!(spoken.contains("existing"), "existing text not spoken");
            assert_eq!(publish(&mut channel, &r), spoken);
        }

        #[test]
        fn stale_announcement_does_not_resurface_after_navigation() {
            // pending_announcement is never cleared, so it must not win again
            // once a later element change has been published.
            let mut r = make_renderer_with_list(&["alpha", "beta"]);
            r.speak_mode_change(None);
            let mut channel = SpeechChannel::default();
            let mode = publish(&mut channel, &r);
            r.list_index = 1;
            assert_eq!(publish(&mut channel, &r), "beta");
            assert_ne!(publish(&mut channel, &r), mode);
        }
    }
}
