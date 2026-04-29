use serde::de::{MapAccess, SeqAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::io;
use std::path::Path;

// ---------------------------------------------------------------------------
// FfonElement
// ---------------------------------------------------------------------------

/// A node in the FFON tree.
///
/// - `Str` is a leaf node (plain string or tagged string like `<input>value</input>`)
/// - `Obj` is a branch node (a named section with children)
///
/// JSON format:  `"text"` → Str,  `{"key": [...children]}` → Obj
#[derive(Debug, Clone, PartialEq)]
pub enum FfonElement {
    Str(String),
    Obj(FfonObject),
}

impl FfonElement {
    pub fn new_str(s: impl Into<String>) -> Self {
        FfonElement::Str(s.into())
    }

    pub fn new_obj(key: impl Into<String>) -> Self {
        FfonElement::Obj(FfonObject { key: key.into(), children: Vec::new() })
    }

    pub fn is_str(&self) -> bool {
        matches!(self, FfonElement::Str(_))
    }

    pub fn is_obj(&self) -> bool {
        matches!(self, FfonElement::Obj(_))
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            FfonElement::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_obj(&self) -> Option<&FfonObject> {
        match self {
            FfonElement::Obj(o) => Some(o),
            _ => None,
        }
    }

    pub fn as_obj_mut(&mut self) -> Option<&mut FfonObject> {
        match self {
            FfonElement::Obj(o) => Some(o),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Serde for FfonElement: untagged — string → Str, object → Obj
// ---------------------------------------------------------------------------

impl Serialize for FfonElement {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            FfonElement::Str(text) => text.serialize(s),
            FfonElement::Obj(obj) => obj.serialize(s),
        }
    }
}

impl<'de> Deserialize<'de> for FfonElement {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct FfonElementVisitor;

        impl<'de> Visitor<'de> for FfonElementVisitor {
            type Value = FfonElement;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "a string or a single-key object")
            }

            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<FfonElement, E> {
                Ok(FfonElement::Str(v.to_owned()))
            }

            fn visit_string<E: serde::de::Error>(self, v: String) -> Result<FfonElement, E> {
                Ok(FfonElement::Str(v))
            }

            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<FfonElement, E> {
                Ok(FfonElement::Str(v.to_string()))
            }

            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<FfonElement, E> {
                Ok(FfonElement::Str(v.to_string()))
            }

            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<FfonElement, E> {
                Ok(FfonElement::Str(v.to_string()))
            }

            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<FfonElement, E> {
                Ok(FfonElement::Str(v.to_string()))
            }

            fn visit_unit<E: serde::de::Error>(self) -> Result<FfonElement, E> {
                Ok(FfonElement::Str("null".to_owned()))
            }

            fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<FfonElement, M::Error> {
                Ok(FfonElement::Obj(FfonObject::deserialize_map(map)?))
            }

            // JSON arrays are converted to an object with key "array" (matches C behaviour)
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<FfonElement, A::Error> {
                let mut children = Vec::new();
                while let Some(child) = seq.next_element::<FfonElement>()? {
                    children.push(child);
                }
                Ok(FfonElement::Obj(FfonObject { key: "array".to_owned(), children }))
            }
        }

        d.deserialize_any(FfonElementVisitor)
    }
}

// ---------------------------------------------------------------------------
// FfonObject
// ---------------------------------------------------------------------------

/// A named branch node: `{"key": [children...]}`
#[derive(Debug, Clone, PartialEq)]
pub struct FfonObject {
    pub key: String,
    pub children: Vec<FfonElement>,
}

impl FfonObject {
    pub fn new(key: impl Into<String>) -> Self {
        FfonObject { key: key.into(), children: Vec::new() }
    }

    pub fn push(&mut self, elem: FfonElement) {
        self.children.push(elem);
    }

    pub fn insert(&mut self, index: usize, elem: FfonElement) {
        let index = index.min(self.children.len());
        self.children.insert(index, elem);
    }

    pub fn remove(&mut self, index: usize) -> Option<FfonElement> {
        if index < self.children.len() { Some(self.children.remove(index)) } else { None }
    }

    fn deserialize_map<'de, M: MapAccess<'de>>(mut map: M) -> Result<Self, M::Error> {
        let key: String =
            map.next_key()?.ok_or_else(|| serde::de::Error::custom("empty FFON object"))?;
        let children: Vec<FfonElement> = map.next_value()?;
        Ok(FfonObject { key, children })
    }
}

impl Serialize for FfonObject {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut map = s.serialize_map(Some(1))?;
        map.serialize_entry(&self.key, &self.children)?;
        map.end()
    }
}

impl<'de> Deserialize<'de> for FfonObject {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct ObjVisitor;
        impl<'de> Visitor<'de> for ObjVisitor {
            type Value = FfonObject;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "a single-key object")
            }
            fn visit_map<M: MapAccess<'de>>(self, map: M) -> Result<FfonObject, M::Error> {
                FfonObject::deserialize_map(map)
            }
        }
        d.deserialize_map(ObjVisitor)
    }
}

// ---------------------------------------------------------------------------
// IdArray — navigation path (stack of integer indices)
// ---------------------------------------------------------------------------

/// A path into the FFON tree: each entry is an index into the children at that depth.
///
/// Equivalent to the C `IdArray` (max depth 32). In Rust, just a `Vec<usize>`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IdArray(Vec<usize>);

impl IdArray {
    pub fn new() -> Self {
        IdArray(Vec::new())
    }

    pub fn depth(&self) -> usize {
        self.0.len()
    }

    pub fn push(&mut self, idx: usize) {
        self.0.push(idx);
    }

    pub fn pop(&mut self) -> Option<usize> {
        self.0.pop()
    }

    pub fn get(&self, depth: usize) -> Option<usize> {
        self.0.get(depth).copied()
    }

    pub fn as_slice(&self) -> &[usize] {
        &self.0
    }

    pub fn to_display_string(&self) -> String {
        self.0.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",")
    }

    /// Replace the last index in the path (used when moving the selection up/down).
    pub fn set_last(&mut self, idx: usize) {
        if let Some(last) = self.0.last_mut() {
            *last = idx;
        }
    }

    /// Replace the index at a specific depth level.
    pub fn set(&mut self, depth: usize, idx: usize) {
        if let Some(slot) = self.0.get_mut(depth) {
            *slot = idx;
        }
    }

    /// Return the last index, or `None` if the path is empty.
    pub fn last(&self) -> Option<usize> {
        self.0.last().copied()
    }
}

// ---------------------------------------------------------------------------
// JSON file I/O
// ---------------------------------------------------------------------------

/// Deserialize a JSON file containing a top-level array of FFON elements.
pub fn load_json_file(path: &Path) -> io::Result<Vec<FfonElement>> {
    let data = std::fs::read_to_string(path)?;
    serde_json::from_str(&data)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Serialize a list of FFON elements to a JSON file (pretty-printed).
pub fn save_json_file(elements: &[FfonElement], path: &Path) -> io::Result<()> {
    let json = serde_json::to_string_pretty(elements)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, json)
}

/// Parse a JSON string into a list of FFON elements.
pub fn parse_json(json: &str) -> Result<Vec<FfonElement>, serde_json::Error> {
    serde_json::from_str(json)
}

/// Convert a single JSON value to an `FfonElement`.
///
/// Port of C's `parseJsonValue` from `lib/lib_ffon/src/ffon.c`:
/// - null → `Str("null")`
/// - bool → `Str("true")` / `Str("false")`
/// - number → `Str("<decimal representation>")`
/// - string → `Str(s)`
/// - array → `Obj("array", [children...])`
/// - object → `Obj(first_key, [children if first_value is array])`
/// - empty object → `Str("")`
pub fn parse_json_value(v: &serde_json::Value) -> FfonElement {
    match v {
        serde_json::Value::Null => FfonElement::Str("null".to_owned()),
        serde_json::Value::Bool(b) => {
            FfonElement::Str(if *b { "true" } else { "false" }.to_owned())
        }
        serde_json::Value::Number(n) => FfonElement::Str(n.to_string()),
        serde_json::Value::String(s) => FfonElement::Str(s.clone()),
        serde_json::Value::Array(arr) => {
            let mut obj = FfonElement::new_obj("array");
            for item in arr {
                obj.as_obj_mut().unwrap().push(parse_json_value(item));
            }
            obj
        }
        serde_json::Value::Object(map) => {
            // Use first key-value pair only (matches C behavior).
            if let Some((key, val)) = map.iter().next() {
                let mut obj = FfonElement::new_obj(key);
                if let serde_json::Value::Array(arr) = val {
                    for item in arr {
                        obj.as_obj_mut().unwrap().push(parse_json_value(item));
                    }
                }
                obj
            } else {
                FfonElement::Str(String::new())
            }
        }
    }
}

/// Serialize a list of FFON elements to a JSON string.
pub fn to_json_string(elements: &[FfonElement]) -> Result<String, serde_json::Error> {
    serde_json::to_string(elements)
}

/// Strict structural check: is this JSON value a valid FFON document?
///
/// An FFON document is a JSON array where every element is recursively either:
/// - a JSON string (leaf), or
/// - a JSON object with **exactly one** key whose value is a JSON array of more FFON elements.
///
/// This matches the round-trip invariant of `ffonElementToJson` / `FfonObject::serialize`.
/// The tolerant `parse_json_value` / `parse_json` functions accept any JSON and coerce it;
/// this function is strict and rejects primitives, multi-key objects, and bare nested arrays.
pub fn is_ffon(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::Array(arr) => arr.iter().all(is_ffon_element),
        _ => false,
    }
}

fn is_ffon_element(v: &serde_json::Value) -> bool {
    match v {
        serde_json::Value::String(_) => true,
        serde_json::Value::Object(map) => {
            map.len() == 1
                && match map.values().next().unwrap() {
                    serde_json::Value::Array(children) => children.iter().all(is_ffon_element),
                    _ => false,
                }
        }
        // null, bool, number, bare array, multi-key object → not FFON
        _ => false,
    }
}

// ---------------------------------------------------------------------------
// Binary serialization (.ffon files)
//
// Format (little-endian):
//   For each node (depth-first):
//     [layer: u32][content_len: u32][content_bytes]
//   Objects: content = "key:" (trailing colon marks it as a branch)
//   Strings: content = raw string bytes (no trailing colon)
// ---------------------------------------------------------------------------

pub fn serialize_binary(elements: &[FfonElement]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(1024);
    for elem in elements {
        write_element_binary(elem, 0, &mut buf);
    }
    buf
}

fn write_element_binary(elem: &FfonElement, layer: u32, buf: &mut Vec<u8>) {
    match elem {
        FfonElement::Str(s) => {
            let bytes = s.as_bytes();
            buf.extend_from_slice(&layer.to_le_bytes());
            buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            buf.extend_from_slice(bytes);
        }
        FfonElement::Obj(obj) => {
            // content = "key:" — trailing colon signals object
            let key_bytes = obj.key.as_bytes();
            let content_len = key_bytes.len() + 1; // +1 for ':'
            buf.extend_from_slice(&layer.to_le_bytes());
            buf.extend_from_slice(&(content_len as u32).to_le_bytes());
            buf.extend_from_slice(key_bytes);
            buf.push(b':');
            // Recursively write children at layer+1
            for child in &obj.children {
                write_element_binary(child, layer + 1, buf);
            }
        }
    }
}

pub fn deserialize_binary(data: &[u8]) -> Vec<FfonElement> {
    // First pass: parse all flat entries
    struct Entry {
        layer: u32,
        content: Vec<u8>,
        is_key: bool,
    }

    let mut entries: Vec<Entry> = Vec::new();
    let mut pos = 0;
    while pos + 8 <= data.len() {
        let layer = u32::from_le_bytes([data[pos], data[pos+1], data[pos+2], data[pos+3]]);
        let content_len = u32::from_le_bytes([data[pos+4], data[pos+5], data[pos+6], data[pos+7]]) as usize;
        pos += 8;
        if pos + content_len > data.len() {
            break;
        }
        let content = data[pos..pos + content_len].to_vec();
        pos += content_len;
        let is_key = content.last() == Some(&b':');
        entries.push(Entry { layer, content, is_key });
    }

    // Second pass: rebuild tree using a depth stack
    let mut result: Vec<FfonElement> = Vec::new();
    // Stack of (layer, mutable index into result or parent's children)
    // We use a parallel Vec to track the element we're building at each depth.
    // Because Rust ownership makes a mutable stack of references tricky,
    // we use indices into a flat `nodes` vec and reconnect at the end.
    //
    // Simpler approach: build nodes list and then fold into tree.
    struct Node {
        layer: u32,
        elem: FfonElement,
    }
    let mut nodes: Vec<Node> = Vec::with_capacity(entries.len());
    for e in &entries {
        let elem = if e.is_key {
            let key = std::str::from_utf8(&e.content[..e.content.len() - 1])
                .unwrap_or("")
                .to_owned();
            FfonElement::new_obj(key)
        } else {
            let s = std::str::from_utf8(&e.content).unwrap_or("").to_owned();
            FfonElement::Str(s)
        };
        nodes.push(Node { layer: e.layer, elem });
    }

    // Build tree: for each node, find its parent (last preceding node with layer == this.layer - 1)
    // We process in reverse and use a stack.
    // We need ownership over all elements. Collect them first, then parent them.
    let mut elems: Vec<(u32, FfonElement)> =
        nodes.into_iter().map(|n| (n.layer, n.elem)).collect();

    // Process from the end so we can move children into parents.
    // Walk backward: each element at layer L is a child of the nearest preceding element at layer L-1.
    // But we need forward order for correct child ordering. Use a forward pass with an owned stack.

    // Strategy: accumulate children into parents using a stack of (layer, FfonElement).
    let mut stack: Vec<(u32, FfonElement)> = Vec::new();

    for (layer, elem) in elems.drain(..) {
        // Pop all stack items that are at the same or deeper layer — they won't get more children.
        // If they are children (layer > their parent's layer), they'll be attached when we pop.
        // Actually: when we encounter an element at layer L, any stack item at layer >= L
        // that is a child of a layer L-1 element should already be in the right parent.
        // Let's use the simpler approach from the C code: a stack of open objects.

        // Pop stack items with layer >= current layer (they are done)
        while stack.last().map_or(false, |(l, _)| *l >= layer) {
            let (_, child) = stack.pop().unwrap();
            if let Some((_, FfonElement::Obj(parent_obj))) = stack.last_mut() {
                parent_obj.children.insert(0, child); // we'll fix order below
            } else {
                // Root level
                result.insert(0, child);
            }
        }

        stack.push((layer, elem));
    }

    // Drain remaining stack
    while let Some((_, elem)) = stack.pop() {
        if let Some((_, FfonElement::Obj(parent_obj))) = stack.last_mut() {
            parent_obj.children.insert(0, elem);
        } else {
            result.insert(0, elem);
        }
    }

    // Fix: the insert(0, ...) approach reverses order. Use a cleaner rebuild.
    // Actually, let's use the simpler correct approach:
    result.clear();
    deserialize_binary_inner(data, &mut result);
    result
}

fn deserialize_binary_inner(data: &[u8], result: &mut Vec<FfonElement>) {
    // Parse all flat entries first
    let mut entries: Vec<(u32, bool, String)> = Vec::new(); // (layer, is_key, content)
    let mut pos = 0;
    while pos + 8 <= data.len() {
        let layer = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap());
        let content_len = u32::from_le_bytes(data[pos+4..pos+8].try_into().unwrap()) as usize;
        pos += 8;
        if pos + content_len > data.len() { break; }
        let raw = &data[pos..pos + content_len];
        pos += content_len;
        let is_key = raw.last() == Some(&b':');
        let content = if is_key {
            std::str::from_utf8(&raw[..raw.len()-1]).unwrap_or("").to_owned()
        } else {
            std::str::from_utf8(raw).unwrap_or("").to_owned()
        };
        entries.push((layer, is_key, content));
    }

    // Build tree using a mutable stack of open objects.
    // Stack entries: (layer, FfonObject in progress)
    let mut obj_stack: Vec<(u32, FfonObject)> = Vec::new();

    for (layer, is_key, content) in entries {
        // Close all objects on the stack that are deeper than or equal to this layer
        // (they are complete — their children come from deeper layers that are now past)
        while obj_stack.last().map_or(false, |(l, _)| *l >= layer) {
            let (_, finished) = obj_stack.pop().unwrap();
            let finished_elem = FfonElement::Obj(finished);
            if let Some((_, parent)) = obj_stack.last_mut() {
                parent.children.push(finished_elem);
            } else {
                result.push(finished_elem);
            }
        }

        if is_key {
            // Open a new object — children will be pushed to it
            obj_stack.push((layer, FfonObject::new(content)));
        } else {
            // Leaf string — attach to parent or result
            let str_elem = FfonElement::Str(content);
            if let Some((_, parent)) = obj_stack.last_mut() {
                parent.children.push(str_elem);
            } else {
                result.push(str_elem);
            }
        }
    }

    // Close remaining open objects
    while let Some((_, finished)) = obj_stack.pop() {
        let finished_elem = FfonElement::Obj(finished);
        if let Some((_, parent)) = obj_stack.last_mut() {
            parent.children.push(finished_elem);
        } else {
            result.push(finished_elem);
        }
    }
}

pub fn save_ffon_file(elements: &[FfonElement], path: &Path) -> io::Result<()> {
    let data = serialize_binary(elements);
    std::fs::write(path, data)
}

pub fn load_ffon_file(path: &Path) -> io::Result<Vec<FfonElement>> {
    let data = std::fs::read(path)?;
    Ok(deserialize_binary(&data))
}

// ---------------------------------------------------------------------------
// FFON tree navigation
// ---------------------------------------------------------------------------

/// Get the children at the given path within the FFON tree.
///
/// - depth 0 → returns the root slice
/// - depth 1 → returns children of `ffon[id[0]]`
/// - etc.
///
/// Returns `None` if any index is out of bounds or a non-object is encountered mid-path.
pub fn get_ffon_at_id<'a>(ffon: &'a [FfonElement], id: &IdArray) -> Option<&'a [FfonElement]> {
    if id.depth() == 0 {
        return Some(ffon);
    }

    let mut current = ffon;

    // Walk all indices except the last — they select the parent chain.
    for depth in 0..id.depth() - 1 {
        let idx = id.get(depth)?;
        let elem = current.get(idx)?;
        match elem {
            FfonElement::Obj(obj) => current = &obj.children,
            _ => return None,
        }
    }

    Some(current)
}

/// Returns true if navigating into `ffon[id]` would have children (i.e. it's an object).
pub fn next_layer_exists(ffon: &[FfonElement], id: &IdArray) -> bool {
    if id.depth() == 0 {
        return false;
    }
    let parent = match get_ffon_at_id(ffon, id) {
        Some(s) => s,
        None => return false,
    };
    let last_idx = id.get(id.depth() - 1).unwrap_or(usize::MAX);
    matches!(parent.get(last_idx), Some(FfonElement::Obj(_)))
}

/// Returns the maximum valid index at the given path (count - 1), or 0 if empty.
pub fn get_ffon_max_id(ffon: &[FfonElement], id: &IdArray) -> usize {
    let arr = match get_ffon_at_id(ffon, id) {
        Some(s) => s,
        None => return 0,
    };
    arr.len().saturating_sub(1)
}

// ---------------------------------------------------------------------------
// HTML → FFON conversion
//
// Parses an HTML document or fragment with `scraper` (html5ever) and converts
// the DOM to a FFON tree. Placed here because FFON is the central abstraction
// and HTML→FFON is fundamentally a FFON construction operation.
// ---------------------------------------------------------------------------

/// Tags we skip entirely (including all their children).
const HTML_SKIP_TAGS: &[&str] = &[
    "script", "style", "noscript", "svg", "head", "nav", "footer",
];

/// Block container tags — recurse into children without emitting a wrapper element.
const HTML_CONTAINER_TAGS: &[&str] = &[
    "div", "section", "article", "main", "header", "aside", "figure",
    "blockquote", "details", "summary",
    // Form structure: these are transparent containers so their children are processed normally
    "label", "fieldset",
];

// ---------------------------------------------------------------------------
// Form-map types (used by html_to_ffon_with_forms)
// ---------------------------------------------------------------------------

/// The kind of a form control node, for use in Phase 2 CDP interaction.
#[derive(Debug, Clone, PartialEq)]
pub enum FormNodeKind {
    TextInput,
    Textarea,
    Checkbox,
    RadioOption { group: String, value: String },
    Submit,
    Select,
}

/// Describes how to find and interact with a form control via CDP.
#[derive(Debug, Clone, PartialEq)]
pub struct FormNode {
    /// A CSS selector that uniquely identifies this element in the live DOM.
    pub css_selector: String,
    pub kind: FormNodeKind,
}

/// Maps FFON path segments (e.g. `"form_1/email"`) to the corresponding
/// live DOM node. Populated by `html_to_ffon_with_forms` and consumed
/// by `WebbrowserProvider` in Phase 2 to drive CDP interactions.
pub type FormMap = std::collections::HashMap<String, FormNode>;

fn html_normalize_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_ws = true;
    for c in s.chars() {
        if c.is_whitespace() {
            if !prev_ws { out.push(' '); prev_ws = true; }
        } else {
            out.push(c);
            prev_ws = false;
        }
    }
    if out.ends_with(' ') { out.pop(); }
    out
}

fn html_heading_level(tag: &str) -> Option<u8> {
    match tag {
        "h1" => Some(1), "h2" => Some(2), "h3" => Some(3),
        "h4" => Some(4), "h5" => Some(5), "h6" => Some(6),
        _ => None,
    }
}

struct HtmlParseCtx<'a> {
    base_url: &'a str,
    root: Vec<FfonElement>,
    stack: Vec<(u8, FfonElement)>,
    pending_id: Option<String>,
    // Form tracking
    form_count: usize,        // total forms encountered in the document (never reset)
    current_form_idx: usize,  // 1-based index of the form being parsed (0 = not in a form)
    form_input_count: usize,  // inputs seen in the current form (for fallback labels)
    form_map: Vec<(String, FormNode)>, // accumulated path → node entries
}

impl<'a> HtmlParseCtx<'a> {
    fn new(base_url: &'a str) -> Self {
        HtmlParseCtx {
            base_url,
            root: Vec::new(),
            stack: Vec::new(),
            pending_id: None,
            form_count: 0,
            current_form_idx: 0,
            form_input_count: 0,
            form_map: Vec::new(),
        }
    }

    fn add_to_current(&mut self, mut elem: FfonElement) {
        if let Some(id) = self.pending_id.take() {
            let prefix = crate::tags::format_id(&id);
            match &mut elem {
                FfonElement::Obj(o) => o.key.insert_str(0, &prefix),
                FfonElement::Str(s) => s.insert_str(0, &prefix),
            }
        }
        if let Some((_, ref mut h)) = self.stack.last_mut() {
            h.as_obj_mut().unwrap().push(elem);
        } else {
            self.root.push(elem);
        }
    }

    fn pop_until_level(&mut self, level: u8) {
        while self.stack.last().map_or(false, |(l, _)| *l >= level) {
            let (_, entry) = self.stack.pop().unwrap();
            if let Some((_, ref mut parent)) = self.stack.last_mut() {
                parent.as_obj_mut().unwrap().push(entry);
            } else {
                self.root.push(entry);
            }
        }
    }

    fn finalize(self) -> Vec<FfonElement> {
        self.finalize_with_forms().0
    }

    fn finalize_with_forms(mut self) -> (Vec<FfonElement>, Vec<(String, FormNode)>) {
        while let Some((_, entry)) = self.stack.pop() {
            if let Some((_, ref mut parent)) = self.stack.last_mut() {
                parent.as_obj_mut().unwrap().push(entry);
            } else {
                self.root.push(entry);
            }
        }
        (self.root, self.form_map)
    }

    fn process_children(&mut self, node: scraper::ElementRef) {
        for child in node.children().filter_map(scraper::ElementRef::wrap) {
            self.process_node(child);
        }
    }

    fn process_node(&mut self, node: scraper::ElementRef) {
        let tag = node.value().name();
        if HTML_SKIP_TAGS.contains(&tag) { return; }

        let prev_id = if let Some(id) = node.value().attr("id").filter(|s| !s.is_empty()) {
            let prev = self.pending_id.take();
            self.pending_id = Some(id.to_owned());
            prev
        } else { None };
        let had_own_id = node.value().attr("id").filter(|s| !s.is_empty()).is_some();

        if let Some(level) = html_heading_level(tag) {
            let text = html_collect_text(node, self.base_url);
            if text.is_empty() { if had_own_id { self.pending_id = prev_id; } return; }
            self.pop_until_level(level);
            let id_prefix = self.pending_id.take()
                .map(|i| crate::tags::format_id(&i)).unwrap_or_default();
            self.stack.push((level, FfonElement::new_obj(format!("{id_prefix}{text}"))));
            if had_own_id { self.pending_id = prev_id; }
            return;
        }

        match tag {
            "br" => { self.add_to_current(FfonElement::new_str(String::new())); }
            "p" => {
                for elem in html_collect_elements(node, self.base_url) {
                    self.add_to_current(elem);
                }
            }
            "ul" | "ol" => {
                let label = if tag == "ol" { "ordered list" } else { "list" };
                let id_prefix = self.pending_id.take()
                    .map(|i| crate::tags::format_id(&i)).unwrap_or_default();
                let mut list_obj = FfonElement::new_obj(format!("{id_prefix}{label}"));
                let li_sel = scraper::Selector::parse("li").unwrap();
                for (i, li) in node.select(&li_sel).enumerate() {
                    for elem in html_collect_elements(li, self.base_url) {
                        let prefixed = match &elem {
                            FfonElement::Str(s) => FfonElement::new_str(
                                if tag == "ol" { format!("{}. {}", i + 1, s) }
                                else { format!("- {}", s) }
                            ),
                            FfonElement::Obj(_) => elem,
                        };
                        list_obj.as_obj_mut().unwrap().push(prefixed);
                    }
                }
                if list_obj.as_obj().map_or(false, |o| !o.children.is_empty()) {
                    self.add_to_current(list_obj);
                }
            }
            "table" => {
                let mut rows: Vec<FfonElement> = Vec::new();
                html_collect_table_rows(node, &mut rows);
                for row in rows { self.add_to_current(row); }
            }
            "pre" | "code" => {
                let text = node.text().collect::<String>();
                let trimmed = text.trim().to_owned();
                if !trimmed.is_empty() { self.add_to_current(FfonElement::new_str(trimmed)); }
            }
            "img" => {
                let alt = node.value().attr("alt").unwrap_or("");
                if !alt.is_empty() && alt != "image" {
                    self.add_to_current(FfonElement::new_str(format!("{alt} [img]")));
                }
            }
            "a" => {
                let href = html_resolve_href(node.value().attr("href").unwrap_or(""), self.base_url);
                let text = html_collect_text(node, self.base_url);
                if !text.is_empty() && !href.is_empty() {
                    self.add_to_current(FfonElement::new_obj(format!("{text} <link>{href}</link>")));
                } else if !text.is_empty() {
                    self.add_to_current(FfonElement::new_str(text));
                }
            }
            "dl" => {
                let id_prefix = self.pending_id.take()
                    .map(|i| crate::tags::format_id(&i)).unwrap_or_default();
                let mut dl_obj = FfonElement::new_obj(format!("{id_prefix}definition list"));
                let mut current_dt: Option<FfonElement> = None;
                for child in node.children().filter_map(scraper::ElementRef::wrap) {
                    let text = html_collect_text(child, self.base_url);
                    if text.is_empty() { continue; }
                    match child.value().name() {
                        "dt" => {
                            if let Some(dt) = current_dt.take() { dl_obj.as_obj_mut().unwrap().push(dt); }
                            current_dt = Some(FfonElement::new_obj(text));
                        }
                        "dd" => {
                            if let Some(ref mut dt) = current_dt {
                                dt.as_obj_mut().unwrap().push(FfonElement::new_str(text));
                            } else {
                                dl_obj.as_obj_mut().unwrap().push(FfonElement::new_str(text));
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(dt) = current_dt { dl_obj.as_obj_mut().unwrap().push(dt); }
                if dl_obj.as_obj().map_or(false, |o| !o.children.is_empty()) {
                    self.add_to_current(dl_obj);
                }
            }
            // ---- Form container ------------------------------------------------
            "form" => {
                let form_idx = self.form_count + 1;
                self.form_count += 1;

                // Isolate: process form children into a fresh root/stack so that
                // heading levels inside the form don't interfere with the outer tree.
                let saved_root = std::mem::take(&mut self.root);
                let saved_stack = std::mem::take(&mut self.stack);
                let saved_form_input_count = std::mem::replace(&mut self.form_input_count, 0);
                let saved_form_idx = std::mem::replace(&mut self.current_form_idx, form_idx);

                let id_prefix = self.pending_id.take()
                    .map(|i| crate::tags::format_id(&i)).unwrap_or_default();

                self.process_children(node);

                // Flush any open heading-level stack entries into root
                while let Some((_, entry)) = self.stack.pop() {
                    self.root.push(entry);
                }
                let form_children = std::mem::replace(&mut self.root, saved_root);
                self.stack = saved_stack;
                self.form_input_count = saved_form_input_count;
                self.current_form_idx = saved_form_idx;

                if !form_children.is_empty() {
                    let mut form_obj = FfonElement::new_obj(format!("{id_prefix}form_{form_idx}"));
                    for child in form_children {
                        form_obj.as_obj_mut().unwrap().push(child);
                    }
                    self.add_to_current(form_obj);
                }
                if had_own_id { self.pending_id = prev_id; }
                return;
            }

            // ---- Input fields --------------------------------------------------
            "input" => {
                // Collect all attributes eagerly to avoid borrow conflicts later.
                let input_type = node.value().attr("type").unwrap_or("text").to_ascii_lowercase();
                let name      = node.value().attr("name").unwrap_or("").to_owned();
                let id_attr   = node.value().attr("id").unwrap_or("").to_owned();
                let ph        = node.value().attr("placeholder").unwrap_or("").to_owned();
                let al        = node.value().attr("aria-label").unwrap_or("").to_owned();
                let value     = node.value().attr("value").unwrap_or("").to_owned();
                let is_checked = node.value().attr("checked").is_some();
                let form_n    = self.current_form_idx;

                // Form controls derive their label from id_attr directly; the pending_id
                // propagation mechanism must not add a spurious <id>X</id> prefix to the
                // FFON string (which would corrupt the form_map key lookup).
                let _ = self.pending_id.take();

                match input_type.as_str() {
                    "hidden" | "file" | "reset" | "image" => {
                        // Not user-visible; skip.
                    }
                    "checkbox" => {
                        let label = html_input_label(&ph, &al, &name, &id_attr,
                                                     &mut self.form_input_count);
                        let tag = if is_checked { "<checkbox checked>" } else { "<checkbox>" };
                        self.add_to_current(FfonElement::new_str(format!("{tag}{label}")));
                        if form_n > 0 {
                            self.form_map.push((
                                format!("form_{form_n}/{label}"),
                                FormNode {
                                    css_selector: html_input_selector(form_n, &name, &id_attr),
                                    kind: FormNodeKind::Checkbox,
                                },
                            ));
                        }
                    }
                    "radio" => {
                        let label = html_input_label(&ph, &al, &name, &id_attr,
                                                     &mut self.form_input_count);
                        let indicator = if is_checked { "(x) " } else { "( ) " };
                        self.add_to_current(FfonElement::new_str(format!("{indicator}{label}")));
                        if form_n > 0 && !name.is_empty() {
                            self.form_map.push((
                                format!("form_{form_n}/{name}/{label}"),
                                FormNode {
                                    css_selector: html_input_selector(form_n, &name, &id_attr),
                                    kind: FormNodeKind::RadioOption {
                                        group: name.clone(),
                                        value: value.clone(),
                                    },
                                },
                            ));
                        }
                    }
                    "submit" | "button" => {
                        let display = if !value.is_empty() { value.clone() } else { "Submit".to_owned() };
                        let fn_name = format!("submit:form_{form_n}");
                        self.add_to_current(FfonElement::new_str(format!("<button>{fn_name}</button>{display}")));
                        if form_n > 0 {
                            self.form_map.push((
                                format!("form_{form_n}/{display}"),
                                FormNode {
                                    css_selector: html_submit_selector(form_n, &name, &id_attr),
                                    kind: FormNodeKind::Submit,
                                },
                            ));
                        }
                    }
                    _ => {
                        // text, email, password, url, search, tel, number, date, …
                        let label = html_input_label(&ph, &al, &name, &id_attr,
                                                     &mut self.form_input_count);
                        self.add_to_current(FfonElement::new_str(
                            format!("{label}: <input>{value}</input>")
                        ));
                        if form_n > 0 {
                            self.form_map.push((
                                format!("form_{form_n}/{label}"),
                                FormNode {
                                    css_selector: html_input_selector(form_n, &name, &id_attr),
                                    kind: FormNodeKind::TextInput,
                                },
                            ));
                        }
                    }
                }
                if had_own_id { self.pending_id = prev_id; }
                return;
            }

            // ---- Textarea ------------------------------------------------------
            "textarea" => {
                let name    = node.value().attr("name").unwrap_or("").to_owned();
                let id_attr = node.value().attr("id").unwrap_or("").to_owned();
                let ph      = node.value().attr("placeholder").unwrap_or("").to_owned();
                let al      = node.value().attr("aria-label").unwrap_or("").to_owned();
                let content = node.text().collect::<String>().trim().to_owned();
                let form_n  = self.current_form_idx;
                let label   = html_input_label(&ph, &al, &name, &id_attr,
                                               &mut self.form_input_count);
                let _ = self.pending_id.take();
                self.add_to_current(FfonElement::new_str(format!("{label}: <input>{content}</input>")));
                if form_n > 0 {
                    self.form_map.push((
                        format!("form_{form_n}/{label}"),
                        FormNode {
                            css_selector: html_input_selector(form_n, &name, &id_attr),
                            kind: FormNodeKind::Textarea,
                        },
                    ));
                }
                if had_own_id { self.pending_id = prev_id; }
                return;
            }

            // ---- Select (dropdown) ---------------------------------------------
            "select" => {
                let name    = node.value().attr("name").unwrap_or("").to_owned();
                let id_attr = node.value().attr("id").unwrap_or("").to_owned();
                let al      = node.value().attr("aria-label").unwrap_or("").to_owned();
                let form_n  = self.current_form_idx;
                let label   = html_input_label("", &al, &name, &id_attr,
                                               &mut self.form_input_count);
                let _ = self.pending_id.take();

                let opt_sel = scraper::Selector::parse("option").unwrap();
                let mut radio_obj = FfonElement::new_obj(format!("<radio>{label}"));
                for opt in node.select(&opt_sel) {
                    let val  = opt.value().attr("value").unwrap_or("").to_owned();
                    let text = opt.text().collect::<String>().trim().to_owned();
                    let selected = opt.value().attr("selected").is_some();
                    let display  = if !text.is_empty() { text.clone() } else { val.clone() };
                    if display.is_empty() { continue; }
                    let entry = if selected {
                        FfonElement::new_str(format!("<checked>{display}</checked>"))
                    } else {
                        FfonElement::new_str(display.clone())
                    };
                    radio_obj.as_obj_mut().unwrap().push(entry);
                    if form_n > 0 {
                        self.form_map.push((
                            format!("form_{form_n}/{label}/{display}"),
                            FormNode {
                                css_selector: html_select_option_selector(form_n, &name, &id_attr, &val),
                                kind: FormNodeKind::Select,
                            },
                        ));
                    }
                }
                if radio_obj.as_obj().map_or(false, |o| !o.children.is_empty()) {
                    self.add_to_current(radio_obj);
                }
                if had_own_id { self.pending_id = prev_id; }
                return;
            }

            // ---- Standalone button (not <input type=button>) -------------------
            "button" => {
                let btn_type = node.value().attr("type").unwrap_or("submit").to_ascii_lowercase();
                let _ = self.pending_id.take();
                if matches!(btn_type.as_str(), "submit" | "button") {
                    let name    = node.value().attr("name").unwrap_or("").to_owned();
                    let id_attr = node.value().attr("id").unwrap_or("").to_owned();
                    let display = html_collect_text(node, self.base_url);
                    let display = if !display.is_empty() { display } else { "Submit".to_owned() };
                    let form_n  = self.current_form_idx;
                    let fn_name = format!("submit:form_{form_n}");
                    self.add_to_current(FfonElement::new_str(format!("<button>{fn_name}</button>{display}")));
                    if form_n > 0 {
                        self.form_map.push((
                            format!("form_{form_n}/{display}"),
                            FormNode {
                                css_selector: html_submit_selector(form_n, &name, &id_attr),
                                kind: FormNodeKind::Submit,
                            },
                        ));
                    }
                }
                if had_own_id { self.pending_id = prev_id; }
                return;
            }

            t if HTML_CONTAINER_TAGS.contains(&t) => { self.process_children(node); }
            _ => {
                let has_block = node.children().filter_map(scraper::ElementRef::wrap).any(|c| {
                    let t = c.value().name();
                    html_heading_level(t).is_some()
                        || matches!(t, "p" | "ul" | "ol" | "table" | "dl"
                                        | "input" | "textarea" | "select" | "button")
                        || HTML_CONTAINER_TAGS.contains(&t)
                });
                if has_block { self.process_children(node); }
                else {
                    let text = html_collect_text(node, self.base_url);
                    if !text.is_empty() { self.add_to_current(FfonElement::new_str(text)); }
                }
            }
        }
        if had_own_id { self.pending_id = prev_id; }
    }
}

// ---------------------------------------------------------------------------
// Form-parsing helpers
// ---------------------------------------------------------------------------

/// Pick a human-readable label for a form control.
/// Priority: placeholder → aria-label → name → id → generated fallback.
fn html_input_label(ph: &str, al: &str, name: &str, id: &str, counter: &mut usize) -> String {
    if !ph.is_empty()   { return ph.to_owned(); }
    if !al.is_empty()   { return al.to_owned(); }
    if !name.is_empty() { return name.to_owned(); }
    if !id.is_empty()   { return id.to_owned(); }
    *counter += 1;
    format!("input_{}", *counter)
}

/// CSS selector for a form input (text/checkbox/radio/textarea).
fn html_input_selector(form_n: usize, name: &str, id: &str) -> String {
    if !id.is_empty()   { return format!("#{id}"); }
    if !name.is_empty() { return format!("form:nth-of-type({form_n}) [name=\"{name}\"]"); }
    format!("form:nth-of-type({form_n}) input")
}

/// CSS selector for a submit button.
fn html_submit_selector(form_n: usize, name: &str, id: &str) -> String {
    if !id.is_empty()   { return format!("#{id}"); }
    if !name.is_empty() { return format!("form:nth-of-type({form_n}) [name=\"{name}\"]"); }
    format!("form:nth-of-type({form_n}) [type=\"submit\"]")
}

/// CSS selector for a specific `<option>` within a `<select>`.
fn html_select_option_selector(form_n: usize, name: &str, id: &str, val: &str) -> String {
    let sel_base = if !id.is_empty() {
        format!("#{id}")
    } else if !name.is_empty() {
        format!("form:nth-of-type({form_n}) select[name=\"{name}\"]")
    } else {
        format!("form:nth-of-type({form_n}) select")
    };
    if val.is_empty() {
        format!("{sel_base} option")
    } else {
        format!("{sel_base} option[value=\"{val}\"]")
    }
}

// ---------------------------------------------------------------------------
// Public HTML → FFON conversion
// ---------------------------------------------------------------------------

/// Convert an HTML string to a list of `FfonElement`s.
///
/// Accepts full documents (`<html>`/`<body>`) and fragments.
/// Pass `base_url` for relative-href resolution; pass `""` when there is none.
pub fn html_to_ffon(html: &str, base_url: &str) -> Vec<FfonElement> {
    use scraper::{Html, Selector};
    let document = Html::parse_document(html);
    let body_sel = Selector::parse("body").unwrap();
    let body = match document.select(&body_sel).next() {
        Some(b) => b,
        None => {
            let mut ctx = HtmlParseCtx::new(base_url);
            ctx.process_children(document.root_element());
            let r = ctx.finalize();
            return if r.is_empty() { vec![FfonElement::new_str("(empty)")] } else { r };
        }
    };
    let mut ctx = HtmlParseCtx::new(base_url);
    ctx.process_children(body);
    let r = ctx.finalize();
    if r.is_empty() { vec![FfonElement::new_str("(empty)")] } else { r }
}

/// Like `html_to_ffon`, but also returns a [`FormMap`] mapping FFON path
/// segments (e.g. `"form_1/email"`) to their CSS selectors and control kinds.
///
/// Used by `WebbrowserProvider` to drive CDP interactions (Phase 2).
pub fn html_to_ffon_with_forms(html: &str, base_url: &str) -> (Vec<FfonElement>, FormMap) {
    use scraper::{Html, Selector};
    let document = Html::parse_document(html);
    let body_sel = Selector::parse("body").unwrap();
    let (r, map_entries) = match document.select(&body_sel).next() {
        Some(body) => {
            let mut ctx = HtmlParseCtx::new(base_url);
            ctx.process_children(body);
            ctx.finalize_with_forms()
        }
        None => {
            let mut ctx = HtmlParseCtx::new(base_url);
            ctx.process_children(document.root_element());
            ctx.finalize_with_forms()
        }
    };
    let elems = if r.is_empty() { vec![FfonElement::new_str("(empty)")] } else { r };
    let form_map: FormMap = map_entries.into_iter().collect();
    (elems, form_map)
}

fn html_collect_text(node: scraper::ElementRef, base_url: &str) -> String {
    use scraper::Node;
    let mut buf = String::new();
    for child in node.children() {
        match child.value() {
            Node::Text(t) => buf.push_str(t),
            Node::Element(e) => {
                if let Some(elem_ref) = scraper::ElementRef::wrap(child) {
                    let name = e.name();
                    if HTML_SKIP_TAGS.contains(&name) { continue; }
                    if name == "br" { buf.push('\n'); }
                    else if name == "a" {
                        let href = html_resolve_href(e.attr("href").unwrap_or(""), base_url);
                        let text = html_collect_text(elem_ref, base_url);
                        if !text.is_empty() && !href.is_empty() {
                            buf.push_str(&format!("{text} <link>{href}</link>"));
                        } else if !text.is_empty() { buf.push_str(&text); }
                    } else { buf.push_str(&html_collect_text(elem_ref, base_url)); }
                }
            }
            _ => {}
        }
    }
    html_normalize_whitespace(&buf)
}

fn html_collect_elements(node: scraper::ElementRef, base_url: &str) -> Vec<FfonElement> {
    use scraper::Node;
    let mut result: Vec<FfonElement> = Vec::new();
    let mut text_buf = String::new();
    for child in node.children() {
        match child.value() {
            Node::Text(t) => text_buf.push_str(t),
            Node::Element(e) => {
                if let Some(elem_ref) = scraper::ElementRef::wrap(child) {
                    let name = e.name();
                    if HTML_SKIP_TAGS.contains(&name) { continue; }
                    if name == "br" { text_buf.push('\n'); }
                    else if name == "a" {
                        let norm = html_normalize_whitespace(&text_buf);
                        if !norm.is_empty() { result.push(FfonElement::new_str(norm)); }
                        text_buf.clear();
                        let href = html_resolve_href(e.attr("href").unwrap_or(""), base_url);
                        let link_text = html_collect_text(elem_ref, base_url);
                        if !link_text.is_empty() && !href.is_empty() {
                            result.push(FfonElement::new_obj(format!("{link_text} <link>{href}</link>")));
                        } else if !link_text.is_empty() { text_buf.push_str(&link_text); }
                    } else { text_buf.push_str(&html_collect_text(elem_ref, base_url)); }
                }
            }
            _ => {}
        }
    }
    let norm = html_normalize_whitespace(&text_buf);
    if !norm.is_empty() { result.push(FfonElement::new_str(norm)); }
    result
}

fn html_collect_table_rows(node: scraper::ElementRef, out: &mut Vec<FfonElement>) {
    let row_sel = scraper::Selector::parse("tr").unwrap();
    for row in node.select(&row_sel) {
        let cell_sel = scraper::Selector::parse("th, td").unwrap();
        let cells: Vec<String> = row.select(&cell_sel)
            .map(|c| html_normalize_whitespace(&c.text().collect::<String>()))
            .filter(|s| !s.is_empty()).collect();
        if !cells.is_empty() { out.push(FfonElement::new_str(cells.join(" | "))); }
    }
}

/// Resolve a potentially relative href against a base URL.
pub fn html_resolve_href(href: &str, base_url: &str) -> String {
    if href.is_empty() { return String::new(); }
    if href.starts_with('#') { return href.to_owned(); }
    if href.contains("://") || href.starts_with("mailto:") || href.starts_with("tel:") {
        return href.to_owned();
    }
    if let Ok(base) = url::Url::parse(base_url) {
        if let Ok(resolved) = base.join(href) { return resolved.to_string(); }
    }
    href.to_owned()
}

// ---------------------------------------------------------------------------
// Tests — port of tests/lib_ffon/
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- FfonElement creation ---

    #[test]
    fn test_create_string_normal() {
        let e = FfonElement::new_str("hello");
        assert_eq!(e.as_str(), Some("hello"));
    }

    #[test]
    fn test_create_string_empty() {
        let e = FfonElement::new_str("");
        assert_eq!(e.as_str(), Some(""));
    }

    #[test]
    fn test_create_string_special_chars() {
        let e = FfonElement::new_str("<input>test</input>");
        assert_eq!(e.as_str(), Some("<input>test</input>"));
    }

    #[test]
    fn test_create_string_is_independent_copy() {
        let s = "original".to_owned();
        let e = FfonElement::new_str(&s);
        // String is cloned, not borrowed
        assert_eq!(e.as_str(), Some("original"));
    }

    #[test]
    fn test_create_object_normal() {
        let e = FfonElement::new_obj("mykey");
        let obj = e.as_obj().unwrap();
        assert_eq!(obj.key, "mykey");
        assert_eq!(obj.children.len(), 0);
    }

    #[test]
    fn test_create_object_empty_key() {
        let e = FfonElement::new_obj("");
        assert_eq!(e.as_obj().unwrap().key, "");
    }

    #[test]
    fn test_clone_string() {
        let orig = FfonElement::new_str("hello");
        let clone = orig.clone();
        assert_eq!(orig, clone);
        // Ensure they are separate values (always true for owned String)
        assert_eq!(clone.as_str(), Some("hello"));
    }

    #[test]
    fn test_clone_object_empty() {
        let orig = FfonElement::new_obj("key");
        let clone = orig.clone();
        assert_eq!(clone.as_obj().unwrap().key, "key");
        assert_eq!(clone.as_obj().unwrap().children.len(), 0);
    }

    #[test]
    fn test_clone_object_with_children() {
        let mut orig = FfonElement::new_obj("parent");
        orig.as_obj_mut().unwrap().push(FfonElement::new_str("child1"));
        orig.as_obj_mut().unwrap().push(FfonElement::new_str("child2"));

        let clone = orig.clone();
        let obj = clone.as_obj().unwrap();
        assert_eq!(obj.children.len(), 2);
        assert_eq!(obj.children[0].as_str(), Some("child1"));
        assert_eq!(obj.children[1].as_str(), Some("child2"));
    }

    #[test]
    fn test_clone_nested_object() {
        let mut root = FfonElement::new_obj("root");
        let mut child = FfonElement::new_obj("child");
        child.as_obj_mut().unwrap().push(FfonElement::new_str("leaf"));
        root.as_obj_mut().unwrap().push(child);

        let clone = root.clone();
        let cloned_child = &clone.as_obj().unwrap().children[0];
        assert_eq!(cloned_child.as_obj().unwrap().key, "child");
        assert_eq!(cloned_child.as_obj().unwrap().children[0].as_str(), Some("leaf"));
    }

    // --- FfonObject add/remove ---

    #[test]
    fn test_object_push_and_len() {
        let mut obj = FfonObject::new("k");
        obj.push(FfonElement::new_str("a"));
        obj.push(FfonElement::new_str("b"));
        assert_eq!(obj.children.len(), 2);
    }

    #[test]
    fn test_object_insert() {
        let mut obj = FfonObject::new("k");
        obj.push(FfonElement::new_str("a"));
        obj.push(FfonElement::new_str("c"));
        obj.insert(1, FfonElement::new_str("b"));
        assert_eq!(obj.children[1].as_str(), Some("b"));
        assert_eq!(obj.children.len(), 3);
    }

    #[test]
    fn test_object_remove() {
        let mut obj = FfonObject::new("k");
        obj.push(FfonElement::new_str("a"));
        obj.push(FfonElement::new_str("b"));
        let removed = obj.remove(0).unwrap();
        assert_eq!(removed.as_str(), Some("a"));
        assert_eq!(obj.children.len(), 1);
        assert_eq!(obj.children[0].as_str(), Some("b"));
    }

    #[test]
    fn test_object_remove_out_of_bounds() {
        let mut obj = FfonObject::new("k");
        assert!(obj.remove(0).is_none());
    }

    // --- IdArray ---

    #[test]
    fn test_id_array_push_pop() {
        let mut id = IdArray::new();
        id.push(2);
        id.push(5);
        assert_eq!(id.depth(), 2);
        assert_eq!(id.pop(), Some(5));
        assert_eq!(id.depth(), 1);
        assert_eq!(id.pop(), Some(2));
        assert_eq!(id.pop(), None);
    }

    #[test]
    fn test_id_array_equality() {
        let mut a = IdArray::new();
        let mut b = IdArray::new();
        a.push(1); a.push(2);
        b.push(1); b.push(2);
        assert_eq!(a, b);
        b.push(3);
        assert_ne!(a, b);
    }

    #[test]
    fn test_id_array_to_string_single() {
        let mut id = IdArray::new();
        id.push(5);
        assert_eq!(id.to_display_string(), "5");
    }

    #[test]
    fn test_id_array_to_string() {
        let mut id = IdArray::new();
        id.push(0); id.push(3); id.push(1);
        assert_eq!(id.to_display_string(), "0,3,1");
    }

    #[test]
    fn test_id_array_empty_string() {
        let id = IdArray::new();
        assert_eq!(id.to_display_string(), "");
    }

    // --- JSON serialization ---

    #[test]
    fn test_json_roundtrip_string() {
        let elems = vec![FfonElement::new_str("hello")];
        let json = to_json_string(&elems).unwrap();
        let parsed = parse_json(&json).unwrap();
        assert_eq!(parsed, elems);
    }

    #[test]
    fn test_json_roundtrip_object() {
        let mut obj = FfonElement::new_obj("Section");
        obj.as_obj_mut().unwrap().push(FfonElement::new_str("child"));
        let elems = vec![obj];
        let json = to_json_string(&elems).unwrap();
        let parsed = parse_json(&json).unwrap();
        assert_eq!(parsed, elems);
    }

    #[test]
    fn test_json_roundtrip_nested() {
        let mut root = FfonElement::new_obj("root");
        let mut nested = FfonElement::new_obj("nested");
        nested.as_obj_mut().unwrap().push(FfonElement::new_str("leaf"));
        root.as_obj_mut().unwrap().push(nested);
        let elems = vec![root];
        let json = to_json_string(&elems).unwrap();
        let parsed = parse_json(&json).unwrap();
        assert_eq!(parsed, elems);
    }

    #[test]
    fn test_json_parse_string() {
        let parsed = parse_json(r#"["hello", "world"]"#).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].as_str(), Some("hello"));
    }

    #[test]
    fn test_json_parse_object() {
        let parsed = parse_json(r#"[{"Section": ["child1", "child2"]}]"#).unwrap();
        assert_eq!(parsed.len(), 1);
        let obj = parsed[0].as_obj().unwrap();
        assert_eq!(obj.key, "Section");
        assert_eq!(obj.children.len(), 2);
    }

    #[test]
    fn test_json_parse_bool_becomes_string() {
        let parsed = parse_json(r#"[true, false]"#).unwrap();
        assert_eq!(parsed[0].as_str(), Some("true"));
        assert_eq!(parsed[1].as_str(), Some("false"));
    }

    #[test]
    fn test_json_parse_null_becomes_string() {
        let parsed = parse_json("[null]").unwrap();
        assert_eq!(parsed[0].as_str(), Some("null"));
    }

    // --- Binary serialization ---

    #[test]
    fn test_binary_roundtrip_strings() {
        let elems = vec![
            FfonElement::new_str("hello"),
            FfonElement::new_str("world"),
        ];
        let data = serialize_binary(&elems);
        let back = deserialize_binary(&data);
        assert_eq!(back, elems);
    }

    #[test]
    fn test_binary_roundtrip_object_with_children() {
        let mut obj = FfonElement::new_obj("Section");
        obj.as_obj_mut().unwrap().push(FfonElement::new_str("child1"));
        obj.as_obj_mut().unwrap().push(FfonElement::new_str("child2"));
        let elems = vec![obj];
        let data = serialize_binary(&elems);
        let back = deserialize_binary(&data);
        assert_eq!(back, elems);
    }

    #[test]
    fn test_binary_roundtrip_nested() {
        let mut root = FfonElement::new_obj("root");
        let mut child = FfonElement::new_obj("child");
        child.as_obj_mut().unwrap().push(FfonElement::new_str("leaf"));
        root.as_obj_mut().unwrap().push(child);
        root.as_obj_mut().unwrap().push(FfonElement::new_str("sibling"));
        let elems = vec![root];
        let data = serialize_binary(&elems);
        let back = deserialize_binary(&data);
        assert_eq!(back, elems);
    }

    #[test]
    fn test_binary_empty_input() {
        let back = deserialize_binary(&[]);
        assert!(back.is_empty());
    }

    #[test]
    fn test_binary_file_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.ffon");
        let elems = vec![
            FfonElement::new_str("a"),
            FfonElement::new_obj("B"),
        ];
        save_ffon_file(&elems, &path).unwrap();
        let back = load_ffon_file(&path).unwrap();
        assert_eq!(back, elems);
    }

    // --- Navigation ---

    fn make_tree() -> Vec<FfonElement> {
        let mut root1 = FfonElement::new_obj("Section A");
        root1.as_obj_mut().unwrap().push(FfonElement::new_str("item 0"));
        root1.as_obj_mut().unwrap().push(FfonElement::new_str("item 1"));

        let mut root2 = FfonElement::new_obj("Section B");
        let mut nested = FfonElement::new_obj("Nested");
        nested.as_obj_mut().unwrap().push(FfonElement::new_str("deep"));
        root2.as_obj_mut().unwrap().push(nested);

        vec![root1, root2]
    }

    #[test]
    fn test_get_ffon_at_id_root() {
        let tree = make_tree();
        let id = IdArray::new();
        let slice = get_ffon_at_id(&tree, &id).unwrap();
        assert_eq!(slice.len(), 2);
    }

    #[test]
    fn test_get_ffon_at_id_first_level() {
        let tree = make_tree();
        let mut id = IdArray::new();
        id.push(0);
        let slice = get_ffon_at_id(&tree, &id).unwrap();
        assert_eq!(slice.len(), 2); // Section A's parent, not children
        // get_ffon_at_id with depth=1 returns the parent (root), not the children
        // The last index selects within the returned slice.
        // This matches the C semantics.
    }

    #[test]
    fn test_next_layer_exists_object() {
        let tree = make_tree();
        let mut id = IdArray::new();
        id.push(0); // Section A is an object
        assert!(next_layer_exists(&tree, &id));
    }

    #[test]
    fn test_next_layer_exists_string() {
        let tree = make_tree();
        // Navigate into Section A, then select "item 0"
        let mut id = IdArray::new();
        id.push(0); // Section A
        let children = tree[0].as_obj().unwrap().children.as_slice();
        let mut child_id = IdArray::new();
        child_id.push(0);
        // "item 0" is a string — not navigable
        assert!(!next_layer_exists(children, &child_id));
    }

    #[test]
    fn test_get_ffon_max_id() {
        let tree = make_tree();
        let id = IdArray::new();
        assert_eq!(get_ffon_max_id(&tree, &id), 1); // two items, max index = 1
    }

    #[test]
    fn test_get_ffon_at_id_out_of_bounds() {
        let tree = make_tree();
        let mut id = IdArray::new();
        id.push(99); // out of bounds
        id.push(0);
        assert!(get_ffon_at_id(&tree, &id).is_none());
    }

    // ---------------------------------------------------------------------------
    // Additional IdArray tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_idarray_new_depth_zero() {
        let id = IdArray::new();
        assert_eq!(id.depth(), 0);
    }

    #[test]
    fn test_idarray_clone_populated() {
        let mut src = IdArray::new();
        src.push(1); src.push(2); src.push(3);
        let dst = src.clone();
        assert_eq!(dst.depth(), 3);
        assert_eq!(dst.get(0), Some(1));
        assert_eq!(dst.get(1), Some(2));
        assert_eq!(dst.get(2), Some(3));
    }

    #[test]
    fn test_idarray_clone_empty() {
        let src = IdArray::new();
        let dst = src.clone();
        assert_eq!(dst.depth(), 0);
    }

    #[test]
    fn test_idarray_clone_is_independent() {
        let mut src = IdArray::new();
        src.push(5);
        let dst = src.clone();
        src.push(10);
        assert_eq!(dst.depth(), 1); // dst unaffected by push on src
    }

    #[test]
    fn test_idarray_equal_both_empty() {
        let a = IdArray::new();
        let b = IdArray::new();
        assert_eq!(a, b);
    }

    #[test]
    fn test_idarray_equal_different_depth() {
        let mut a = IdArray::new();
        let b = IdArray::new();
        a.push(1);
        assert_ne!(a, b);
    }

    #[test]
    fn test_idarray_equal_different_values() {
        let mut a = IdArray::new();
        let mut b = IdArray::new();
        a.push(1);
        b.push(2);
        assert_ne!(a, b);
    }

    #[test]
    fn test_idarray_push_increments_depth() {
        let mut arr = IdArray::new();
        arr.push(42);
        assert_eq!(arr.depth(), 1);
        assert_eq!(arr.get(0), Some(42));
    }

    #[test]
    fn test_idarray_push_multiple() {
        let mut arr = IdArray::new();
        arr.push(10); arr.push(20); arr.push(30);
        assert_eq!(arr.depth(), 3);
        assert_eq!(arr.get(0), Some(10));
        assert_eq!(arr.get(1), Some(20));
        assert_eq!(arr.get(2), Some(30));
    }

    #[test]
    fn test_idarray_pop_returns_value() {
        let mut arr = IdArray::new();
        arr.push(5); arr.push(10);
        assert_eq!(arr.pop(), Some(10));
        assert_eq!(arr.depth(), 1);
    }

    #[test]
    fn test_idarray_pop_empty_returns_none() {
        let mut arr = IdArray::new();
        assert_eq!(arr.pop(), None);
        assert_eq!(arr.depth(), 0);
    }

    #[test]
    fn test_idarray_pop_all() {
        let mut arr = IdArray::new();
        arr.push(1); arr.push(2);
        assert_eq!(arr.pop(), Some(2));
        assert_eq!(arr.pop(), Some(1));
        assert_eq!(arr.pop(), None);
    }

    #[test]
    fn test_idarray_tostring_single() {
        let mut id = IdArray::new();
        id.push(42);
        assert_eq!(id.to_display_string(), "42");
    }

    #[test]
    fn test_idarray_tostring_multiple() {
        let mut id = IdArray::new();
        id.push(1); id.push(2); id.push(3);
        assert_eq!(id.to_display_string(), "1,2,3");
    }

    // ---------------------------------------------------------------------------
    // Additional FfonObject tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_object_new_normal() {
        let obj = FfonObject::new("testkey");
        assert_eq!(obj.key, "testkey");
        assert_eq!(obj.children.len(), 0);
    }

    #[test]
    fn test_object_new_empty_key() {
        let obj = FfonObject::new("");
        assert_eq!(obj.key, "");
        assert_eq!(obj.children.len(), 0);
    }

    #[test]
    fn test_object_add_multiple() {
        let mut obj = FfonObject::new("k");
        for i in 0..5 {
            obj.push(FfonElement::new_str(format!("item{i}")));
        }
        assert_eq!(obj.children.len(), 5);
        assert_eq!(obj.children[0].as_str(), Some("item0"));
        assert_eq!(obj.children[4].as_str(), Some("item4"));
    }

    #[test]
    fn test_object_insert_at_end() {
        let mut obj = FfonObject::new("k");
        obj.push(FfonElement::new_str("a"));
        obj.insert(1, FfonElement::new_str("b")); // index == len → appended
        assert_eq!(obj.children.len(), 2);
        assert_eq!(obj.children[0].as_str(), Some("a"));
        assert_eq!(obj.children[1].as_str(), Some("b"));
    }

    #[test]
    fn test_object_insert_beyond_count_clamped() {
        let mut obj = FfonObject::new("k");
        obj.push(FfonElement::new_str("a"));
        obj.insert(100, FfonElement::new_str("b")); // clamped to len → appended
        assert_eq!(obj.children.len(), 2);
        assert_eq!(obj.children[0].as_str(), Some("a"));
        assert_eq!(obj.children[1].as_str(), Some("b"));
    }

    #[test]
    fn test_object_remove_middle() {
        let mut obj = FfonObject::new("k");
        obj.push(FfonElement::new_str("a"));
        obj.push(FfonElement::new_str("b"));
        obj.push(FfonElement::new_str("c"));
        let removed = obj.remove(1).unwrap();
        assert_eq!(removed.as_str(), Some("b"));
        assert_eq!(obj.children.len(), 2);
        assert_eq!(obj.children[0].as_str(), Some("a"));
        assert_eq!(obj.children[1].as_str(), Some("c"));
    }

    #[test]
    fn test_object_remove_last() {
        let mut obj = FfonObject::new("k");
        obj.push(FfonElement::new_str("a"));
        obj.push(FfonElement::new_str("b"));
        let removed = obj.remove(1).unwrap();
        assert_eq!(removed.as_str(), Some("b"));
        assert_eq!(obj.children.len(), 1);
        assert_eq!(obj.children[0].as_str(), Some("a"));
    }

    // ---------------------------------------------------------------------------
    // Additional navigation tests (using C test tree structure)
    // ---------------------------------------------------------------------------

    /// Build the same tree used in C navigation tests:
    ///   [0] "string0"
    ///   [1] obj "parent" → [0]"child0", [1]"child1", [2] obj "nested" → [0]"leaf"
    ///   [2] "string2"
    fn make_nav_tree() -> Vec<FfonElement> {
        let mut parent = FfonElement::new_obj("parent");
        parent.as_obj_mut().unwrap().push(FfonElement::new_str("child0"));
        parent.as_obj_mut().unwrap().push(FfonElement::new_str("child1"));
        let mut nested = FfonElement::new_obj("nested");
        nested.as_obj_mut().unwrap().push(FfonElement::new_str("leaf"));
        parent.as_obj_mut().unwrap().push(nested);

        vec![
            FfonElement::new_str("string0"),
            parent,
            FfonElement::new_str("string2"),
        ]
    }

    #[test]
    fn test_get_ffon_at_id_depth_two() {
        let tree = make_nav_tree();
        let mut id = IdArray::new();
        id.push(1); id.push(2); // into "parent", look at "nested"
        let slice = get_ffon_at_id(&tree, &id).unwrap();
        // depth=2: walked [1] → parent's children (3 children)
        assert_eq!(slice.len(), 3); // parent has 3 children
    }

    #[test]
    fn test_get_ffon_at_id_depth_three() {
        let tree = make_nav_tree();
        let mut id = IdArray::new();
        id.push(1); id.push(2); id.push(0); // into parent→nested, look at "leaf"
        let slice = get_ffon_at_id(&tree, &id).unwrap();
        // depth=3: walked [1]→parent, [2]→nested's children (1 child)
        assert_eq!(slice.len(), 1); // nested has 1 child
    }

    #[test]
    fn test_get_ffon_at_id_non_object_at_path() {
        let tree = make_nav_tree();
        let mut id = IdArray::new();
        id.push(0); id.push(0); // string0 is not an object — can't walk into it
        assert!(get_ffon_at_id(&tree, &id).is_none());
    }

    #[test]
    fn test_next_layer_exists_empty_id() {
        let tree = make_nav_tree();
        let id = IdArray::new();
        assert!(!next_layer_exists(&tree, &id));
    }

    #[test]
    fn test_next_layer_exists_out_of_bounds() {
        let tree = make_nav_tree();
        let mut id = IdArray::new();
        id.push(99);
        assert!(!next_layer_exists(&tree, &id));
    }

    #[test]
    fn test_next_layer_exists_nested_object() {
        let tree = make_nav_tree();
        let mut id = IdArray::new();
        id.push(1); id.push(2); // parent[2] = "nested" (an object)
        assert!(next_layer_exists(&tree[1].as_obj().unwrap().children, &{
            let mut child_id = IdArray::new(); child_id.push(2); child_id
        }));
    }

    #[test]
    fn test_next_layer_exists_nested_string() {
        let tree = make_nav_tree();
        let children = &tree[1].as_obj().unwrap().children;
        let mut id = IdArray::new();
        id.push(0); // children[0] = "child0" (a string)
        assert!(!next_layer_exists(children, &id));
    }

    #[test]
    fn test_get_ffon_max_id_nested() {
        let tree = make_nav_tree();
        let mut id = IdArray::new();
        id.push(1); // into "parent" which has 3 children
        assert_eq!(get_ffon_max_id(&tree, &id), 2); // max index = 2
    }

    #[test]
    fn test_get_ffon_max_id_deep_nested() {
        let tree = make_nav_tree();
        let mut id = IdArray::new();
        id.push(1); id.push(2); id.push(0); // nested has 1 child → max index = 0
        assert_eq!(get_ffon_max_id(&tree, &id), 0);
    }

    // ---------------------------------------------------------------------------
    // Additional serialization tests
    // ---------------------------------------------------------------------------

    #[test]
    fn test_load_ffon_file_nonexistent() {
        let result = load_ffon_file(Path::new("/nonexistent/path.ffon"));
        assert!(result.is_err());
    }

    #[test]
    fn test_json_parse_integer_becomes_string() {
        let parsed = parse_json("[42]").unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].as_str(), Some("42"));
    }

    #[test]
    fn test_json_parse_nested_array_becomes_object() {
        // An inner JSON array becomes FfonElement::Obj{key:"array", ...}
        let parsed = parse_json(r#"[["x","y"]]"#).unwrap();
        assert_eq!(parsed.len(), 1);
        let obj = parsed[0].as_obj().unwrap();
        assert_eq!(obj.key, "array");
        assert_eq!(obj.children.len(), 2);
        assert_eq!(obj.children[0].as_str(), Some("x"));
        assert_eq!(obj.children[1].as_str(), Some("y"));
    }

    #[test]
    fn test_load_json_file_valid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.json");
        std::fs::write(&path, r#"["item1", {"key": ["child"]}]"#).unwrap();
        let result = load_json_file(&path).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].as_str(), Some("item1"));
        let obj = result[1].as_obj().unwrap();
        assert_eq!(obj.key, "key");
        assert_eq!(obj.children.len(), 1);
        assert_eq!(obj.children[0].as_str(), Some("child"));
    }

    #[test]
    fn test_load_json_file_nonexistent() {
        let result = load_json_file(Path::new("/nonexistent/file.json"));
        assert!(result.is_err());
    }

    #[test]
    fn test_json_serialize_string_element() {
        let elems = vec![FfonElement::new_str("hello world")];
        let json = to_json_string(&elems).unwrap();
        assert!(json.contains("hello world"));
        // Verify it round-trips
        let back = parse_json(&json).unwrap();
        assert_eq!(back, elems);
    }

    #[test]
    fn test_json_serialize_object_element() {
        let mut elem = FfonElement::new_obj("mykey");
        elem.as_obj_mut().unwrap().push(FfonElement::new_str("child1"));
        elem.as_obj_mut().unwrap().push(FfonElement::new_str("child2"));
        let json = to_json_string(&[elem.clone()]).unwrap();
        assert!(json.contains("mykey"));
        // Round-trip
        let back = parse_json(&json).unwrap();
        assert_eq!(back, vec![elem]);
    }

    #[test]
    fn test_json_roundtrip_with_tags() {
        // Tags like <input>...</input> and <radio> must survive JSON round-trip
        let elems = vec![
            FfonElement::new_str("<radio>lang"),
            FfonElement::new_str("<input>test</input>"),
        ];
        let json = to_json_string(&elems).unwrap();
        let back = parse_json(&json).unwrap();
        assert_eq!(back, elems);
    }

    // ---------------------------------------------------------------------------
    // Additional element / object / idarray tests (port of remaining C tests)
    // ---------------------------------------------------------------------------

    #[test]
    fn test_create_string_null_input_is_empty() {
        // C: ffonElementCreateString(NULL) → string with ""
        // Rust equivalent: new_str("") works identically
        let e = FfonElement::new_str("");
        assert_eq!(e.as_str(), Some(""));
    }

    #[test]
    fn test_create_object_null_key_is_empty() {
        // C: ffonElementCreateObject(NULL) → Obj with key ""
        let e = FfonElement::new_obj("");
        assert_eq!(e.as_obj().unwrap().key, "");
        assert_eq!(e.as_obj().unwrap().children.len(), 0);
    }

    #[test]
    fn test_object_insert_at_beginning() {
        let mut obj = FfonObject::new("k");
        obj.push(FfonElement::new_str("b"));
        obj.push(FfonElement::new_str("c"));
        obj.insert(0, FfonElement::new_str("a"));
        assert_eq!(obj.children.len(), 3);
        assert_eq!(obj.children[0].as_str(), Some("a"));
        assert_eq!(obj.children[1].as_str(), Some("b"));
        assert_eq!(obj.children[2].as_str(), Some("c"));
    }

    #[test]
    fn test_object_insert_at_middle() {
        let mut obj = FfonObject::new("k");
        obj.push(FfonElement::new_str("a"));
        obj.push(FfonElement::new_str("c"));
        obj.insert(1, FfonElement::new_str("b"));
        assert_eq!(obj.children.len(), 3);
        assert_eq!(obj.children[0].as_str(), Some("a"));
        assert_eq!(obj.children[1].as_str(), Some("b"));
        assert_eq!(obj.children[2].as_str(), Some("c"));
    }

    #[test]
    fn test_object_add_triggers_capacity_growth() {
        // Verify that adding many elements (>10) works correctly (Vec grows as needed)
        let mut obj = FfonObject::new("k");
        for i in 0..15 {
            obj.push(FfonElement::new_str(format!("item{i}")));
        }
        assert_eq!(obj.children.len(), 15);
        assert_eq!(obj.children[14].as_str(), Some("item14"));
    }

    #[test]
    fn test_object_remove_first() {
        let mut obj = FfonObject::new("k");
        obj.push(FfonElement::new_str("a"));
        obj.push(FfonElement::new_str("b"));
        obj.push(FfonElement::new_str("c"));
        let removed = obj.remove(0).unwrap();
        assert_eq!(removed.as_str(), Some("a"));
        assert_eq!(obj.children.len(), 2);
        assert_eq!(obj.children[0].as_str(), Some("b"));
        assert_eq!(obj.children[1].as_str(), Some("c"));
    }

    #[test]
    fn test_idarray_push_grows_unbounded() {
        // C IdArray had MAX_ID_DEPTH=32; Rust IdArray is Vec-backed (no limit).
        // Verify many pushes work correctly.
        let mut id = IdArray::new();
        for i in 0..32 {
            id.push(i);
        }
        assert_eq!(id.depth(), 32);
        id.push(100); // one more — Rust allows it
        assert_eq!(id.depth(), 33);
        assert_eq!(id.get(32), Some(100));
    }

    #[test]
    fn test_idarray_new_is_empty() {
        // C: idArrayInit sets depth to 0 and zeros all ids.
        // Rust: new() returns an empty Vec.
        let id = IdArray::new();
        assert_eq!(id.depth(), 0);
        assert_eq!(id.last(), None);
        assert_eq!(id.get(0), None);
    }

    #[test]
    fn test_save_load_json_file_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("roundtrip.json");
        let elems = vec![
            FfonElement::new_str("version"),
            {
                let mut obj = FfonElement::new_obj("settings");
                obj.as_obj_mut().unwrap().push(FfonElement::new_str("<radio>lang"));
                obj.as_obj_mut().unwrap().push(FfonElement::new_str("English"));
                obj
            },
        ];
        save_json_file(&elems, &path).unwrap();
        let loaded = load_json_file(&path).unwrap();
        assert_eq!(loaded, elems);
    }

    // Deeply-nested JSON roundtrip — two levels with tagged content
    #[test]
    fn test_json_nested_two_levels_with_tags() {
        // [{"project": [{"id": ["<input>test</input>"]}]}]
        let mut inner = FfonElement::new_obj("id");
        inner.as_obj_mut().unwrap().push(FfonElement::new_str("<input>test</input>"));
        let mut outer = FfonElement::new_obj("project");
        outer.as_obj_mut().unwrap().push(inner);
        let elems = vec![outer];
        let json = to_json_string(&elems).unwrap();
        let parsed = parse_json(&json).unwrap();
        assert_eq!(parsed, elems);
    }

    // Object insert: verify negative conceptual index (0-based) clamps to front
    #[test]
    fn test_object_insert_at_zero_goes_to_front() {
        let mut obj = FfonElement::new_obj("key");
        obj.as_obj_mut().unwrap().push(FfonElement::new_str("b"));
        obj.as_obj_mut().unwrap().insert(0, FfonElement::new_str("a"));
        let children = &obj.as_obj().unwrap().children;
        assert_eq!(children[0].as_str(), Some("a"));
        assert_eq!(children[1].as_str(), Some("b"));
    }

    // JSON serialization of an empty element list
    #[test]
    fn test_json_serialize_empty_element_list() {
        let elems: Vec<FfonElement> = vec![];
        let json = to_json_string(&elems).unwrap();
        let parsed = parse_json(&json).unwrap();
        assert!(parsed.is_empty());
    }

    // IdArray: push multiple and verify depth
    #[test]
    fn test_idarray_push_multiple_increments_depth() {
        let mut id = IdArray::new();
        id.push(0);
        id.push(1);
        id.push(2);
        assert_eq!(id.depth(), 3);
        assert_eq!(id.get(0), Some(0));
        assert_eq!(id.get(1), Some(1));
        assert_eq!(id.get(2), Some(2));
    }

    #[test]
    fn test_object_add_one_element_count_is_one() {
        let mut obj = FfonObject { key: "root".to_string(), children: vec![] };
        obj.push(FfonElement::new_str("hello"));
        assert_eq!(obj.children.len(), 1);
    }

    // --- is_ffon ---

    #[test]
    fn test_is_ffon_empty_array() {
        let v = serde_json::json!([]);
        assert!(is_ffon(&v));
    }

    #[test]
    fn test_is_ffon_flat_strings() {
        let v = serde_json::json!(["hello", "world"]);
        assert!(is_ffon(&v));
    }

    #[test]
    fn test_is_ffon_single_key_object() {
        let v = serde_json::json!([{"section": ["child1", "child2"]}]);
        assert!(is_ffon(&v));
    }

    #[test]
    fn test_is_ffon_nested() {
        let v = serde_json::json!([{"k": [{"k2": ["leaf"]}]}]);
        assert!(is_ffon(&v));
    }

    #[test]
    fn test_is_ffon_rejects_non_array_root() {
        assert!(!is_ffon(&serde_json::json!({"key": "value"})));
        assert!(!is_ffon(&serde_json::json!("string")));
        assert!(!is_ffon(&serde_json::json!(42)));
        assert!(!is_ffon(&serde_json::json!(null)));
    }

    #[test]
    fn test_is_ffon_rejects_number_element() {
        assert!(!is_ffon(&serde_json::json!([1, 2, 3])));
    }

    #[test]
    fn test_is_ffon_rejects_bool_element() {
        assert!(!is_ffon(&serde_json::json!([true])));
    }

    #[test]
    fn test_is_ffon_rejects_null_element() {
        assert!(!is_ffon(&serde_json::json!([null])));
    }

    #[test]
    fn test_is_ffon_rejects_bare_nested_array() {
        // A bare array as an element (not wrapped in a single-key object) is not FFON.
        assert!(!is_ffon(&serde_json::json!([["nested"]])));
    }

    #[test]
    fn test_is_ffon_rejects_multi_key_object() {
        assert!(!is_ffon(&serde_json::json!([{"a": [], "b": []}])));
    }

    #[test]
    fn test_is_ffon_rejects_object_with_non_array_value() {
        assert!(!is_ffon(&serde_json::json!([{"a": "string_value"}])));
    }

    #[test]
    fn test_is_ffon_rejects_deeply_invalid() {
        // Valid outer shape but invalid leaf (number inside nested array)
        assert!(!is_ffon(&serde_json::json!([{"k": [42]}])));
    }

    #[test]
    fn test_is_ffon_sf_json() {
        // Matches the structure of lib/lib_tutorial/assets/sf.json
        let v = serde_json::json!([
            {"0 é": ["0, 0", {"0, 1": ["0, 1, 0", "0, 1, 1", "0, 1, 2"]}]},
            "1"
        ]);
        assert!(is_ffon(&v));
    }

    #[test]
    fn test_is_ffon_rejects_plugin_manifest() {
        // sdk/examples/c/plugin.json style: root is an object, not an array
        let v = serde_json::json!({"name": "my-plugin", "type": "script", "entry": "plugin.sh"});
        assert!(!is_ffon(&v));
    }

    // --- html_to_ffon tests ---

    #[test]
    fn html_plain_paragraph() {
        let elems = html_to_ffon("<p>Hello world</p>", "");
        assert_eq!(elems.len(), 1);
        assert!(matches!(&elems[0], FfonElement::Str(s) if s == "Hello world"));
    }

    #[test]
    fn html_heading_groups_children() {
        let elems = html_to_ffon("<h2>Section</h2><p>content</p>", "");
        assert_eq!(elems.len(), 1);
        let obj = elems[0].as_obj().unwrap();
        assert_eq!(obj.key, "Section");
        assert_eq!(obj.children.len(), 1);
    }

    #[test]
    fn html_br_becomes_empty_str() {
        let elems = html_to_ffon("<p>line1</p><br><p>line2</p>", "");
        assert!(elems.len() >= 2);
    }

    #[test]
    fn html_fragment_without_body() {
        let elems = html_to_ffon("<p>Fragment</p>", "");
        assert!(!elems.is_empty());
    }

    #[test]
    fn html_link_in_paragraph() {
        let elems = html_to_ffon(r#"<p>See <a href="https://example.com">here</a></p>"#, "");
        let found = elems.iter().any(|e| match e {
            FfonElement::Obj(o) => o.key.contains("<link>https://example.com</link>"),
            _ => false,
        });
        assert!(found, "expected link obj in {elems:?}");
    }

    // ---- Form parsing tests ------------------------------------------------

    #[test]
    fn html_form_becomes_form_obj() {
        let html = r#"<form><input type="text" name="q" placeholder="Search"></form>"#;
        let elems = html_to_ffon(html, "");
        assert_eq!(elems.len(), 1, "expected one top-level form obj");
        let form = elems[0].as_obj().expect("expected Obj for form");
        assert_eq!(form.key, "form_1");
        assert_eq!(form.children.len(), 1);
        let field = form.children[0].as_str().unwrap();
        assert!(field.contains("<input>"), "expected editable cell: {field}");
        assert!(field.starts_with("Search: "), "expected placeholder label: {field}");
    }

    #[test]
    fn html_form_text_input_uses_name_fallback() {
        let html = r#"<form><input type="text" name="username"></form>"#;
        let (elems, map) = html_to_ffon_with_forms(html, "");
        let form = elems[0].as_obj().unwrap();
        let field = form.children[0].as_str().unwrap();
        assert!(field.starts_with("username: "), "got: {field}");
        assert!(map.contains_key("form_1/username"), "missing key in form_map: {map:?}");
        let node = &map["form_1/username"];
        assert_eq!(node.kind, FormNodeKind::TextInput);
    }

    #[test]
    fn html_form_email_input() {
        let html = r#"<form><input type="email" name="email" value="user@example.com"></form>"#;
        let (elems, _) = html_to_ffon_with_forms(html, "");
        let field = elems[0].as_obj().unwrap().children[0].as_str().unwrap();
        assert_eq!(field, "email: <input>user@example.com</input>");
    }

    #[test]
    fn html_form_submit_button_renders_as_button_tag() {
        let html = r#"<form><input type="submit" value="Sign in"></form>"#;
        let elems = html_to_ffon(html, "");
        let btn = elems[0].as_obj().unwrap().children[0].as_str().unwrap();
        assert!(btn.contains("<button>submit:form_1</button>"), "got: {btn}");
        assert!(btn.contains("Sign in"), "got: {btn}");
    }

    #[test]
    fn html_form_standalone_button_element() {
        let html = r#"<form><button type="submit">Go</button></form>"#;
        let elems = html_to_ffon(html, "");
        let btn = elems[0].as_obj().unwrap().children[0].as_str().unwrap();
        assert!(btn.contains("<button>submit:form_1</button>"), "got: {btn}");
        assert!(btn.ends_with("Go"), "got: {btn}");
    }

    #[test]
    fn html_form_checkbox_unchecked() {
        let html = r#"<form><input type="checkbox" name="remember">Remember me</form>"#;
        let elems = html_to_ffon(html, "");
        let children = &elems[0].as_obj().unwrap().children;
        let found = children.iter().any(|e| {
            e.as_str().map_or(false, |s| s.starts_with("<checkbox>"))
        });
        assert!(found, "expected <checkbox> element; children: {children:?}");
    }

    #[test]
    fn html_form_checkbox_checked() {
        let html = r#"<form><input type="checkbox" name="agree" checked></form>"#;
        let elems = html_to_ffon(html, "");
        let children = &elems[0].as_obj().unwrap().children;
        let found = children.iter().any(|e| {
            e.as_str().map_or(false, |s| s.starts_with("<checkbox checked>"))
        });
        assert!(found, "expected <checkbox checked>; children: {children:?}");
    }

    #[test]
    fn html_form_select_becomes_radio_obj() {
        let html = r#"<form><select name="country">
            <option value="us">United States</option>
            <option value="ca" selected>Canada</option>
        </select></form>"#;
        let elems = html_to_ffon(html, "");
        let form_children = &elems[0].as_obj().unwrap().children;
        let radio = form_children.iter().find(|e| {
            e.as_obj().map_or(false, |o| o.key.starts_with("<radio>"))
        }).expect("expected <radio> obj for select");
        let opts = &radio.as_obj().unwrap().children;
        assert_eq!(opts.len(), 2);
        let canada = opts.iter().find(|e| {
            e.as_str().map_or(false, |s| s.contains("Canada"))
        }).expect("Canada option missing");
        assert!(canada.as_str().unwrap().starts_with("<checked>"),
            "selected option should use <checked> tag: {canada:?}");
    }

    #[test]
    fn html_form_textarea() {
        let html = r#"<form><textarea name="message" placeholder="Your message"></textarea></form>"#;
        let (elems, map) = html_to_ffon_with_forms(html, "");
        let field = elems[0].as_obj().unwrap().children[0].as_str().unwrap();
        assert!(field.starts_with("Your message: <input>"), "got: {field}");
        assert!(map.contains_key("form_1/Your message"));
        assert_eq!(map["form_1/Your message"].kind, FormNodeKind::Textarea);
    }

    #[test]
    fn html_two_forms_numbered_separately() {
        let html = r#"
            <form><input type="text" name="q"></form>
            <form><input type="email" name="email"></form>
        "#;
        let elems = html_to_ffon(html, "");
        let forms: Vec<_> = elems.iter().filter(|e| e.is_obj()).collect();
        assert_eq!(forms.len(), 2);
        assert_eq!(forms[0].as_obj().unwrap().key, "form_1");
        assert_eq!(forms[1].as_obj().unwrap().key, "form_2");
    }

    #[test]
    fn html_form_hidden_and_file_inputs_skipped() {
        let html = r#"<form>
            <input type="hidden" name="csrf" value="token">
            <input type="file" name="upload">
            <input type="text" name="visible">
        </form>"#;
        let elems = html_to_ffon(html, "");
        let children = &elems[0].as_obj().unwrap().children;
        assert_eq!(children.len(), 1, "only visible input expected; got {children:?}");
        assert!(children[0].as_str().unwrap().contains("visible"));
    }

    #[test]
    fn html_form_map_css_selector_by_name() {
        let html = r#"<form><input type="email" name="email"></form>"#;
        let (_, map) = html_to_ffon_with_forms(html, "");
        let node = &map["form_1/email"];
        assert!(node.css_selector.contains("name=\"email\""), "got: {}", node.css_selector);
    }

    #[test]
    fn html_form_map_css_selector_prefers_id() {
        let html = r#"<form><input type="text" id="search-box" name="q"></form>"#;
        let (_, map) = html_to_ffon_with_forms(html, "");
        // label derived from name "q" (no placeholder), so key is form_1/q
        let node = &map["form_1/q"];
        assert_eq!(node.css_selector, "#search-box");
    }

    #[test]
    fn html_label_wrapping_input_exposes_input() {
        // <label> is in HTML_CONTAINER_TAGS, so its children are processed normally.
        let html = r#"<form><label>Email <input type="email" name="email"></label></form>"#;
        let (elems, map) = html_to_ffon_with_forms(html, "");
        let form_children = &elems[0].as_obj().unwrap().children;
        let has_input = form_children.iter().any(|e| {
            e.as_str().map_or(false, |s| s.contains("<input>"))
        });
        assert!(has_input, "labeled input not found in form children: {form_children:?}");
        assert!(map.contains_key("form_1/email"), "form_map missing labeled input key");
    }

    #[test]
    fn html_form_input_with_id_no_spurious_id_prefix() {
        // An input with an id= attribute must NOT get a <id>X</id> prefix in its
        // FFON string. The pending_id mechanism must be suppressed for form controls.
        let html = r#"<form><input type="text" id="search-input" name="q"></form>"#;
        let (elems, map) = html_to_ffon_with_forms(html, "");
        let field = elems[0].as_obj().unwrap().children[0].as_str().unwrap();
        assert!(!field.contains("<id>"), "spurious <id> tag in form field: {field}");
        assert!(field.starts_with("q: "), "expected name-derived label: {field}");
        assert!(map.contains_key("form_1/q"), "form_map key must match bare label");
    }

    #[test]
    fn html_form_input_id_only_no_name_no_spurious_prefix() {
        // Input with only an id (no name/placeholder): label = id, still no <id> tag prefix.
        let html = r#"<form><input type="text" id="email-field"></form>"#;
        let (elems, map) = html_to_ffon_with_forms(html, "");
        let field = elems[0].as_obj().unwrap().children[0].as_str().unwrap();
        assert!(!field.contains("<id>"), "spurious <id> tag in form field: {field}");
        assert!(field.starts_with("email-field: "), "expected id-derived label: {field}");
        assert!(map.contains_key("form_1/email-field"), "form_map key must match bare id");
    }
}
