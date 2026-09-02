//! Sales demo provider — a Rust port of the former `sales_demo.ts`.
//!
//! Walks a product-configuration tree (`assets/equipment1.json`) and renders it as
//! FFON: mandatory entries appear directly, optional ones are offered under an
//! "Add element:" section as `<button>` tags.
//!
//! # Why this is no longer a script
//!
//! It used to be TypeScript run through `bun`, wrapped in a `ScriptProvider`. That
//! meant shipping and spawning a general-purpose interpreter, which cannot pass an
//! App Store sandbox — the same reason third-party plugins moved to WebAssembly.
//! `bun` was never bundled in release archives either, so the demo only worked in a
//! dev checkout. Follows `lib_tutorial` and `lib_remote`, both already ported the
//! same way.
//!
//! It is a *built-in*, not a plugin, so it stays native and compiled-in: built-ins
//! are the trusted computing base. The sandbox is for third-party code.

use serde::Deserialize;
use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};
use sicompass_sdk::{FfonElement, FfonObject, Provider};

/// The product tree, embedded at compile time.
///
/// The script read this from disk next to itself, which meant a dev-checkout path
/// and a runtime failure mode. It is fixed data, so it belongs in the binary.
const EQUIPMENT_JSON: &str = include_str!("../assets/equipment1.json");

/// The diagram shown by the `d` key, compiled in next to `equipment1.json`.
///
/// It used to be a loose file under the repository's top-level `assets/` tree,
/// because the host loads and scales images itself and so needed a *path*. It still
/// gets a name rather than bytes — but now an `asset:` URI, which the host resolves
/// through the SDK registry. Nothing to ship, nothing to list in five packaging
/// manifests, nothing to go missing.
const DASHBOARD_IMAGE: &[u8] =
    include_bytes!("../assets/115-Draw-through-Air-Handling-Unit-Diagram-1.webp");

/// What `dashboard_image_path` hands the host. Hyphenated, not the `"sales demo"`
/// the provider itself is registered as: this string ends up inside a URI, where a
/// space is bad hygiene for no gain. A built-in picks its own asset namespace, so
/// the two need not match. (A WASM plugin does not get that freedom — the host keys
/// its namespace on the manifest name.)
const DASHBOARD_IMAGE_URI: &str =
    "asset:sales-demo/115-Draw-through-Air-Handling-Unit-Diagram-1.webp";

/// The cardinality vocabulary. An entry's first element is one of these; anything
/// else means the entry is not a configuration node and is skipped.
const CARDINALITIES: [&str; 3] = ["one mand", "one opt", "many opt"];

fn is_cardinality(s: &str) -> bool {
    CARDINALITIES.contains(&s)
}

// ---------------------------------------------------------------------------
// Order-preserving JSON
// ---------------------------------------------------------------------------

/// A JSON value that remembers the order of object keys.
///
/// `serde_json::Value` does not: its `Map` is a `BTreeMap` unless the
/// `preserve_order` feature is on, so keys come back alphabetised. This tree is
/// ordered deliberately by whoever wrote `equipment1.json` — version, then
/// settings, then the equipment itself — and a salesperson walks it top to bottom.
/// Sorting it would scramble the demo.
///
/// Kept local rather than enabling `preserve_order` workspace-wide, which would
/// also change the key order of every `settings.json` the app writes.
#[derive(Debug, Clone, PartialEq)]
enum Node {
    Null,
    /// A JSON string.
    Str(String),
    /// A number or boolean. The script stringified these (`String(x)`), and only
    /// ever displayed them, so the distinction is not worth carrying.
    Scalar(String),
    Array(Vec<Node>),
    Object(Vec<(String, Node)>),
}

impl Node {
    fn as_str(&self) -> Option<&str> {
        match self {
            Node::Str(s) | Node::Scalar(s) => Some(s),
            _ => None,
        }
    }

    fn as_array(&self) -> Option<&[Node]> {
        match self {
            Node::Array(v) => Some(v),
            _ => None,
        }
    }

    fn as_object(&self) -> Option<&[(String, Node)]> {
        match self {
            Node::Object(v) => Some(v),
            _ => None,
        }
    }

    fn get(&self, key: &str) -> Option<&Node> {
        self.as_object()?
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }

    /// Render as the display string the script's `String(x)` produced.
    fn to_display(&self) -> String {
        match self {
            Node::Str(s) | Node::Scalar(s) => s.clone(),
            Node::Null => "null".to_owned(),
            // Arrays and objects never reach a leaf slot in this data; the script
            // would have produced "[object Object]". An empty string is less
            // confusing on screen than that.
            _ => String::new(),
        }
    }
}

impl<'de> Deserialize<'de> for Node {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct NodeVisitor;

        impl<'de> Visitor<'de> for NodeVisitor {
            type Value = Node;

            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.write_str("any JSON value")
            }

            fn visit_unit<E: de::Error>(self) -> Result<Node, E> {
                Ok(Node::Null)
            }
            fn visit_none<E: de::Error>(self) -> Result<Node, E> {
                Ok(Node::Null)
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Node, E> {
                Ok(Node::Scalar(v.to_string()))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Node, E> {
                Ok(Node::Scalar(v.to_string()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Node, E> {
                Ok(Node::Scalar(v.to_string()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Node, E> {
                Ok(Node::Scalar(v.to_string()))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Node, E> {
                Ok(Node::Str(v.to_owned()))
            }

            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Node, A::Error> {
                let mut out = Vec::new();
                while let Some(v) = seq.next_element()? {
                    out.push(v);
                }
                Ok(Node::Array(out))
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Node, A::Error> {
                // A `Vec` rather than a map type: insertion order is the point.
                let mut out = Vec::new();
                while let Some((k, v)) = map.next_entry::<String, Node>()? {
                    out.push((k, v));
                }
                Ok(Node::Object(out))
            }
        }

        d.deserialize_any(NodeVisitor)
    }
}

// ---------------------------------------------------------------------------
// Tree walking
// ---------------------------------------------------------------------------

/// Follow `parts` into the raw tree.
///
/// Returns an object node to keep descending, or a string array for a leaf whose
/// content is a list of choices (paint colours, say). `None` means the path does
/// not exist, which the caller renders as an empty view.
fn raw_at_path<'a>(raw: &'a Node, parts: &[&str]) -> Option<&'a Node> {
    let Some((head, rest)) = parts.split_first() else {
        return Some(raw);
    };

    let entry = raw.get(head)?.as_array()?;
    // A node is only navigable if it opens with a cardinality marker.
    if !entry.first()?.as_str().is_some_and(is_cardinality) {
        return None;
    }

    let content = entry.get(1)?;
    if matches!(content, Node::Null) {
        return None;
    }
    if content.as_array().is_some() {
        // A leaf list is only valid as the final segment.
        return if rest.is_empty() { Some(content) } else { None };
    }
    // Only an object node can be descended into. A string leaf (`version`, say)
    // renders its value but has nothing below it.
    content.as_object()?;
    raw_at_path(content, rest)
}

/// Build the visible children of an object node.
///
/// Mandatory entries render directly; optional ones collect into an "Add element:"
/// section as `<button>` tags, which is how the app offers them for insertion. The
/// `one-opt:` prefix marks a choice that may be added at most once.
fn build_display_children(obj: &Node) -> Vec<FfonElement> {
    let mut result = Vec::new();
    let mut add_items = Vec::new();

    let Some(entries) = obj.as_object() else {
        return result;
    };

    for (key, value) in entries {
        let Some(raw) = value.as_array() else {
            continue;
        };
        let Some(card) = raw.first().and_then(Node::as_str) else {
            continue;
        };
        if !is_cardinality(card) {
            continue;
        }

        if card.contains("opt") {
            let prefix = if card == "one opt" { "one-opt:" } else { "" };
            add_items.push(FfonElement::new_str(format!(
                "<button>{prefix}{key}</button>{key}"
            )));
        } else {
            result.push(build_item(key, raw));
        }
    }

    if !add_items.is_empty() {
        let mut section = FfonObject::new("Add element:");
        for item in add_items {
            section.push(item);
        }
        result.push(FfonElement::Obj(section));
    }

    result
}

/// Render one entry.
///
/// Two shapes occur in the data: `["cardinality", content]`, and a list of
/// `[cardinality, value]` pairs (used by `version`). Anything that fits neither
/// degrades to a bare label rather than vanishing.
fn build_item(key: &str, raw: &[Node]) -> FfonElement {
    if raw.is_empty() {
        return FfonElement::new_str(key);
    }

    // Shape A: ["cardinality", content?]
    if raw[0].as_str().is_some_and(is_cardinality) {
        let Some(content) = raw.get(1) else {
            return FfonElement::new_str(key);
        };
        return match content {
            Node::Null => FfonElement::new_str(key),
            Node::Array(items) => {
                let mut obj = FfonObject::new(key);
                for item in items {
                    obj.push(FfonElement::new_str(item.to_display()));
                }
                FfonElement::Obj(obj)
            }
            Node::Str(s) | Node::Scalar(s) => {
                let mut obj = FfonObject::new(key);
                obj.push(FfonElement::new_str(s.clone()));
                FfonElement::Obj(obj)
            }
            Node::Object(_) => {
                let mut obj = FfonObject::new(key);
                for child in build_display_children(content) {
                    obj.push(child);
                }
                FfonElement::Obj(obj)
            }
        };
    }

    // Shape B: [[cardinality, value], ...]
    if raw[0].as_array().is_some() {
        let mut obj = FfonObject::new(key);
        let mut any = false;
        for entry in raw {
            let Some(pair) = entry.as_array() else {
                continue;
            };
            let Some(card) = pair.first().and_then(Node::as_str) else {
                continue;
            };
            if !is_cardinality(card) {
                continue;
            }
            if let Some(Node::Str(value)) = pair.get(1) {
                obj.push(FfonElement::new_str(value.clone()));
                any = true;
            }
        }
        return if any {
            FfonElement::Obj(obj)
        } else {
            FfonElement::new_str(key)
        };
    }

    FfonElement::new_str(key)
}

// ---------------------------------------------------------------------------
// Provider
// ---------------------------------------------------------------------------

/// Renders the product tree, one level at a time.
pub struct SalesDemoProvider {
    root: Node,
    path: String,
}

impl Default for SalesDemoProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl SalesDemoProvider {
    pub fn new() -> Self {
        // A parse failure means the embedded asset is malformed, which is a build
        // problem rather than a runtime one; an empty tree renders as an empty view
        // instead of taking the app down.
        let root = serde_json::from_str::<Node>(EQUIPMENT_JSON).unwrap_or_else(|e| {
            eprintln!("sales demo: equipment1.json is malformed: {e}");
            Node::Object(Vec::new())
        });
        SalesDemoProvider {
            root,
            path: "/".to_owned(),
        }
    }

    fn path_parts(&self) -> Vec<&str> {
        self.path.split('/').filter(|s| !s.is_empty()).collect()
    }

    fn at_root(&self) -> bool {
        self.path_parts().is_empty()
    }
}

#[async_trait::async_trait]
impl Provider for SalesDemoProvider {
    fn name(&self) -> &str {
        "sales demo"
    }

    fn display_name(&self) -> String {
        "sales demo".to_owned()
    }

    fn version(&self) -> Option<&str> {
        Some(env!("CARGO_PKG_VERSION"))
    }

    fn fetch(&mut self) -> Vec<FfonElement> {
        let parts = self.path_parts();
        match raw_at_path(&self.root, &parts) {
            // A leaf list of choices renders as its options directly.
            Some(node) if node.as_array().is_some() => node
                .as_array()
                .unwrap_or_default()
                .iter()
                .map(|n| FfonElement::new_str(n.to_display()))
                .collect(),
            Some(node) => build_display_children(node),
            None => Vec::new(),
        }
    }

    /// Structural editing is available exactly where the "Add element:"
    /// section is, which is the level whose entries the user configures.
    ///
    /// The app used to work this out by looking for an `Obj` keyed
    /// "Add element:" among the cursor's siblings. That reads a *shape* to
    /// infer an *intent*, and any provider that happened to render a section
    /// with that key inherited a keymap it never asked for. Answering here says
    /// the same thing directly, and from the side that built the section.
    fn supports_structural_edit(&self) -> bool {
        let parts = self.path_parts();
        raw_at_path(&self.root, &parts)
            .filter(|node| node.as_array().is_none())
            .map(|node| {
                build_display_children(node)
                    .iter()
                    .any(|e| matches!(e, FfonElement::Obj(o) if o.key == "Add element:"))
            })
            .unwrap_or(false)
    }

    fn current_path(&self) -> &str {
        &self.path
    }

    fn set_current_path(&mut self, path: &str) {
        self.path = path.to_owned();
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
        match self.path.rfind('/') {
            Some(0) | None => self.path = "/".to_owned(),
            Some(slash) => self.path.truncate(slash),
        }
    }

    fn dashboard_image_path(&self) -> Option<&str> {
        // Only at the root, matching the script: it emitted `dashboardImage` in the
        // root payload only, and `ScriptProvider` cleared the field on every
        // subsequent fetch.
        if self.at_root() {
            Some(DASHBOARD_IMAGE_URI)
        } else {
            None
        }
    }

    /// Build the element an "Add element:" button inserts.
    ///
    /// This is what the demo is *for*: the buttons under "Add element:" add optional
    /// parts to a product configuration, and without this they do nothing. The
    /// button carries `one-opt:` when the entry may be added at most once, and the
    /// inserted node is tagged accordingly so the app knows which rule applies.
    ///
    /// An entry that is itself an input field is inserted as a bare tagged string;
    /// anything else becomes a section pre-filled with the children it has in the
    /// tree, so adding "heating" brings its options along.
    fn create_element(&mut self, element_key: &str) -> Option<FfonElement> {
        let (key, tagged) = match element_key.strip_prefix("one-opt:") {
            Some(key) => (key, sicompass_sdk::tags::format_one_opt(key)),
            None => (
                element_key,
                sicompass_sdk::tags::format_many_opt(element_key),
            ),
        };

        if sicompass_sdk::tags::has_input(key) {
            return Some(FfonElement::new_str(tagged));
        }

        let mut obj = FfonObject::new(tagged);
        // Populate from the node's own position in the tree, one level below where
        // the user currently is.
        let mut parts = self.path_parts();
        parts.push(key);
        if let Some(node) = raw_at_path(&self.root, &parts) {
            let children = match node.as_array() {
                Some(items) => items
                    .iter()
                    .map(|n| FfonElement::new_str(n.to_display()))
                    .collect(),
                None => build_display_children(node),
            };
            for child in children {
                obj.push(child);
            }
        }
        Some(FfonElement::Obj(obj))
    }

    fn supports_config_files(&self) -> bool {
        true
    }
}

// ---------------------------------------------------------------------------
// SDK registration
// ---------------------------------------------------------------------------

/// Register the sales demo with the SDK factory and manifest registries.
pub fn register() {
    // The diagram, published before the factory so a provider built by the next line
    // already resolves it. Overwrites, so calling this twice is harmless.
    sicompass_sdk::assets::register_bytes(
        "sales-demo",
        "115-Draw-through-Air-Handling-Unit-Diagram-1.webp",
        DASHBOARD_IMAGE,
    );

    sicompass_sdk::register_provider_factory("sales demo", || Box::new(SalesDemoProvider::new()));
    sicompass_sdk::register_builtin_manifest(
        sicompass_sdk::BuiltinManifest::new("sales demo", "sales demo").with_settings(vec![
            sicompass_sdk::SettingDecl::text(
                "sales demo",
                "save folder (product configuration)",
                "saveFolder",
                "Downloads",
            ),
        ]),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider() -> SalesDemoProvider {
        SalesDemoProvider::new()
    }

    #[test]
    fn register_does_not_panic() {
        // Double-registration is safe (the registry is append-only).
        super::register();
    }

    // --- the embedded asset ---

    #[test]
    fn the_embedded_equipment_tree_parses() {
        // `include_str!` guarantees the file exists at build time; this guarantees it
        // is valid JSON, which the old script only found out at runtime.
        let root: Node = serde_json::from_str(EQUIPMENT_JSON).expect("valid JSON");
        assert!(root.as_object().is_some_and(|o| !o.is_empty()));
    }

    #[test]
    fn object_key_order_is_preserved() {
        // The whole reason for the local `Node` type. `serde_json::Value` sorts keys,
        // which would reorder a product tree a salesperson reads top to bottom.
        let node: Node =
            serde_json::from_str(r#"{"zebra":1,"apple":2,"middle":3}"#).expect("parses");
        let keys: Vec<&str> = node
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, _)| k.as_str())
            .collect();
        assert_eq!(
            keys,
            vec!["zebra", "apple", "middle"],
            "keys were reordered"
        );
    }

    #[test]
    fn the_real_tree_keeps_its_authored_order() {
        let root: Node = serde_json::from_str(EQUIPMENT_JSON).unwrap();
        let keys: Vec<&str> = root
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, _)| k.as_str())
            .collect();
        // `version` is written first in the asset; alphabetical order would not put
        // it there, so this catches a silent switch back to `serde_json::Value`.
        assert_eq!(keys.first(), Some(&"version"), "got {keys:?}");
        let mut sorted = keys.clone();
        sorted.sort_unstable();
        assert_ne!(
            keys, sorted,
            "the asset happens to be sorted; pick a better assertion"
        );
    }

    // --- fetch, ported from the bun tests ---

    #[test]
    fn the_root_returns_children() {
        let mut p = provider();
        assert!(!p.fetch().is_empty());
    }

    #[test]
    fn the_root_offers_a_dashboard_image() {
        let p = provider();
        let path = p
            .dashboard_image_path()
            .expect("root should offer an image");
        assert!(path.ends_with(".webp"), "got {path}");
    }

    #[test]
    fn the_diagram_resolves_and_is_still_a_webp() {
        // The host decodes these bytes itself, so being registered is not enough:
        // they also have to still be an image the `image` crate can sniff.
        register();
        let bytes = sicompass_sdk::assets::resolve(DASHBOARD_IMAGE_URI)
            .expect("the diagram must resolve through the registry");
        assert_eq!(&bytes[..4], b"RIFF", "not a RIFF container any more");
        assert_eq!(&bytes[8..12], b"WEBP", "not a WebP any more");
    }

    #[test]
    fn the_uri_constant_matches_the_key_register_publishes_under() {
        // A literal typo here would compile fine and show up only as a dashboard
        // that draws nothing.
        assert_eq!(
            DASHBOARD_IMAGE_URI,
            sicompass_sdk::assets::uri(
                "sales-demo",
                "115-Draw-through-Air-Handling-Unit-Diagram-1.webp"
            )
        );
    }

    #[test]
    fn the_dashboard_image_path_names_the_diagram() {
        let p = provider();
        assert_eq!(p.dashboard_image_path(), Some(DASHBOARD_IMAGE_URI));
    }

    #[test]
    fn the_dashboard_image_is_only_offered_at_the_root() {
        // Matches the script: it emitted `dashboardImage` in the root payload only.
        let mut p = provider();
        p.push_path("settings");
        assert!(p.dashboard_image_path().is_none());
    }

    #[test]
    fn the_root_shows_mandatory_items_directly() {
        let mut p = provider();
        let non_add: Vec<_> = p
            .fetch()
            .into_iter()
            .filter(|e| e.as_obj().is_none_or(|o| o.key != "Add element:"))
            .collect();
        assert!(
            !non_add.is_empty(),
            "expected mandatory entries outside Add element:"
        );
    }

    #[test]
    fn optional_items_are_offered_as_buttons_under_add_element() {
        let mut p = provider();
        let elems = p.fetch();
        let Some(section) = elems
            .iter()
            .find_map(|e| e.as_obj().filter(|o| o.key == "Add element:"))
        else {
            // The asset may legitimately have no optional entries at the root.
            return;
        };

        assert!(!section.children.is_empty());
        for child in &section.children {
            let s = child.as_str().expect("button entries are plain strings");
            assert!(s.contains("<button>"), "got {s}");
            assert!(s.contains("</button>"), "got {s}");
        }
    }

    // --- create_element: what the "Add element:" buttons actually do ---

    #[test]
    fn adding_an_optional_element_produces_a_tagged_section_with_its_children() {
        // This is the point of the demo: the buttons add optional parts to a product
        // configuration. Without `create_element` they do nothing at all.
        let mut p = provider();
        p.root = serde_json::from_str(
            r#"{"heating":["many opt",{"power":["one mand",["3kW","6kW"]]}]}"#,
        )
        .unwrap();

        let elem = p
            .create_element("heating")
            .expect("a button must produce an element");
        let obj = elem
            .as_obj()
            .expect("a node with children, not a bare label");

        // Tagged so the app knows the cardinality rule that applies to it.
        assert!(obj.key.contains("heating"), "got {:?}", obj.key);
        assert_ne!(obj.key, "heating", "the key should carry a cardinality tag");

        // Pre-filled from the tree, so adding "heating" brings its options along.
        assert!(
            !obj.children.is_empty(),
            "the added node should carry its children"
        );
    }

    #[test]
    fn the_one_opt_prefix_selects_a_different_tag_than_many_opt() {
        // The button label carries `one-opt:` when an entry may be added at most
        // once; the inserted node has to reflect that or the app applies the wrong
        // rule.
        let mut p = provider();
        p.root = serde_json::from_str(r#"{"roof":["one opt",{"a":["one mand","x"]}]}"#).unwrap();

        let one = p.create_element("one-opt:roof").unwrap();
        let many = p.create_element("roof").unwrap();

        let one_key = &one.as_obj().unwrap().key;
        let many_key = &many.as_obj().unwrap().key;
        assert_ne!(
            one_key, many_key,
            "one-opt and many-opt must tag differently"
        );
        assert!(one_key.contains("roof") && many_key.contains("roof"));
    }

    #[test]
    fn adding_an_element_that_is_an_input_field_yields_a_bare_string() {
        // An entry that is itself an input has nothing to expand into.
        let mut p = provider();
        let elem = p
            .create_element("<input>serial</input>")
            .expect("still produces an element");
        assert!(
            elem.as_str().is_some(),
            "an input entry should be a plain string"
        );
    }

    #[test]
    fn adding_an_unknown_element_still_produces_a_node() {
        // The user pressed a button; producing nothing would look like a dead key.
        let mut p = provider();
        let elem = p
            .create_element("nonexistent")
            .expect("should not return None");
        assert!(elem.as_obj().is_some_and(|o| o.children.is_empty()));
    }

    #[test]
    fn adding_an_element_resolves_relative_to_the_current_path() {
        // The buttons live at whatever depth the user is browsing, so the lookup has
        // to start there rather than at the root.
        let mut p = provider();
        p.root = serde_json::from_str(
            r#"{"unit":["one mand",{"heating":["many opt",{"power":["one mand",["3kW"]]}]}]}"#,
        )
        .unwrap();
        p.set_current_path("/unit");

        let obj = p.create_element("heating").unwrap();
        assert!(
            !obj.as_obj().unwrap().children.is_empty(),
            "should have found `heating` under /unit"
        );
    }

    #[test]
    fn an_invalid_path_returns_nothing() {
        let mut p = provider();
        p.set_current_path("/definitely/not/a/real/path");
        assert!(p.fetch().is_empty());
    }

    #[test]
    fn navigating_into_a_top_level_entry_returns_its_children() {
        let mut p = provider();

        // Rendering as an Obj is not the same as being navigable: a leaf whose
        // content is a plain string renders as a section so its value is visible,
        // but there is nothing below it. Pick something the tree can actually
        // descend into.
        let root = p.root.clone();
        let Some(key) = root
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, _)| k.as_str())
            .find(|k| raw_at_path(&root, &[k]).is_some())
        else {
            panic!("expected at least one navigable entry at the root");
        };
        let key = key.to_owned();

        p.push_path(&key);
        assert_eq!(p.current_path(), format!("/{key}"));
        assert!(
            !p.fetch().is_empty(),
            "navigating into {key:?} produced nothing"
        );
    }

    #[test]
    fn a_string_leaf_shows_its_value_but_is_not_navigable() {
        // `version` is `["one mand", "accoflo 0.1"]`. It renders as a section so the
        // value appears on screen, yet descending into it yields nothing — the
        // script behaved the same way (`typeof content !== "object"` stops the
        // walk). Easy to "fix" into a behaviour change, so pin it.
        let root: Node = serde_json::from_str(r#"{"version":["one mand","accoflo 0.1"]}"#).unwrap();

        let rendered = build_display_children(&root);
        let obj = rendered[0].as_obj().expect("renders as a section");
        assert_eq!(obj.key, "version");
        assert_eq!(obj.children[0].as_str(), Some("accoflo 0.1"));

        assert!(
            raw_at_path(&root, &["version"]).is_none(),
            "must not be navigable"
        );
    }

    #[test]
    fn navigation_pushes_and_pops() {
        let mut p = provider();
        p.push_path("settings");
        assert_eq!(p.current_path(), "/settings");
        p.push_path("language");
        assert_eq!(p.current_path(), "/settings/language");
        p.pop_path();
        assert_eq!(p.current_path(), "/settings");
        p.pop_path();
        assert_eq!(p.current_path(), "/");
        p.pop_path();
        assert_eq!(
            p.current_path(),
            "/",
            "popping at the root should stay there"
        );
    }

    #[test]
    fn config_files_are_supported() {
        // The script wrapper set this; the demo saves a product configuration.
        assert!(provider().supports_config_files());
    }

    // --- tree-walking edge cases ---

    #[test]
    fn a_leaf_list_renders_as_its_options() {
        let root: Node =
            serde_json::from_str(r#"{"paint":["one mand",["black","grey"]]}"#).unwrap();
        let node = raw_at_path(&root, &["paint"]).expect("path exists");
        let opts: Vec<String> = node
            .as_array()
            .unwrap()
            .iter()
            .map(Node::to_display)
            .collect();
        assert_eq!(opts, vec!["black".to_owned(), "grey".to_owned()]);
    }

    #[test]
    fn a_leaf_list_is_not_navigable_any_deeper() {
        let root: Node = serde_json::from_str(r#"{"paint":["one mand",["black"]]}"#).unwrap();
        assert!(raw_at_path(&root, &["paint", "black"]).is_none());
    }

    #[test]
    fn an_entry_without_a_cardinality_marker_is_not_navigable() {
        // Guards the vocabulary: anything not opening with a known cardinality is
        // data, not a configuration node.
        let root: Node = serde_json::from_str(r#"{"junk":["nonsense",{"a":[]}]}"#).unwrap();
        assert!(raw_at_path(&root, &["junk"]).is_none());
    }

    #[test]
    fn optional_cardinalities_are_prefixed_correctly() {
        let root: Node = serde_json::from_str(
            r#"{"a":["one opt",null],"b":["many opt",null],"c":["one mand","x"]}"#,
        )
        .unwrap();
        let children = build_display_children(&root);

        let section = children
            .iter()
            .find_map(|e| e.as_obj().filter(|o| o.key == "Add element:"))
            .expect("optional entries should produce a section");
        let labels: Vec<&str> = section.children.iter().filter_map(|c| c.as_str()).collect();

        // `one opt` carries the prefix so the app knows it may be added once;
        // `many opt` does not.
        assert!(
            labels.contains(&"<button>one-opt:a</button>a"),
            "got {labels:?}"
        );
        assert!(labels.contains(&"<button>b</button>b"), "got {labels:?}");

        // The mandatory entry stays outside the section.
        assert!(
            children
                .iter()
                .any(|e| e.as_obj().is_some_and(|o| o.key == "c"))
        );
    }

    #[test]
    fn the_pair_list_shape_is_flattened_to_children() {
        // Shape B, used by `version`: [[cardinality, value], ...].
        let raw: Node = serde_json::from_str(r#"[["one mand","1.0"],["one mand","2.0"]]"#).unwrap();
        let item = build_item("version", raw.as_array().unwrap());
        let obj = item.as_obj().expect("should render as a section");
        let values: Vec<&str> = obj.children.iter().filter_map(|c| c.as_str()).collect();
        assert_eq!(values, vec!["1.0", "2.0"]);
    }

    #[test]
    fn an_entry_with_no_content_degrades_to_a_label() {
        let raw: Node = serde_json::from_str(r#"["one mand"]"#).unwrap();
        assert_eq!(
            build_item("bare", raw.as_array().unwrap()).as_str(),
            Some("bare")
        );

        let empty: Node = serde_json::from_str("[]").unwrap();
        assert_eq!(
            build_item("empty", empty.as_array().unwrap()).as_str(),
            Some("empty")
        );
    }
}
