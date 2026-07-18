use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::localize;
use sicompass_sdk::provider::Provider;
use std::path::Path;
use std::sync::OnceLock;

/// Register this crate's translation bundles with the SDK localizer.
/// Idempotent.
pub fn register_translations() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let _ = localize::register_bundle("en-US", include_str!("../locales/en-US.ftl"));
        let _ = localize::register_bundle("nl-BE", include_str!("../locales/nl-BE.ftl"));
        let _ = localize::register_bundle("fr-BE", include_str!("../locales/fr-BE.ftl"));
        let _ = localize::register_bundle("de-BE", include_str!("../locales/de-BE.ftl"));
    });
}

// ---------------------------------------------------------------------------
// Tutorial content tree
// ---------------------------------------------------------------------------

/// A node in the tutorial content tree.
enum Node {
    Leaf(&'static str),
    Branch { key: &'static str, children: &'static [Node] },
}

use Node::{Branch, Leaf};

fn lorem_ipsum() -> &'static str {
    "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut \
     labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco \
     laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in \
     voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat \
     cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum. \
     Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium doloremque \
     laudantium. Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor \
     incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud \
     exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor \
     in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur \
     sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est \
     laborum. Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium \
     doloremque laudantium. \
     Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut \
     labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco \
     laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in \
     voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat \
     cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum. \
     Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium doloremque \
     laudantium. Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor \
     incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud \
     exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor \
     in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur \
     sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est \
     laborum. Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium \
     doloremque laudantium. \
     Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut \
     labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco \
     laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in \
     voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat \
     cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum. \
     Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium doloremque \
     laudantium. Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor \
     incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud \
     exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor \
     in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur \
     sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est \
     laborum. Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium \
     doloremque laudantium. \
     Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor incididunt ut \
     labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud exercitation ullamco \
     laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor in reprehenderit in \
     voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur sint occaecat \
     cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est laborum. \
     Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium doloremque \
     laudantium. Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor \
     incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud \
     exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure dolor \
     in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. Excepteur \
     sint occaecat cupidatat non proident, sunt in culpa qui officia deserunt mollit anim id est \
     laborum. Sed ut perspiciatis unde omnis iste natus error sit voluptatem accusantium \
     doloremque laudantium."
}

// The tutorial is intentionally short and guided. It teaches the core of using
// Sicompass by doing, keeps a single keyboard-shortcut reference, gives one
// short leaf per program, and points to the repo/SDK for plugin development
// rather than inlining a manual. See docs/tutorial-guidelines.md.
//
// Section headings use `tutorial-sec-*` keys, content leaves use semantic keys
// per section. Asset paths (TEXTURE_JPG, FFON_JSON) and the lorem-ipsum block
// are substituted at runtime by `apply_asset_placeholders`.

static SECTIONS: &[Node] = &[
    // 1. Getting Started: a guided do-and-confirm path. Each step asks for one
    //    keypress and relies on the screen-reader announcement as confirmation.
    Branch { key: "tutorial-sec-getting-started",
        children: &[
            Leaf("tutorial-gs-intro"),
            Leaf("tutorial-gs-moved"),
            Branch { key: "tutorial-gs-step", children: &[
                Leaf("tutorial-gs-inside"),
                Leaf("tutorial-gs-back"),
            ]},
            Leaf("tutorial-gs-checkbox-intro"),
            Leaf("tutorial-gs-checkbox"),
            Leaf("tutorial-gs-input-intro"),
            Leaf("tutorial-gs-input"),
            Leaf("tutorial-gs-modes"),
            Leaf("tutorial-gs-done"),
        ],
    },
    // 2. Shortcuts at a glance: the single source for every key, grouped by mode.
    //    Every leaf leads with the key so the screen reader speaks it first.
    Branch { key: "tutorial-sec-shortcuts",
        children: &[
            Leaf("tutorial-sc-intro"),
            Branch { key: "tutorial-sc-general", children: &[
                Leaf("tutorial-sc-gen-updown"),
                Leaf("tutorial-sc-gen-rightleft"),
                Leaf("tutorial-sc-gen-enter"),
                Leaf("tutorial-sc-gen-escape"),
                Leaf("tutorial-sc-gen-page"),
                Leaf("tutorial-sc-gen-f5"),
                Leaf("tutorial-sc-gen-whereami"),
                Leaf("tutorial-sc-gen-meta"),
                Leaf("tutorial-sc-gen-dashboard"),
            ]},
            Branch { key: "tutorial-sc-insert", children: &[
                Leaf("tutorial-sc-in-i"),
                Leaf("tutorial-sc-in-a"),
                Leaf("tutorial-sc-in-enter"),
                Leaf("tutorial-sc-in-backspace"),
            ]},
            Branch { key: "tutorial-sc-command", children: &[
                Leaf("tutorial-sc-cmd-colon"),
                Leaf("tutorial-sc-cmd-tab"),
                Leaf("tutorial-sc-cmd-ctrlf"),
                Leaf("tutorial-sc-cmd-scroll"),
                Leaf("tutorial-sc-cmd-history"),
            ]},
            Branch { key: "tutorial-sc-tabs", children: &[
                Leaf("tutorial-sc-tab-new"),
                Leaf("tutorial-sc-tab-mru"),
                Leaf("tutorial-sc-tab-number"),
                Leaf("tutorial-sc-tab-palette"),
                Leaf("tutorial-sc-tab-controls"),
            ]},
            Branch { key: "tutorial-sc-files", children: &[
                Leaf("tutorial-sc-file-undo"),
                Leaf("tutorial-sc-file-clipboard"),
                Leaf("tutorial-sc-file-save"),
                Leaf("tutorial-sc-file-update"),
            ]},
        ],
    },
    // 3. How it works: the mental model, lean. No key dumps (those live above).
    Branch { key: "tutorial-sec-how-it-works",
        children: &[
            Leaf("tutorial-hiw-tree"),
            Leaf("tutorial-hiw-programs"),
            Leaf("tutorial-hiw-modes"),
            Leaf("tutorial-hiw-editing"),
            Leaf("tutorial-hiw-undo"),
            Leaf("tutorial-hiw-undo-caveats"),
            Leaf("tutorial-hiw-accessibility"),
        ],
    },
    // 4. The programs: one short leaf each.
    Branch { key: "tutorial-sec-programs",
        children: &[
            Leaf("tutorial-prog-intro"),
            Leaf("tutorial-prog-filebrowser"),
            Leaf("tutorial-prog-texteditor"),
            Leaf("tutorial-prog-web"),
            Leaf("tutorial-prog-terminal"),
            Leaf("tutorial-prog-chat"),
            Leaf("tutorial-prog-email"),
            Leaf("tutorial-prog-email-gmail"),
            Leaf("tutorial-prog-salesdemo"),
            Leaf("tutorial-prog-remote"),
            Leaf("tutorial-prog-settings"),
        ],
    },
    // 5. Interactive playground: hands-on element types. Asset placeholders are
    //    filled in at runtime by TutorialProvider.
    Branch { key: "tutorial-sec-playground",
        children: &[
            Leaf("tutorial-play-intro"),
            Leaf("tutorial-play-checkbox"),
            Leaf("tutorial-play-button"),
            Leaf("tutorial-play-input"),
            Leaf("tutorial-play-radio-intro"),
            Branch { key: "tutorial-play-radio", children: &[
                Leaf("tutorial-play-radio-blue"),
                Leaf("tutorial-play-radio-green"),
                Leaf("tutorial-play-radio-red"),
            ]},
            Leaf("tutorial-play-image-intro"),
            Leaf("tutorial-play-image"),
            // A navigable Obj (empty children) whose key carries the <link> tag.
            // Pressing Right lazy-loads the linked file's contents as children
            // (see resolve_link_to_elements). A Str leaf would not be navigable.
            Branch { key: "tutorial-play-link", children: &[] },
            Leaf("tutorial-play-scroll"),
            Leaf("tutorial-play-lorem"),
        ],
    },
    // 6. Settings and config.
    Branch { key: "tutorial-sec-config",
        children: &[
            Leaf("tutorial-cfg-file"),
            Leaf("tutorial-cfg-logs"),
            Leaf("tutorial-cfg-settings"),
            Leaf("tutorial-cfg-saveload"),
            Leaf("tutorial-cfg-updates"),
        ],
    },
    // 7. Extending Sicompass: a pointer to the real docs, not an inline manual.
    Branch { key: "tutorial-sec-extending",
        children: &[
            Leaf("tutorial-ext-build"),
            Leaf("tutorial-ext-docs"),
        ],
    },
];

// ---------------------------------------------------------------------------
// Navigation helper
// ---------------------------------------------------------------------------

/// Returns the children slice for `path_parts` within `nodes`, or `None` if not found.
fn get_children_at_path<'a>(
    nodes: &'a [Node],
    path_parts: &[&str],
) -> Option<&'a [Node]> {
    if path_parts.is_empty() {
        return Some(nodes);
    }
    let (head, rest) = (&path_parts[0], &path_parts[1..]);
    for node in nodes {
        if let Node::Branch { key, children } = node {
            // Path segments are the *stripped* display text of an Obj key (the
            // app pushes `tags::strip_display(&o.key)` on navigation). The `key`
            // here is a Fluent message ID, so we translate it through the same
            // chain `node_to_ffon` uses, then strip display tags. Otherwise the
            // recorded path can never resolve back to the branch.
            let translated = translate_node_string(key);
            if sicompass_sdk::tags::strip_display(&translated) == **head {
                return get_children_at_path(children, rest);
            }
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Convert static tree to FfonElement vec, substituting asset paths
// ---------------------------------------------------------------------------

fn node_to_ffon(node: &Node, texture_jpg: &str, ffon_json: &str) -> FfonElement {
    match node {
        Node::Leaf(s) => {
            // Resolve the translation key first, then run asset-placeholder
            // substitution on the resolved value (sentinels like __TEXTURE_JPG__
            // live in the FTL value).
            let translated = translate_node_string(s);
            FfonElement::Str(apply_asset_placeholders(&translated, texture_jpg, ffon_json))
        }
        Node::Branch { key, children } => {
            let translated = translate_node_string(key);
            let resolved_key = apply_asset_placeholders(&translated, texture_jpg, ffon_json);
            let mut obj = FfonElement::new_obj(resolved_key);
            for child in *children {
                obj.as_obj_mut()
                    .unwrap()
                    .push(node_to_ffon(child, texture_jpg, ffon_json));
            }
            obj
        }
    }
}

/// Resolve a SECTIONS node string through the localizer. Strings that look like
/// Fluent message IDs (alphanumeric + hyphens, no whitespace or other special
/// chars) are routed through `t()`; everything else is returned as-is.
fn translate_node_string(s: &str) -> String {
    register_translations();
    let looks_like_key = !s.is_empty()
        && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if !looks_like_key {
        return s.to_owned();
    }
    let resolved = localize::t(s);
    if resolved == s {
        // Unknown key — leave the literal in place so a missed entry shows up
        // loudly rather than silently.
        s.to_owned()
    } else {
        resolved
    }
}

fn apply_asset_placeholders(s: &str, texture_jpg: &str, ffon_json: &str) -> String {
    // Substring substitution (not whole-value), so a localized leaf can wrap an
    // asset in prefix/suffix text, e.g. "caption: __TEXTURE_JPG__ end". The
    // screen reader then reads the prefix, the image, and the suffix in order,
    // which is why the image example carries surrounding text.
    s.replace("__TEXTURE_JPG__", &format!("<image>{texture_jpg}</image>"))
        .replace("__FFON_JSON__", &format!("<link>{ffon_json}</link>"))
        .replace("__LOREM_IPSUM__", lorem_ipsum())
}

fn nodes_to_ffon(nodes: &[Node], texture_jpg: &str, ffon_json: &str) -> Vec<FfonElement> {
    nodes.iter().map(|n| node_to_ffon(n, texture_jpg, ffon_json)).collect()
}

// ---------------------------------------------------------------------------
// TutorialProvider
// ---------------------------------------------------------------------------

/// The tutorial provider: a short, guided, read-only introduction to Sicompass.
///
/// `assets_dir` should point to the directory containing `texture.jpg` and `ffon.json`.
pub struct TutorialProvider {
    current_path: String,
    texture_jpg: String,
    ffon_json: String,
    /// A one-shot screen-reader announcement, drained by `take_error`. The demo
    /// button in the playground sets this so activating it confirms with a short
    /// spoken line instead of silently re-fetching the list.
    pending_announce: Option<String>,
}

impl TutorialProvider {
    /// Create with explicit asset directory.
    pub fn new(assets_dir: &Path) -> Self {
        let texture_jpg = assets_dir.join("texture.jpg").to_string_lossy().replace('\\', "/");
        let ffon_json = assets_dir.join("ffon.json").to_string_lossy().replace('\\', "/");
        TutorialProvider {
            current_path: "/".to_owned(),
            texture_jpg,
            ffon_json,
            pending_announce: None,
        }
    }

    /// Convenience: create with an empty asset path (for tests that don't need images/links).
    pub fn new_headless() -> Self {
        TutorialProvider {
            current_path: "/".to_owned(),
            texture_jpg: "/missing/texture.jpg".to_owned(),
            ffon_json: "/missing/ffon.json".to_owned(),
            pending_announce: None,
        }
    }

    fn path_parts(&self) -> Vec<&str> {
        if self.current_path == "/" {
            vec![]
        } else {
            self.current_path.split('/').filter(|s| !s.is_empty()).collect()
        }
    }
}

impl Provider for TutorialProvider {
    fn name(&self) -> &str { "tutorial" }

    fn display_name(&self) -> String {
        register_translations();
        localize::t("tutorial-display-name")
    }

    fn fetch(&mut self) -> Vec<FfonElement> {
        let parts = self.path_parts();
        match get_children_at_path(SECTIONS, &parts) {
            Some(nodes) => nodes_to_ffon(nodes, &self.texture_jpg, &self.ffon_json),
            None => vec![],
        }
    }

    fn push_path(&mut self, segment: &str) {
        if self.current_path == "/" {
            self.current_path = format!("/{segment}");
        } else {
            self.current_path.push('/');
            self.current_path.push_str(segment);
        }
    }

    fn pop_path(&mut self) {
        if let Some(slash) = self.current_path.rfind('/') {
            if slash == 0 {
                self.current_path = "/".to_owned();
            } else {
                self.current_path.truncate(slash);
            }
        }
    }

    fn current_path(&self) -> &str { &self.current_path }

    fn set_current_path(&mut self, path: &str) {
        self.current_path = path.to_owned();
    }

    /// The tutorial is read-only, so its only button (the playground demo) has no
    /// real action. Instead of letting the generic press path silently re-fetch
    /// the list, confirm with a short spoken line so activating it has a clear,
    /// expected outcome.
    fn on_button_press(&mut self, _function_name: &str) {
        register_translations();
        self.pending_announce = Some(localize::t("tutorial-play-button-pressed"));
    }

    fn take_error(&mut self) -> Option<String> {
        self.pending_announce.take()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> TutorialProvider {
        TutorialProvider::new_headless()
    }

    fn joined(elems: &[FfonElement]) -> String {
        elems
            .iter()
            .filter_map(|e| e.as_str().map(|s| s.to_owned()))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn section_keys(elems: &[FfonElement]) -> Vec<String> {
        elems
            .iter()
            .filter_map(|e| e.as_obj().map(|o| o.key.to_owned()))
            .collect()
    }

    #[test]
    fn test_demo_button_only_announces_and_changes_nothing() {
        let mut p = provider();
        p.push_path("Interactive playground");
        let before = p.fetch();
        let path_before = p.current_path().to_owned();

        p.on_button_press("demo");

        // It confirms with a spoken line (drained once)...
        let msg = p.take_error().expect("demo button must announce a confirmation");
        assert!(!msg.is_empty());
        assert!(p.take_error().is_none(), "announcement must be one-shot");
        // ...and otherwise leaves the path and rendered list untouched.
        assert_eq!(p.current_path(), path_before, "button press must not navigate");
        assert_eq!(joined(&p.fetch()), joined(&before), "button press must not mutate the list");
    }

    const SECTION_NAMES: [&str; 7] = [
        "Getting Started",
        "Shortcuts at a glance",
        "How it works",
        "The programs",
        "Interactive playground",
        "Settings and config",
        "Extending Sicompass",
    ];

    #[test]
    fn test_root_has_the_seven_sections_getting_started_first() {
        let mut p = provider();
        let elems = p.fetch();
        let keys = section_keys(&elems);
        assert_eq!(keys.len(), 7, "expected exactly 7 top-level sections, got: {keys:?}");
        assert!(keys[0].starts_with("Getting Started"), "Getting Started must be first, got: {:?}", keys[0]);
        // The Getting Started heading carries a "go right to visit" hint, so it
        // is matched by prefix; the others match exactly.
        for name in SECTION_NAMES {
            assert!(keys.iter().any(|k| k.starts_with(name)), "missing section {name}, got: {keys:?}");
        }
    }

    #[test]
    fn test_getting_started_is_interactive() {
        let mut p = provider();
        // Enter via the rendered heading key (it carries a display hint suffix).
        let gs_key = section_keys(&p.fetch())[0].clone();
        p.push_path(&gs_key);
        let elems = p.fetch();
        let text = joined(&elems);
        assert!(text.contains("<checkbox>"), "Getting Started must contain a checkbox to toggle");
        assert!(text.contains("<input>"), "Getting Started must contain an input to edit");
        assert!(
            section_keys(&elems).iter().any(|k| k.starts_with("Step inside")),
            "Getting Started must contain a sub-branch to step into"
        );
    }

    #[test]
    fn test_shortcuts_has_the_five_mode_groups() {
        let mut p = provider();
        p.push_path("Shortcuts at a glance");
        let keys = section_keys(&p.fetch());
        for group in [
            "General mode",
            "Insert and edit mode",
            "Command and search",
            "Tabs and window",
            "Files, undo, and save",
        ] {
            assert!(keys.iter().any(|k| k == group), "missing shortcut group {group}, got: {keys:?}");
        }
    }

    #[test]
    fn test_general_shortcuts_lead_with_the_key() {
        let mut p = provider();
        p.set_current_path("/Shortcuts at a glance/General mode");
        let elems = p.fetch();
        for line in elems.iter().filter_map(|e| e.as_str()) {
            assert!(line.contains(':'), "every shortcut line must lead with a key then ':', got: {line}");
        }
        let text = joined(&elems);
        for token in ["Right", "Left", "Enter", "Escape", "w:", "m:"] {
            assert!(text.contains(token), "general shortcuts must mention {token}, got:\n{text}");
        }
    }

    #[test]
    fn test_tabs_group_lists_t_and_c_palettes_separately() {
        let mut p = provider();
        p.set_current_path("/Shortcuts at a glance/Tabs and window");
        let elems = p.fetch();
        // The tab switcher (t) and window controls (c) each get their own line.
        let has_t = elems.iter().filter_map(|e| e.as_str()).any(|s| s.starts_with("t:"));
        let has_c = elems.iter().filter_map(|e| e.as_str()).any(|s| s.starts_with("c:"));
        assert!(has_t, "tabs group must have a dedicated t: line");
        assert!(has_c, "tabs group must have a dedicated c: window-controls line");
    }

    #[test]
    fn test_how_it_works_covers_the_model_and_undo_caveats() {
        let mut p = provider();
        p.push_path("How it works");
        let text = joined(&p.fetch()).to_lowercase();
        assert!(text.contains("tree of lists"), "must explain the tree-of-lists model");
        assert!(text.contains("undo"), "must mention undo");
        assert!(
            text.contains("4 mib") || text.contains("redact"),
            "must mention an irreversibility caveat"
        );
    }

    #[test]
    fn test_programs_lists_each_program_once() {
        let mut p = provider();
        p.push_path("The programs");
        let text = joined(&p.fetch());
        for token in ["File browser", "Text editor", "Web browser", "Terminal", "Chat", "Email", "Settings"] {
            assert!(text.contains(token), "programs section must mention {token}, got:\n{text}");
        }
    }

    #[test]
    fn test_programs_documents_gmail_setup() {
        let mut p = provider();
        p.push_path("The programs");
        let text = joined(&p.fetch());
        // The Gmail setup leaf must call out the mail scope (the actual fix) and
        // the recovery colon commands.
        assert!(text.contains("https://mail.google.com/"), "must name the mail scope, got:\n{text}");
        assert!(text.contains(":refresh"), "must mention the :refresh colon command");
        assert!(text.contains(":logout"), "must mention re-authorizing via :logout");
    }

    #[test]
    fn test_playground_has_every_element_type() {
        let mut p = provider();
        p.push_path("Interactive playground");
        let elems = p.fetch();
        let text = joined(&elems);
        assert!(text.contains("<checkbox checked>"), "must contain a checked checkbox");
        assert!(text.contains("<button>"), "must contain a button");
        assert!(text.contains("<input>"), "must contain an input");
        assert!(text.contains("<image>"), "must contain an inline image");
        assert!(text.contains("Lorem ipsum"), "must contain the long passage for scroll practice");
        // The link must be a navigable Obj (key carries <link>), not a Str leaf,
        // otherwise pressing Right cannot lazy-load its contents.
        assert!(
            section_keys(&elems).iter().any(|k| k.contains("<link>")),
            "must contain a navigable link Obj"
        );
        assert!(
            section_keys(&elems).iter().any(|k| k.starts_with("<radio>")),
            "must contain a radio group"
        );
        // The image must carry text before and after it, so a screen reader
        // reads prefix, image, suffix rather than an unlabelled gap.
        let image_line = elems
            .iter()
            .filter_map(|e| e.as_str())
            .find(|s| s.contains("<image>"))
            .expect("an image leaf");
        let before = &image_line[..image_line.find("<image>").unwrap()];
        let after = &image_line[image_line.find("</image>").unwrap() + "</image>".len()..];
        assert!(!before.trim().is_empty(), "image must have prefix text, got: {image_line}");
        assert!(!after.trim().is_empty(), "image must have suffix text, got: {image_line}");
    }

    #[test]
    fn test_config_section_documents_logs() {
        let mut p = provider();
        p.push_path("Settings and config");
        let text = joined(&p.fetch());
        assert!(text.contains("sicompass.log"), "config section must document the log file");
        assert!(text.contains("RUST_LOG"), "config section must mention RUST_LOG for stderr logging");
        // All three platform log locations must be documented.
        assert!(text.contains(".local/state"), "must give the Linux log path");
        assert!(text.contains("Library/Logs"), "must give the macOS log path");
        assert!(text.contains("LOCALAPPDATA"), "must give the Windows log path");
    }

    #[test]
    fn test_extending_points_to_docs_not_inline_abi() {
        let mut p = provider();
        p.push_path("Extending Sicompass");
        let text = joined(&p.fetch());
        assert!(text.contains("lib/lib_sales_demo/"), "must point to the reference program");
        assert!(
            text.contains("Rust") && text.contains("TypeScript") && text.contains("ProviderOps"),
            "must mention Rust (the standard), TypeScript, and C/ProviderOps plugin kinds"
        );
    }

    // Path mechanics

    #[test]
    fn test_unknown_path_returns_empty() {
        let mut p = provider();
        p.push_path("NonExistentSection");
        assert!(p.fetch().is_empty());
    }

    #[test]
    fn test_nested_path_round_trips() {
        // A translated, display-stripped branch key must resolve back to its
        // children (the path the app records when you navigate in).
        let mut p = provider();
        p.set_current_path("/Shortcuts at a glance/General mode");
        let elems = p.fetch();
        assert!(!elems.is_empty());
        assert!(elems.iter().all(|e| e.is_str()), "General mode holds only shortcut leaves");
    }

    #[test]
    fn test_pop_path_returns_to_root() {
        let mut p = provider();
        p.push_path("The programs");
        p.pop_path();
        assert_eq!(p.current_path(), "/");
        assert_eq!(section_keys(&p.fetch()).len(), 7);
    }
}

// ---------------------------------------------------------------------------
// SDK registration
// ---------------------------------------------------------------------------

/// Register the tutorial with the SDK factory and manifest registries.
pub fn register() {
    sicompass_sdk::register_provider_factory("tutorial", || {
        let assets = sicompass_sdk::platform::resolve_repo_asset("lib/lib_tutorial/assets");
        Box::new(TutorialProvider::new(&assets))
    });
    sicompass_sdk::register_builtin_manifest(
        sicompass_sdk::BuiltinManifest::new("tutorial", "tutorial").enable_by_default(),
    );
}
