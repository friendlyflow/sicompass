//! Web browser provider — Rust port of `lib_webbrowser/`.
//!
//! Fetches a URL via a real Chrome browser (via chromiumoxide + xvfb-run on
//! Linux, or headless Chrome on macOS/Windows), parses the rendered HTML with
//! scraper (html5ever), and converts the DOM to a flat FFON tree of strings
//! and objects that mirrors the C provider's lexbor-based output.
//!
//! ## FFON tree layout
//!
//! ```text
//! meta             (obj)  — keyboard shortcut hints
//! <url-bar>        (obj when page loaded, str when no page)
//!   heading        (obj)  — h1-h6 → nested objects
//!   paragraph      (str)  — plain text
//!   link text      (str)  — "anchor text <link>url</link>"
//!   list           (obj)  — ul/ol wrapper
//!     - item       (str)
//!   table          (str)  — "cell1 | cell2 | …"
//!   image          (str)  — "alt text [img]"
//! ```

use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::page::AddScriptToEvaluateOnNewDocumentParams;
use futures::StreamExt as _;
use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::provider::Provider;
use sicompass_sdk::ffon::{html_to_ffon, html_resolve_href};

// ---------------------------------------------------------------------------
// Cached page
// ---------------------------------------------------------------------------

struct CachedPage {
    #[allow(dead_code)]
    url: String,
    elements: Vec<FfonElement>,
}

// ---------------------------------------------------------------------------
// WebbrowserProvider
// ---------------------------------------------------------------------------

pub struct WebbrowserProvider {
    current_url: String,
    cached_page: Option<CachedPage>,
}

impl WebbrowserProvider {
    pub fn new() -> Self {
        WebbrowserProvider {
            current_url: String::new(),
            cached_page: None,
        }
    }

    /// Fetch `url` over HTTP and parse to a Vec of FfonElements.
    fn load_url(&mut self, url: &str) {
        let html = match fetch_html_chromium(url) {
            Ok(h) => h,
            Err(e) => {
                let msg = format!("Error loading {url}: {e}");
                self.cached_page = Some(CachedPage {
                    url: url.to_owned(),
                    elements: vec![FfonElement::new_str(msg)],
                });
                self.current_url = url.to_owned();
                return;
            }
        };
        let elements = html_to_ffon(&html, url);
        self.cached_page = Some(CachedPage { url: url.to_owned(), elements });
        self.current_url = url.to_owned();
    }
}

impl Default for WebbrowserProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for WebbrowserProvider {
    fn name(&self) -> &str { "webbrowser" }
    fn display_name(&self) -> &str { "web browser" }

    fn fetch(&mut self) -> Vec<FfonElement> {
        let mut result = Vec::new();

        // URL bar element
        let url_bar = format!(
            "<input>{}</input>",
            if self.current_url.is_empty() { "https://" } else { &self.current_url }
        );

        if let Some(ref page) = self.cached_page {
            // Page loaded: wrap URL bar + page content in an Obj
            let mut page_obj = FfonElement::new_obj(&url_bar);
            let o = page_obj.as_obj_mut().unwrap();
            for elem in &page.elements {
                o.push(elem.clone());
            }
            result.push(page_obj);
        } else {
            // No page yet: just the URL bar as a string
            result.push(FfonElement::new_str(url_bar));
        }

        result
    }

    fn commit_edit(&mut self, _old: &str, new_content: &str) -> bool {
        let url = new_content.trim().to_owned();
        if url.is_empty() { return false; }
        // Prepend https:// if no scheme
        let full_url = if url.contains("://") {
            url
        } else {
            format!("https://{url}")
        };
        self.load_url(&full_url);
        true
    }

    fn commands(&self) -> Vec<String> {
        vec!["refresh".to_owned()]
    }

    fn handle_command(
        &mut self,
        cmd: &str,
        _elem_key: &str,
        _elem_type: i32,
        _error: &mut String,
    ) -> Option<FfonElement> {
        if cmd == "refresh" {
            let url = self.current_url.clone();
            if !url.is_empty() {
                self.cached_page = None;
                self.load_url(&url);
            }
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Chromium — per-fetch browser launch + shared async runtime
// ---------------------------------------------------------------------------

// Multi-thread runtime (2 workers) for chromiumoxide. Kept alive for the
// process lifetime so repeated fetches reuse the same thread pool.
static CHROMIUM_RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> =
    std::sync::OnceLock::new();

// ---------------------------------------------------------------------------
// Windows: hide Chrome windows that appear during headed launch
//
// Chrome must run headed to pass bot-detection on sites like gva.be — headless
// mode is fingerprinted and blocked.  On Linux, xvfb-run provides an invisible
// X11 display.  On Windows we use Browser::launch (which chromiumoxide manages)
// with `with_head()`, and a background thread that calls ShowWindow(SW_HIDE)
// on any Chrome windows that appear while the browser is starting up.
// The window is hidden within one paint frame (~50 ms) — invisible in practice.
// user32.dll is always linked on Windows — no extra crates.
// ---------------------------------------------------------------------------
// ---------------------------------------------------------------------------
// Windows: keep Chrome windows off-screen (not hidden)
//
// We must never call ShowWindow(SW_HIDE) on Chrome windows.  Hiding a window
// sends WM_SHOWWINDOW(FALSE) to Chrome's message loop, which drives
// RenderWidget::SetHidden() inside Blink — this forces
// document.visibilityState = "hidden" and kills JavaScript timer resolution
// (React / consent-page apps never hydrate).  Moving the window to a position
// far off all monitors keeps Chrome thinking the window is fully visible while
// making it invisible to the user.  Chrome's JS runs at full speed.
#[cfg(target_os = "windows")]
mod win_hide {
    extern "system" {
        fn EnumWindows(lp_enum_func: unsafe extern "system" fn(isize, isize) -> i32, l_param: isize) -> i32;
        fn GetWindowThreadProcessId(hwnd: isize, lp_dw_process_id: *mut u32) -> u32;
        fn IsWindowVisible(hwnd: isize) -> i32;
        fn OpenProcess(dw_desired_access: u32, b_inherit_handle: i32, dw_process_id: u32) -> isize;
        fn CloseHandle(h_object: isize) -> i32;
        fn QueryFullProcessImageNameW(
            h_process: isize,
            dw_flags: u32,
            lp_exe_name: *mut u16,
            lp_size: *mut u32,
        ) -> i32;
        fn SetWindowPos(
            hwnd: isize,
            hwnd_insert_after: isize,
            x: i32, y: i32,
            cx: i32, cy: i32,
            u_flags: u32,
        ) -> i32;
    }

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    /// Do not resize the window.
    const SWP_NOSIZE:     u32 = 0x0001;
    /// Do not change z-order.
    const SWP_NOZORDER:   u32 = 0x0004;
    /// Do not activate/focus the window.
    const SWP_NOACTIVATE: u32 = 0x0010;

    /// Off-screen position: far beyond any realistic monitor layout.
    const OFFSCREEN_X: i32 = -10_000;
    const OFFSCREEN_Y: i32 = -10_000;

    /// Collect all currently-visible top-level window handles.
    pub fn snapshot_windows() -> Vec<isize> {
        let mut hwnds: Vec<isize> = Vec::new();
        unsafe extern "system" fn callback(hwnd: isize, lparam: isize) -> i32 {
            let vec = &mut *(lparam as *mut Vec<isize>);
            vec.push(hwnd);
            1 // continue
        }
        unsafe { EnumWindows(callback, &mut hwnds as *mut Vec<isize> as isize) };
        hwnds
    }

    /// Move every visible top-level Chrome/Edge window that was NOT present in
    /// `before` to an off-screen position.  The window remains "visible" to
    /// Chrome (no SW_HIDE, no occlusion) so JavaScript timers are never throttled.
    pub fn hide_new_browser_windows(before: &[isize]) {
        let mut current: Vec<isize> = Vec::new();
        unsafe extern "system" fn callback(hwnd: isize, lparam: isize) -> i32 {
            let vec = &mut *(lparam as *mut Vec<isize>);
            vec.push(hwnd);
            1
        }
        unsafe { EnumWindows(callback, &mut current as *mut Vec<isize> as isize) };

        for hwnd in current {
            if before.contains(&hwnd) { continue; }
            if unsafe { IsWindowVisible(hwnd) } == 0 { continue; }

            // Get the process ID for this window.
            let mut pid: u32 = 0;
            unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
            if pid == 0 { continue; }

            // Open the process to query its image name.
            let h_proc = unsafe {
                OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid)
            };
            if h_proc == 0 { continue; }

            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            let ok = unsafe {
                QueryFullProcessImageNameW(h_proc, 0, buf.as_mut_ptr(), &mut len)
            };
            unsafe { CloseHandle(h_proc) };

            if ok == 0 { continue; }
            let path = String::from_utf16_lossy(&buf[..len as usize]).to_lowercase();
            if path.contains("chrome") || path.contains("msedge") {
                // Move off-screen without resizing, changing z-order, or
                // activating.  Never hide — see module comment.
                unsafe {
                    SetWindowPos(
                        hwnd, 0,
                        OFFSCREEN_X, OFFSCREEN_Y,
                        0, 0,
                        SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
            }
        }
    }
}

/// On Linux, write a one-shot wrapper script that invokes Chrome through
/// `xvfb-run -a` so it runs on an auto-allocated virtual display (invisible).
/// Returns the path to the wrapper, or the plain Chrome binary if xvfb-run
/// is not available (Chrome will open visibly in that case).
#[cfg(target_os = "linux")]
fn chrome_via_xvfb() -> Result<std::path::PathBuf, String> {
    let chrome = find_chrome_executable()
        .ok_or_else(|| "Chrome/Chromium not found in PATH".to_owned())?;

    if which::which("xvfb-run").is_err() {
        return Ok(chrome);
    }

    let wrapper = std::env::temp_dir().join("sicompass-xvfb-chrome.sh");
    let script = format!(
        "#!/bin/sh\nunset WAYLAND_DISPLAY\nexec xvfb-run -a {} --ozone-platform=x11 \"$@\"\n",
        chrome.to_string_lossy()
    );
    std::fs::write(&wrapper, &script)
        .map_err(|e| format!("failed to write Xvfb wrapper: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755));
    }
    Ok(wrapper)
}

fn chromium_runtime() -> &'static tokio::runtime::Runtime {
    CHROMIUM_RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("failed to build chromium tokio runtime")
    })
}

/// Locate a usable Chrome/Chromium/Edge executable.
///
/// Priority:
/// 1. `SICOMPASS_CHROME_PATH` environment variable
/// 2. Common binary names on `PATH` (works on Linux; unlikely on Windows/macOS)
/// 3. Well-known installation paths for the current OS
fn find_chrome_executable() -> Option<std::path::PathBuf> {
    // 1. Explicit override
    if let Ok(p) = std::env::var("SICOMPASS_CHROME_PATH") {
        let pb = std::path::PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }

    // 2. PATH lookup (reliable on Linux; included here for all platforms)
    const PATH_CANDIDATES: &[&str] = &[
        "google-chrome",
        "google-chrome-stable",
        "google-chrome-beta",
        "chromium",
        "chromium-browser",
        "chrome",
    ];
    if let Some(p) = PATH_CANDIDATES.iter().find_map(|n| which::which(n).ok()) {
        return Some(p);
    }

    // 3. Well-known installation locations
    #[cfg(target_os = "windows")]
    {
        let env_candidates: &[(&str, &str)] = &[
            ("ProgramFiles",      r"Google\Chrome\Application\chrome.exe"),
            ("ProgramFiles",      r"Chromium\Application\chrome.exe"),
            ("ProgramFiles",      r"Microsoft\Edge\Application\msedge.exe"),
            ("ProgramFiles(x86)", r"Google\Chrome\Application\chrome.exe"),
            ("ProgramFiles(x86)", r"Chromium\Application\chrome.exe"),
            ("ProgramFiles(x86)", r"Microsoft\Edge\Application\msedge.exe"),
            ("LocalAppData",      r"Google\Chrome\Application\chrome.exe"),
        ];
        for (env, rel) in env_candidates {
            if let Ok(base) = std::env::var(env) {
                let full = std::path::PathBuf::from(base).join(rel);
                if full.exists() {
                    return Some(full);
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        const FIXED: &[&str] = &[
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Google Chrome Canary.app/Contents/MacOS/Google Chrome Canary",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
            "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        ];
        for p in FIXED {
            let pb = std::path::PathBuf::from(p);
            if pb.exists() {
                return Some(pb);
            }
        }
        // ~/Applications
        if let Ok(home) = std::env::var("HOME") {
            let p = std::path::PathBuf::from(&home)
                .join("Applications/Google Chrome.app/Contents/MacOS/Google Chrome");
            if p.exists() {
                return Some(p);
            }
        }
    }

    None
}

// ── BrowserSession ───────────────────────────────────────────────────────────
// Owns the Browser handle; chromiumoxide manages the Chrome process lifetime.
// On Windows, also owns the hider-thread stop signal: dropping it shuts the
// thread down (channel disconnects → thread exits its recv_timeout loop).

struct BrowserSession {
    browser: Browser,
    /// Windows only: dropping this signals the window-hider thread to stop.
    #[cfg(target_os = "windows")]
    _hider_stop: std::sync::mpsc::SyncSender<()>,
}

// ── Platform-specific browser launch ─────────────────────────────────────────

/// Linux: use xvfb-run so Chrome runs headed on an invisible X11 display.
#[cfg(target_os = "linux")]
async fn launch_browser() -> Result<BrowserSession, String> {
    let exe = chrome_via_xvfb()?;

    // Use a fixed profile dir so Chrome doesn't open first-run dialogs and so
    // we can clean up any stale SingletonLock left by a previous crashed launch.
    let profile_dir = std::env::temp_dir().join("sicompass-chrome");
    let _ = std::fs::remove_file(profile_dir.join("SingletonLock"));

    let config = BrowserConfig::builder()
        .with_head()
        .arg("--disable-blink-features=AutomationControlled")
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .user_data_dir(&profile_dir)
        .window_size(1920, 1080)
        .chrome_executable(exe)
        .build()
        .map_err(|e| format!("chromium config error: {e}"))?;
    let (browser, mut handler) = Browser::launch(config).await
        .map_err(|e| format!("failed to launch Chrome (xvfb): {e}"))?;
    tokio::spawn(async move { while handler.next().await.is_some() {} });
    Ok(BrowserSession { browser })
}

/// Windows: launch headed Chrome positioned off-screen, with a background
/// thread that moves any newly-visible Chrome windows off-screen every 50 ms.
///
/// We never call ShowWindow(SW_HIDE) — hiding a window sends WM_SHOWWINDOW
/// to Chrome's message loop, driving RenderWidget::SetHidden(), which sets
/// document.visibilityState = "hidden" and kills JS timer resolution.
/// Instead we start Chrome with --window-position=-10000,-10000 and keep the
/// mover thread running to catch any window that Chrome opens after launch.
/// The thread stops automatically when `BrowserSession` is dropped.
#[cfg(target_os = "windows")]
async fn launch_browser() -> Result<BrowserSession, String> {
    let exe = find_chrome_executable().ok_or_else(|| {
        "Chrome/Chromium/Edge not found. \
         Install Chrome or set SICOMPASS_CHROME_PATH to the browser executable."
            .to_owned()
    })?;

    // Use a fixed profile dir so Chrome doesn't open first-run dialogs and so
    // we can clean up any stale SingletonLock left by a previous crashed launch.
    let profile_dir = std::env::temp_dir().join("sicompass-chrome");
    let _ = std::fs::remove_file(profile_dir.join("SingletonLock"));

    let config = BrowserConfig::builder()
        .with_head()
        .arg("--disable-blink-features=AutomationControlled")
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        // Start the window far off all monitors so it is never on-screen.
        // Negative coordinates are valid on Windows; the window is "visible"
        // to Chrome (no SW_HIDE) so JS timers and rendering run at full speed.
        .arg("--window-position=-10000,-10000")
        // Belt-and-suspenders: also disable renderer backgrounding in case
        // Chrome ever detects that its window is off all monitors.
        .arg("--disable-backgrounding-occluded-windows")
        .arg("--disable-renderer-backgrounding")
        .arg("--disable-background-timer-throttling")
        .user_data_dir(&profile_dir)
        .window_size(1920, 1080)
        .chrome_executable(&exe)
        .build()
        .map_err(|e| format!("chromium config error: {e}"))?;

    // Snapshot existing windows before launch so we only target new ones.
    let before = win_hide::snapshot_windows();

    // Channel: when stop_tx is dropped (BrowserSession dropped), recv_timeout
    // returns Disconnected and the thread exits cleanly.
    let (stop_tx, stop_rx) = std::sync::mpsc::sync_channel::<()>(0);

    // Background mover: runs for the entire session lifetime.
    // Moves any new Chrome windows off-screen (never hides them).
    std::thread::spawn(move || {
        use std::sync::mpsc::RecvTimeoutError;
        loop {
            match stop_rx.recv_timeout(std::time::Duration::from_millis(50)) {
                Err(RecvTimeoutError::Timeout) => {
                    win_hide::hide_new_browser_windows(&before);
                }
                _ => break, // Disconnected → BrowserSession dropped
            }
        }
    });

    let (browser, mut handler) = Browser::launch(config).await.map_err(|e| format!(
        "failed to launch Chrome at {} — \
         is Chrome installed? (set SICOMPASS_CHROME_PATH to override): {e}",
        exe.display()
    ))?;
    tokio::spawn(async move { while handler.next().await.is_some() {} });

    Ok(BrowserSession { browser, _hider_stop: stop_tx })
}

/// macOS / other: headed Chrome via chromiumoxide's built-in launcher.
/// Chrome may briefly appear in the Dock; headless would be invisible but
/// risks bot-detection blocks on some sites.
#[cfg(not(any(target_os = "linux", target_os = "windows")))]
async fn launch_browser() -> Result<BrowserSession, String> {
    let exe = find_chrome_executable().ok_or_else(|| {
        "Chrome/Chromium not found. \
         Install Chrome or set SICOMPASS_CHROME_PATH to the browser executable."
            .to_owned()
    })?;
    let config = BrowserConfig::builder()
        .with_head()
        .arg("--disable-blink-features=AutomationControlled")
        .arg("--no-first-run")
        .arg("--no-default-browser-check")
        .window_size(1920, 1080)
        .chrome_executable(&exe)
        .build()
        .map_err(|e| format!("chromium config error: {e}"))?;
    let (browser, mut handler) = Browser::launch(config).await
        .map_err(|e| format!(
            "failed to launch Chrome at {} — \
             is Chrome installed? (set SICOMPASS_CHROME_PATH to override): {e}",
            exe.display()
        ))?;
    tokio::spawn(async move { while handler.next().await.is_some() {} });
    Ok(BrowserSession { browser })
}


// Full stealth script injected before every page load.
// Based on puppeteer-extra-plugin-stealth patches — hides headless Chrome from
// Cloudflare's bot-detection fingerprinting.
const STEALTH_SCRIPT: &str = r#"
// ── 1. navigator.webdriver ───────────────────────────────────────────────────
// The primary signal Cloudflare checks. Remove it entirely.
Object.defineProperty(navigator, 'webdriver', { get: () => undefined });

// ── 2. navigator.plugins ─────────────────────────────────────────────────────
// Headless Chrome has zero plugins; real Chrome has several.
const _makePlugin = (name, desc, filename, mimeTypes) => {
    const plugin = Object.create(Plugin.prototype);
    Object.defineProperties(plugin, {
        name: { value: name }, description: { value: desc },
        filename: { value: filename }, length: { value: mimeTypes.length },
    });
    mimeTypes.forEach((mt, i) => { plugin[i] = mt; });
    return plugin;
};
const _pdf = Object.create(MimeType.prototype);
Object.defineProperties(_pdf, {
    type: { value: 'application/pdf' }, suffixes: { value: 'pdf' },
    description: { value: 'Portable Document Format' },
});
const _plugins = [
    _makePlugin('PDF Viewer', 'Portable Document Format', 'internal-pdf-viewer', [_pdf]),
    _makePlugin('Chrome PDF Viewer', 'Portable Document Format', 'internal-pdf-viewer', [_pdf]),
    _makePlugin('Chromium PDF Viewer', 'Portable Document Format', 'internal-pdf-viewer', [_pdf]),
    _makePlugin('Microsoft Edge PDF Viewer', 'Portable Document Format', 'internal-pdf-viewer', [_pdf]),
    _makePlugin('WebKit built-in PDF', 'Portable Document Format', 'internal-pdf-viewer', [_pdf]),
];
Object.defineProperty(navigator, 'plugins', {
    get: () => {
        const arr = [..._plugins];
        Object.setPrototypeOf(arr, PluginArray.prototype);
        arr.item = (i) => arr[i]; arr.namedItem = (n) => arr.find(p => p.name === n);
        arr.refresh = () => {};
        return arr;
    }
});
Object.defineProperty(navigator, 'mimeTypes', {
    get: () => {
        const arr = [_pdf];
        Object.setPrototypeOf(arr, MimeTypeArray.prototype);
        arr.item = (i) => arr[i]; arr.namedItem = (n) => arr.find(m => m.type === n);
        return arr;
    }
});

// ── 3. navigator.vendor + languages ──────────────────────────────────────────
Object.defineProperty(navigator, 'vendor', { get: () => 'Google Inc.' });
Object.defineProperty(navigator, 'languages', { get: () => ['nl-BE', 'nl', 'en-US', 'en'] });

// ── 4. window.chrome ─────────────────────────────────────────────────────────
// Real Chrome exposes window.chrome with loadTimes, csi, etc.
if (!window.chrome) {
    window.chrome = {
        app: {
            isInstalled: false,
            InstallState: { DISABLED: 'disabled', INSTALLED: 'installed', NOT_INSTALLED: 'not_installed' },
            RunningState: { CANNOT_RUN: 'cannot_run', READY_TO_RUN: 'ready_to_run', RUNNING: 'running' },
            getDetails: () => null, getIsInstalled: () => false,
            installState: () => 'not_installed',
        },
        runtime: {
            OnInstalledReason: { CHROME_UPDATE: 'chrome_update', INSTALL: 'install', SHARED_MODULE_UPDATE: 'shared_module_update', UPDATE: 'update' },
            OnRestartRequiredReason: { APP_UPDATE: 'app_update', GC_PRESSURE: 'gc_pressure', OS_UPDATE: 'os_update' },
            PlatformArch: { ARM: 'arm', ARM64: 'arm64', MIPS: 'mips', MIPS64: 'mips64', X86_32: 'x86-32', X86_64: 'x86-64' },
            PlatformNaclArch: { ARM: 'arm', MIPS: 'mips', MIPS64: 'mips64', X86_32: 'x86-32', X86_64: 'x86-64' },
            PlatformOs: { ANDROID: 'android', CROS: 'cros', LINUX: 'linux', MAC: 'mac', OPENBSD: 'openbsd', WIN: 'win' },
            RequestUpdateCheckStatus: { NO_UPDATE: 'no_update', THROTTLED: 'throttled', UPDATE_AVAILABLE: 'update_available' },
            connect: () => {}, sendMessage: () => {},
        },
        loadTimes: () => ({
            requestTime: performance.timing.navigationStart / 1000,
            startLoadTime: performance.timing.navigationStart / 1000,
            commitLoadTime: performance.timing.responseStart / 1000,
            finishDocumentLoadTime: performance.timing.domContentLoadedEventEnd / 1000,
            finishLoadTime: performance.timing.loadEventEnd / 1000,
            firstPaintTime: 0, firstPaintAfterLoadTime: 0,
            navigationType: 'Other', wasFetchedViaSpdy: false, wasNpnNegotiated: false,
            npnNegotiatedProtocol: 'unknown', wasAlternateProtocolAvailable: false,
            connectionInfo: 'http/1.1',
        }),
        csi: () => ({
            startE: performance.timing.navigationStart,
            onloadT: performance.timing.loadEventEnd,
            pageT: performance.now(), tran: 15,
        }),
    };
}

// ── 5. permissions.query ──────────────────────────────────────────────────────
const _origQuery = navigator.permissions.query.bind(navigator.permissions);
navigator.permissions.query = (p) =>
    p.name === 'notifications'
        ? Promise.resolve({ state: Notification.permission })
        : _origQuery(p);

// ── 6. iframe contentWindow ───────────────────────────────────────────────────
// Headless iframes expose webdriver in their contentWindow; patch them too.
const _origGet = Object.getOwnPropertyDescriptor(HTMLIFrameElement.prototype, 'contentWindow').get;
Object.defineProperty(HTMLIFrameElement.prototype, 'contentWindow', {
    get() {
        const cw = _origGet.call(this);
        if (cw) {
            try { Object.defineProperty(cw.navigator, 'webdriver', { get: () => undefined }); } catch(_) {}
        }
        return cw;
    }
});

// ── 7. Screen dimensions ──────────────────────────────────────────────────────
// Headless Chrome reports tiny/zero outerHeight. Real browsers have chrome UI.
if (window.outerHeight === 0) {
    Object.defineProperty(window, 'outerHeight', { get: () => window.innerHeight + 74 });
    Object.defineProperty(window, 'outerWidth',  { get: () => window.innerWidth });
}

// ── 8. Page visibility ────────────────────────────────────────────────────────
// On Windows the browser window is hidden via ShowWindow(SW_HIDE), which can
// cause Chrome to report visibilityState = "hidden".  Consent-page React apps
// (e.g. DPG Media / myprivacy.dpgmedia.be) check this before rendering; if the
// page appears hidden they skip hydration entirely, leaving no accept button in
// the DOM.  Always report the page as visible.
try {
    Object.defineProperty(document, 'visibilityState', { get: () => 'visible' });
    Object.defineProperty(document, 'hidden',          { get: () => false });
} catch(_) {}
"#;

/// Returns true if the page body is Cloudflare's "Sorry, you have been blocked" wall.
fn is_cf_blocked_html(html: &str) -> bool {
    html.contains("Sorry, you have been blocked")
        || html.contains("cf-error-1010")
        || html.contains("cf-error-1020")
}

/// Fetch a URL via Chromium, parse the HTML, and return as FFON elements.
/// Used by the main app's `fetch_url_to_elements` bridge.
pub fn fetch_url_to_ffon(url: &str) -> Vec<FfonElement> {
    match fetch_html_chromium(url) {
        Ok(html) => html_to_ffon(&html, url),
        Err(e) => vec![FfonElement::new_str(format!("Error loading {url}: {e}"))],
    }
}

fn fetch_html_chromium(url: &str) -> Result<String, String> {
    chromium_runtime().block_on(async move {
        tokio::time::timeout(
            tokio::time::Duration::from_secs(60),
            fetch_html_inner(url),
        )
        .await
        .unwrap_or_else(|_| Err(format!("timed out loading {url} (60 s)")))
    })
}

async fn fetch_html_inner(url: &str) -> Result<String, String> {
    // Launch a fresh Chrome process for this fetch.
    let session = launch_browser().await?;

    // Do the actual page fetch.  Keeping result separate lets us close Chrome
    // gracefully on both success and error paths before dropping the session.
    let result = fetch_page(&session, url).await;

    // Send Browser.close so Chrome exits cleanly (WebSocket close handshake
    // completes) instead of being killed abruptly (which logs a spurious
    // "ConnectionReset" error from the chromiumoxide handler task).
    use chromiumoxide::cdp::browser_protocol::browser::CloseParams;
    let _ = tokio::time::timeout(
        tokio::time::Duration::from_millis(500),
        session.browser.execute(CloseParams::default()),
    )
    .await;

    // session dropped here; hider thread (Windows) and browser process stop.
    result
}

/// Open a tab, navigate to `url`, and return the rendered HTML.
/// Called by `fetch_html_inner` which handles Chrome lifecycle around it.
async fn fetch_page(session: &BrowserSession, url: &str) -> Result<String, String> {
    let t = tokio::time::Duration::from_secs;

    let page = tokio::time::timeout(t(15), session.browser.new_page("about:blank"))
        .await
        .map_err(|_| "Chrome took >15 s to open a tab".to_owned())?
        .map_err(|e| format!("failed to open tab: {e}"))?;

    tokio::time::timeout(
        t(10),
        page.execute(AddScriptToEvaluateOnNewDocumentParams::new(STEALTH_SCRIPT)),
    )
    .await
    .map_err(|_| "stealth script injection timed out".to_owned())?
    .map_err(|e| format!("stealth script injection failed: {e}"))?;

    tokio::time::timeout(t(30), page.goto(url))
        .await
        .map_err(|_| format!("navigation to {url} timed out after 30 s"))?
        .map_err(|e| format!("navigation to {url} failed: {e}"))?;

    // Poll until the URL stabilises or a consent-wall URL is detected (up to 5 s).
    // On Windows the JS redirect from the original page to the consent wall can
    // fire well after Chrome's load event, so a single wait_for_navigation call
    // (which may return before the redirect) is not reliable cross-platform.
    let current_url = await_stable_url(&page, tokio::time::Duration::from_secs(5)).await;

    // If we landed on a consent wall, try to auto-accept and follow back.
    if is_consent_wall_str(&current_url) {
        // Retry up to 4 times with 1-second pauses between attempts.  The
        // consent page uses client-side JS (React/Next.js) and the accept button
        // may not be in the DOM yet when the first attempt runs, especially on
        // Windows where page hydration can lag behind the URL change.
        let mut accepted = false;
        for attempt in 0..4u32 {
            if attempt > 0 {
                tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
            }
            accepted = tokio::time::timeout(t(5), try_accept_consent(&page))
                .await.unwrap_or(false);
            if accepted { break; }
        }
        if accepted {
            // Poll until we leave the consent wall (up to 8 s) then let the
            // page settle for 500 ms before reading its content.
            wait_until_off_consent(&page, tokio::time::Duration::from_secs(8)).await;
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
        let post_url = tokio::time::timeout(t(5), page.url())
            .await.ok().and_then(|r| r.ok()).flatten().unwrap_or_default();
        if is_consent_wall_str(&post_url) {
            // Capture a snippet of the page HTML to diagnose why acceptance failed.
            let diag_html = tokio::time::timeout(t(5), page.content())
                .await.ok().and_then(|r| r.ok()).unwrap_or_default();
            let snippet: String = diag_html.chars().take(2000).collect();
            eprintln!("=== consent-wall debug ===");
            eprintln!("Consent URL : {current_url}");
            eprintln!("Button found: {accepted}");
            eprintln!("Post URL    : {post_url}");
            eprintln!("Page snippet:\n{snippet}");
            eprintln!("=== end consent-wall debug ===");
            let _ = tokio::time::timeout(t(3), page.close()).await;
            return Err(
                "Site redirected to a cookie-consent page that could not be \
                 auto-accepted. Try visiting the site in a real browser first \
                 to accept cookies, then reload here."
                    .to_owned(),
            );
        }
    }

    let html = tokio::time::timeout(t(15), page.content())
        .await
        .map_err(|_| "timed out waiting for page content (15 s)".to_owned())?
        .map_err(|e| format!("failed to get page content: {e}"))?;
    let _ = tokio::time::timeout(t(3), page.close()).await;

    if is_cf_blocked_html(&html) {
        return Err(format!(
            "{url} blocked the request. The site may require a CAPTCHA or has \
             restricted automated access entirely."
        ));
    }
    Ok(html)
}

// ---------------------------------------------------------------------------
// Consent-wall auto-accept helpers
// ---------------------------------------------------------------------------

/// CMP-specific CSS selectors for "accept all" buttons, tried in priority order.
/// Covers DPG Media, OneTrust, Didomi, TrustArc, Sourcepoint, Quantcast,
/// CookieBot, Cookie Information, Usercentrics, and generic patterns.
const CMP_SELECTORS: &[&str] = &[
    // DPG Media (hln.be, vtm.be, ad.nl, volkskrant.nl, …)
    r#"button[data-testid="pur-accept-button"]"#,
    r#"button[data-testid="pur-all-accept-button"]"#,
    r#"button[class*="pur-accept"]"#,
    // OneTrust
    "#onetrust-accept-btn-handler",
    "button.onetrust-close-btn-handler.accept-btn",
    // Didomi
    "#didomi-notice-agree-button",
    // TrustArc
    "#truste-consent-button",
    ".trustarc-agree-btn",
    // Sourcepoint
    r#"button[title="Accept All"]"#,
    ".message-button.accept-all",
    "button.sp_choice_type_11",
    // Quantcast
    "button.qc-cmp2-summary-buttons button:last-child",
    // CookieBot
    "#CybotCookiebotDialogBodyLevelButtonLevelOptinAllowAll",
    "#CybotCookiebotDialogBodyButtonAccept",
    // Cookie Information
    "#coi-banner-accept",
    // Usercentrics
    r#"button[data-testid="uc-accept-all-button"]"#,
    // Generic broad patterns
    r#"[data-role="accept-all"]"#,
    r#"[data-testid="accept-all"]"#,
    r#"button[id*="accept-all"]"#,
    r#"button[class*="accept-all"]"#,
    r#"button[class*="acceptAll"]"#,
    r#"button[id*="accept"][id*="all"]"#,
    r#"button[class*="AcceptAll"]"#,
    r#"[data-cy*="accept-all"]"#,
    r#"[aria-label*="accept all" i]"#,
    r#"[aria-label*="Alles accepteren" i]"#,
    r#"[aria-label*="akzeptieren" i]"#,
];

/// Button text substrings indicating a *reject* or settings action.
/// Checked before ACCEPT_KEYWORDS to prevent clicking "decline all" buttons
/// whose text happens to contain an accept keyword fragment.
const REJECT_KEYWORDS: &[&str] = &[
    "reject", "decline", "weiger", "refuser", "ablehnen", "rifiuta", "rechazar",
    "only necessary", "alleen noodzakelijke", "nur notwendige",
    "manage", "settings", "instellingen", "personnaliser", "preferences",
];

/// Button text substrings indicating an "accept all" action, in 6 languages.
const ACCEPT_KEYWORDS: &[&str] = &[
    // English
    "accept all", "allow all", "agree and continue", "i accept", "got it",
    // Dutch
    "alles accepteren", "accepteer alles", "akkoord", "ja, ik accepteer",
    "ik ga akkoord", "alles toestaan",
    // French
    "tout accepter", "j'accepte", "accepter et fermer", "continuer et accepter",
    // German
    "alle akzeptieren", "alles annehmen", "zustimmen", "einverstanden",
    "akzeptieren und weiter",
    // Italian
    "accetta tutto", "accetto", "acconsento",
    // Spanish
    "aceptar todo", "acepto", "aceptar y continuar",
];

/// Returns `true` if the button text looks like a reject/settings action.
#[cfg_attr(not(test), allow(dead_code))]
fn is_reject_text(text: &str) -> bool {
    let lower = text.to_lowercase();
    REJECT_KEYWORDS.iter().any(|k| lower.contains(k))
}

/// Returns `true` if the button text looks like an "accept all" action.
/// Always returns `false` when `is_reject_text` is true (guard takes priority).
#[cfg_attr(not(test), allow(dead_code))]
fn is_accept_keyword(text: &str) -> bool {
    if is_reject_text(text) {
        return false;
    }
    let lower = text.to_lowercase();
    ACCEPT_KEYWORDS.iter().any(|k| lower.contains(k))
}

/// Serialise a Rust string slice as a JSON array for embedding in JS.
fn js_array(items: &[&str]) -> String {
    let quoted: Vec<String> = items.iter().map(|s| format!("{s:?}")).collect();
    format!("[{}]", quoted.join(","))
}

/// Attempt to accept a GDPR consent wall on the current page.
///
/// Strategy 1 — DPG Media shortcut (hln.be, vtm.be, ad.nl, …):
///   The consent page sets `window.cmpProperties.siteUrl` inline, before the
///   external `consent.js` loads.  That URL is the exact callback hln.be uses
///   to record consent and set cookies.  We navigate to it directly, bypassing
///   the consent UI entirely.  This is reliable cross-platform because it needs
///   no external script to load.
///
/// Strategy 2 — Generic button click:
///   For all other CMPs, scan the DOM (and same-origin iframes) for a
///   recognisable "accept all" button and click it.
///
/// Returns `true` if a navigation was triggered.
async fn try_accept_consent(page: &chromiumoxide::Page) -> bool {
    // ── Strategy 1: DPG Media ────────────────────────────────────────────────
    // cmpProperties is set by an inline <script> tag, so it is available the
    // instant the HTML is parsed — no need to wait for consent.js.
    let dpg_js = r#"(function() {
        try {
            const u = window.cmpProperties && window.cmpProperties.siteUrl;
            if (u && u.length > 0) { window.location.href = u; return true; }
        } catch(_) {}
        return false;
    })()"#;
    let dpg_accepted = page.evaluate(dpg_js).await
        .ok()
        .and_then(|r| r.into_value::<bool>().ok())
        .unwrap_or(false);
    if dpg_accepted { return true; }

    // ── Strategy 2: generic button click ────────────────────────────────────
    let cmp_sels    = js_array(CMP_SELECTORS);
    let reject_kws  = js_array(REJECT_KEYWORDS);
    let accept_kws  = js_array(ACCEPT_KEYWORDS);
    let js = format!(r#"(function() {{
        const cmpSels   = {cmp_sels};
        const rejectKws = {reject_kws};
        const acceptKws = {accept_kws};

        function trySelectors(doc) {{
            for (const sel of cmpSels) {{
                try {{
                    const el = doc.querySelector(sel);
                    if (el) {{ el.click(); return true; }}
                }} catch(e) {{}}
            }}
            return false;
        }}

        function tryKeywords(doc) {{
            for (const btn of doc.querySelectorAll('button, [role="button"]')) {{
                const t = btn.textContent.trim().toLowerCase();
                if (rejectKws.some(k => t.includes(k))) continue;
                if (acceptKws.some(k => t.includes(k))) {{ btn.click(); return true; }}
            }}
            return false;
        }}

        function scanDoc(doc) {{
            return trySelectors(doc) || tryKeywords(doc);
        }}

        // Main document
        if (scanDoc(document)) return true;

        // One level of same-origin iframes (e.g. Sourcepoint)
        for (const iframe of document.querySelectorAll('iframe')) {{
            try {{
                const doc = iframe.contentDocument;
                if (doc && scanDoc(doc)) return true;
            }} catch(e) {{ /* cross-origin: skip */ }}
        }}

        return false;
    }})()"#);
    page.evaluate(js).await
        .ok()
        .and_then(|r| r.into_value::<bool>().ok())
        .unwrap_or(false)
}

fn is_consent_wall_str(url: &str) -> bool {
    url.contains("myprivacy.dpgmedia.be")
        || url.contains("sp-prod.net")
        || url.contains("privacy-mgmt.com")
        || url.contains("/consent")
        || url.contains("cookie-consent")
        || url.contains("consent.")
        || url.contains("cmp.")
}

/// Poll `page.url()` every 250 ms until we are no longer on a consent wall,
/// or until `budget` elapses.  Returns `true` if we left the wall.
async fn wait_until_off_consent(
    page: &chromiumoxide::Page,
    budget: tokio::time::Duration,
) -> bool {
    let interval = tokio::time::Duration::from_millis(250);
    let deadline = tokio::time::Instant::now() + budget;
    loop {
        tokio::time::sleep(interval).await;
        let url = tokio::time::timeout(
            tokio::time::Duration::from_secs(3),
            page.url(),
        )
        .await
        .ok()
        .and_then(|r| r.ok())
        .flatten()
        .unwrap_or_default();
        if !is_consent_wall_str(&url) {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
    }
}


/// Poll `page.url()` every 300 ms until the URL stabilises or a consent-wall
/// URL is detected, up to `budget`.  Returns the final URL seen.
///
/// "Stable" means the URL was the same on two consecutive polls AND at least
/// `MIN_WAIT` has elapsed since the call started — this prevents returning
/// prematurely before a slow JS redirect has had a chance to fire (a common
/// problem on Windows where the redirect can arrive after the load event).
async fn await_stable_url(page: &chromiumoxide::Page, budget: tokio::time::Duration) -> String {
    const MIN_WAIT: tokio::time::Duration = tokio::time::Duration::from_millis(1500);
    let poll_interval = tokio::time::Duration::from_millis(300);
    let deadline = tokio::time::Instant::now() + budget;
    let start    = tokio::time::Instant::now();
    let mut prev_url = String::new();
    loop {
        tokio::time::sleep(poll_interval).await;
        let url = tokio::time::timeout(
            tokio::time::Duration::from_secs(3),
            page.url(),
        )
        .await
        .ok()
        .and_then(|r| r.ok())
        .flatten()
        .unwrap_or_default();

        if is_consent_wall_str(&url) {
            return url; // Consent wall detected — stop early
        }
        if url == prev_url && start.elapsed() >= MIN_WAIT {
            return url; // URL has stabilised after minimum wait
        }
        if tokio::time::Instant::now() >= deadline {
            return url;
        }
        prev_url = url;
    }
}

// html_to_ffon and resolve_href live in sicompass-html (re-exported above).


// ---------------------------------------------------------------------------
// Tests — port of tests/lib_webbrowser/test_webbrowser.c (16 tests)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // ---- html_to_ffon unit tests ----

    #[test]
    fn test_paragraph_under_heading_becomes_child() {
        let result = html_to_ffon(
            "<html><body><h2>Section</h2><p>Content</p></body></html>",
            "https://example.com",
        );
        let section = result.iter().find(|e| e.as_obj().map_or(false, |o| o.key == "Section"));
        assert!(section.is_some(), "h2 should become an Obj");
        let children = &section.unwrap().as_obj().unwrap().children;
        assert!(
            children.iter().any(|c| c.as_str().map_or(false, |s| s.contains("Content"))),
            "paragraph should be a child of the heading, not a sibling"
        );
    }

    #[test]
    fn test_nested_headings_build_outline() {
        let result = html_to_ffon(
            "<html><body><h1>Top</h1><h2>Sub</h2><p>Leaf</p></body></html>",
            "https://example.com",
        );
        let top = result.iter().find(|e| e.as_obj().map_or(false, |o| o.key == "Top"));
        assert!(top.is_some(), "h1 should be at the top level");
        let top_children = &top.unwrap().as_obj().unwrap().children;
        let sub = top_children.iter().find(|e| e.as_obj().map_or(false, |o| o.key == "Sub"));
        assert!(sub.is_some(), "h2 should be a child of h1");
        let sub_children = &sub.unwrap().as_obj().unwrap().children;
        assert!(
            sub_children.iter().any(|c| c.as_str().map_or(false, |s| s.contains("Leaf"))),
            "paragraph should be a child of h2"
        );
    }

    #[test]
    fn test_whitespace_normalized_in_paragraph() {
        let result = html_to_ffon(
            "<html><body><p>Hello\n    world\t  here</p></body></html>",
            "https://example.com",
        );
        assert!(result.iter().any(|e| e.as_str().map_or(false, |s| s == "Hello world here")),
            "internal whitespace should be collapsed to single spaces");
    }

    #[test]
    fn test_empty_page_returns_placeholder() {
        let result = html_to_ffon("<html><body></body></html>", "https://example.com");
        // empty body → placeholder
        assert!(result.len() <= 1);
    }

    #[test]
    fn test_paragraph_becomes_str() {
        let result = html_to_ffon(
            "<html><body><p>Hello world</p></body></html>",
            "https://example.com",
        );
        assert!(result.iter().any(|e| e.as_str().map_or(false, |s| s.contains("Hello world"))));
    }

    #[test]
    fn test_heading_becomes_obj() {
        let result = html_to_ffon(
            "<html><body><h1>Title</h1></body></html>",
            "https://example.com",
        );
        assert!(result.iter().any(|e| e.as_obj().map_or(false, |o| o.key.contains("Title"))));
    }

    #[test]
    fn test_script_skipped() {
        let result = html_to_ffon(
            "<html><body><script>alert('x')</script><p>visible</p></body></html>",
            "https://example.com",
        );
        // No element should contain script content
        for e in &result {
            if let Some(s) = e.as_str() {
                assert!(!s.contains("alert"));
            }
        }
        assert!(result.iter().any(|e| e.as_str().map_or(false, |s| s.contains("visible"))));
    }

    #[test]
    fn test_nav_skipped() {
        let result = html_to_ffon(
            "<html><body><nav><a href='/'>Home</a></nav><p>content</p></body></html>",
            "https://example.com",
        );
        for e in &result {
            if let Some(s) = e.as_str() {
                assert!(!s.contains("Home") || s.contains("content"));
            }
        }
    }

    #[test]
    fn test_link_gets_link_tag() {
        let result = html_to_ffon(
            "<html><body><p><a href='https://rust-lang.org'>Rust</a></p></body></html>",
            "https://example.com",
        );
        // Links inside <p> are now Obj elements with <link> in the key
        let found = result.iter().any(|e| {
            e.as_obj().map_or(false, |o| o.key.contains("<link>") && o.key.contains("rust-lang.org"))
        });
        assert!(found, "link should be an Obj with <link> tag in key");
    }

    #[test]
    fn test_relative_link_resolved() {
        let result = html_to_ffon(
            "<html><body><p><a href='/page'>Page</a></p></body></html>",
            "https://example.com",
        );
        // Links inside <p> are now Obj elements with <link> in the key
        let found = result.iter().any(|e| {
            e.as_obj().map_or(false, |o| o.key.contains("example.com/page"))
        });
        assert!(found, "relative link should be resolved and appear as Obj key");
    }

    #[test]
    fn test_unordered_list_becomes_obj() {
        let result = html_to_ffon(
            "<html><body><ul><li>Alpha</li><li>Beta</li></ul></body></html>",
            "https://example.com",
        );
        let list = result.iter().find(|e| {
            e.as_obj().map_or(false, |o| o.key == "list")
        });
        assert!(list.is_some());
        let children = &list.unwrap().as_obj().unwrap().children;
        assert_eq!(children.len(), 2);
        assert!(children[0].as_str().unwrap().contains("Alpha"));
        assert!(children[1].as_str().unwrap().contains("Beta"));
    }

    #[test]
    fn test_ordered_list_numbered() {
        let result = html_to_ffon(
            "<html><body><ol><li>First</li><li>Second</li></ol></body></html>",
            "https://example.com",
        );
        let list = result.iter().find(|e| {
            e.as_obj().map_or(false, |o| o.key == "ordered list")
        });
        assert!(list.is_some());
        let children = &list.unwrap().as_obj().unwrap().children;
        assert!(children[0].as_str().unwrap().starts_with("1."));
        assert!(children[1].as_str().unwrap().starts_with("2."));
    }

    #[test]
    fn test_table_row_pipe_delimited() {
        let result = html_to_ffon(
            "<html><body><table><tr><td>A</td><td>B</td></tr></table></body></html>",
            "https://example.com",
        );
        let row = result.iter().find(|e| {
            e.as_str().map_or(false, |s| s.contains(" | "))
        });
        assert!(row.is_some());
        assert!(row.unwrap().as_str().unwrap().contains("A | B"));
    }

    #[test]
    fn test_image_shows_alt() {
        let result = html_to_ffon(
            "<html><body><img alt='A diagram' src='x.png'/></body></html>",
            "https://example.com",
        );
        let img = result.iter().find(|e| {
            e.as_str().map_or(false, |s| s.contains("A diagram") && s.contains("[img]"))
        });
        assert!(img.is_some());
    }

    #[test]
    fn test_fetch_returns_meta_and_url_bar() {
        let mut p = WebbrowserProvider::new();
        let items = p.fetch();
        // Index 0: url bar (no page loaded → str)
        assert!(items[0].as_str().is_some());
    }

    #[test]
    fn test_fetch_url_bar_contains_input_tag() {
        let mut p = WebbrowserProvider::new();
        let items = p.fetch();
        let url_bar = items[0].as_str().unwrap();
        assert!(url_bar.contains("<input>") && url_bar.contains("</input>"));
    }

    #[test]
    fn test_commit_edit_prepends_https() {
        let mut p = WebbrowserProvider::new();
        // commit_edit triggers fetch — use a URL that won't resolve in tests
        // We just test the URL normalization logic directly via current_url
        // by patching: simulate commit fail gracefully
        let _ = p.commit_edit("https://", "example.com");
        // After commit (whether fetch succeeds or fails), current_url is set
        assert_eq!(p.current_url, "https://example.com");
    }

    #[test]
    fn test_commands_includes_refresh() {
        let p = WebbrowserProvider::new();
        assert!(p.commands().contains(&"refresh".to_owned()));
    }

    #[test]
    fn test_resolve_href_absolute() {
        let result = html_resolve_href("https://other.com/page", "https://base.com");
        assert_eq!(result, "https://other.com/page");
    }

    #[test]
    fn test_resolve_href_relative() {
        let result = html_resolve_href("/path/to/page", "https://example.com/current");
        assert!(result.contains("example.com/path/to/page"));
    }

    #[test]
    fn test_resolve_href_anchor_preserved() {
        // Fragment-only hrefs are returned as-is for in-page navigation.
        let result = html_resolve_href("#section", "https://example.com");
        assert_eq!(result, "#section");
    }

    #[test]
    fn test_resolve_href_anchor_complex() {
        let result = html_resolve_href("#page-main-content", "https://www.hln.be/");
        assert_eq!(result, "#page-main-content");
    }

    // ---- fragment link parsing ----

    #[test]
    fn test_fragment_link_in_p_becomes_navigable_obj() {
        // <a href="#foo"> inside a paragraph should produce an Obj with <link>#foo</link>
        let result = html_to_ffon(
            "<html><body><p><a href=\"#foo\">skip to foo</a></p></body></html>",
            "https://example.com",
        );
        let found = result.iter().any(|e| {
            e.as_obj().map_or(false, |o| o.key.contains("<link>#foo</link>"))
        });
        assert!(found, "fragment link should become an Obj with <link>#foo</link>: {result:?}");
    }

    #[test]
    fn test_heading_with_id_gets_id_tag() {
        let result = html_to_ffon(
            "<html><body><h2 id=\"bar\">Section</h2></body></html>",
            "https://example.com",
        );
        let heading = result.iter().find(|e| e.as_obj().map_or(false, |o| o.key.contains("Section")));
        assert!(heading.is_some(), "heading should exist: {result:?}");
        let key = &heading.unwrap().as_obj().unwrap().key;
        assert!(key.contains("<id>bar</id>"), "heading key should contain <id>bar</id>: {key}");
    }

    #[test]
    fn test_container_with_id_propagates_to_first_child() {
        // <main id="x"><p>hi</p></main> — main is a CONTAINER_TAG, its id
        // should propagate to the first emitted element (the paragraph).
        let result = html_to_ffon(
            "<html><body><main id=\"x\"><p>hi</p></main></body></html>",
            "https://example.com",
        );
        let has_id_tag = result.iter().any(|e| match e {
            FfonElement::Str(s) => s.contains("<id>x</id>"),
            FfonElement::Obj(o) => o.key.contains("<id>x</id>"),
        });
        assert!(has_id_tag, "first element inside <main id=x> should have <id>x</id>: {result:?}");
    }

    #[test]
    fn test_ul_with_id_annotates_wrapper_not_item() {
        let result = html_to_ffon(
            "<html><body><ul id=\"things\"><li>a</li><li>b</li></ul></body></html>",
            "https://example.com",
        );
        // The list wrapper Obj (not an li) should carry the id tag.
        let list = result.iter().find(|e| {
            e.as_obj().map_or(false, |o| o.key.contains("list") && o.key.contains("<id>things</id>"))
        });
        assert!(list.is_some(), "list wrapper should have <id>things</id>: {result:?}");
        // Items should NOT have the id tag
        if let Some(FfonElement::Obj(l)) = list {
            let item_has_id = l.children.iter().any(|c| match c {
                FfonElement::Str(s) => s.contains("<id>things</id>"),
                FfonElement::Obj(o) => o.key.contains("<id>things</id>"),
            });
            assert!(!item_has_id, "list items should not have the id tag");
        }
    }

    #[test]
    fn test_skip_link_and_target_end_to_end() {
        // Full skip-link scenario: <a href="#foo"> + <main id="foo">
        let result = html_to_ffon(
            "<html><body>\
             <a href=\"#foo\">skip</a>\
             <main id=\"foo\"><p>Main content</p></main>\
             </body></html>",
            "https://example.com",
        );
        // Should contain a link obj pointing to #foo
        let has_link = result.iter().any(|e| {
            e.as_obj().map_or(false, |o| o.key.contains("<link>#foo</link>"))
        });
        assert!(has_link, "should have a navigable link to #foo: {result:?}");
        // Should contain an element tagged with <id>foo</id>
        let has_target = result.iter().any(|e| match e {
            FfonElement::Str(s) => s.contains("<id>foo</id>"),
            FfonElement::Obj(o) => o.key.contains("<id>foo</id>"),
        });
        assert!(has_target, "should have a target element with <id>foo</id>: {result:?}");
    }

    // ---- is_consent_wall unit tests ----

    #[test]
    fn test_is_consent_wall_detects_dpgmedia() {
        assert!(is_consent_wall_str(
            "https://myprivacy.dpgmedia.be/consent?siteKey=Uqxf9TXhjmaG4pbQ&callbackUrl=https%3A%2F%2Fwww.hln.be%2F"
        ));
    }

    #[test]
    fn test_is_consent_wall_passes_normal_url() {
        assert!(!is_consent_wall_str("https://www.hln.be/sport"));
    }

    #[test]
    fn test_is_consent_wall_detects_generic_consent_path() {
        assert!(is_consent_wall_str("https://example.com/consent?redirect=/"));
    }

    #[test]
    fn test_is_consent_wall_detects_cookie_consent_path() {
        assert!(is_consent_wall_str("https://example.com/page/cookie-consent/accept"));
    }

    #[test]
    fn test_is_consent_wall_detects_sourcepoint() {
        assert!(is_consent_wall_str("https://cdn.sp-prod.net/unified/v2/notice.html"));
    }

    #[test]
    fn test_is_consent_wall_detects_consent_subdomain() {
        assert!(is_consent_wall_str("https://consent.youtube.com/m?continue=https%3A%2F%2Fwww.youtube.com%2F"));
    }

    #[test]
    fn test_is_consent_wall_detects_privacy_mgmt() {
        assert!(is_consent_wall_str("https://privacy-mgmt.com/cmp?redirect=https://example.com"));
    }

    #[test]
    fn test_is_consent_wall_detects_cmp_subdomain() {
        assert!(is_consent_wall_str("https://cmp.example.com/notice"));
    }

    // ---- is_reject_text unit tests ----

    #[test]
    fn test_is_reject_text_english() {
        assert!(is_reject_text("Reject all"));
        assert!(is_reject_text("Decline cookies"));
        assert!(is_reject_text("Only necessary cookies"));
        assert!(is_reject_text("Manage preferences"));
        assert!(is_reject_text("Settings"));
    }

    #[test]
    fn test_is_reject_text_dutch() {
        assert!(is_reject_text("Weigeren"));
        assert!(is_reject_text("Instellingen"));
        assert!(is_reject_text("Alleen noodzakelijke"));
    }

    #[test]
    fn test_is_reject_text_german() {
        assert!(is_reject_text("Ablehnen"));
        assert!(is_reject_text("Nur notwendige"));
    }

    #[test]
    fn test_is_reject_text_false_for_accept() {
        assert!(!is_reject_text("Accept all"));
        assert!(!is_reject_text("Alles accepteren"));
        assert!(!is_reject_text("Tout accepter"));
    }

    // ---- is_accept_keyword unit tests ----

    #[test]
    fn test_is_accept_keyword_english() {
        assert!(is_accept_keyword("Accept all cookies"));
        assert!(is_accept_keyword("Allow all"));
        assert!(is_accept_keyword("Agree and continue"));
        assert!(is_accept_keyword("I accept"));
        assert!(is_accept_keyword("Got it"));
    }

    #[test]
    fn test_is_accept_keyword_dutch() {
        assert!(is_accept_keyword("Alles accepteren"));
        assert!(is_accept_keyword("Accepteer alles"));
        assert!(is_accept_keyword("Akkoord"));
        assert!(is_accept_keyword("Ja, ik accepteer"));
        assert!(is_accept_keyword("Ik ga akkoord"));
        assert!(is_accept_keyword("Alles toestaan"));
    }

    #[test]
    fn test_is_accept_keyword_french() {
        assert!(is_accept_keyword("Tout accepter"));
        assert!(is_accept_keyword("J'accepte"));
        assert!(is_accept_keyword("Accepter et fermer"));
        assert!(is_accept_keyword("Continuer et accepter"));
    }

    #[test]
    fn test_is_accept_keyword_german() {
        assert!(is_accept_keyword("Alle akzeptieren"));
        assert!(is_accept_keyword("Alles annehmen"));
        assert!(is_accept_keyword("Zustimmen"));
        assert!(is_accept_keyword("Einverstanden"));
        assert!(is_accept_keyword("Akzeptieren und weiter"));
    }

    #[test]
    fn test_is_accept_keyword_italian() {
        assert!(is_accept_keyword("Accetta tutto"));
        assert!(is_accept_keyword("Accetto"));
        assert!(is_accept_keyword("Acconsento"));
    }

    #[test]
    fn test_is_accept_keyword_spanish() {
        assert!(is_accept_keyword("Aceptar todo"));
        assert!(is_accept_keyword("Acepto"));
        assert!(is_accept_keyword("Aceptar y continuar"));
    }

    #[test]
    fn test_is_accept_keyword_reject_guard_takes_priority() {
        // "alles ablehnen" contains "alles" (part of "alles toestaan" keyword)
        // but the reject guard must fire first
        assert!(!is_accept_keyword("Alles ablehnen"));
        assert!(!is_accept_keyword("Reject all cookies"));
        assert!(!is_accept_keyword("Decline and manage settings"));
    }

    // ---- js_array unit tests ----

    #[test]
    fn test_js_array_produces_valid_json_array() {
        let result = js_array(&["foo", "bar"]);
        assert_eq!(result, r#"["foo","bar"]"#);
    }

    #[test]
    fn test_js_array_escapes_double_quotes() {
        let result = js_array(&[r#"button[data-testid="pur-accept-button"]"#]);
        // Must produce valid JSON (double-quotes escaped)
        assert!(result.contains(r#"\""#), "embedded quotes must be escaped: {result}");
    }

    // ---- is_cf_blocked_html unit tests ----

    #[test]
    fn test_is_cf_blocked_html_detects_sorry_blocked() {
        let html = r#"<html><body><h1>Sorry, you have been blocked</h1></body></html>"#;
        assert!(is_cf_blocked_html(html));
    }

    #[test]
    fn test_is_cf_blocked_html_detects_error_code() {
        let html = r#"<html><body><div class="cf-error-1010">Access denied</div></body></html>"#;
        assert!(is_cf_blocked_html(html));
    }

    #[test]
    fn test_is_cf_blocked_html_passes_normal_page() {
        let html = r#"<html><head><title>News</title></head><body><p>Article content</p></body></html>"#;
        assert!(!is_cf_blocked_html(html));
    }

    // Real-browser integration test — requires Chrome/Chromium and network access.
    // Run with: cargo test -p sicompass-webbrowser -- --ignored
    #[test]
    #[ignore]
    fn test_chromium_fetches_real_cloudflare_site() {
        let result = fetch_html_chromium("https://www.gva.be");
        assert!(result.is_ok(), "fetch failed: {:?}", result.err());
        let html = result.unwrap();
        assert!(!is_cf_blocked_html(&html), "response is a CF block page");
        assert!(!html.is_empty(), "expected non-empty HTML from gva.be");
    }
}

// ---------------------------------------------------------------------------
// SDK registration
// ---------------------------------------------------------------------------

/// Register the web browser with the SDK factory and manifest registries.
pub fn register() {
    sicompass_sdk::register_provider_factory("webbrowser", || {
        Box::new(WebbrowserProvider::new())
    });
    sicompass_sdk::register_builtin_manifest(
        sicompass_sdk::BuiltinManifest::new("webbrowser", "web browser"),
    );
    sicompass_sdk::register_url_fetcher(fetch_url_to_ffon);
}
