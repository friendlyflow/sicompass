//! Dynamic plugin loading — equivalent to the `PLUGIN_NATIVE` and
//! `PLUGIN_SCRIPT` branches in `src/sicompass/programs.c`.
//!
//! ## Native plugins
//!
//! A native plugin is a shared library that exports a single C-ABI function:
//!
//! ```c
//! const ProviderOpsC* sicompass_plugin_init(void);
//! ```
//!
//! The returned `ProviderOpsC` is a `#[repr(C)]` vtable struct mirroring the
//! SDK's `ProviderOps`.  [`NativePlugin`] wraps the open library handle and
//! delegates all [`Provider`] calls to the vtable.
//!
//! ## Script providers
//!
//! A script provider is a TypeScript/JavaScript file executed via `bun run`.
//! The script receives subcommands on `argv`: `fetch <path>`,
//! `commit <path> <old> <new>`, `getcommands`, etc.  JSON is written to
//! stdout.  [`ScriptProvider`] implements [`Provider`] by spawning the
//! interpreter and parsing the output.

use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::provider::Provider;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// C ABI types  (mirror of sdk/include/ffon.h + provider_interface.h)
// ---------------------------------------------------------------------------

/// Mirror of C's anonymous `enum { FFON_STRING, FFON_OBJECT }`.
#[repr(u32)]
#[allow(dead_code)]
enum FfonTypeC {
    String = 0,
    Object = 1,
}

/// Mirror of C's `FfonObject`.
#[repr(C)]
struct FfonObjectC {
    key: *mut c_char,
    elements: *mut *mut FfonElementC,
    count: c_int,
    _capacity: c_int,
}

/// Mirror of C's `FfonElement`.
#[repr(C)]
struct FfonElementC {
    element_type: u32,
    // union { char *string; FfonObject *object; } data
    data: *mut std::ffi::c_void,
}

/// Convert a `*mut *mut FfonElementC` array into a Rust `Vec<FfonElement>`.
///
/// # Safety
/// `ptr` must be a valid pointer to `count` consecutive `*mut FfonElementC`
/// pointers, each individually valid (or null, which is skipped).
unsafe fn c_elements_to_rust(ptr: *mut *mut FfonElementC, count: c_int) -> Vec<FfonElement> {
    if ptr.is_null() || count <= 0 {
        return Vec::new();
    }
    let slice = unsafe { std::slice::from_raw_parts(ptr, count as usize) };
    let mut out = Vec::with_capacity(count as usize);
    for &elem_ptr in slice {
        if elem_ptr.is_null() {
            continue;
        }
        let elem = unsafe { &*elem_ptr };
        if elem.element_type == FfonTypeC::Object as u32 {
            let obj_ptr = elem.data as *mut FfonObjectC;
            if obj_ptr.is_null() {
                continue;
            }
            let obj = unsafe { &*obj_ptr };
            let key = if obj.key.is_null() {
                String::new()
            } else {
                unsafe { CStr::from_ptr(obj.key) }
                    .to_string_lossy()
                    .into_owned()
            };
            let children = unsafe { c_elements_to_rust(obj.elements, obj.count) };
            let mut rust_obj = FfonElement::new_obj(&key);
            for child in children {
                rust_obj.as_obj_mut().unwrap().push(child);
            }
            out.push(rust_obj);
        } else {
            // FFON_STRING
            let s = if elem.data.is_null() {
                String::new()
            } else {
                unsafe { CStr::from_ptr(elem.data as *const c_char) }
                    .to_string_lossy()
                    .into_owned()
            };
            out.push(FfonElement::Str(s));
        }
    }
    out
}

/// Mirror of C's `ProviderOpsC` vtable that native plugins export.
///
/// All function pointers except `name` and `display_name` may be null,
/// which means "not supported".
#[repr(C)]
pub struct ProviderOpsC {
    pub name: *const c_char,
    pub display_name: *const c_char,

    /// `FfonElement** (*fetch)(const char *path, int *outCount)`
    pub fetch: Option<
        unsafe extern "C" fn(path: *const c_char, out_count: *mut c_int)
            -> *mut *mut FfonElementC,
    >,

    /// `bool (*commit)(const char *path, const char *old, const char *new)`
    pub commit: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            old_name: *const c_char,
            new_name: *const c_char,
        ) -> bool,
    >,

    pub create_directory: Option<
        unsafe extern "C" fn(path: *const c_char, name: *const c_char) -> bool,
    >,

    pub create_file:
        Option<unsafe extern "C" fn(path: *const c_char, name: *const c_char) -> bool>,

    pub delete_item:
        Option<unsafe extern "C" fn(path: *const c_char, name: *const c_char) -> bool>,

    /// `const char** (*getCommands)(int *outCount)`
    pub get_commands:
        Option<unsafe extern "C" fn(out_count: *mut c_int) -> *mut *const c_char>,

    /// `const char** (*getMeta)(int *outCount)`
    pub get_meta:
        Option<unsafe extern "C" fn(out_count: *mut c_int) -> *mut *const c_char>,
}

// Safety: ProviderOpsC is a read-only vtable living in the loaded .so.
// NativePlugin holds the library alive, so the pointer remains valid.
unsafe impl Send for ProviderOpsC {}
unsafe impl Sync for ProviderOpsC {}

/// The symbol name that native plugins must export.
const INIT_SYMBOL: &[u8] = b"sicompass_plugin_init\0";

// ---------------------------------------------------------------------------
// NativePlugin
// ---------------------------------------------------------------------------

/// A provider backed by a dynamically-loaded shared library.
///
/// Keeps the [`libloading::Library`] alive so that the `ProviderOpsC` pointer
/// (which lives inside the `.so`) remains valid for the lifetime of this struct.
pub struct NativePlugin {
    /// The open library — must outlive `ops`.
    _lib: libloading::Library,
    ops: *const ProviderOpsC,
    current_path: String,
    cached_name: String,
    cached_display_name: String,
    error_message: String,
}

// Safety: libloading::Library is Send but not Sync.  We only access `ops`
// from a single thread (the main app thread).
unsafe impl Send for NativePlugin {}

impl NativePlugin {
    /// Open `path` and call `sicompass_plugin_init`.
    ///
    /// Returns `None` if the library cannot be opened, the symbol is missing,
    /// or the init function returns null.
    pub fn open(path: &std::path::Path) -> Option<Self> {
        // SAFETY: loading a shared library has inherent safety risks — we
        // trust that the plugin was installed by the user.
        let lib = unsafe { libloading::Library::new(path) }.ok()?;

        type InitFn = unsafe extern "C" fn() -> *const ProviderOpsC;
        let init: libloading::Symbol<InitFn> =
            unsafe { lib.get(INIT_SYMBOL) }.ok()?;

        let ops: *const ProviderOpsC = unsafe { init() };
        if ops.is_null() {
            return None;
        }

        let (name, display_name) = unsafe {
            let ops_ref = &*ops;
            let n = if ops_ref.name.is_null() {
                "unknown".to_owned()
            } else {
                CStr::from_ptr(ops_ref.name).to_string_lossy().into_owned()
            };
            let d = if ops_ref.display_name.is_null() {
                n.clone()
            } else {
                CStr::from_ptr(ops_ref.display_name)
                    .to_string_lossy()
                    .into_owned()
            };
            (n, d)
        };

        Some(NativePlugin {
            _lib: lib,
            ops,
            current_path: "/".to_owned(),
            cached_name: name,
            cached_display_name: display_name,
            error_message: String::new(),
        })
    }
}

impl Provider for NativePlugin {
    fn name(&self) -> &str {
        &self.cached_name
    }

    fn display_name(&self) -> &str {
        &self.cached_display_name
    }

    fn fetch(&mut self) -> Vec<FfonElement> {
        let ops = unsafe { &*self.ops };
        let Some(fetch_fn) = ops.fetch else {
            return Vec::new();
        };
        let c_path = CString::new(self.current_path.as_str()).unwrap_or_default();
        let mut count: c_int = 0;
        let ptr = unsafe { fetch_fn(c_path.as_ptr(), &mut count) };
        unsafe { c_elements_to_rust(ptr, count) }
    }

    fn commit_edit(&mut self, old: &str, new: &str) -> bool {
        let ops = unsafe { &*self.ops };
        let Some(commit_fn) = ops.commit else {
            return false;
        };
        let c_path = CString::new(self.current_path.as_str()).unwrap_or_default();
        let c_old = CString::new(old).unwrap_or_default();
        let c_new = CString::new(new).unwrap_or_default();
        unsafe { commit_fn(c_path.as_ptr(), c_old.as_ptr(), c_new.as_ptr()) }
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
        if self.current_path == "/" {
            return;
        }
        if let Some(idx) = self.current_path.rfind('/') {
            if idx == 0 {
                self.current_path = "/".to_owned();
            } else {
                self.current_path.truncate(idx);
            }
        }
    }

    fn current_path(&self) -> &str {
        &self.current_path
    }

    fn take_error(&mut self) -> Option<String> {
        if self.error_message.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.error_message))
        }
    }

    fn create_directory(&mut self, name: &str) -> bool {
        let ops = unsafe { &*self.ops };
        let Some(f) = ops.create_directory else { return false; };
        let c_path = CString::new(self.current_path.as_str()).unwrap_or_default();
        let c_name = CString::new(name).unwrap_or_default();
        unsafe { f(c_path.as_ptr(), c_name.as_ptr()) }
    }

    fn create_file(&mut self, name: &str) -> bool {
        let ops = unsafe { &*self.ops };
        let Some(f) = ops.create_file else { return false; };
        let c_path = CString::new(self.current_path.as_str()).unwrap_or_default();
        let c_name = CString::new(name).unwrap_or_default();
        unsafe { f(c_path.as_ptr(), c_name.as_ptr()) }
    }

    fn delete_item(&mut self, name: &str) -> bool {
        let ops = unsafe { &*self.ops };
        let Some(f) = ops.delete_item else { return false; };
        let c_path = CString::new(self.current_path.as_str()).unwrap_or_default();
        let c_name = CString::new(name).unwrap_or_default();
        unsafe { f(c_path.as_ptr(), c_name.as_ptr()) }
    }

    fn commands(&self) -> Vec<String> {
        let ops = unsafe { &*self.ops };
        let Some(f) = ops.get_commands else { return Vec::new(); };
        let mut count: c_int = 0;
        let ptr = unsafe { f(&mut count) };
        if ptr.is_null() || count <= 0 {
            return Vec::new();
        }
        let slice = unsafe { std::slice::from_raw_parts(ptr, count as usize) };
        slice
            .iter()
            .filter(|&&p| !p.is_null())
            .map(|&p| unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
            .collect()
    }

    fn meta(&self) -> Vec<String> {
        let ops = unsafe { &*self.ops };
        let Some(f) = ops.get_meta else { return Vec::new(); };
        let mut count: c_int = 0;
        let ptr = unsafe { f(&mut count) };
        if ptr.is_null() || count <= 0 {
            return Vec::new();
        }
        let slice = unsafe { std::slice::from_raw_parts(ptr, count as usize) };
        slice
            .iter()
            .filter(|&&p| !p.is_null())
            .map(|&p| unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// ScriptProvider
// ---------------------------------------------------------------------------

/// A provider backed by a `bun run <script>` subprocess.
///
/// The script receives subcommands on argv (e.g. `fetch /`, `getcommands`).
/// JSON arrays / strings are written to stdout.
///
/// Mirrors `scriptProviderCreate()` in `lib/lib_provider/src/provider.c`.
pub struct ScriptProvider {
    name: String,
    display_name: String,
    script_path: PathBuf,
    current_path: String,
    error_message: String,
    dashboard_image: String,
    supports_config_files: bool,
}

impl ScriptProvider {
    pub fn new(name: &str, display_name: &str, script_path: PathBuf) -> Self {
        ScriptProvider {
            name: name.to_owned(),
            display_name: display_name.to_owned(),
            script_path,
            current_path: "/".to_owned(),
            error_message: String::new(),
            dashboard_image: String::new(),
            supports_config_files: false,
        }
    }

    pub fn with_supports_config_files(mut self, val: bool) -> Self {
        self.supports_config_files = val;
        self
    }

    /// Run the script with the given arguments and return trimmed stdout.
    fn run(&self, args: &[&str]) -> Option<String> {
        let output = std::process::Command::new("bun")
            .arg("run")
            .arg(&self.script_path)
            .args(args)
            .output()
            .ok()?;
        if output.status.success() {
            Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
        } else {
            None
        }
    }

    /// Mirror C's `scriptResponseOk`: returns `true` only when the JSON response
    /// is an object with `"ok": true` and no `"error"` field.
    ///
    /// Scripts must output `{"ok":true}` to signal a successful mutation
    /// (commit, createDirectory, createFile, deleteItem, copyItem).
    /// Any other output — including arrays, empty strings, or `{"error":...}` —
    /// is treated as failure so callers do NOT trigger side-effects like refreshing
    /// the FFON tree.
    fn script_response_ok(output: &str) -> bool {
        let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(output) else {
            return false;
        };
        if map.contains_key("error") {
            return false;
        }
        map.get("ok").and_then(|v| v.as_bool()).unwrap_or(false)
    }

    /// Parse a JSON string into FFON elements.
    ///
    /// Accepts either a plain JSON array (backward compat) or an object with a
    /// `"children"` array plus optional `"dashboardImage"` string metadata —
    /// matching the C ScriptProvider's protocol.
    ///
    /// Returns `(elements, dashboard_image_path)`.
    fn parse_json_output(json: &str) -> (Vec<FfonElement>, String) {
        let Ok(val) = serde_json::from_str::<serde_json::Value>(json) else {
            return (Vec::new(), String::new());
        };
        match val {
            serde_json::Value::Array(arr) => {
                let elems = arr.into_iter().filter_map(json_value_to_ffon).collect();
                (elems, String::new())
            }
            serde_json::Value::Object(ref map) => {
                let children = map
                    .get("children")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().cloned().filter_map(json_value_to_ffon).collect())
                    .unwrap_or_default();
                let dashboard = map
                    .get("dashboardImage")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned();
                (children, dashboard)
            }
            _ => (Vec::new(), String::new()),
        }
    }
}

fn json_value_to_ffon(v: serde_json::Value) -> Option<FfonElement> {
    match v {
        serde_json::Value::String(s) => Some(FfonElement::Str(s)),
        serde_json::Value::Object(map) => {
            // Each object should have exactly one key whose value is an array.
            let (key, children_val) = map.into_iter().next()?;
            let children: Vec<FfonElement> = match children_val {
                serde_json::Value::Array(arr) => {
                    arr.into_iter().filter_map(json_value_to_ffon).collect()
                }
                _ => Vec::new(),
            };
            let mut obj = FfonElement::new_obj(&key);
            for child in children {
                obj.as_obj_mut().unwrap().push(child);
            }
            Some(obj)
        }
        _ => None,
    }
}

impl Provider for ScriptProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn display_name(&self) -> &str {
        &self.display_name
    }

    fn supports_config_files(&self) -> bool {
        self.supports_config_files
    }

    fn fetch(&mut self) -> Vec<FfonElement> {
        // C ScriptProvider passes just the current path (no subcommand) for fetch.
        let path = self.current_path.clone();
        match self.run(&[&path]) {
            Some(json) => {
                let (elems, dashboard) = Self::parse_json_output(&json);
                self.dashboard_image = dashboard;
                elems
            }
            None => Vec::new(),
        }
    }

    fn dashboard_image_path(&self) -> Option<&str> {
        if self.dashboard_image.is_empty() { None } else { Some(&self.dashboard_image) }
    }

    fn commit_edit(&mut self, old: &str, new: &str) -> bool {
        let path = self.current_path.clone();
        self.run(&["commit", &path, old, new])
            .map(|out| Self::script_response_ok(&out))
            .unwrap_or(false)
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
        if self.current_path == "/" {
            return;
        }
        if let Some(idx) = self.current_path.rfind('/') {
            if idx == 0 {
                self.current_path = "/".to_owned();
            } else {
                self.current_path.truncate(idx);
            }
        }
    }

    fn current_path(&self) -> &str {
        &self.current_path
    }

    fn take_error(&mut self) -> Option<String> {
        if self.error_message.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.error_message))
        }
    }

    fn create_directory(&mut self, name: &str) -> bool {
        let path = self.current_path.clone();
        self.run(&["create-dir", &path, name])
            .map(|out| Self::script_response_ok(&out))
            .unwrap_or(false)
    }

    fn create_file(&mut self, name: &str) -> bool {
        let path = self.current_path.clone();
        self.run(&["create-file", &path, name])
            .map(|out| Self::script_response_ok(&out))
            .unwrap_or(false)
    }

    fn delete_item(&mut self, name: &str) -> bool {
        let path = self.current_path.clone();
        self.run(&["delete", &path, name])
            .map(|out| Self::script_response_ok(&out))
            .unwrap_or(false)
    }

    fn create_element(&mut self, element_key: &str) -> Option<FfonElement> {
        let is_one_opt = element_key.starts_with("one-opt:");
        let key = if is_one_opt { &element_key[8..] } else { element_key };

        let tagged = if is_one_opt {
            sicompass_sdk::tags::format_one_opt(key)
        } else {
            sicompass_sdk::tags::format_many_opt(key)
        };

        if sicompass_sdk::tags::has_input(key) || sicompass_sdk::tags::has_input_all(key) {
            return Some(FfonElement::Str(tagged));
        }

        let mut obj = FfonElement::new_obj(&tagged);
        let child_path = if self.current_path.ends_with('/') {
            format!("{}{}", self.current_path, key)
        } else {
            format!("{}/{}", self.current_path, key)
        };

        if let Some(json) = self.run(&[&child_path]) {
            let (children, _) = Self::parse_json_output(&json);
            if let Some(obj_inner) = obj.as_obj_mut() {
                for child in children {
                    obj_inner.push(child);
                }
            }
        }

        Some(obj)
    }

    fn commands(&self) -> Vec<String> {
        let Some(json) = self.run(&["getcommands"]) else {
            return Vec::new();
        };
        serde_json::from_str::<Vec<String>>(&json).unwrap_or_default()
    }

    fn meta(&self) -> Vec<String> {
        let Some(json) = self.run(&["getmeta"]) else {
            return Vec::new();
        };
        serde_json::from_str::<Vec<String>>(&json).unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- json_value_to_ffon ---

    #[test]
    fn json_string_becomes_ffon_string() {
        let v = serde_json::Value::String("hello".to_owned());
        let elem = json_value_to_ffon(v).unwrap();
        assert!(matches!(elem, FfonElement::Str(s) if s == "hello"));
    }

    #[test]
    fn json_object_becomes_ffon_obj() {
        let json = r#"{"mykey": ["child1", "child2"]}"#;
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        let elem = json_value_to_ffon(v).unwrap();
        let obj = elem.as_obj().unwrap();
        assert_eq!(obj.key, "mykey");
        assert_eq!(obj.children.len(), 2);
    }

    #[test]
    fn json_null_returns_none() {
        assert!(json_value_to_ffon(serde_json::Value::Null).is_none());
    }

    #[test]
    fn json_number_returns_none() {
        assert!(json_value_to_ffon(serde_json::Value::Number(42.into())).is_none());
    }

    // --- parse_json_output ---

    #[test]
    fn parse_empty_array() {
        assert!(ScriptProvider::parse_json_output("[]").0.is_empty());
    }

    #[test]
    fn parse_string_array() {
        let (elems, _) = ScriptProvider::parse_json_output(r#"["a","b","c"]"#);
        assert_eq!(elems.len(), 3);
        assert!(matches!(&elems[0], FfonElement::Str(s) if s == "a"));
    }

    #[test]
    fn parse_mixed_array() {
        let (elems, _) =
            ScriptProvider::parse_json_output(r#"["hello",{"mySection":["item1","item2"]}]"#);
        assert_eq!(elems.len(), 2);
        assert!(matches!(&elems[0], FfonElement::Str(_)));
        let obj = elems[1].as_obj().unwrap();
        assert_eq!(obj.key, "mySection");
        assert_eq!(obj.children.len(), 2);
    }

    #[test]
    fn parse_invalid_json_returns_empty() {
        assert!(ScriptProvider::parse_json_output("not json").0.is_empty());
    }

    #[test]
    fn parse_wrapped_object_with_children() {
        let json = r#"{"children":["a","b"],"dashboardImage":"/path/to/img.webp"}"#;
        let (elems, dashboard) = ScriptProvider::parse_json_output(json);
        assert_eq!(elems.len(), 2);
        assert!(matches!(&elems[0], FfonElement::Str(s) if s == "a"));
        assert_eq!(dashboard, "/path/to/img.webp");
    }

    #[test]
    fn parse_wrapped_object_no_dashboard() {
        let json = r#"{"children":["item"]}"#;
        let (elems, dashboard) = ScriptProvider::parse_json_output(json);
        assert_eq!(elems.len(), 1);
        assert!(dashboard.is_empty());
    }

    // --- ScriptProvider script_response_ok ---

    #[test]
    fn script_response_ok_requires_ok_true() {
        // Success case: {"ok":true}
        assert!(ScriptProvider::script_response_ok(r#"{"ok":true}"#));
        // Error field present → false even with ok:true
        assert!(!ScriptProvider::script_response_ok(r#"{"ok":true,"error":"oops"}"#));
        // ok:false → false
        assert!(!ScriptProvider::script_response_ok(r#"{"ok":false}"#));
        // No ok field → false
        assert!(!ScriptProvider::script_response_ok(r#"{"result":"done"}"#));
        // Array (sales demo commit returns []) → false
        assert!(!ScriptProvider::script_response_ok("[]"));
        // Empty string → false
        assert!(!ScriptProvider::script_response_ok(""));
        // SDK error convention → false
        assert!(!ScriptProvider::script_response_ok(r#"{"error":"unsupported: commit"}"#));
    }

    // --- ScriptProvider path management ---

    #[test]
    fn script_provider_push_pop_path() {
        let mut p = ScriptProvider::new("test", "Test", PathBuf::from("test.ts"));
        assert_eq!(p.current_path(), "/");
        p.push_path("foo");
        assert_eq!(p.current_path(), "/foo");
        p.push_path("bar");
        assert_eq!(p.current_path(), "/foo/bar");
        p.pop_path();
        assert_eq!(p.current_path(), "/foo");
        p.pop_path();
        assert_eq!(p.current_path(), "/");
        p.pop_path(); // at root — no-op
        assert_eq!(p.current_path(), "/");
    }

    #[test]
    fn script_provider_name() {
        let p = ScriptProvider::new("myprov", "My Provider", PathBuf::from("p.ts"));
        assert_eq!(p.name(), "myprov");
        assert_eq!(p.display_name(), "My Provider");
    }

    // --- NativePlugin path management (via a mock that skips dlopen) ---

    fn make_native_plugin_stub() -> NativePlugin {
        // Build a ProviderOpsC with no function pointers.
        // We leak it to get a stable 'static pointer (acceptable in tests).
        let ops: &'static ProviderOpsC = Box::leak(Box::new(ProviderOpsC {
            name: b"stub\0".as_ptr() as *const c_char,
            display_name: b"Stub\0".as_ptr() as *const c_char,
            fetch: None,
            commit: None,
            create_directory: None,
            create_file: None,
            delete_item: None,
            get_commands: None,
            get_meta: None,
        }));
        // SAFETY: We construct a NativePlugin without calling dlopen.
        // The _lib field is replaced with a dummy that does nothing on drop.
        // This is a test-only path.
        NativePlugin {
            _lib: unsafe {
                // `Library::this_process()` calls dlopen(NULL,...) which always
                // succeeds and gives us a valid handle for the test.
                libloading::os::unix::Library::this().into()
            },
            ops: ops as *const ProviderOpsC,
            current_path: "/".to_owned(),
            cached_name: "stub".to_owned(),
            cached_display_name: "Stub".to_owned(),
            error_message: String::new(),
        }
    }

    #[test]
    fn native_plugin_push_pop_path() {
        let mut p = make_native_plugin_stub();
        assert_eq!(p.current_path(), "/");
        p.push_path("alpha");
        assert_eq!(p.current_path(), "/alpha");
        p.push_path("beta");
        assert_eq!(p.current_path(), "/alpha/beta");
        p.pop_path();
        assert_eq!(p.current_path(), "/alpha");
        p.pop_path();
        assert_eq!(p.current_path(), "/");
        p.pop_path(); // root — no-op
        assert_eq!(p.current_path(), "/");
    }

    #[test]
    fn native_plugin_fetch_null_ops_returns_empty() {
        let mut p = make_native_plugin_stub();
        assert!(p.fetch().is_empty());
    }

    #[test]
    fn native_plugin_commands_null_ops_returns_empty() {
        let p = make_native_plugin_stub();
        assert!(p.commands().is_empty());
    }

    #[test]
    fn native_plugin_commit_null_ops_returns_false() {
        let mut p = make_native_plugin_stub();
        assert!(!p.commit_edit("old", "new"));
    }

    #[test]
    fn native_plugin_open_nonexistent_returns_none() {
        assert!(NativePlugin::open(std::path::Path::new("/no/such/plugin.so")).is_none());
    }
}
