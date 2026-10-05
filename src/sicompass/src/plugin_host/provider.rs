//! [`ProcessProvider`]: a plugin process wearing the SDK's `Provider` trait.
//!
//! The rest of the app cannot tell a plugin process from a compiled-in
//! built-in or a WASM component: all three arrive as `Box<dyn Provider>`. This
//! follows `wasm_host::WasmProvider` closely, so the two behave alike: the same
//! batched `poll` once per frame, the same caches, the same error shown once.
//!
//! ## Failure
//!
//! A plugin can crash, hang or answer nonsense. None of that may take the app
//! down, so every call goes through [`ProcessProvider::call`], which turns a
//! panic, an exit or a missed [`super::channel::CALL_DEADLINE`] (after which the
//! process is killed) into an error row, and **poisons** the provider: every
//! later call answers like an inert provider. A broken plugin is visible as a
//! broken plugin, never as a broken app.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use sicompass_sdk::plugin_ipc::{
    self as ipc, CursorStyle, Descriptor, InitInfo, PollResult, Request, Response,
};
use sicompass_sdk::{
    CellAttrs, DashboardCell, DashboardCursor, DashboardFrame, DashboardKey, DashboardKeysym,
    DashboardKind, DashboardPalette, DashboardRequest, DashboardSelection, FfonElement, ListItem,
    NavigationRequest, Provider, SearchResultItem, TimelineEntry, ffon,
};

use super::channel::{Answer, CallError, Channel};
use super::services::{Renders, Services};

/// What a call mutates, behind a `RefCell` because five `Provider` methods
/// that reach the plugin take `&self`. The plugin answers one call at a time
/// and never calls back into its own provider, so no borrow ever conflicts.
struct State {
    /// An error waiting to be shown as a row.
    pending_error: Option<String>,
    /// Set after a failure. No further calls go out.
    poisoned: bool,
    /// Where the last reply said the plugin moved to, until it is taken.
    moved_to: Option<String>,
}

/// Where a plugin process lives and what it is granted.
pub struct Spec<'a> {
    /// The executable: `entry` from `plugin.json` resolved against
    /// `plugin_dir`, without the platform's `.exe`.
    pub entry_path: &'a Path,
    /// The manifest `name`.
    pub plugin_name: &'a str,
    /// The manifest `displayName`, which names its settings section.
    pub settings_section: &'a str,
    pub plugin_dir: &'a Path,
    pub grants: crate::wasm_host::Grants,
}

/// A plugin process driven through the `Provider` trait.
pub struct ProcessProvider {
    channel: Channel,
    state: RefCell<State>,
    /// The manifest name, which names the plugin in messages and assets.
    plugin_name: String,
    descriptor: Descriptor,
    /// Mirror of the plugin's path, kept by the navigation replies and
    /// `moved_to`, so `current_path` needs no call.
    current_path: String,
    /// The last poll. The `take_*` methods drain it.
    polled: PollResult,
    /// The checked image of a `DashboardKind::Image` plugin.
    dashboard_image: Option<String>,
    setting_keys: Vec<String>,
    renders_pages: bool,
    renders: Arc<Renders>,
    /// The app's language when the plugin last heard of it.
    locale: String,
}

/// The executable for `entry_path` on this platform.
pub fn executable(entry_path: &Path) -> PathBuf {
    let suffix = std::env::consts::EXE_SUFFIX;
    if suffix.is_empty() || entry_path.to_string_lossy().ends_with(suffix) {
        entry_path.to_path_buf()
    } else {
        let mut s = entry_path.as_os_str().to_owned();
        s.push(suffix);
        PathBuf::from(s)
    }
}

impl ProcessProvider {
    /// Start a plugin process and initialise it.
    pub fn open(spec: Spec<'_>) -> Result<Self, String> {
        let Spec {
            entry_path,
            plugin_name,
            settings_section,
            plugin_dir,
            grants,
        } = spec;

        // Before `init`, which may already translate the display name.
        for refusal in crate::wasm_host::register_plugin_locales(plugin_name, plugin_dir) {
            tracing::warn!(target: "plugin", plugin = %plugin_name, "{refusal}");
            eprintln!("plugin '{plugin_name}': {refusal}");
        }

        let storage_dir = match &grants.storage_dir {
            Some(dir) => {
                std::fs::create_dir_all(dir)
                    .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
                Some(dir.to_string_lossy().into_owned())
            }
            None => None,
        };

        let renders = Arc::new(Renders::default());
        let closed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let services = Arc::new(Services {
            plugin_name: plugin_name.to_owned(),
            settings_section: settings_section.to_owned(),
            setting_defaults: grants.setting_defaults.clone(),
            setting_keys: grants.settings.clone(),
            service_tier: grants.service_tier.clone(),
            renders: renders.clone(),
            closed: closed.clone(),
        });
        let answer: Answer = {
            let services = services.clone();
            Arc::new(move |request| services.answer(request))
        };
        let exe = executable(entry_path);
        // The channel sets `closed` when the plugin is gone, which also ends a
        // sign-in it is waiting on.
        let channel = Channel::spawn(&exe, plugin_dir, plugin_name, answer, closed)
            .map_err(|e| format!("{}: {e}", exe.display()))?;

        let mut me = ProcessProvider {
            channel,
            state: RefCell::new(State {
                pending_error: None,
                poisoned: false,
                moved_to: None,
            }),
            plugin_name: plugin_name.to_owned(),
            descriptor: default_descriptor(plugin_name),
            current_path: "/".to_owned(),
            polled: PollResult::default(),
            dashboard_image: None,
            setting_keys: grants.settings.clone(),
            renders_pages: false,
            renders,
            locale: sicompass_sdk::localize::current_locale(),
        };

        // `init` before `describe`, so a plugin can compute its display name.
        expect_unit(me.call(
            "init",
            Request::Init(InitInfo {
                plugin_dir: plugin_dir.to_string_lossy().into_owned(),
                storage_dir,
                settings: services.settings(),
            }),
        ))?;
        let descriptor = match me.call("describe", Request::Describe)? {
            Response::Descriptor(d) => d,
            other => return Err(me.wrong_answer("describe", &other)),
        };

        // `asset:<plugin-name>/<file>` resolves to this plugin's `assets/`,
        // before the dashboard image below is checked against it.
        crate::wasm_host::register_plugin_assets(plugin_name, plugin_dir);
        let dashboard_image = if descriptor.dashboard_kind == ipc::DashboardKind::Image {
            me.resolve_dashboard_image(plugin_dir)
        } else {
            None
        };
        me.descriptor = descriptor;
        me.dashboard_image = dashboard_image;
        // Where `init` put it: the path the app records before the first poll
        // has to be the real one, or undoing the first navigation goes to `/`.
        me.take_moved_to();
        if grants.renders_pages {
            me.renders_pages = true;
            sicompass_sdk::url_fetcher::set_renderer_available(true);
        }
        Ok(me)
    }

    /// One call, with failures turned into a poisoned provider and an error
    /// row. Takes `&self` so the `&self` trait methods reach the plugin too.
    fn call(&self, what: &str, request: Request) -> Result<Response, String> {
        {
            let Ok(state) = self.state.try_borrow() else {
                return Err(format!(
                    "plugin `{}` was called re-entrantly during `{what}`",
                    self.plugin_name
                ));
            };
            if state.poisoned {
                return Err(format!(
                    "plugin `{}` is disabled after an earlier failure",
                    self.plugin_name
                ));
            }
        }
        let result = outside_async(|| self.channel.call(request));
        let failure = match result {
            Ok((Response::Failed(msg), _)) => {
                format!(
                    "plugin `{}` panicked during `{what}`: {msg}; it has been disabled",
                    self.plugin_name
                )
            }
            Ok((Response::Unsupported, _)) => {
                // An older plugin that does not know this call: answer as the
                // trait's default would, and keep it running.
                return Err(format!(
                    "plugin `{}` does not support `{what}`",
                    self.plugin_name
                ));
            }
            Ok((response, moved_to)) => {
                if let Some(path) = moved_to
                    && let Ok(mut s) = self.state.try_borrow_mut()
                {
                    s.moved_to = Some(path);
                }
                return Ok(response);
            }
            Err(CallError::TimedOut) => format!(
                "plugin `{}` took too long during `{what}` and was stopped; it has been disabled",
                self.plugin_name
            ),
            Err(CallError::Closed(why)) => format!(
                "plugin `{}` stopped during `{what}`: {why}; it has been disabled",
                self.plugin_name
            ),
        };
        self.poison(failure.clone());
        Err(failure)
    }

    fn poison(&self, msg: String) {
        tracing::error!(target: "plugin", plugin = %self.plugin_name, "{msg}");
        if let Ok(mut s) = self.state.try_borrow_mut() {
            s.poisoned = true;
            if s.pending_error.is_none() {
                s.pending_error = Some(msg);
            }
        }
        self.channel.kill();
    }

    /// An answer of the wrong shape: a broken plugin, poisoned like a crash.
    fn wrong_answer(&self, what: &str, got: &Response) -> String {
        let msg = format!(
            "plugin `{}` answered `{what}` with {got:?}; it has been disabled",
            self.plugin_name
        );
        self.poison(msg.clone());
        msg
    }

    /// Queue a message for `take_error` without stopping the plugin.
    fn note_error(&self, msg: String) {
        tracing::warn!(target: "plugin", plugin = %self.plugin_name, "{msg}");
        if let Ok(mut s) = self.state.try_borrow_mut()
            && s.pending_error.is_none()
        {
            s.pending_error = Some(msg);
        }
    }

    /// Whether this provider has been shut down by a failure.
    pub fn is_poisoned(&self) -> bool {
        self.state.borrow().poisoned
    }

    /// How long one call may take before the plugin is stopped. For tests,
    /// and for a host that knows better than the default.
    pub fn set_call_deadline(&self, deadline: Option<std::time::Duration>) {
        self.channel.set_deadline(deadline);
    }

    /// The plugin process's own id (not the program it runs for the user).
    pub fn plugin_pid(&self) -> u32 {
        self.channel.pid()
    }

    /// Take where the plugin said it moved to.
    fn take_moved_to(&mut self) {
        let moved = self.state.get_mut().moved_to.take();
        if let Some(path) = moved {
            self.current_path = path;
        }
    }

    /// A call from a `&mut self` method: the answer, with the path it moved to
    /// applied straight away.
    fn req(&mut self, what: &str, request: Request) -> Result<Response, String> {
        let r = self.call(what, request);
        self.take_moved_to();
        r
    }

    fn req_bool(&mut self, what: &str, request: Request) -> bool {
        match self.req(what, request) {
            Ok(Response::Bool(b)) => b,
            Ok(other) => {
                self.wrong_answer(what, &other);
                false
            }
            Err(_) => false,
        }
    }

    fn req_unit(&mut self, what: &str, request: Request) {
        match self.req(what, request) {
            Ok(Response::Unit) | Err(_) => {}
            Ok(other) => {
                self.wrong_answer(what, &other);
            }
        }
    }

    /// A navigation call: the reply is the new path.
    fn navigate(&mut self, what: &str, request: Request) {
        match self.req(what, request) {
            Ok(Response::Str(p)) => {
                self.current_path = p;
                self.repoll();
            }
            Ok(other) => {
                self.wrong_answer(what, &other);
            }
            Err(_) => {}
        }
    }

    /// Keep a poll's answers. An error goes to the pending slot; a request an
    /// earlier poll left untaken survives a poll that has none.
    fn take_poll(&mut self, mut p: PollResult) {
        if let Some(err) = p.error.take() {
            let s = self.state.get_mut();
            if s.pending_error.is_none() {
                s.pending_error = Some(err);
            }
        }
        if p.dashboard_request.is_none() {
            p.dashboard_request = self.polled.dashboard_request.take();
        }
        if p.navigation_request.is_none() {
            p.navigation_request = self.polled.navigation_request.take();
        }
        if p.announcement.is_none() {
            p.announcement = self.polled.announcement.take();
        }
        p.needs_refresh |= self.polled.needs_refresh;
        self.polled = p;
    }

    fn poll(&mut self) -> Result<(), String> {
        match self.req("poll", Request::Poll)? {
            Response::Poll(p) => {
                self.take_poll(p);
                Ok(())
            }
            other => Err(self.wrong_answer("poll", &other)),
        }
    }

    /// Poll right after navigating, so the per-level answers are for the
    /// level the user is now on.
    fn repoll(&mut self) {
        let _ = self.poll();
    }

    fn call_ffon(&mut self, what: &str, request: Request) -> Vec<FfonElement> {
        match self.req(what, request) {
            Ok(Response::Ffon(blob)) => ffon::deserialize_binary(&blob),
            Ok(other) => {
                self.wrong_answer(what, &other);
                Vec::new()
            }
            Err(_) => Vec::new(),
        }
    }

    /// The plugin's dashboard image, checked: its own `asset:` namespace, or
    /// a path inside its own directory.
    fn resolve_dashboard_image(&self, plugin_dir: &Path) -> Option<String> {
        let rel = match self.call("dashboard-image-path", Request::DashboardImagePath) {
            Ok(Response::OptStr(p)) => p?,
            _ => return None,
        };
        match sicompass_sdk::assets::parse_uri(&rel) {
            Some((provider, _)) if provider == self.plugin_name => return Some(rel),
            Some((provider, _)) => {
                self.note_error(format!(
                    "plugin `{}` asked for `{provider}`'s asset `{rel}`",
                    self.plugin_name
                ));
                return None;
            }
            None => {}
        }
        match crate::wasm_host::confine_in(plugin_dir, &rel) {
            Ok(path) => Some(path.to_string_lossy().into_owned()),
            Err(e) => {
                self.note_error(format!(
                    "plugin `{}` asked for dashboard image `{rel}`: {e}",
                    self.plugin_name
                ));
                None
            }
        }
    }

    /// Ask the plugin to render each URL a link is waiting on. One it
    /// declines, or cannot be asked about, is answered at once with a row
    /// saying so, so the link does not wait forever.
    fn hand_out_render_requests(&mut self) {
        for url in sicompass_sdk::url_fetcher::take_render_requests() {
            self.renders.ask(&url);
            let taken = self.req_bool(
                "execute-command",
                Request::ExecuteCommand {
                    cmd: sicompass_sdk::plugin_abi::RENDER_URL_COMMAND.to_owned(),
                    selection: url.clone(),
                },
            );
            if !taken {
                self.renders.forget(&url);
                sicompass_sdk::url_fetcher::deliver_render(
                    &url,
                    vec![FfonElement::new_str(format!("{url} could not be rendered"))],
                );
            }
        }
    }

    /// Tell the plugin when the app's language changed since it last heard.
    fn follow_locale(&mut self) {
        let now = sicompass_sdk::localize::current_locale();
        if now != self.locale {
            self.locale = now;
            self.req_unit("locale-changed", Request::LocaleChanged);
        }
    }
}

fn expect_unit(r: Result<Response, String>) -> Result<(), String> {
    match r? {
        Response::Unit => Ok(()),
        other => Err(format!("answered `init` with {other:?}")),
    }
}

/// Run a call so that waiting is allowed even from inside async code: the app
/// reaches `undo` and `redo` from inside `sicompass_sdk::block_on`. See the
/// same function in `wasm_host::provider`.
fn outside_async<T>(f: impl FnOnce() -> T) -> T {
    match tokio::runtime::Handle::try_current().map(|h| h.runtime_flavor()) {
        Ok(tokio::runtime::RuntimeFlavor::MultiThread) => tokio::task::block_in_place(f),
        _ => f(),
    }
}

/// Values used before `describe` has run, and after a failure.
fn default_descriptor(name: &str) -> Descriptor {
    Descriptor {
        name: name.to_owned(),
        display_name: name.to_owned(),
        ..Default::default()
    }
}

fn hears_setting(declared: &[String], key: &str) -> bool {
    declared.iter().any(|k| k == key)
}

fn first_element(blob: &[u8]) -> Option<FfonElement> {
    ffon::deserialize_binary(blob).into_iter().next()
}

fn to_sdk_kind(k: ipc::DashboardKind) -> DashboardKind {
    match k {
        ipc::DashboardKind::None => DashboardKind::None,
        ipc::DashboardKind::Image => DashboardKind::Image,
        ipc::DashboardKind::Interactive => DashboardKind::Interactive,
    }
}

fn to_sdk_request(r: ipc::DashboardRequest) -> DashboardRequest {
    match r {
        ipc::DashboardRequest::Enter => DashboardRequest::Enter,
        ipc::DashboardRequest::Leave => DashboardRequest::Leave,
    }
}

fn to_ipc_key(key: DashboardKey) -> ipc::Key {
    use ipc::Keysym as K;
    ipc::Key {
        sym: match key.keysym {
            DashboardKeysym::Enter => K::Enter,
            DashboardKeysym::Backspace => K::Backspace,
            DashboardKeysym::Tab => K::Tab,
            DashboardKeysym::Escape => K::Escape,
            DashboardKeysym::Up => K::Up,
            DashboardKeysym::Down => K::Down,
            DashboardKeysym::Left => K::Left,
            DashboardKeysym::Right => K::Right,
            DashboardKeysym::Home => K::Home,
            DashboardKeysym::End => K::End,
            DashboardKeysym::PageUp => K::PageUp,
            DashboardKeysym::PageDown => K::PageDown,
            DashboardKeysym::Insert => K::Insert,
            DashboardKeysym::Delete => K::Delete,
            DashboardKeysym::F(n) => K::F(n),
            DashboardKeysym::Char(c) => K::Ch(c),
            DashboardKeysym::Unknown => K::Unknown,
        },
        ctrl: key.ctrl,
        shift: key.shift,
        alt: key.alt,
    }
}

/// A plugin's frame as the renderer draws it, or `None` when its cell count
/// disagrees with its own grid (the renderer indexes `row * cols + col`). A
/// cursor or selection outside the grid is dropped, as for a WASM plugin.
fn to_sdk_frame(f: ipc::Frame) -> Option<DashboardFrame> {
    if f.cells.len() != f.cols as usize * f.rows as usize {
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
    let cursor = f.cursor.filter(|(col, row)| *col < f.cols && *row < f.rows);
    let selection = f.selection.and_then(|s| {
        (s.col < f.cols && s.row < f.rows).then(|| DashboardSelection {
            col: s.col,
            row: s.row,
            cols: s.cols.min(f.cols - s.col),
            rows: s.rows.min(f.rows - s.row),
        })
    });
    let half_gap_rows = f.half_gap_rows.into_iter().filter(|r| *r < f.rows).collect();
    Some(DashboardFrame {
        cols: f.cols,
        rows: f.rows,
        cells,
        cursor,
        selection,
        half_gap_rows,
        cursor_style: match f.cursor_style {
            CursorStyle::Block => DashboardCursor::Block,
            CursorStyle::Bar => DashboardCursor::Bar,
        },
    })
}

/// The plugin's own record of a timeline entry, if it could have emitted it.
fn to_ipc_op(entry: &TimelineEntry) -> Option<ipc::ProviderOp> {
    match entry {
        TimelineEntry::ProviderOp {
            command,
            payload,
            label,
            ..
        } => Some(ipc::ProviderOp {
            command: command.clone(),
            payload: ffon::serialize_binary(std::slice::from_ref(payload)),
            label: label.clone(),
        }),
        _ => None,
    }
}

#[async_trait::async_trait]
impl Provider for ProcessProvider {
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
        self.call_ffon("fetch", Request::Fetch)
    }

    fn fetch_subtree_children(&mut self) -> Option<Vec<FfonElement>> {
        match self.req("fetch-subtree-children", Request::FetchSubtreeChildren) {
            Ok(Response::OptFfon(blob)) => blob.map(|b| ffon::deserialize_binary(&b)),
            Ok(other) => {
                self.wrong_answer("fetch-subtree-children", &other);
                None
            }
            Err(_) => None,
        }
    }

    fn fetch_subtree_parent_key(&mut self) -> Option<String> {
        match self.req("fetch-subtree-parent-key", Request::FetchSubtreeParentKey) {
            Ok(Response::OptStr(k)) => k,
            Ok(other) => {
                self.wrong_answer("fetch-subtree-parent-key", &other);
                None
            }
            Err(_) => None,
        }
    }

    fn sync_ffon_body_children(&mut self, children: &[FfonElement]) {
        self.req_unit(
            "sync-ffon-body-children",
            Request::SyncFfonBodyChildren(ffon::serialize_binary(children)),
        );
    }

    // ---- Lifecycle ---------------------------------------------------------

    fn init(&mut self) {
        // Already initialised in `open`, so the provider is usable the moment it
        // exists. A second `init` would be visible to the plugin.
    }

    fn cleanup(&mut self) {
        self.req_unit("cleanup", Request::Cleanup);
    }

    // ---- Per-frame: one call, then cached ----------------------------------

    fn tick(&mut self) -> bool {
        if self.is_poisoned() {
            return false;
        }
        self.follow_locale();
        if self.renders_pages {
            self.hand_out_render_requests();
        }
        match self.poll() {
            Ok(()) => self.polled.redraw,
            // A failure already queued an error; redraw so it shows promptly.
            Err(_) => true,
        }
    }

    fn needs_refresh(&self) -> bool {
        self.polled.needs_refresh
    }

    fn supports_structural_edit(&self) -> bool {
        self.descriptor.supports_structural_edit && self.polled.structural_edit_here
    }

    fn clear_needs_refresh(&mut self) {
        self.polled.needs_refresh = false;
    }

    fn is_busy(&self) -> bool {
        self.polled.is_busy
    }

    /// The program this plugin runs for the user (a shell, a CLI session),
    /// which the tab switcher names, as the plugin reported it.
    fn process_id(&self) -> Option<u32> {
        self.polled.child_pid
    }

    fn at_root(&self) -> bool {
        self.polled.at_root
    }

    fn take_error(&mut self) -> Option<String> {
        self.state.get_mut().pending_error.take()
    }

    fn take_announcement(&mut self) -> Option<String> {
        self.polled.announcement.take()
    }

    fn dashboard_uses_app_undo(&self) -> bool {
        self.descriptor.dashboard_uses_app_undo
    }

    fn take_navigation_request(&mut self) -> Option<NavigationRequest> {
        self.polled.navigation_request.take().map(|r| match r {
            ipc::NavigationRequest::EnterChildren => NavigationRequest::EnterChildren,
            ipc::NavigationRequest::SelectPath(path) => {
                NavigationRequest::SelectPath(path.into_iter().map(|i| i as usize).collect())
            }
        })
    }

    // ---- Navigation --------------------------------------------------------

    fn current_path(&self) -> &str {
        &self.current_path
    }

    fn push_path(&mut self, segment: &str) {
        self.navigate("push-path", Request::PushPath(segment.to_owned()));
    }

    fn pop_path(&mut self) {
        self.navigate("pop-path", Request::PopPath);
    }

    fn set_current_path(&mut self, path: &str) {
        self.navigate("set-current-path", Request::SetCurrentPath(path.to_owned()));
    }

    // ---- Editing and file operations ---------------------------------------

    fn commit_edit(&mut self, old: &str, new: &str) -> bool {
        self.req_bool(
            "commit-edit",
            Request::CommitEdit {
                old: old.to_owned(),
                new: new.to_owned(),
            },
        )
    }

    fn create_directory(&mut self, name: &str) -> bool {
        self.req_bool("create-directory", Request::CreateDirectory(name.to_owned()))
    }

    fn create_file(&mut self, name: &str) -> bool {
        self.req_bool("create-file", Request::CreateFile(name.to_owned()))
    }

    fn delete_item(&mut self, name: &str) -> bool {
        self.req_bool("delete-item", Request::DeleteItem(name.to_owned()))
    }

    fn copy_item(&mut self, src_dir: &str, src_name: &str, dest_dir: &str, dest_name: &str) -> bool {
        self.req_bool(
            "copy-item",
            Request::CopyItem {
                src_dir: src_dir.to_owned(),
                src_name: src_name.to_owned(),
                dest_dir: dest_dir.to_owned(),
                dest_name: dest_name.to_owned(),
            },
        )
    }

    // ---- Commands ----------------------------------------------------------

    fn commands(&self) -> Vec<String> {
        match self.call("commands", Request::Commands) {
            Ok(Response::Strings(c)) => c,
            Ok(other) => {
                self.wrong_answer("commands", &other);
                Vec::new()
            }
            Err(_) => Vec::new(),
        }
    }

    fn command_label(&self, cmd: &str) -> String {
        match self.call("command-label", Request::CommandLabel(cmd.to_owned())) {
            Ok(Response::Str(l)) => l,
            Ok(other) => {
                self.wrong_answer("command-label", &other);
                cmd.to_owned()
            }
            Err(_) => cmd.to_owned(),
        }
    }

    fn handle_command(
        &mut self,
        cmd: &str,
        elem_key: &str,
        elem_type: i32,
        error: &mut String,
    ) -> Option<FfonElement> {
        let out = match self.req(
            "handle-command",
            Request::HandleCommand {
                cmd: cmd.to_owned(),
                elem_key: elem_key.to_owned(),
                elem_type,
            },
        ) {
            Ok(Response::Command(Ok(blob))) => blob.as_deref().and_then(first_element),
            Ok(Response::Command(Err(msg))) | Err(msg) => {
                *error = msg;
                None
            }
            Ok(other) => {
                *error = self.wrong_answer("handle-command", &other);
                None
            }
        };
        // A command can move the plugin, and the app asks `at_root` straight
        // after, before the next frame's poll.
        self.repoll();
        out
    }

    fn command_list_items(&self, cmd: &str) -> Vec<ListItem> {
        match self.call("command-list-items", Request::CommandListItems(cmd.to_owned())) {
            Ok(Response::ListItems(items)) => items
                .into_iter()
                .map(|i| ListItem {
                    label: i.label,
                    data: i.data,
                })
                .collect(),
            Ok(other) => {
                self.wrong_answer("command-list-items", &other);
                Vec::new()
            }
            Err(_) => Vec::new(),
        }
    }

    fn execute_command(&mut self, cmd: &str, selection: &str) -> bool {
        let done = self.req_bool(
            "execute-command",
            Request::ExecuteCommand {
                cmd: cmd.to_owned(),
                selection: selection.to_owned(),
            },
        );
        self.repoll();
        done
    }

    fn create_element(&mut self, key: &str) -> Option<FfonElement> {
        match self.req("create-element", Request::CreateElement(key.to_owned())) {
            Ok(Response::OptFfon(blob)) => blob.as_deref().and_then(first_element),
            Ok(other) => {
                self.wrong_answer("create-element", &other);
                None
            }
            Err(_) => None,
        }
    }

    // ---- Interactive callbacks ---------------------------------------------

    fn on_radio_change(&mut self, group: &str, value: &str) {
        self.req_unit(
            "on-radio-change",
            Request::OnRadioChange {
                group: group.to_owned(),
                value: value.to_owned(),
            },
        );
    }

    fn on_button_press(&mut self, function_name: &str) {
        self.req_unit("on-button-press", Request::OnButtonPress(function_name.to_owned()));
    }

    fn on_checkbox_change(&mut self, label: &str, checked: bool) {
        self.req_unit(
            "on-checkbox-change",
            Request::OnCheckboxChange {
                label: label.to_owned(),
                checked,
            },
        );
    }

    fn set_input_value(&mut self, value: &str) {
        self.req_unit("set-input-value", Request::SetInputValue(value.to_owned()));
    }

    fn on_setting_change(&mut self, key: &str, value: &str) {
        // The app broadcasts every change to every provider. A plugin hears
        // only about the settings it declared.
        if !hears_setting(&self.setting_keys, key) {
            return;
        }
        let value = crate::plugin_manifest::expand_home(value);
        self.req_unit(
            "on-setting-change",
            Request::OnSettingChange {
                key: key.to_owned(),
                value,
            },
        );
    }

    // ---- Timeline undo/redo ------------------------------------------------

    fn take_timeline_entries(&mut self) -> Vec<TimelineEntry> {
        match self.req("take-timeline-entries", Request::TakeTimelineEntries) {
            Ok(Response::Ops(ops)) => ops
                .into_iter()
                .map(|op| TimelineEntry::ProviderOp {
                    provider_idx: 0,
                    command: op.command,
                    payload: first_element(&op.payload)
                        .unwrap_or_else(|| FfonElement::new_str("")),
                    label: op.label,
                })
                .collect(),
            Ok(other) => {
                self.wrong_answer("take-timeline-entries", &other);
                Vec::new()
            }
            Err(_) => Vec::new(),
        }
    }

    async fn undo(&mut self, entry: &TimelineEntry, error: &mut String) {
        if let Some(op) = to_ipc_op(entry) {
            match self.req("undo", Request::Undo(op)) {
                Ok(Response::Done(Ok(()))) => {}
                Ok(Response::Done(Err(msg))) | Err(msg) => *error = msg,
                Ok(other) => *error = self.wrong_answer("undo", &other),
            }
        }
    }

    async fn redo(&mut self, entry: &TimelineEntry, error: &mut String) {
        if let Some(op) = to_ipc_op(entry) {
            match self.req("redo", Request::Redo(op)) {
                Ok(Response::Done(Ok(()))) => {}
                Ok(Response::Done(Err(msg))) | Err(msg) => *error = msg,
                Ok(other) => *error = self.wrong_answer("redo", &other),
            }
        }
    }

    // ---- Extended search ---------------------------------------------------

    fn collect_extended_search_items(&self) -> Option<Vec<SearchResultItem>> {
        match self.call(
            "collect-extended-search-items",
            Request::CollectExtendedSearchItems,
        ) {
            Ok(Response::Search(items)) => items.map(|items| {
                items
                    .into_iter()
                    .map(|i| SearchResultItem {
                        label: i.label,
                        breadcrumb: i.breadcrumb,
                        nav_path: i.nav_path,
                    })
                    .collect()
            }),
            Ok(other) => {
                self.wrong_answer("collect-extended-search-items", &other);
                None
            }
            Err(_) => None,
        }
    }

    // ---- Behaviour flags ---------------------------------------------------

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

    // ---- Persistent config: the app owns the file --------------------------

    fn load_config(&mut self, path: &Path) -> bool {
        let Ok(contents) = std::fs::read(path) else {
            return false;
        };
        self.req_bool("load-config", Request::LoadConfig(contents))
    }

    fn save_config(&self, path: &Path) -> bool {
        match self.call("save-config", Request::SaveConfig) {
            Ok(Response::OptBytes(Some(bytes))) => std::fs::write(path, bytes).is_ok(),
            Ok(Response::OptBytes(None)) | Err(_) => false,
            Ok(other) => {
                self.wrong_answer("save-config", &other);
                false
            }
        }
    }

    // ---- Dashboard ---------------------------------------------------------

    fn dashboard_kind(&self) -> DashboardKind {
        to_sdk_kind(self.descriptor.dashboard_kind)
    }

    fn manual_dashboard_entry_allowed(&self) -> bool {
        self.descriptor.manual_dashboard_entry_allowed && self.polled.dashboard_here
    }

    fn dashboard_image_path(&self) -> Option<&str> {
        if !self.polled.dashboard_here {
            return None;
        }
        self.dashboard_image.as_deref()
    }

    fn take_dashboard_request(&mut self) -> Option<DashboardRequest> {
        self.polled.dashboard_request.take().map(to_sdk_request)
    }

    fn dashboard_render(&mut self, cols: u16, rows: u16) -> DashboardFrame {
        match self.req("dashboard-render", Request::DashboardRender { cols, rows }) {
            Ok(Response::Frame(frame)) => match to_sdk_frame(frame) {
                Some(f) => f,
                None => {
                    // The plugin's bug, but not one worth stopping it for: it
                    // may be an off-by-one during a resize.
                    self.note_error(format!(
                        "plugin `{}` returned a dashboard frame whose cell count did \
                         not match its own {cols}x{rows} grid",
                        self.plugin_name
                    ));
                    DashboardFrame::empty(cols, rows)
                }
            },
            Ok(other) => {
                self.wrong_answer("dashboard-render", &other);
                DashboardFrame::empty(cols, rows)
            }
            Err(_) => DashboardFrame::empty(cols, rows),
        }
    }

    fn dashboard_key(&mut self, key: DashboardKey) -> bool {
        self.req_bool("dashboard-key", Request::DashboardKey(to_ipc_key(key)))
    }

    fn dashboard_text(&mut self, text: &str) {
        self.req_unit("dashboard-text", Request::DashboardText(text.to_owned()));
    }

    fn dashboard_paste(&mut self, text: &str) {
        self.req_unit("dashboard-paste", Request::DashboardPaste(text.to_owned()));
    }

    fn dashboard_resize(&mut self, rows: u16, cols: u16) {
        self.req_unit("dashboard-resize", Request::DashboardResize { rows, cols });
    }

    fn set_dashboard_entry(&mut self, path: &[usize]) {
        let path = path
            .iter()
            .map(|&i| u32::try_from(i).unwrap_or(u32::MAX))
            .collect();
        self.req_unit("set-dashboard-entry", Request::SetDashboardEntry(path));
    }

    fn set_dashboard_palette(&mut self, palette: DashboardPalette) {
        self.req_unit(
            "set-dashboard-palette",
            Request::SetDashboardPalette(ipc::Palette {
                background: palette.background,
                text: palette.text,
                header_sep: palette.header_sep,
                selected: palette.selected,
                ext_search: palette.ext_search,
                scroll_search: palette.scroll_search,
                error: palette.error,
            }),
        );
    }

    fn enter_dashboard(&mut self) {
        self.req_unit("enter-dashboard", Request::EnterDashboard);
    }

    /// Polled again straight away: leaving is where a plugin asks for the
    /// list cursor to follow it.
    fn leave_dashboard(&mut self) {
        self.req_unit("leave-dashboard", Request::LeaveDashboard);
        self.repoll();
    }
}

impl Drop for ProcessProvider {
    fn drop(&mut self) {
        if self.renders_pages {
            sicompass_sdk::url_fetcher::set_renderer_available(false);
        }
        // Dropping the channel lets the process go.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_executable_gets_the_platform_suffix_once() {
        let exe = executable(Path::new("/p/plugin"));
        assert_eq!(
            exe,
            PathBuf::from(format!("/p/plugin{}", std::env::consts::EXE_SUFFIX))
        );
        assert_eq!(executable(&exe), exe);
    }

    #[test]
    fn a_frame_whose_cells_disagree_with_its_grid_is_refused() {
        let mut f = sicompass_sdk::plugin::blank_frame(4, 2);
        assert!(to_sdk_frame(f.clone()).is_some());
        f.cells.pop();
        assert!(to_sdk_frame(f).is_none());
    }

    #[test]
    fn a_cursor_and_selection_outside_the_grid_are_dropped_or_clamped() {
        let mut f = sicompass_sdk::plugin::blank_frame(4, 2);
        f.cursor = Some((4, 0));
        f.selection = Some(ipc::Selection {
            col: 2,
            row: 1,
            cols: 9,
            rows: 9,
        });
        let out = to_sdk_frame(f).unwrap();
        assert_eq!(out.cursor, None);
        let s = out.selection.unwrap();
        assert_eq!((s.cols, s.rows), (2, 1));
    }

    #[test]
    fn only_provider_ops_go_back_to_a_plugin() {
        let entry = TimelineEntry::ProviderOp {
            provider_idx: 7,
            command: "greet".to_owned(),
            payload: FfonElement::new_str("world"),
            label: "greet world".to_owned(),
        };
        let op = to_ipc_op(&entry).unwrap();
        assert_eq!(first_element(&op.payload), Some(FfonElement::new_str("world")));
        let fs = TimelineEntry::FsOp {
            provider_idx: 0,
            id: sicompass_sdk::IdArray::new(),
            op: sicompass_sdk::FsOpKind::Create,
            before: None,
            after: None,
            side_effect: sicompass_sdk::FsSideEffect::None,
        };
        assert!(to_ipc_op(&fs).is_none());
    }

    #[test]
    fn every_keysym_crosses_without_collapsing() {
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
        let mapped: std::collections::BTreeSet<String> = all
            .iter()
            .map(|k| {
                format!(
                    "{:?}",
                    to_ipc_key(DashboardKey {
                        keysym: *k,
                        ctrl: false,
                        shift: false,
                        alt: false
                    })
                    .sym
                )
            })
            .collect();
        assert_eq!(mapped.len(), all.len());
        // And back, through the SDK's own conversion.
        let k: DashboardKey = to_ipc_key(DashboardKey {
            keysym: DashboardKeysym::Char('c'),
            ctrl: true,
            shift: false,
            alt: true,
        })
        .into();
        assert_eq!(k.keysym, DashboardKeysym::Char('c'));
        assert!(k.ctrl && k.alt && !k.shift);
    }
}
