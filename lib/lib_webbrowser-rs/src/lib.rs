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

/// Fetch a URL via Chromium, parse the HTML, and return as FFON elements.
/// Used by the main app's `fetch_url_to_elements` bridge.
pub fn fetch_url_to_ffon(url: &str) -> Vec<FfonElement> {
    match fetch_html_chromium(url) {
        Ok(html) => html_to_ffon(&html, url),
        Err(e) => vec![FfonElement::new_str(format!("Error loading {url}: {e}"))],
    }
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

        // Wait briefly for JS-initiated redirects (e.g. consent-wall redirects).
        // A short timeout avoids blocking 30 s on pages with no redirect.
        let _ = tokio::time::timeout(
            tokio::time::Duration::from_secs(3),
            page.wait_for_navigation(),
        ).await;

        // If we landed on a consent wall, try to auto-accept and follow back.
        let current_url = page.url().await.ok().flatten().unwrap_or_default();
        if is_consent_wall_str(&current_url) {
            let accepted = try_accept_consent(&page).await;
            if accepted {
                let _ = tokio::time::timeout(
                    tokio::time::Duration::from_secs(5),
                    page.wait_for_navigation(),
                ).await;
            }
            // Whether or not we auto-accepted, re-check where we are.
            let post_url = page.url().await.ok().flatten().unwrap_or_default();
            if is_consent_wall_str(&post_url) {
                let _ = page.close().await;
                return Err(format!(
                    "Site redirected to a cookie-consent page that could not be \
                     auto-accepted. Try visiting the site in a real browser first \
                     to accept cookies, then reload here."
                ));
            }
        }

        let html = page.content().await
            .map_err(|e| format!("failed to get page content: {e}"))?;
        let _ = page.close().await;

        if is_cf_blocked_html(&html) {
            return Err(format!(
                "{url} blocked the request. The site may require a CAPTCHA or has \
                 restricted automated access entirely."
            ));
        }
        Ok(html)
    })
}

/// Attempt to accept a GDPR consent wall on the current page by clicking a
/// common "accept all" button. Returns `true` if a button was found and clicked.
async fn try_accept_consent(page: &chromiumoxide::Page) -> bool {
    let js = r#"(function() {
        const attrSels = [
            '[data-role="accept-all"]', '[data-testid="accept-all"]',
            'button[id*="accept-all"]', 'button[class*="accept-all"]',
            'button[class*="acceptAll"]', '[aria-label*="Accept all"]',
            '[aria-label*="Alles accepteren"]',
        ];
        for (const sel of attrSels) {
            const el = document.querySelector(sel);
            if (el) { el.click(); return true; }
        }
        const keywords = [
            'accept all', 'alles accepteren', 'accepteer alles',
            'tout accepter', 'alle akzeptieren', 'alles annehmen',
        ];
        for (const btn of document.querySelectorAll('button, [role="button"]')) {
            const t = btn.textContent.trim().toLowerCase();
            if (keywords.some(k => t.includes(k))) { btn.click(); return true; }
        }
        return false;
    })()"#;
    page.evaluate(js).await
        .ok()
        .and_then(|r| r.into_value::<bool>().ok())
        .unwrap_or(false)
}

fn is_consent_wall_str(url: &str) -> bool {
    url.contains("myprivacy.dpgmedia.be")
        || url.contains("/consent")
        || url.contains("cookie-consent")
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
