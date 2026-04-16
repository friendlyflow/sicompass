//! Dynamic plugin loading — `NativePlugin` (shared library) and `ScriptProvider` (bun).
//!
//! Moved from `src/sicompass/src/plugin_loader.rs` into the SDK so lib crates
//! (e.g. `lib_sales_demo`) can construct `ScriptProvider` without depending on
//! the app crate.
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
//! The script receives subcommands on `argv`: `<path>` (fetch), `commit`,
//! `createDirectory`, `createFile`, `deleteItem`, `copyItem`, `commands`,
//! `handleCommand`, `commandListItems`, `executeCommand`, `deepSearch`.
//! JSON is written to stdout.  [`ScriptProvider`] implements [`Provider`] by
//! spawning the interpreter and parsing the output.

use crate::ffon::FfonElement;
use crate::provider::{ListItem, Provider, SearchResultItem};
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

/// Mirror of C's `ProviderListItem` (sdk/include/provider_interface.h:20-23).
#[repr(C)]
struct ProviderListItemC {
    label: *mut c_char,
    data: *mut c_char,
}

/// Mirror of C's `SearchResultItem` (sdk/include/provider_interface.h:11-15).
#[repr(C)]
struct SearchResultItemC {
    label: *mut c_char,
    breadcrumb: *mut c_char,
    nav_path: *mut c_char,
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
        if let Some(elem) = unsafe { c_element_to_rust(elem_ptr) } {
            out.push(elem);
        }
    }
    out
}

/// Convert a single `*mut FfonElementC` into a Rust `FfonElement`.
///
/// Returns `None` if `ptr` is null.
///
/// # Safety
/// `ptr` must be a valid, non-null `FfonElementC` pointer.
unsafe fn c_element_to_rust(ptr: *mut FfonElementC) -> Option<FfonElement> {
    if ptr.is_null() {
        return None;
    }
    let elem = unsafe { &*ptr };
    if elem.element_type == FfonTypeC::Object as u32 {
        let obj_ptr = elem.data as *mut FfonObjectC;
        if obj_ptr.is_null() {
            return None;
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
        Some(rust_obj)
    } else {
        // FFON_STRING
        let s = if elem.data.is_null() {
            String::new()
        } else {
            unsafe { CStr::from_ptr(elem.data as *const c_char) }
                .to_string_lossy()
                .into_owned()
        };
        Some(FfonElement::Str(s))
    }
}

/// Mirror of C's `ProviderOps` vtable that native plugins export.
///
/// Field order and types MUST match `ProviderOps` in
/// `sdk/include/provider_interface.h` exactly — `#[repr(C)]` layout is
/// position-based, so any divergence corrupts function-pointer reads.
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

    pub create_file: Option<
        unsafe extern "C" fn(path: *const c_char, name: *const c_char) -> bool,
    >,

    pub delete_item: Option<
        unsafe extern "C" fn(path: *const c_char, name: *const c_char) -> bool,
    >,

    /// `bool (*copyItem)(const char *srcDir, const char *srcName, const char *destDir, const char *destName)`
    pub copy_item: Option<
        unsafe extern "C" fn(
            src_dir: *const c_char,
            src_name: *const c_char,
            dest_dir: *const c_char,
            dest_name: *const c_char,
        ) -> bool,
    >,

    /// `const char** (*getCommands)(int *outCount)`
    pub get_commands: Option<
        unsafe extern "C" fn(out_count: *mut c_int) -> *mut *const c_char,
    >,

    /// `FfonElement* (*handleCommand)(const char *path, const char *command, const char *elementKey, int elementType, char *errorMsg, int errorMsgSize)`
    pub handle_command: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            command: *const c_char,
            element_key: *const c_char,
            element_type: c_int,
            error_msg: *mut c_char,
            error_msg_size: c_int,
        ) -> *mut FfonElementC,
    >,

    /// `ProviderListItem* (*getCommandListItems)(const char *path, const char *command, int *outCount)`
    pub get_command_list_items: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            command: *const c_char,
            out_count: *mut c_int,
        ) -> *mut ProviderListItemC,
    >,

    /// `bool (*executeCommand)(const char *path, const char *command, const char *selection)`
    pub execute_command: Option<
        unsafe extern "C" fn(
            path: *const c_char,
            command: *const c_char,
            selection: *const c_char,
        ) -> bool,
    >,

    /// `SearchResultItem* (*collectDeepSearchItems)(const char *rootPath, int *outCount)`
    pub collect_deep_search_items: Option<
        unsafe extern "C" fn(
            root_path: *const c_char,
            out_count: *mut c_int,
        ) -> *mut SearchResultItemC,
    >,
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

    fn copy_item(&mut self, src_dir: &str, src_name: &str, dest_dir: &str, dest_name: &str) -> bool {
        let ops = unsafe { &*self.ops };
        let Some(f) = ops.copy_item else { return false; };
        let c_src_dir = CString::new(src_dir).unwrap_or_default();
        let c_src_name = CString::new(src_name).unwrap_or_default();
        let c_dest_dir = CString::new(dest_dir).unwrap_or_default();
        let c_dest_name = CString::new(dest_name).unwrap_or_default();
        unsafe { f(c_src_dir.as_ptr(), c_src_name.as_ptr(), c_dest_dir.as_ptr(), c_dest_name.as_ptr()) }
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

    fn handle_command(
        &mut self,
        cmd: &str,
        elem_key: &str,
        elem_type: i32,
        error: &mut String,
    ) -> Option<FfonElement> {
        let ops = unsafe { &*self.ops };
        let Some(f) = ops.handle_command else { return None; };
        let c_path = CString::new(self.current_path.as_str()).unwrap_or_default();
        let c_cmd = CString::new(cmd).unwrap_or_default();
        let c_key = CString::new(elem_key).unwrap_or_default();
        let mut err_buf = [0u8; 1024];
        let result_ptr = unsafe {
            f(
                c_path.as_ptr(),
                c_cmd.as_ptr(),
                c_key.as_ptr(),
                elem_type as c_int,
                err_buf.as_mut_ptr() as *mut c_char,
                err_buf.len() as c_int,
            )
        };
        if err_buf[0] != 0 {
            let err_str = unsafe { CStr::from_ptr(err_buf.as_ptr() as *const c_char) }
                .to_string_lossy()
                .into_owned();
            error.push_str(&err_str);
            return None;
        }
        unsafe { c_element_to_rust(result_ptr) }
    }

    fn command_list_items(&self, cmd: &str) -> Vec<ListItem> {
        let ops = unsafe { &*self.ops };
        let Some(f) = ops.get_command_list_items else { return vec![]; };
        let c_path = CString::new(self.current_path.as_str()).unwrap_or_default();
        let c_cmd = CString::new(cmd).unwrap_or_default();
        let mut count: c_int = 0;
        let ptr = unsafe { f(c_path.as_ptr(), c_cmd.as_ptr(), &mut count) };
        if ptr.is_null() || count <= 0 {
            return vec![];
        }
        let slice = unsafe { std::slice::from_raw_parts(ptr, count as usize) };
        slice
            .iter()
            .map(|item| {
                let label = if item.label.is_null() {
                    String::new()
                } else {
                    unsafe { CStr::from_ptr(item.label) }.to_string_lossy().into_owned()
                };
                let data = if item.data.is_null() {
                    String::new()
                } else {
                    unsafe { CStr::from_ptr(item.data) }.to_string_lossy().into_owned()
                };
                ListItem { label, data }
            })
            .collect()
    }

    fn execute_command(&mut self, cmd: &str, selection: &str) -> bool {
        let ops = unsafe { &*self.ops };
        let Some(f) = ops.execute_command else { return false; };
        let c_path = CString::new(self.current_path.as_str()).unwrap_or_default();
        let c_cmd = CString::new(cmd).unwrap_or_default();
        let c_sel = CString::new(selection).unwrap_or_default();
        unsafe { f(c_path.as_ptr(), c_cmd.as_ptr(), c_sel.as_ptr()) }
    }

    fn collect_deep_search_items(&self) -> Option<Vec<SearchResultItem>> {
        let ops = unsafe { &*self.ops };
        let Some(f) = ops.collect_deep_search_items else { return None; };
        let c_path = CString::new(self.current_path.as_str()).unwrap_or_default();
        let mut count: c_int = 0;
        let ptr = unsafe { f(c_path.as_ptr(), &mut count) };
        if ptr.is_null() || count <= 0 {
            return Some(vec![]);
        }
        let slice = unsafe { std::slice::from_raw_parts(ptr, count as usize) };
        let items = slice
            .iter()
            .map(|item| {
                let label = if item.label.is_null() {
                    String::new()
                } else {
                    unsafe { CStr::from_ptr(item.label) }.to_string_lossy().into_owned()
                };
                let breadcrumb = if item.breadcrumb.is_null() {
                    String::new()
                } else {
                    unsafe { CStr::from_ptr(item.breadcrumb) }.to_string_lossy().into_owned()
                };
                let nav_path = if item.nav_path.is_null() {
                    String::new()
                } else {
                    unsafe { CStr::from_ptr(item.nav_path) }.to_string_lossy().into_owned()
                };
                SearchResultItem { label, breadcrumb, nav_path }
            })
            .collect();
        Some(items)
    }
}

// ---------------------------------------------------------------------------
// ScriptProvider
// ---------------------------------------------------------------------------

/// A provider backed by a `bun run <script>` subprocess.
///
/// The script receives subcommands on argv matching the C vocabulary
/// (`createDirectory`, `createFile`, `deleteItem`, `copyItem`, `commands`,
/// `handleCommand`, `commandListItems`, `executeCommand`, `deepSearch`).
/// A bare path with no subcommand is the fetch call.
/// JSON is written to stdout.
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
    fn run(&mut self, args: &[&str]) -> Option<String> {
        crate::platform::ensure_bun_on_path();

        if !self.script_path.exists() {
            let msg = format!(
                "script provider '{}': script not found at {}",
                self.name,
                self.script_path.display()
            );
            eprintln!("{msg}");
            self.error_message = msg;
            return None;
        }

        let output = match std::process::Command::new("bun")
            .arg("run")
            .arg(&self.script_path)
            .args(args)
            .output()
        {
            Ok(o) => o,
            Err(e) => {
                let msg = format!(
                    "script provider '{}': failed to run bun ({e}). \
                     Is bun installed? On Windows check %USERPROFILE%\\.bun\\bin\\bun.exe",
                    self.name
                );
                eprintln!("{msg}");
                self.error_message = msg;
                return None;
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let msg = format!(
                "script provider '{}': bun exited with {} — {}",
                self.name,
                output.status,
                stderr.trim()
            );
            eprintln!("{msg}");
            self.error_message = msg;
            return None;
        }

        Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
    }

    /// Read-only variant for trait methods that take `&self` — calls
    /// `ensure_bun_on_path` and runs bun, but does not mutate `error_message`.
    fn run_silent(&self, args: &[&str]) -> Option<String> {
        crate::platform::ensure_bun_on_path();
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
    /// `"children"` array plus optional `"dashboardImage"` and `"meta"` fields.
    ///
    /// Returns `(elements, dashboard_image_path, meta_element)`.
    pub fn parse_json_output(json: &str) -> (Vec<FfonElement>, String, Option<FfonElement>) {
        let Ok(val) = serde_json::from_str::<serde_json::Value>(json) else {
            return (Vec::new(), String::new(), None);
        };
        match val {
            serde_json::Value::Array(arr) => {
                let elems = arr.iter().map(parse_ffon_json_value).collect();
                (elems, String::new(), None)
            }
            serde_json::Value::Object(ref map) => {
                let children = map
                    .get("children")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().map(parse_ffon_json_value).collect())
                    .unwrap_or_default();
                let dashboard = map
                    .get("dashboardImage")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned();
                let meta = map.get("meta").map(parse_ffon_json_value);
                (children, dashboard, meta)
            }
            _ => (Vec::new(), String::new(), None),
        }
    }
}

fn parse_ffon_json_value(v: &serde_json::Value) -> FfonElement {
    crate::ffon::parse_json_value(v)
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
        let path = self.current_path.clone();
        let Some(json) = self.run(&[&path]) else {
            return Vec::new();
        };

        if serde_json::from_str::<serde_json::Value>(&json).is_err() {
            let msg = format!(
                "script provider '{}': failed to parse JSON output ({} bytes)",
                self.name,
                json.len()
            );
            eprintln!("{msg}\n--- output ---\n{json}\n--- end ---");
            self.error_message = msg;
            return Vec::new();
        }

        let (elems, dashboard, _meta) = Self::parse_json_output(&json);
        self.dashboard_image = dashboard;
        elems
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
        self.run(&["createDirectory", &path, name])
            .map(|out| Self::script_response_ok(&out))
            .unwrap_or(false)
    }

    fn create_file(&mut self, name: &str) -> bool {
        let path = self.current_path.clone();
        self.run(&["createFile", &path, name])
            .map(|out| Self::script_response_ok(&out))
            .unwrap_or(false)
    }

    fn delete_item(&mut self, name: &str) -> bool {
        let path = self.current_path.clone();
        self.run(&["deleteItem", &path, name])
            .map(|out| Self::script_response_ok(&out))
            .unwrap_or(false)
    }

    fn copy_item(&mut self, src_dir: &str, src_name: &str, dest_dir: &str, dest_name: &str) -> bool {
        self.run(&["copyItem", src_dir, src_name, dest_dir, dest_name])
            .map(|out| Self::script_response_ok(&out))
            .unwrap_or(false)
    }

    fn commands(&self) -> Vec<String> {
        let Some(json) = self.run_silent(&["commands"]) else {
            return Vec::new();
        };
        serde_json::from_str::<Vec<String>>(&json).unwrap_or_default()
    }

    fn handle_command(
        &mut self,
        cmd: &str,
        elem_key: &str,
        elem_type: i32,
        error: &mut String,
    ) -> Option<FfonElement> {
        let path = self.current_path.clone();
        let type_str = elem_type.to_string();
        let output = self.run(&["handleCommand", &path, cmd, elem_key, &type_str])?;
        let Ok(val) = serde_json::from_str::<serde_json::Value>(&output) else {
            return None;
        };
        if let serde_json::Value::Object(ref map) = val {
            if let Some(err_val) = map.get("error") {
                *error = err_val.as_str().unwrap_or("unknown error").to_owned();
                return None;
            }
        }
        Some(parse_ffon_json_value(&val))
    }

    fn command_list_items(&self, cmd: &str) -> Vec<ListItem> {
        let path = self.current_path.clone();
        let Some(json) = self.run_silent(&["commandListItems", &path, cmd]) else {
            return vec![];
        };
        let Ok(serde_json::Value::Array(arr)) = serde_json::from_str::<serde_json::Value>(&json) else {
            return vec![];
        };
        arr.into_iter()
            .filter_map(|v| {
                let obj = v.as_object()?.clone();
                let label = obj.get("label").and_then(|v| v.as_str()).unwrap_or("").to_owned();
                let data = obj.get("data").and_then(|v| v.as_str()).unwrap_or("").to_owned();
                Some(ListItem { label, data })
            })
            .collect()
    }

    fn execute_command(&mut self, cmd: &str, selection: &str) -> bool {
        let path = self.current_path.clone();
        self.run(&["executeCommand", &path, cmd, selection])
            .map(|out| Self::script_response_ok(&out))
            .unwrap_or(false)
    }

    fn collect_deep_search_items(&self) -> Option<Vec<SearchResultItem>> {
        let path = self.current_path.clone();
        let json = self.run_silent(&["deepSearch", &path])?;
        let Ok(serde_json::Value::Array(arr)) = serde_json::from_str::<serde_json::Value>(&json) else {
            return None;
        };
        let items = arr
            .into_iter()
            .filter_map(|v| {
                let obj = v.as_object()?.clone();
                let label = obj.get("label").and_then(|v| v.as_str()).unwrap_or("").to_owned();
                let breadcrumb = obj.get("breadcrumb").and_then(|v| v.as_str()).unwrap_or("").to_owned();
                let nav_path = obj.get("navPath").and_then(|v| v.as_str()).unwrap_or("").to_owned();
                Some(SearchResultItem { label, breadcrumb, nav_path })
            })
            .collect();
        Some(items)
    }

    fn create_element(&mut self, element_key: &str) -> Option<FfonElement> {
        let is_one_opt = element_key.starts_with("one-opt:");
        let key = if is_one_opt { &element_key[8..] } else { element_key };

        let tagged = if is_one_opt {
            crate::tags::format_one_opt(key)
        } else {
            crate::tags::format_many_opt(key)
        };

        if crate::tags::has_input(key) {
            return Some(FfonElement::Str(tagged));
        }

        let mut obj = FfonElement::new_obj(&tagged);
        let child_path = if self.current_path.ends_with('/') {
            format!("{}{}", self.current_path, key)
        } else {
            format!("{}/{}", self.current_path, key)
        };

        if let Some(json) = self.run(&[&child_path]) {
            let (children, _, _) = Self::parse_json_output(&json);
            if let Some(obj_inner) = obj.as_obj_mut() {
                for child in children {
                    obj_inner.push(child);
                }
            }
        }

        Some(obj)
    }

    fn cleanup(&mut self) {}
}

// ---------------------------------------------------------------------------
// Tests (migrated from src/sicompass/src/plugin_loader.rs)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_ffon_json_value ---

    #[test]
    fn json_string_becomes_ffon_string() {
        let v = serde_json::Value::String("hello".to_owned());
        let elem = parse_ffon_json_value(&v);
        assert!(matches!(elem, FfonElement::Str(s) if s == "hello"));
    }

    #[test]
    fn json_object_becomes_ffon_obj() {
        let json = r#"{"mykey": ["child1", "child2"]}"#;
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        let elem = parse_ffon_json_value(&v);
        let obj = elem.as_obj().unwrap();
        assert_eq!(obj.key, "mykey");
        assert_eq!(obj.children.len(), 2);
    }

    #[test]
    fn json_null_becomes_str_null() {
        let elem = parse_ffon_json_value(&serde_json::Value::Null);
        assert!(matches!(elem, FfonElement::Str(s) if s == "null"));
    }

    #[test]
    fn json_number_becomes_str() {
        let elem = parse_ffon_json_value(&serde_json::Value::Number(42.into()));
        assert!(matches!(elem, FfonElement::Str(s) if s == "42"));
    }

    #[test]
    fn json_bool_true_becomes_str() {
        let elem = parse_ffon_json_value(&serde_json::Value::Bool(true));
        assert!(matches!(elem, FfonElement::Str(s) if s == "true"));
    }

    #[test]
    fn json_bool_false_becomes_str() {
        let elem = parse_ffon_json_value(&serde_json::Value::Bool(false));
        assert!(matches!(elem, FfonElement::Str(s) if s == "false"));
    }

    #[test]
    fn json_array_becomes_obj_named_array() {
        let json = r#"["x", "y"]"#;
        let v: serde_json::Value = serde_json::from_str(json).unwrap();
        let elem = parse_ffon_json_value(&v);
        let obj = elem.as_obj().unwrap();
        assert_eq!(obj.key, "array");
        assert_eq!(obj.children.len(), 2);
    }

    #[test]
    fn json_empty_object_becomes_empty_str() {
        let v: serde_json::Value = serde_json::from_str("{}").unwrap();
        let elem = parse_ffon_json_value(&v);
        assert!(matches!(elem, FfonElement::Str(s) if s.is_empty()));
    }

    // --- parse_json_output ---

    #[test]
    fn parse_empty_array() {
        assert!(ScriptProvider::parse_json_output("[]").0.is_empty());
    }

    #[test]
    fn parse_string_array() {
        let (elems, _, _) = ScriptProvider::parse_json_output(r#"["a","b","c"]"#);
        assert_eq!(elems.len(), 3);
        assert!(matches!(&elems[0], FfonElement::Str(s) if s == "a"));
    }

    #[test]
    fn parse_mixed_array() {
        let (elems, _, _) =
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
        let (elems, dashboard, _meta) = ScriptProvider::parse_json_output(json);
        assert_eq!(elems.len(), 2);
        assert!(matches!(&elems[0], FfonElement::Str(s) if s == "a"));
        assert_eq!(dashboard, "/path/to/img.webp");
    }

    #[test]
    fn parse_wrapped_object_no_dashboard() {
        let json = r#"{"children":["item"]}"#;
        let (elems, dashboard, _meta) = ScriptProvider::parse_json_output(json);
        assert_eq!(elems.len(), 1);
        assert!(dashboard.is_empty());
    }

    #[test]
    fn parse_json_output_extracts_meta() {
        let json = r#"{"children":["item"],"meta":{"Shortcuts":["Ctrl+X Cut"]}}"#;
        let (elems, _, meta) = ScriptProvider::parse_json_output(json);
        assert_eq!(elems.len(), 1);
        let meta_elem = meta.expect("meta should be present");
        let obj = meta_elem.as_obj().unwrap();
        assert_eq!(obj.key, "Shortcuts");
        assert_eq!(obj.children.len(), 1);
    }

    // --- ScriptProvider script_response_ok ---

    #[test]
    fn script_response_ok_requires_ok_true() {
        assert!(ScriptProvider::script_response_ok(r#"{"ok":true}"#));
        assert!(!ScriptProvider::script_response_ok(r#"{"ok":true,"error":"oops"}"#));
        assert!(!ScriptProvider::script_response_ok(r#"{"ok":false}"#));
        assert!(!ScriptProvider::script_response_ok(r#"{"result":"done"}"#));
        assert!(!ScriptProvider::script_response_ok("[]"));
        assert!(!ScriptProvider::script_response_ok(""));
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
        p.pop_path();
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
        let ops: &'static ProviderOpsC = Box::leak(Box::new(ProviderOpsC {
            name: b"stub\0".as_ptr() as *const c_char,
            display_name: b"Stub\0".as_ptr() as *const c_char,
            fetch: None,
            commit: None,
            create_directory: None,
            create_file: None,
            delete_item: None,
            copy_item: None,
            get_commands: None,
            handle_command: None,
            get_command_list_items: None,
            execute_command: None,
            collect_deep_search_items: None,
        }));
        NativePlugin {
            _lib: unsafe {
                #[cfg(unix)]
                { libloading::os::unix::Library::this().into() }
                #[cfg(windows)]
                { libloading::os::windows::Library::this().expect("this always succeeds").into() }
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
        p.pop_path();
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
    fn native_plugin_copy_item_null_ops_returns_false() {
        let mut p = make_native_plugin_stub();
        assert!(!p.copy_item("/src", "file.txt", "/dst", "file.txt"));
    }

    #[test]
    fn native_plugin_execute_command_null_ops_returns_false() {
        let mut p = make_native_plugin_stub();
        assert!(!p.execute_command("open", "file.txt"));
    }

    #[test]
    fn native_plugin_command_list_items_null_ops_returns_empty() {
        let p = make_native_plugin_stub();
        assert!(p.command_list_items("open with").is_empty());
    }

    #[test]
    fn native_plugin_collect_deep_search_null_ops_returns_none() {
        let p = make_native_plugin_stub();
        assert!(p.collect_deep_search_items().is_none());
    }

    #[test]
    fn native_plugin_handle_command_null_ops_returns_none() {
        let mut p = make_native_plugin_stub();
        let mut error = String::new();
        assert!(p.handle_command("cmd", "key", 0, &mut error).is_none());
        assert!(error.is_empty());
    }

    #[test]
    fn native_plugin_open_nonexistent_returns_none() {
        assert!(NativePlugin::open(std::path::Path::new("/no/such/plugin.so")).is_none());
    }
}
