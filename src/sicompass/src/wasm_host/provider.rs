//! [`WasmProvider`] — a sandboxed WASM component wearing the SDK's `Provider` trait.
//!
//! The rest of the app cannot tell a WASM plugin from a compiled-in built-in: both
//! arrive as `Box<dyn Provider>`. Everything specific to WASM is contained here.
//!
//! ## Trap containment
//!
//! A guest can loop forever, exhaust memory, or panic. None of that may take the
//! host down, so every call goes through [`WasmProvider::call`], which:
//!
//! 1. re-arms fuel and the epoch deadline ([`super::limits::refresh_for_call`]),
//! 2. runs the guest export,
//! 3. on `Trap` records the message for `take_error()` and **poisons** the provider.
//!
//! Poisoning is deliberate. After a trap the guest's linear memory is in an
//! arbitrary state, so continuing to call it would produce garbage rather than
//! errors. A poisoned provider answers every later call the way an inert provider
//! would and keeps surfacing its error, so the user sees a broken plugin instead of
//! a broken app. This mirrors how a failed load already logs and skips in
//! `programs::load_user_plugins`.
//!
//! ## Batched reads
//!
//! The app calls `tick` + `take_dashboard_request` + `take_navigation_request` +
//! `needs_refresh` + `take_error` for *every* provider on *every* frame. Mirroring
//! that across the boundary would be 5 crossings per provider at 60fps, which Pulley
//! (the no-JIT backend) would not absorb. Instead the guest has one `poll()` export
//! whose result is cached here, and those five trait methods read the cache.
//! `describe()` does the same for values that never change.
//!
//! ## Why `RefCell`
//!
//! Five trait methods (`commands`, `command_label`, `command_list_items`,
//! `collect_extended_search_items`, `save_config`) take `&self`, but a guest call needs
//! `&mut Store`. Caching their answers would trade a borrow problem for a staleness
//! bug, so the mutable guts live behind a `RefCell` instead.
//!
//! That is sound rather than a dodge: a component instance is single-threaded and
//! exclusively owned by this provider, and a guest cannot re-enter its own provider
//! because no host function touches it. So the `RefCell` can never observe a
//! conflicting borrow. `borrow_mut` is still handled without panicking, so a future
//! re-entrant host function degrades to an error rather than aborting.

use std::cell::RefCell;
use std::path::Path;

use sicompass_sdk::{
    CellAttrs, DashboardCell, DashboardFrame, DashboardKey, DashboardKeysym, DashboardKind,
    DashboardRequest, FfonElement, ListItem, NavigationRequest, Provider, SearchResultItem,
    TimelineEntry, ffon,
};
use wasmtime::Store;
use wasmtime::component::Component;

use super::exports::sicompass::plugin::provider::Guest;
use super::{HostState, Plugin, limits, wit_types};

/// The parts that a guest call mutates.
struct Inner {
    store: Store<HostState>,
    instance: Plugin,
    /// Errors waiting to be surfaced as a row: guest-reported, or a trap.
    pending_error: Option<String>,
    /// Set after a trap. All further guest calls are skipped.
    poisoned: bool,
}

/// A WASM component driven through the `Provider` trait.
pub struct WasmProvider {
    inner: RefCell<Inner>,

    /// Constant guest properties, fetched once via `describe()`.
    descriptor: wit_types::Descriptor,

    /// Mirror of the guest's current path. Kept in step by the path mutators, which
    /// return the resulting path, so `current_path()` needs no boundary crossing.
    current_path: String,

    /// Last `poll()` result. The `take_*` methods drain from here, preserving the
    /// trait's documented two-call semantics.
    polled: wit_types::PollResult,

    /// Confined absolute path for a `DashboardKind::Image` plugin, resolved once at
    /// construction.
    ///
    /// The trait returns a borrow, which a guest call cannot produce. Resolving it
    /// here also means the confinement check — no absolute paths, no `..`, always
    /// inside the plugin's own directory — happens once, before the host ever opens
    /// the file, rather than on every frame.
    dashboard_image: Option<String>,
}

impl WasmProvider {
    /// Load, instantiate and initialise a plugin.
    ///
    /// `allowed_hosts` comes from the manifest and decides whether the network
    /// interface is linked at all — see [`super::linker_for`].
    /// `settings_section` is the manifest's `displayName`, which is the section
    /// `programs::inject_plugin_settings` registers this plugin's settings under and
    /// therefore what `get_setting` must read back from.
    pub fn open(
        wasm_path: &Path,
        plugin_name: &str,
        settings_section: &str,
        plugin_dir: &Path,
        allowed_hosts: Vec<String>,
    ) -> Result<Self, String> {
        let component = super::load_component(wasm_path)?;

        // Audit before instantiating. Instantiation would refuse an over-reaching
        // component anyway (the interface simply would not be linked), but the
        // failure would be an opaque link error; this names the mismatch.
        super::audit_component_imports(&component, &allowed_hosts)
            .map_err(|e| format!("{}: {e}", wasm_path.display()))?;

        Self::from_component(
            &component,
            plugin_name,
            settings_section,
            plugin_dir,
            allowed_hosts,
        )
    }

    /// Instantiate an already-parsed component. Split out so tests can build a
    /// component once and instantiate it repeatedly.
    pub fn from_component(
        component: &Component,
        plugin_name: &str,
        settings_section: &str,
        plugin_dir: &Path,
        allowed_hosts: Vec<String>,
    ) -> Result<Self, String> {
        let state = HostState::new(plugin_name, settings_section, plugin_dir, allowed_hosts);
        let linker = super::linker_for(&state)?;

        let mut store = Store::new(super::engine(), state);
        // Wire the memory/table/instance caps. `limiter` takes a closure pulling the
        // limiter out of store data, which is why `HostState` owns it.
        store.limiter(|s: &mut HostState| &mut s.limits);
        limits::refresh_for_call(&mut store)?;

        // Instantiation is where a missing import surfaces: a component that
        // references `sicompass:plugin/net` without `allowedHosts` fails right here,
        // which is the enforcement point for the whole capability model.
        let instance = Plugin::instantiate(&mut store, component, &linker)
            .map_err(|e| format!("instantiate {plugin_name}: {e}"))?;

        let me = WasmProvider {
            inner: RefCell::new(Inner {
                store,
                instance,
                pending_error: None,
                poisoned: false,
            }),
            descriptor: default_descriptor(plugin_name),
            current_path: "/".to_owned(),
            polled: default_poll(),
            dashboard_image: None,
        };

        // `init` before `describe`, so a plugin can compute its display name.
        me.call("init", |g, s| g.call_init(s))?;
        let descriptor = me.call("describe", |g, s| g.call_describe(s))?;

        // Resolve the static image path once, and only for a plugin that says it
        // has one.
        let dashboard_image = if descriptor.dashboard_kind == wit_types::DashboardKind::Image {
            me.resolve_dashboard_image(plugin_dir)
        } else {
            None
        };

        Ok(WasmProvider {
            descriptor,
            dashboard_image,
            ..me
        })
    }

    /// Ask the guest for its dashboard image and confine the answer.
    ///
    /// A sandboxed guest has no business naming a host path, so anything absolute or
    /// containing `..` is refused rather than normalized — the *host* is the one that
    /// opens this file, and without the check a plugin could point it at any file the
    /// user can read.
    fn resolve_dashboard_image(&self, plugin_dir: &Path) -> Option<String> {
        let rel = self
            .call("dashboard-image-path", |g, s| {
                g.call_dashboard_image_path(s)
            })
            .ok()
            .flatten()?;

        match HostState::new("", "", plugin_dir, Vec::new()).confine(&rel) {
            Ok(path) => Some(path.to_string_lossy().into_owned()),
            Err(e) => {
                self.note_error(format!(
                    "plugin `{}` asked for dashboard image `{rel}`: {e}",
                    self.descriptor.name
                ));
                None
            }
        }
    }

    /// Queue a message for `take_error` without poisoning the instance.
    ///
    /// For guest misbehaviour that is reportable but survivable — a malformed frame,
    /// an out-of-bounds image path — where killing the plugin outright would be a
    /// harsher response than the fault warrants.
    fn note_error(&self, msg: String) {
        tracing::warn!(target: "wasm_plugin", plugin = %self.descriptor.name, "{msg}");
        if let Ok(mut inner) = self.inner.try_borrow_mut()
            && inner.pending_error.is_none()
        {
            inner.pending_error = Some(msg);
        }
    }

    /// Run one guest export with limits re-armed and traps contained.
    ///
    /// Takes `&self` so the five `&self` trait methods can reach the guest too; see
    /// the module docs on why the `RefCell` is sound.
    ///
    /// Returns `Err` on trap or poison. Most callers translate that into the trait's
    /// "nothing happened" answer (`false`, `None`, empty) — the error still reaches
    /// the user through `take_error()`.
    fn call<T>(
        &self,
        what: &str,
        f: impl FnOnce(&Guest, &mut Store<HostState>) -> wasmtime::Result<T>,
    ) -> Result<T, String> {
        let Ok(mut guard) = self.inner.try_borrow_mut() else {
            // Unreachable today (no host function re-enters the provider). Degrading
            // to an error rather than panicking keeps a future one from aborting.
            return Err(format!(
                "plugin `{}` was called re-entrantly during `{what}`",
                self.descriptor.name
            ));
        };
        let inner = &mut *guard;

        if inner.poisoned {
            return Err(format!(
                "plugin `{}` is disabled after an earlier failure",
                self.descriptor.name
            ));
        }

        if let Err(e) = limits::refresh_for_call(&mut inner.store) {
            let msg = format!("{what}: {e}");
            poison(inner, &self.descriptor.name, msg.clone());
            return Err(msg);
        }

        // Disjoint field borrows: `guest` reads `instance`, and `store` is separate.
        let guest = inner.instance.sicompass_plugin_provider();
        match f(guest, &mut inner.store) {
            Ok(v) => Ok(v),
            Err(e) => {
                // Fuel exhaustion, epoch deadline, memory cap and guest panic all
                // arrive here. The distinction matters for the message, not the
                // handling: the instance is unusable either way.
                let msg = describe_trap(what, &self.descriptor.name, &e);
                poison(inner, &self.descriptor.name, msg.clone());
                Err(msg)
            }
        }
    }

    /// Whether this provider has been shut down by a trap.
    pub fn is_poisoned(&self) -> bool {
        self.inner.borrow().poisoned
    }

    /// Call a guest export returning an FFON blob, decoding it. A failure yields an
    /// empty tree; the reason is already queued for `take_error`.
    fn call_ffon(
        &self,
        what: &str,
        f: impl FnOnce(&Guest, &mut Store<HostState>) -> wasmtime::Result<Vec<u8>>,
    ) -> Vec<FfonElement> {
        match self.call(what, f) {
            Ok(blob) => ffon::deserialize_binary(&blob),
            Err(_) => Vec::new(),
        }
    }
}

/// Mark the instance unusable and queue the reason for display.
fn poison(inner: &mut Inner, plugin: &str, msg: String) {
    tracing::error!(target: "wasm_plugin", plugin = %plugin, "{msg}");
    inner.poisoned = true;
    // Do not clobber an earlier, more informative error.
    if inner.pending_error.is_none() {
        inner.pending_error = Some(msg);
    }
}

/// Values used before `describe()` has run, and after a trap.
fn default_descriptor(name: &str) -> wit_types::Descriptor {
    wit_types::Descriptor {
        name: name.to_owned(),
        display_name: name.to_owned(),
        version: None,
        supports_config_files: false,
        no_cache: false,
        path_is_filesystem: false,
        stable_root_key: false,
        has_editor_semantics: false,
        manual_dashboard_entry_allowed: true,
        dashboard_kind: wit_types::DashboardKind::None,
    }
}

fn default_poll() -> wit_types::PollResult {
    wit_types::PollResult {
        redraw: false,
        needs_refresh: false,
        is_busy: false,
        at_root: true,
        error: None,
        dashboard_request: None,
        navigation_request: None,
    }
}

/// Turn a wasmtime error into something a user can act on.
///
/// Wasmtime's own messages are accurate but assume the reader knows what fuel and
/// epochs are; a plugin user does not.
fn describe_trap(what: &str, plugin: &str, err: &wasmtime::Error) -> String {
    if let Some(trap) = err.downcast_ref::<wasmtime::Trap>() {
        let reason = match trap {
            wasmtime::Trap::OutOfFuel => "used too much CPU (possible infinite loop)".to_owned(),
            wasmtime::Trap::Interrupt => "took too long and was stopped".to_owned(),
            wasmtime::Trap::UnreachableCodeReached => "panicked".to_owned(),
            other => format!("faulted ({other})"),
        };
        return format!("plugin `{plugin}` {reason} during `{what}`; it has been disabled");
    }
    // Not a trap: a host-function error, or a memory-limit refusal surfaced as a
    // plain error. Keep wasmtime's text, which names the limit.
    format!("plugin `{plugin}` failed during `{what}`: {err}; it has been disabled")
}

/// Decode a blob the guest produced for a single element.
fn first_element(blob: &[u8]) -> Option<FfonElement> {
    ffon::deserialize_binary(blob).into_iter().next()
}

// ---------------------------------------------------------------------------
// Dashboard conversions
// ---------------------------------------------------------------------------

fn to_sdk_kind(k: wit_types::DashboardKind) -> DashboardKind {
    match k {
        wit_types::DashboardKind::None => DashboardKind::None,
        wit_types::DashboardKind::Image => DashboardKind::Image,
        wit_types::DashboardKind::Interactive => DashboardKind::Interactive,
    }
}

fn to_sdk_request(r: wit_types::DashboardRequest) -> DashboardRequest {
    match r {
        wit_types::DashboardRequest::Enter => DashboardRequest::Enter,
        wit_types::DashboardRequest::Leave => DashboardRequest::Leave,
    }
}

fn to_wit_keysym(sym: DashboardKeysym) -> wit_types::Keysym {
    use wit_types::Keysym as W;
    match sym {
        DashboardKeysym::Enter => W::Enter,
        DashboardKeysym::Backspace => W::Backspace,
        DashboardKeysym::Tab => W::Tab,
        DashboardKeysym::Escape => W::Escape,
        DashboardKeysym::Up => W::Up,
        DashboardKeysym::Down => W::Down,
        DashboardKeysym::Left => W::Left,
        DashboardKeysym::Right => W::Right,
        DashboardKeysym::Home => W::Home,
        DashboardKeysym::End => W::End,
        DashboardKeysym::PageUp => W::PageUp,
        DashboardKeysym::PageDown => W::PageDown,
        DashboardKeysym::Insert => W::Insert,
        DashboardKeysym::Delete => W::Delete,
        DashboardKeysym::F(n) => W::F(n),
        DashboardKeysym::Char(c) => W::Ch(c),
        DashboardKeysym::Unknown => W::Unknown,
    }
}

fn to_wit_key(key: DashboardKey) -> wit_types::Key {
    wit_types::Key {
        sym: to_wit_keysym(key.keysym),
        ctrl: key.ctrl,
        shift: key.shift,
        alt: key.alt,
    }
}

/// Turn a guest frame into one the renderer can draw.
///
/// Returns `None` when the guest's `cells` length disagrees with its own
/// `cols * rows`. That is not a recoverable frame: the renderer indexes by
/// `row * cols + col`, so a short buffer would panic and a long one would silently
/// draw the wrong thing. The caller substitutes a blank frame at the size the host
/// asked for.
fn to_sdk_frame(f: wit_types::Frame) -> Option<DashboardFrame> {
    let expected = f.cols as usize * f.rows as usize;
    if f.cells.len() != expected {
        return None;
    }

    let cells = f
        .cells
        .into_iter()
        .map(|c| DashboardCell {
            ch: c.ch,
            fg: c.fg,
            bg: c.bg,
            attrs: CellAttrs {
                bold: c.attrs.bold,
                underline: c.attrs.underline,
                reverse: c.attrs.reverse,
            },
        })
        .collect();

    // A cursor outside the grid is dropped rather than clamped: guessing where the
    // guest meant would put the screen reader's focus somewhere it never asked for.
    let cursor = f.cursor.filter(|(col, row)| *col < f.cols && *row < f.rows);

    Some(DashboardFrame {
        cols: f.cols,
        rows: f.rows,
        cells,
        cursor,
    })
}

// ---------------------------------------------------------------------------
// Provider impl
// ---------------------------------------------------------------------------

#[async_trait::async_trait]
impl Provider for WasmProvider {
    // ---- Identity: served from the cached descriptor ------------------------

    fn name(&self) -> &str {
        &self.descriptor.name
    }

    fn display_name(&self) -> String {
        self.descriptor.display_name.clone()
    }

    fn version(&self) -> Option<&str> {
        self.descriptor.version.as_deref()
    }

    // ---- Data source -------------------------------------------------------

    fn fetch(&mut self) -> Vec<FfonElement> {
        self.call_ffon("fetch", |g, s| g.call_fetch(s))
    }

    fn fetch_subtree_children(&mut self) -> Option<Vec<FfonElement>> {
        self.call("fetch-subtree-children", |g, s| {
            g.call_fetch_subtree_children(s)
        })
        .ok()
        .flatten()
        .map(|blob| ffon::deserialize_binary(&blob))
    }

    fn fetch_subtree_parent_key(&mut self) -> Option<String> {
        self.call("fetch-subtree-parent-key", |g, s| {
            g.call_fetch_subtree_parent_key(s)
        })
        .ok()
        .flatten()
    }

    fn sync_ffon_body_children(&mut self, children: &[FfonElement]) {
        let blob = ffon::serialize_binary(children);
        let _ = self.call("sync-ffon-body-children", |g, s| {
            g.call_sync_ffon_body_children(s, &blob)
        });
    }

    // ---- Lifecycle ---------------------------------------------------------

    fn init(&mut self) {
        // Already called in `from_component`, so the provider is usable the moment it
        // exists. Calling `init` twice would be visible to a guest.
    }

    fn cleanup(&mut self) {
        let _ = self.call("cleanup", |g, s| g.call_cleanup(s));
    }

    // ---- Per-frame: one crossing, then cached ------------------------------

    fn tick(&mut self) -> bool {
        match self.call("poll", |g, s| g.call_poll(s)) {
            Ok(p) => {
                self.polled = p;
                if let Some(err) = self.polled.error.take() {
                    let mut inner = self.inner.borrow_mut();
                    if inner.pending_error.is_none() {
                        inner.pending_error = Some(err);
                    }
                }
                self.polled.redraw
            }
            // A trap during poll already queued an error and poisoned us; ask for a
            // redraw so the error row appears promptly.
            Err(_) => true,
        }
    }

    fn needs_refresh(&self) -> bool {
        self.polled.needs_refresh
    }

    fn clear_needs_refresh(&mut self) {
        self.polled.needs_refresh = false;
    }

    fn is_busy(&self) -> bool {
        self.polled.is_busy
    }

    fn at_root(&self) -> bool {
        self.polled.at_root
    }

    fn take_error(&mut self) -> Option<String> {
        self.inner.borrow_mut().pending_error.take()
    }

    fn take_navigation_request(&mut self) -> Option<NavigationRequest> {
        // `take`, not read: the trait requires a second call to return None.
        self.polled.navigation_request.take().map(|r| match r {
            wit_types::NavigationRequest::EnterChildren => NavigationRequest::EnterChildren,
        })
    }

    // ---- Navigation --------------------------------------------------------

    fn current_path(&self) -> &str {
        &self.current_path
    }

    fn push_path(&mut self, segment: &str) {
        if let Ok(p) = self.call("push-path", |g, s| g.call_push_path(s, segment)) {
            self.current_path = p;
        }
    }

    fn pop_path(&mut self) {
        if let Ok(p) = self.call("pop-path", |g, s| g.call_pop_path(s)) {
            self.current_path = p;
        }
    }

    fn set_current_path(&mut self, path: &str) {
        if let Ok(p) = self.call("set-current-path", |g, s| g.call_set_current_path(s, path)) {
            self.current_path = p;
        }
    }

    // ---- Editing and file operations ---------------------------------------

    fn commit_edit(&mut self, old: &str, new: &str) -> bool {
        self.call("commit-edit", |g, s| g.call_commit_edit(s, old, new))
            .unwrap_or(false)
    }

    fn create_directory(&mut self, name: &str) -> bool {
        self.call("create-directory", |g, s| g.call_create_directory(s, name))
            .unwrap_or(false)
    }

    fn create_file(&mut self, name: &str) -> bool {
        self.call("create-file", |g, s| g.call_create_file(s, name))
            .unwrap_or(false)
    }

    fn delete_item(&mut self, name: &str) -> bool {
        self.call("delete-item", |g, s| g.call_delete_item(s, name))
            .unwrap_or(false)
    }

    fn copy_item(
        &mut self,
        src_dir: &str,
        src_name: &str,
        dest_dir: &str,
        dest_name: &str,
    ) -> bool {
        self.call("copy-item", |g, s| {
            g.call_copy_item(s, src_dir, src_name, dest_dir, dest_name)
        })
        .unwrap_or(false)
    }

    // ---- Commands ----------------------------------------------------------

    fn commands(&self) -> Vec<String> {
        self.call("commands", |g, s| g.call_commands(s))
            .unwrap_or_default()
    }

    fn command_label(&self, cmd: &str) -> String {
        self.call("command-label", |g, s| g.call_command_label(s, cmd))
            .unwrap_or_else(|_| cmd.to_owned())
    }

    fn handle_command(
        &mut self,
        cmd: &str,
        elem_key: &str,
        elem_type: i32,
        error: &mut String,
    ) -> Option<FfonElement> {
        match self.call("handle-command", |g, s| {
            g.call_handle_command(s, cmd, elem_key, elem_type)
        }) {
            // The guest reported a domain error through `result<_, string>`.
            Ok(Err(msg)) => {
                *error = msg;
                None
            }
            Ok(Ok(blob)) => blob.as_deref().and_then(first_element),
            Err(trap) => {
                *error = trap;
                None
            }
        }
    }

    fn command_list_items(&self, cmd: &str) -> Vec<ListItem> {
        self.call("command-list-items", |g, s| {
            g.call_command_list_items(s, cmd)
        })
        .map(|items| {
            items
                .into_iter()
                .map(|i| ListItem {
                    label: i.label,
                    data: i.data,
                })
                .collect()
        })
        .unwrap_or_default()
    }

    fn execute_command(&mut self, cmd: &str, selection: &str) -> bool {
        self.call("execute-command", |g, s| {
            g.call_execute_command(s, cmd, selection)
        })
        .unwrap_or(false)
    }

    fn create_element(&mut self, key: &str) -> Option<FfonElement> {
        self.call("create-element", |g, s| g.call_create_element(s, key))
            .ok()
            .flatten()
            .as_deref()
            .and_then(first_element)
    }

    // ---- Interactive callbacks ---------------------------------------------

    fn on_radio_change(&mut self, group: &str, value: &str) {
        let _ = self.call("on-radio-change", |g, s| {
            g.call_on_radio_change(s, group, value)
        });
    }

    fn on_button_press(&mut self, function_name: &str) {
        let _ = self.call("on-button-press", |g, s| {
            g.call_on_button_press(s, function_name)
        });
    }

    fn on_checkbox_change(&mut self, label: &str, checked: bool) {
        let _ = self.call("on-checkbox-change", |g, s| {
            g.call_on_checkbox_change(s, label, checked)
        });
    }

    fn set_input_value(&mut self, value: &str) {
        let _ = self.call("set-input-value", |g, s| g.call_set_input_value(s, value));
    }

    fn on_setting_change(&mut self, key: &str, value: &str) {
        let _ = self.call("on-setting-change", |g, s| {
            g.call_on_setting_change(s, key, value)
        });
    }

    // ---- Timeline undo/redo ------------------------------------------------
    //
    // A guest may only emit `ProviderOp`; `provider_idx` is the host's to fill in,
    // and it is the only side that knows it. The app patches it when pushing onto the
    // tab's timeline, so 0 here is a placeholder, not a claim.

    fn take_timeline_entries(&mut self) -> Vec<TimelineEntry> {
        let ops = self
            .call("take-timeline-entries", |g, s| {
                g.call_take_timeline_entries(s)
            })
            .unwrap_or_default();
        ops.into_iter()
            .map(|op| TimelineEntry::ProviderOp {
                provider_idx: 0,
                command: op.command,
                payload: first_element(&op.payload).unwrap_or_else(|| FfonElement::new_str("")),
                label: op.label,
            })
            .collect()
    }

    async fn undo(&mut self, entry: &TimelineEntry, error: &mut String) {
        // Synchronous guest call; the trait is async only because native providers
        // may await real I/O. Nothing to await here.
        if let Some(op) = to_wit_op(entry) {
            match self.call("undo", |g, s| g.call_undo(s, &op)) {
                Ok(Err(msg)) | Err(msg) => *error = msg,
                Ok(Ok(())) => {}
            }
        }
    }

    async fn redo(&mut self, entry: &TimelineEntry, error: &mut String) {
        if let Some(op) = to_wit_op(entry) {
            match self.call("redo", |g, s| g.call_redo(s, &op)) {
                Ok(Err(msg)) | Err(msg) => *error = msg,
                Ok(Ok(())) => {}
            }
        }
    }

    // ---- Extended search ---------------------------------------------------

    fn collect_extended_search_items(&self) -> Option<Vec<SearchResultItem>> {
        self.call("collect-extended-search-items", |g, s| {
            g.call_collect_extended_search_items(s)
        })
        .ok()
        .flatten()
        .map(|items| {
            items
                .into_iter()
                .map(|i| SearchResultItem {
                    label: i.label,
                    breadcrumb: i.breadcrumb,
                    nav_path: i.nav_path,
                })
                .collect()
        })
    }

    // ---- Behaviour flags: cached descriptor --------------------------------

    fn supports_config_files(&self) -> bool {
        self.descriptor.supports_config_files
    }

    fn no_cache(&self) -> bool {
        self.descriptor.no_cache
    }

    fn path_is_filesystem(&self) -> bool {
        self.descriptor.path_is_filesystem
    }

    fn stable_root_key(&self) -> bool {
        self.descriptor.stable_root_key
    }

    fn has_editor_semantics(&self) -> bool {
        self.descriptor.has_editor_semantics
    }

    // ---- Persistent config -------------------------------------------------
    //
    // The host owns the file; the guest only ever sees bytes.

    fn load_config(&mut self, path: &Path) -> bool {
        let Ok(contents) = std::fs::read(path) else {
            return false;
        };
        self.call("load-config", |g, s| g.call_load_config(s, &contents))
            .unwrap_or(false)
    }

    fn save_config(&self, path: &Path) -> bool {
        match self.call("save-config", |g, s| g.call_save_config(s)) {
            Ok(Some(bytes)) => std::fs::write(path, bytes).is_ok(),
            // The guest declined to produce a config, or trapped.
            Ok(None) | Err(_) => false,
        }
    }

    // ---- Dashboard ---------------------------------------------------------

    fn dashboard_kind(&self) -> DashboardKind {
        to_sdk_kind(self.descriptor.dashboard_kind)
    }

    fn manual_dashboard_entry_allowed(&self) -> bool {
        self.descriptor.manual_dashboard_entry_allowed
    }

    fn dashboard_image_path(&self) -> Option<&str> {
        // Resolved and confined once at construction. The trait returns a borrow,
        // which a guest call cannot produce, and re-confining a path on every frame
        // would be wasted work for something that does not change.
        self.dashboard_image.as_deref()
    }

    fn take_dashboard_request(&mut self) -> Option<DashboardRequest> {
        // `take`, not read: a second call without an intervening request must
        // return None.
        self.polled.dashboard_request.take().map(to_sdk_request)
    }

    /// Render one frame of the interactive dashboard.
    ///
    /// # Measured cost, and a real limitation
    ///
    /// Profiled with `profile_dashboard_render_cost` against the `hello` fixture:
    ///
    /// | grid            | Cranelift | Pulley  | µs/cell (Pulley) |
    /// |-----------------|-----------|---------|------------------|
    /// | 20x6   (120)    |           | 3.06ms  | 25.5             |
    /// | 40x12  (480)    |           | 8.11ms  | 16.9             |
    /// | 80x24  (1920)   | 0.49ms    | 26.16ms | 13.6             |
    /// | 160x48 (7680)   |           | 98.6ms  | 12.8             |
    ///
    /// A 60fps frame is 16.67ms for *everything*, Vulkan and AccessKit included.
    ///
    /// So: **fine under Cranelift (3% of a frame at 80x24), and over budget under
    /// Pulley** — 157% at 80x24, i.e. the App Store build cannot drive a full-screen
    /// interactive dashboard at 60fps. Smaller grids are usable (40x12 is ~49%).
    ///
    /// The cost is linear in cell count with negligible fixed overhead (`poll()`,
    /// a small record, costs 43µs), so it is not the crossing — it is the guest
    /// lowering N six-field records through the Canonical ABI under an interpreter.
    /// The structural fix is a flatter wire format (parallel `list<u32>`, which
    /// lowers closer to a memcpy) rather than calling less often; that is a WIT
    /// change and has not been made. Recorded here so the next person has the
    /// numbers rather than a hunch.
    fn dashboard_render(&mut self, cols: u16, rows: u16) -> DashboardFrame {
        match self.call("dashboard-render", |g, s| {
            g.call_dashboard_render(s, cols, rows)
        }) {
            Ok(frame) => match to_sdk_frame(frame) {
                Some(f) => f,
                None => {
                    // A malformed frame is the guest's bug, but it must not become
                    // an index panic in the renderer. Report it once and keep
                    // drawing blanks; poisoning would be a harsh response to what
                    // may be a transient off-by-one during a resize.
                    self.note_error(format!(
                        "plugin `{}` returned a dashboard frame whose cell count did \
                         not match its own {cols}x{rows} grid",
                        self.descriptor.name
                    ));
                    DashboardFrame::empty(cols, rows)
                }
            },
            // Trapped: already poisoned and reported.
            Err(_) => DashboardFrame::empty(cols, rows),
        }
    }

    fn dashboard_key(&mut self, key: DashboardKey) -> bool {
        let k = to_wit_key(key);
        self.call("dashboard-key", |g, s| g.call_dashboard_key(s, k))
            .unwrap_or(false)
    }

    fn dashboard_text(&mut self, text: &str) {
        let _ = self.call("dashboard-text", |g, s| g.call_dashboard_text(s, text));
    }

    fn dashboard_paste(&mut self, text: &str) {
        // Distinct from `dashboard_text` at the WIT level too, so a guest can wrap
        // it in bracketed-paste markers if it is emulating a terminal.
        let _ = self.call("dashboard-paste", |g, s| g.call_dashboard_paste(s, text));
    }

    fn dashboard_resize(&mut self, rows: u16, cols: u16) {
        let _ = self.call("dashboard-resize", |g, s| {
            g.call_dashboard_resize(s, rows, cols)
        });
    }

    fn enter_dashboard(&mut self) {
        let _ = self.call("enter-dashboard", |g, s| g.call_enter_dashboard(s));
    }

    fn leave_dashboard(&mut self) {
        let _ = self.call("leave-dashboard", |g, s| g.call_leave_dashboard(s));
    }
}

/// Convert a timeline entry back into the guest's record, if the guest could have
/// emitted it. Anything else is a host-only variant and is not the guest's to undo.
fn to_wit_op(entry: &TimelineEntry) -> Option<wit_types::ProviderOp> {
    match entry {
        TimelineEntry::ProviderOp {
            command,
            payload,
            label,
            ..
        } => Some(wit_types::ProviderOp {
            command: command.clone(),
            payload: ffon::serialize_binary(std::slice::from_ref(payload)),
            label: label.clone(),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_descriptor_uses_the_manifest_name_until_describe_runs() {
        let d = default_descriptor("my-plugin");
        assert_eq!(d.name, "my-plugin");
        assert_eq!(d.display_name, "my-plugin");
        // Matches the SDK trait defaults, so a plugin that traps during `describe`
        // still behaves like a plain, inert provider.
        assert!(d.manual_dashboard_entry_allowed);
        assert!(!d.supports_config_files);
        assert!(!d.no_cache);
        assert!(!d.has_editor_semantics);
    }

    #[test]
    fn default_poll_is_quiet_and_at_root() {
        let p = default_poll();
        assert!(!p.redraw);
        assert!(!p.needs_refresh);
        assert!(!p.is_busy);
        assert!(p.at_root);
        assert!(p.error.is_none());
        assert!(p.navigation_request.is_none());
        assert!(p.dashboard_request.is_none());
    }

    #[test]
    fn trap_messages_name_the_plugin_and_the_cause() {
        // Users see these as an error row, so they must say which plugin and why, in
        // terms that mean something outside wasmtime.
        let fuel: wasmtime::Error = wasmtime::Trap::OutOfFuel.into();
        let msg = describe_trap("fetch", "weather", &fuel);
        assert!(msg.contains("weather"), "{msg}");
        assert!(msg.contains("too much CPU"), "{msg}");
        assert!(msg.contains("fetch"), "{msg}");
        assert!(msg.contains("disabled"), "{msg}");

        let timeout: wasmtime::Error = wasmtime::Trap::Interrupt.into();
        assert!(describe_trap("poll", "w", &timeout).contains("took too long"));

        let panicked: wasmtime::Error = wasmtime::Trap::UnreachableCodeReached.into();
        assert!(describe_trap("fetch", "w", &panicked).contains("panicked"));
    }

    #[test]
    fn non_trap_errors_keep_wasmtime_text() {
        // Memory-limit refusals arrive as plain errors, and their text names the
        // limit, which is the useful part.
        let err = wasmtime::Error::msg("memory minimum size of 5000 pages exceeds limit");
        let msg = describe_trap("fetch", "hoarder", &err);
        assert!(msg.contains("exceeds limit"), "{msg}");
        assert!(msg.contains("hoarder"), "{msg}");
    }

    // --- timeline mapping ---

    #[test]
    fn only_provider_op_round_trips_to_the_guest() {
        let entry = TimelineEntry::ProviderOp {
            provider_idx: 7,
            command: "greet".to_owned(),
            payload: FfonElement::new_str("world"),
            label: "greet world".to_owned(),
        };
        let op = to_wit_op(&entry).expect("ProviderOp must convert");
        assert_eq!(op.command, "greet");
        assert_eq!(op.label, "greet world");
        assert_eq!(
            first_element(&op.payload),
            Some(FfonElement::new_str("world"))
        );
    }

    #[test]
    fn host_only_timeline_variants_are_not_offered_to_a_guest() {
        // FsOp and friends carry side effects the host would have to replay for the
        // guest, so a guest must never be asked to undo one.
        let entry = TimelineEntry::FsOp {
            provider_idx: 0,
            id: sicompass_sdk::IdArray::new(),
            op: sicompass_sdk::FsOpKind::Create,
            before: None,
            after: None,
            side_effect: sicompass_sdk::FsSideEffect::None,
        };
        assert!(to_wit_op(&entry).is_none());
    }

    // --- dashboard conversions ---

    fn wit_cell(ch: char) -> wit_types::Cell {
        wit_types::Cell {
            ch,
            fg: 0xFFFF_FFFF,
            bg: 0,
            attrs: wit_types::CellAttrs {
                bold: false,
                underline: false,
                reverse: false,
            },
        }
    }

    fn wit_frame(cols: u16, rows: u16, cell_count: usize) -> wit_types::Frame {
        wit_types::Frame {
            cols,
            rows,
            cells: (0..cell_count).map(|_| wit_cell('x')).collect(),
            cursor: None,
        }
    }

    #[test]
    fn a_well_formed_frame_converts_intact() {
        let f = to_sdk_frame(wit_frame(4, 2, 8)).expect("8 cells for a 4x2 grid");
        assert_eq!(f.cols, 4);
        assert_eq!(f.rows, 2);
        assert_eq!(f.cells.len(), 8);
        assert_eq!(f.cells[0].ch, 'x');
        assert_eq!(f.cells[0].fg, 0xFFFF_FFFF);
    }

    #[test]
    fn a_frame_whose_cell_count_disagrees_with_its_grid_is_rejected() {
        // The renderer indexes by `row * cols + col`, so a short buffer would panic
        // and a long one would silently draw the wrong thing. Neither is acceptable
        // from untrusted code, so the frame is refused rather than patched up.
        assert!(to_sdk_frame(wit_frame(4, 2, 7)).is_none(), "short buffer");
        assert!(to_sdk_frame(wit_frame(4, 2, 9)).is_none(), "long buffer");
        assert!(to_sdk_frame(wit_frame(4, 2, 0)).is_none(), "empty buffer");
    }

    #[test]
    fn a_zero_sized_frame_is_accepted_when_it_is_consistent() {
        // Legitimate during a resize to nothing; 0 cells for a 0x0 grid is coherent.
        assert!(to_sdk_frame(wit_frame(0, 0, 0)).is_some());
    }

    #[test]
    fn a_cursor_outside_the_grid_is_dropped_rather_than_clamped() {
        // Clamping would guess where the guest meant, and the cursor is where the
        // screen reader's focus goes — a wrong guess is worse than none.
        let mut f = wit_frame(4, 2, 8);
        f.cursor = Some((4, 0));
        assert_eq!(to_sdk_frame(f).unwrap().cursor, None);

        let mut f = wit_frame(4, 2, 8);
        f.cursor = Some((0, 2));
        assert_eq!(to_sdk_frame(f).unwrap().cursor, None);

        let mut f = wit_frame(4, 2, 8);
        f.cursor = Some((3, 1));
        assert_eq!(to_sdk_frame(f).unwrap().cursor, Some((3, 1)));
    }

    #[test]
    fn cell_attributes_survive_the_boundary() {
        let mut f = wit_frame(1, 1, 1);
        f.cells[0].attrs = wit_types::CellAttrs {
            bold: true,
            underline: false,
            reverse: true,
        };
        f.cells[0].bg = 0x1234_5678;
        let out = to_sdk_frame(f).unwrap();
        assert!(out.cells[0].attrs.bold);
        assert!(!out.cells[0].attrs.underline);
        assert!(out.cells[0].attrs.reverse);
        assert_eq!(out.cells[0].bg, 0x1234_5678);
    }

    #[test]
    fn every_keysym_maps_across_without_collapsing() {
        // A dropped variant would silently turn one key into another, so check the
        // mapping is injective over the whole set rather than spot-checking.
        let all = [
            DashboardKeysym::Enter,
            DashboardKeysym::Backspace,
            DashboardKeysym::Tab,
            DashboardKeysym::Escape,
            DashboardKeysym::Up,
            DashboardKeysym::Down,
            DashboardKeysym::Left,
            DashboardKeysym::Right,
            DashboardKeysym::Home,
            DashboardKeysym::End,
            DashboardKeysym::PageUp,
            DashboardKeysym::PageDown,
            DashboardKeysym::Insert,
            DashboardKeysym::Delete,
            DashboardKeysym::F(5),
            DashboardKeysym::Char('q'),
            DashboardKeysym::Unknown,
        ];
        let mapped: Vec<String> = all
            .iter()
            .map(|k| format!("{:?}", to_wit_keysym(*k)))
            .collect();
        let unique: std::collections::BTreeSet<&String> = mapped.iter().collect();
        assert_eq!(unique.len(), all.len(), "two keysyms collapsed: {mapped:?}");

        assert!(matches!(
            to_wit_keysym(DashboardKeysym::F(5)),
            wit_types::Keysym::F(5)
        ));
        assert!(matches!(
            to_wit_keysym(DashboardKeysym::Char('q')),
            wit_types::Keysym::Ch('q')
        ));
    }

    #[test]
    fn key_modifiers_are_carried_across() {
        let k = to_wit_key(DashboardKey {
            keysym: DashboardKeysym::Char('c'),
            ctrl: true,
            shift: false,
            alt: true,
        });
        assert!(k.ctrl);
        assert!(!k.shift);
        assert!(k.alt);
    }

    #[test]
    fn dashboard_kind_and_request_map_one_to_one() {
        assert_eq!(
            to_sdk_kind(wit_types::DashboardKind::None),
            DashboardKind::None
        );
        assert_eq!(
            to_sdk_kind(wit_types::DashboardKind::Image),
            DashboardKind::Image
        );
        assert_eq!(
            to_sdk_kind(wit_types::DashboardKind::Interactive),
            DashboardKind::Interactive
        );
        assert_eq!(
            to_sdk_request(wit_types::DashboardRequest::Enter),
            DashboardRequest::Enter
        );
        assert_eq!(
            to_sdk_request(wit_types::DashboardRequest::Leave),
            DashboardRequest::Leave
        );
    }

    #[test]
    fn first_element_of_an_empty_blob_is_none() {
        assert!(first_element(&ffon::serialize_binary(&[])).is_none());
    }

    #[test]
    fn first_element_decodes_a_single_element_payload() {
        let blob = ffon::serialize_binary(&[FfonElement::new_str("only")]);
        assert_eq!(first_element(&blob), Some(FfonElement::new_str("only")));
    }
}
