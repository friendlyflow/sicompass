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

/// Serialize a list of FFON elements to a JSON string.
pub fn to_json_string(elements: &[FfonElement]) -> Result<String, serde_json::Error> {
    serde_json::to_string(elements)
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
}
