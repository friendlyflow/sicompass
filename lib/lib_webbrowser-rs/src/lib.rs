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

    fn meta(&self) -> Vec<String> {
        vec![
            "I   Edit URL".to_owned(),
            "/   Search".to_owned(),
            "Ctrl+F  Extended search".to_owned(),
            "F5  Refresh".to_owned(),
            ":   Commands".to_owned(),
        ]
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
// Chromium singleton — shared Chrome process + dedicated runtime
// ---------------------------------------------------------------------------

// Multi-thread runtime (2 workers) for chromiumoxide. Kept alive for the
// process lifetime so the CDP handler task keeps running between fetches.
static CHROMIUM_RUNTIME: std::sync::OnceLock<tokio::runtime::Runtime> =
    std::sync::OnceLock::new();

// The Browser value is Send + Sync, so a &'static Browser is safe to pass
// into async blocks on the multi-thread runtime.
static CHROMIUM_BROWSER: std::sync::OnceLock<Result<Browser, String>> =
    std::sync::OnceLock::new();

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

    let wrapper = std::path::PathBuf::from("/tmp/sicompass-xvfb-chrome.sh");
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

/// Try common Chrome/Chromium binary names and return the first one found in PATH.
/// chromiumoxide's built-in detection only looks for `chrome` and `chromium`;
/// on many Linux distros the binary is `google-chrome` or `google-chrome-stable`.
fn find_chrome_executable() -> Option<std::path::PathBuf> {
    const CANDIDATES: &[&str] = &[
        "google-chrome",
        "google-chrome-stable",
        "google-chrome-beta",
        "chromium",
        "chromium-browser",
        "chrome",
    ];
    CANDIDATES
        .iter()
        .find_map(|name| which::which(name).ok())
}

fn acquire_browser() -> Result<&'static Browser, String> {
    CHROMIUM_BROWSER
        .get_or_init(|| {
            chromium_runtime().block_on(async {
                // Remove any leftover SingletonLock from a previous crashed run.
                let profile_dir = "/tmp/sicompass-chrome";
                let _ = std::fs::remove_file(
                    std::path::Path::new(profile_dir).join("SingletonLock"),
                );

                // On Linux: wrap Chrome with xvfb-run so it renders on an
                // auto-allocated virtual display — invisible, no DISPLAY juggling.
                // On macOS/Windows: new headless mode suffices.
                #[cfg(target_os = "linux")]
                let mut builder = {
                    let exe = if let Ok(p) = std::env::var("SICOMPASS_CHROME_PATH") {
                        std::path::PathBuf::from(p)
                    } else {
                        chrome_via_xvfb()?
                    };
                    BrowserConfig::builder()
                        .with_head()
                        .arg("--disable-blink-features=AutomationControlled")
                        .user_data_dir(profile_dir)
                        .window_size(1920, 1080)
                        .chrome_executable(exe)
                };

                #[cfg(not(target_os = "linux"))]
                let mut builder = {
                    let mut b = BrowserConfig::builder()
                        .new_headless_mode()
                        .arg("--disable-blink-features=AutomationControlled")
                        .user_data_dir(profile_dir)
                        .window_size(1920, 1080);
                    if let Ok(p) = std::env::var("SICOMPASS_CHROME_PATH") {
                        b = b.chrome_executable(p);
                    } else if let Some(p) = find_chrome_executable() {
                        b = b.chrome_executable(p);
                    }
                    b
                };

                let config = builder
                    .build()
                    .map_err(|e| format!("chromium config error: {e}"))?;

                let (browser, mut handler) =
                    Browser::launch(config).await.map_err(|e| {
                        format!(
                            "failed to launch Chrome — is Chrome/Chromium installed? \
                             (set SICOMPASS_CHROME_PATH to override): {e}"
                        )
                    })?;

                // Drive the CDP event loop in a background task; it must keep
                // running or all browser operations will stall.
                tokio::spawn(async move {
                    while handler.next().await.is_some() {}
                });

                Ok(browser)
            })
        })
        .as_ref()
        .map_err(|e| e.clone())
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
"#;

/// Returns true if the page body is Cloudflare's "Sorry, you have been blocked" wall.
fn is_cf_blocked_html(html: &str) -> bool {
    html.contains("Sorry, you have been blocked")
        || html.contains("cf-error-1010")
        || html.contains("cf-error-1020")
}

fn fetch_html_chromium(url: &str) -> Result<String, String> {
    let browser: &'static Browser = acquire_browser()?;

    chromium_runtime().block_on(async move {
        let page = browser
            .new_page("about:blank")
            .await
            .map_err(|e| format!("failed to open tab: {e}"))?;

        page.execute(AddScriptToEvaluateOnNewDocumentParams::new(STEALTH_SCRIPT))
            .await
            .map_err(|e| format!("stealth script injection failed: {e}"))?;

        page.goto(url)
            .await
            .map_err(|e| format!("navigation to {url} failed: {e}"))?;

        page.wait_for_navigation()
            .await
            .map_err(|e| format!("navigation failed: {e}"))?;

        let final_url = page.url().await.ok().flatten();
        let html = page.content().await
            .map_err(|e| format!("failed to get page content: {e}"))?;
        let _ = page.close().await;

        if is_cf_blocked_html(&html) {
            return Err(format!(
                "{url} blocked the request. The site may require a CAPTCHA or has \
                 restricted automated access entirely."
            ));
        }
        if let Some(u) = final_url {
            if is_consent_wall_str(&u) {
                return Err(format!(
                    "Site redirected to a cookie-consent page ({}) — sicompass cannot \
                     complete JS-based consent flows.",
                    url::Url::parse(&u)
                        .ok()
                        .and_then(|p| p.host_str().map(str::to_owned))
                        .unwrap_or(u)
                ));
            }
        }
        Ok(html)
    })
}

fn is_consent_wall_str(url: &str) -> bool {
    url.contains("myprivacy.dpgmedia.be")
        || url.contains("/consent")
        || url.contains("cookie-consent")
}


// ---------------------------------------------------------------------------
// HTML → FFON conversion
// ---------------------------------------------------------------------------

/// Tags we skip entirely (including all their children).
const SKIP_TAGS: &[&str] = &[
    "script", "style", "noscript", "svg", "head", "nav", "footer",
];

/// Block container tags — recurse into children without emitting a wrapper element.
const CONTAINER_TAGS: &[&str] = &[
    "div", "section", "article", "main", "header", "aside", "figure",
    "blockquote", "details", "summary",
];

/// Collapse all whitespace (including newlines/tabs) to single spaces, trim ends.
fn normalize_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_ws = true; // start true → leading whitespace is trimmed
    for c in s.chars() {
        if c.is_whitespace() {
            if !prev_ws {
                out.push(' ');
                prev_ws = true;
            }
        } else {
            out.push(c);
            prev_ws = false;
        }
    }
    if out.ends_with(' ') { out.pop(); }
    out
}

fn heading_level(tag: &str) -> Option<u8> {
    match tag {
        "h1" => Some(1), "h2" => Some(2), "h3" => Some(3),
        "h4" => Some(4), "h5" => Some(5), "h6" => Some(6),
        _ => None,
    }
}

/// Builds a document outline using a heading stack, mirroring the C `ParseContext`.
///
/// Headings create Obj nodes that collect all following content (paragraphs,
/// lists, etc.) as children until a same-or-higher-level heading replaces them.
struct ParseCtx<'a> {
    base_url: &'a str,
    /// Top-level elements (before the first heading, or after all headings are popped).
    root: Vec<FfonElement>,
    /// Open heading stack: (level, accumulated Obj). Innermost is last.
    stack: Vec<(u8, FfonElement)>,
}

impl<'a> ParseCtx<'a> {
    fn new(base_url: &'a str) -> Self {
        ParseCtx { base_url, root: Vec::new(), stack: Vec::new() }
    }

    /// Add `elem` as a child of the current heading, or to root if no heading is open.
    fn add_to_current(&mut self, elem: FfonElement) {
        if let Some((_, ref mut h)) = self.stack.last_mut() {
            h.as_obj_mut().unwrap().push(elem);
        } else {
            self.root.push(elem);
        }
    }

    /// Pop all stack entries with level >= `level`, nesting each into its parent.
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

    /// Drain the stack (innermost first) and return the completed top-level elements.
    fn finalize(mut self) -> Vec<FfonElement> {
        while let Some((_, entry)) = self.stack.pop() {
            if let Some((_, ref mut parent)) = self.stack.last_mut() {
                parent.as_obj_mut().unwrap().push(entry);
            } else {
                self.root.push(entry);
            }
        }
        self.root
    }

    fn process_children(&mut self, node: scraper::ElementRef) {
        for child in node.children().filter_map(scraper::ElementRef::wrap) {
            self.process_node(child);
        }
    }

    fn process_node(&mut self, node: scraper::ElementRef) {
        let tag = node.value().name();

        if SKIP_TAGS.contains(&tag) { return; }

        // Headings push onto the stack; following content becomes their children.
        if let Some(level) = heading_level(tag) {
            let text = collect_text(node, self.base_url);
            if text.is_empty() { return; }
            self.pop_until_level(level);
            self.stack.push((level, FfonElement::new_obj(text)));
            return;
        }

        match tag {
            "p" => {
                for elem in collect_elements(node, self.base_url) {
                    self.add_to_current(elem);
                }
            }
            "ul" | "ol" => {
                let label = if tag == "ol" { "ordered list" } else { "list" };
                let mut list_obj = FfonElement::new_obj(label);
                let li_sel = scraper::Selector::parse("li").unwrap();
                for (i, li) in node.select(&li_sel).enumerate() {
                    let elems = collect_elements(li, self.base_url);
                    for elem in elems {
                        let prefixed = match &elem {
                            FfonElement::Str(s) => {
                                let item = if tag == "ol" {
                                    format!("{}. {}", i + 1, s)
                                } else {
                                    format!("- {}", s)
                                };
                                FfonElement::new_str(item)
                            }
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
                collect_table_rows(node, &mut rows);
                for row in rows {
                    self.add_to_current(row);
                }
            }
            "pre" | "code" => {
                let text = node.text().collect::<String>();
                let trimmed = text.trim().to_owned();
                if !trimmed.is_empty() {
                    self.add_to_current(FfonElement::new_str(trimmed));
                }
            }
            "img" => {
                let alt = node.value().attr("alt").unwrap_or("");
                if !alt.is_empty() && alt != "image" {
                    self.add_to_current(FfonElement::new_str(format!("{alt} [img]")));
                }
            }
            "a" => {
                let href = resolve_href(node.value().attr("href").unwrap_or(""), self.base_url);
                let text = collect_text(node, self.base_url);
                if !text.is_empty() && !href.is_empty() {
                    self.add_to_current(FfonElement::new_obj(format!("{text} <link>{href}</link>")));
                } else if !text.is_empty() {
                    self.add_to_current(FfonElement::new_str(text));
                }
            }
            "dl" => {
                let mut dl_obj = FfonElement::new_obj("definition list");
                let mut current_dt: Option<FfonElement> = None;
                for child in node.children().filter_map(scraper::ElementRef::wrap) {
                    let text = collect_text(child, self.base_url);
                    if text.is_empty() { continue; }
                    match child.value().name() {
                        "dt" => {
                            if let Some(dt) = current_dt.take() {
                                dl_obj.as_obj_mut().unwrap().push(dt);
                            }
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
            t if CONTAINER_TAGS.contains(&t) => {
                self.process_children(node);
            }
            _ => {
                // Unknown element: recurse if it has block-level children, else emit text.
                let has_block = node.children()
                    .filter_map(scraper::ElementRef::wrap)
                    .any(|c| {
                        let t = c.value().name();
                        heading_level(t).is_some()
                            || matches!(t, "p" | "ul" | "ol" | "table" | "dl")
                            || CONTAINER_TAGS.contains(&t)
                    });
                if has_block {
                    self.process_children(node);
                } else {
                    let text = collect_text(node, self.base_url);
                    if !text.is_empty() {
                        self.add_to_current(FfonElement::new_str(text));
                    }
                }
            }
        }
    }
}

/// Convert an HTML string to a flat list of FfonElements.
pub fn html_to_ffon(html: &str, base_url: &str) -> Vec<FfonElement> {
    use scraper::{Html, Selector};

    let document = Html::parse_document(html);
    let body_sel = Selector::parse("body").unwrap();

    let body = match document.select(&body_sel).next() {
        Some(b) => b,
        None => return vec![FfonElement::new_str("(empty page)")],
    };

    let mut ctx = ParseCtx::new(base_url);
    ctx.process_children(body);
    let result = ctx.finalize();

    if result.is_empty() {
        vec![FfonElement::new_str("(empty page)")]
    } else {
        result
    }
}

/// Collect all text within a node, converting <a> tags to `text <link>url</link>`,
/// normalizing all whitespace sequences to single spaces.
fn collect_text(node: scraper::ElementRef, base_url: &str) -> String {
    use scraper::Node;
    let mut buf = String::new();
    for child in node.children() {
        match child.value() {
            Node::Text(t) => buf.push_str(t),
            Node::Element(e) => {
                if let Some(elem_ref) = scraper::ElementRef::wrap(child) {
                    let name = e.name();
                    if SKIP_TAGS.contains(&name) { continue; }
                    if name == "a" {
                        let href = resolve_href(e.attr("href").unwrap_or(""), base_url);
                        let text = collect_text(elem_ref, base_url);
                        if !text.is_empty() && !href.is_empty() {
                            buf.push_str(&format!("{text} <link>{href}</link>"));
                        } else if !text.is_empty() {
                            buf.push_str(&text);
                        }
                    } else {
                        buf.push_str(&collect_text(elem_ref, base_url));
                    }
                }
            }
            _ => {}
        }
    }
    normalize_whitespace(&buf)
}

/// Walk a node's direct inline content, splitting text runs as Str and
/// `<a>` tags as Obj with `<link>` in the key. Used for `<p>` and `<li>`.
fn collect_elements(node: scraper::ElementRef, base_url: &str) -> Vec<FfonElement> {
    use scraper::Node;
    let mut result: Vec<FfonElement> = Vec::new();
    let mut text_buf = String::new();

    for child in node.children() {
        match child.value() {
            Node::Text(t) => text_buf.push_str(t),
            Node::Element(e) => {
                if let Some(elem_ref) = scraper::ElementRef::wrap(child) {
                    let name = e.name();
                    if SKIP_TAGS.contains(&name) { continue; }
                    if name == "a" {
                        // Flush accumulated text before the link
                        let normalized = normalize_whitespace(&text_buf);
                        if !normalized.is_empty() {
                            result.push(FfonElement::new_str(normalized));
                        }
                        text_buf.clear();
                        let href = resolve_href(e.attr("href").unwrap_or(""), base_url);
                        let link_text = collect_text(elem_ref, base_url);
                        if !link_text.is_empty() && !href.is_empty() {
                            result.push(FfonElement::new_obj(
                                format!("{link_text} <link>{href}</link>"),
                            ));
                        } else if !link_text.is_empty() {
                            text_buf.push_str(&link_text);
                        }
                    } else {
                        text_buf.push_str(&collect_text(elem_ref, base_url));
                    }
                }
            }
            _ => {}
        }
    }

    // Flush remaining text
    let normalized = normalize_whitespace(&text_buf);
    if !normalized.is_empty() {
        result.push(FfonElement::new_str(normalized));
    }

    result
}

fn collect_table_rows(node: scraper::ElementRef, out: &mut Vec<FfonElement>) {
    let row_sel = scraper::Selector::parse("tr").unwrap();
    for row in node.select(&row_sel) {
        let cell_sel = scraper::Selector::parse("th, td").unwrap();
        let cells: Vec<String> = row
            .select(&cell_sel)
            .map(|c| normalize_whitespace(&c.text().collect::<String>()))
            .filter(|s| !s.is_empty())
            .collect();
        if !cells.is_empty() {
            out.push(FfonElement::new_str(cells.join(" | ")));
        }
    }
}

/// Resolve a potentially relative href against the base URL.
fn resolve_href(href: &str, base_url: &str) -> String {
    if href.is_empty() || href.starts_with('#') {
        return String::new();
    }
    if href.contains("://") || href.starts_with("mailto:") || href.starts_with("tel:") {
        return href.to_owned();
    }
    // Relative URL resolution
    if let Ok(base) = url::Url::parse(base_url) {
        if let Ok(resolved) = base.join(href) {
            return resolved.to_string();
        }
    }
    href.to_owned()
}

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
        let result = resolve_href("https://other.com/page", "https://base.com");
        assert_eq!(result, "https://other.com/page");
    }

    #[test]
    fn test_resolve_href_relative() {
        let result = resolve_href("/path/to/page", "https://example.com/current");
        assert!(result.contains("example.com/path/to/page"));
    }

    #[test]
    fn test_resolve_href_anchor_empty() {
        let result = resolve_href("#section", "https://example.com");
        assert!(result.is_empty());
    }

    // ---- is_consent_wall unit tests ----

    #[test]
    fn test_is_consent_wall_detects_dpgmedia() {
        let url = url::Url::parse(
            "https://myprivacy.dpgmedia.be/consent?siteKey=Uqxf9TXhjmaG4pbQ&callbackUrl=https%3A%2F%2Fwww.hln.be%2F"
        ).unwrap();
        assert!(is_consent_wall_str(url.as_str()));
    }

    #[test]
    fn test_is_consent_wall_passes_normal_url() {
        let url = url::Url::parse("https://www.hln.be/sport").unwrap();
        assert!(!is_consent_wall_str(url.as_str()));
    }

    #[test]
    fn test_is_consent_wall_detects_generic_consent_path() {
        let url = url::Url::parse("https://example.com/consent?redirect=/").unwrap();
        assert!(is_consent_wall_str(url.as_str()));
    }

    #[test]
    fn test_is_consent_wall_detects_cookie_consent_path() {
        let url = url::Url::parse("https://example.com/page/cookie-consent/accept").unwrap();
        assert!(is_consent_wall_str(url.as_str()));
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
