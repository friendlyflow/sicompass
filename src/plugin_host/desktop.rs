//! The user's desktop, for plugins: open a URL or a file, the installed
//! applications, the OS trash, and a browser sign-in.
//!
//! A plugin process asks for these through `sicompass_sdk::plugin::desktop`
//! ([`super::services`]), and the app's own code uses some of them too (the
//! Store moves a data folder to the trash with [`trash_delete`]).
//!
//! # Tests never touch the real desktop
//!
//! Two stubs, switched on by the integration tests (`ensure_builtins()`) and in
//! this crate's unit tests by default, following the pattern
//! `tests/hygiene.rs` enforces for every crate that can trash:
//!
//! - `TEST_NO_TRASH` ([`_set_test_no_trash`]): "trash" into a private temp
//!   directory instead of the OS trash. Tests once filled a developer's real
//!   trash with tens of thousands of fixtures, which is why this exists.
//! - `TEST_NO_OPEN` ([`_set_test_no_open`]): record what would have been opened
//!   instead of launching a browser or an application.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

static TEST_NO_TRASH: AtomicBool = AtomicBool::new(cfg!(test));
static TEST_NO_OPEN: AtomicBool = AtomicBool::new(cfg!(test));
static RECORDED: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Under `TEST_NO_OPEN`, the sign-in URLs `oauth-redirect` would have opened:
/// a list of their own, so a test can find its port while other tests drain
/// [`RECORDED`].
static SIGN_INS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static TEST_TRASH: Mutex<Vec<(PathBuf, PathBuf)>> = Mutex::new(Vec::new());

/// Test hook: trash into a private temp directory instead of the OS trash.
pub fn _set_test_no_trash(on: bool) {
    TEST_NO_TRASH.store(on, Ordering::Release);
}

/// Test hook: record opens instead of launching anything.
pub fn _set_test_no_open(on: bool) {
    TEST_NO_OPEN.store(on, Ordering::Release);
}

/// Test hook: both of the above.
pub fn _set_test_mode(on: bool) {
    _set_test_no_trash(on);
    _set_test_no_open(on);
}

/// Test hook: what was "opened" since the last call, as `open-url:<url>` or
/// `open-path:<host path>`.
pub fn _take_recorded() -> Vec<String> {
    RECORDED
        .lock()
        .map(|mut r| std::mem::take(&mut *r))
        .unwrap_or_default()
}

fn no_open() -> bool {
    TEST_NO_OPEN.load(Ordering::Acquire)
}

fn no_trash() -> bool {
    TEST_NO_TRASH.load(Ordering::Acquire)
}

/// Move `path` to the trash: the OS trash, or under `TEST_NO_TRASH` a private
/// temp directory that [`trash_restore`] can undo. Also what the Store's
/// "move its data folder to the trash" goes through (`programs.rs`).
pub(crate) fn trash_delete(path: &Path) -> Result<(), String> {
    if !no_trash() {
        return os_trash_delete(path);
    }
    let mut t = TEST_TRASH.lock().map_err(|e| e.to_string())?;
    let dir = std::env::temp_dir()
        .join("sicompass-test-trash")
        .join(format!("{}-{}", std::process::id(), t.len()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let moved = dir.join(path.file_name().unwrap_or_default());
    std::fs::rename(path, &moved).map_err(|e| e.to_string())?;
    t.push((path.to_path_buf(), moved));
    Ok(())
}

/// The one place this crate reaches the `trash` crate (tests/hygiene.rs), in
/// two variants: on macOS the crate's default goes
/// through Finder over AppleScript (slow, needs Automation permission and a
/// running Finder), so `NsFileManager` is used instead.
#[cfg(target_os = "macos")]
fn os_trash_delete(path: &Path) -> Result<(), String> {
    use trash::macos::{DeleteMethod, TrashContextExtMacos};
    let mut ctx = trash::TrashContext::default();
    ctx.set_delete_method(DeleteMethod::NsFileManager);
    ctx.delete(path).map_err(|e| e.to_string())
}

/// See the macOS variant above; everywhere else the crate default is fine.
#[cfg(not(target_os = "macos"))]
fn os_trash_delete(path: &Path) -> Result<(), String> {
    trash::delete(path).map_err(|e| e.to_string())
}

/// Undo [`trash_delete`] for `original`.
fn trash_restore(original: &Path) -> Result<(), String> {
    if !no_trash() {
        return sicompass_sdk::fs_trash::restore_from_os_trash(original);
    }
    let mut t = TEST_TRASH.lock().map_err(|e| e.to_string())?;
    let i = t
        .iter()
        .rposition(|(orig, _)| orig == original)
        .ok_or("no matching item found in the trash")?;
    let (orig, moved) = t.remove(i);
    std::fs::rename(&moved, &orig).map_err(|e| e.to_string())
}

/// How often the sign-in wait looks at the listener and the task's cancel flag.
const OAUTH_POLL: std::time::Duration = std::time::Duration::from_millis(50);

/// The longest a sign-in may wait for the browser.
const OAUTH_MAX_SECS: u32 = 600;

/// Percent-encode for a URL query value (RFC 3986 unreserved kept).
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The query of the request line `GET /?query HTTP/1.1`, if it has one.
fn request_query(request: &str) -> Option<String> {
    let target = request.lines().next()?.split_whitespace().nth(1)?;
    target.split_once('?').map(|(_, q)| q.to_owned())
}

/// Wait on `listener` for the browser's redirect, answering it with a page the
/// user can close. A request without a query (a favicon) is answered and
/// waited past.
fn await_redirect(
    listener: &std::net::TcpListener,
    deadline: std::time::Instant,
    cancel: Option<&std::sync::atomic::AtomicBool>,
) -> Result<String, String> {
    use std::io::{Read, Write};
    loop {
        if cancel.is_some_and(|c| c.load(Ordering::Acquire)) {
            return Err("sign-in cancelled".to_owned());
        }
        if std::time::Instant::now() > deadline {
            return Err("the sign-in timed out".to_owned());
        }
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));
                let mut buf = [0u8; 8192];
                let n = stream.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]);
                let Some(query) = request_query(&request) else {
                    let _ =
                        stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
                    continue;
                };
                let failed = query.split('&').any(|kv| kv.starts_with("error="));
                let (status, text) = if failed {
                    ("400 Bad Request", "Sign-in failed.")
                } else {
                    ("200 OK", "Signed in.")
                };
                let body = format!(
                    "<html><body><h2>{text}</h2><p>You can close this tab and return to Sicompass.</p></body></html>"
                );
                let _ = stream.write_all(
                    format!(
                        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                );
                return Ok(query);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(OAUTH_POLL),
            Err(e) => return Err(format!("the sign-in listener failed: {e}")),
        }
    }
}

// ---------------------------------------------------------------------------
// The desktop itself, for both plugin runtimes
// ---------------------------------------------------------------------------
//
// A WASM guest names paths inside its sandbox, which `HostState::confine_granted`
// maps and checks first; a plugin process names real paths. Either way, what
// happens to the desktop is one of these.

/// The browser comes back to a loopback port only this call listens on, and
/// only once. Waits until it does, `timeout_secs` passes, or `cancel` is set.
/// Returns `(redirect_uri, query)`.
pub(crate) fn sign_in(
    auth_url: &str,
    timeout_secs: u32,
    cancel: &AtomicBool,
) -> Result<(String, String), String> {
    if !auth_url.to_ascii_lowercase().starts_with("https://") {
        return Err("the sign-in URL must be https".to_owned());
    }
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("no loopback port for the sign-in: {e}"))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}");
    let url = auth_url.replace("{redirect-uri}", &percent_encode(&redirect_uri));
    if no_open() {
        if let Ok(mut r) = SIGN_INS.lock() {
            r.push(url);
        }
    } else {
        open_url(&url)?;
    }
    let deadline = std::time::Instant::now()
        + std::time::Duration::from_secs(timeout_secs.clamp(1, OAUTH_MAX_SECS).into());
    let query = await_redirect(&listener, deadline, Some(cancel))?;
    Ok((redirect_uri, query))
}

/// Open a URL in the user's browser or mail client: `http`, `https` and
/// `mailto` only.
pub(crate) fn open_url(url: &str) -> Result<(), String> {
    let scheme = url.split_once(':').map(|(s, _)| s.to_ascii_lowercase());
    if !matches!(scheme.as_deref(), Some("http" | "https" | "mailto")) {
        return Err("only http, https and mailto URLs can be opened".to_owned());
    }
    if no_open() {
        if let Ok(mut r) = RECORDED.lock() {
            r.push(format!("open-url:{url}"));
        }
        return Ok(());
    }
    if sicompass_sdk::platform::open_with_default(url) {
        Ok(())
    } else {
        Err("the desktop could not open that URL".to_owned())
    }
}

/// Open a file with the application the desktop associates with it.
pub(crate) fn open_path(host: &Path) -> Result<(), String> {
    if no_open() {
        if let Ok(mut r) = RECORDED.lock() {
            r.push(format!("open-path:{}", host.display()));
        }
        return Ok(());
    }
    if sicompass_sdk::platform::open_with_default(&host.to_string_lossy()) {
        Ok(())
    } else {
        Err("the desktop could not open that file".to_owned())
    }
}

/// The installed applications as `(name, id)`. Read from the system each
/// time, so an application installed while sicompass runs is there. The id is
/// the command the system launches it with, which [`open_with`] checks
/// against this same list.
pub(crate) fn applications() -> Vec<(String, String)> {
    sicompass_sdk::platform::get_applications()
        .into_iter()
        .map(|a| (a.name, a.exec))
        .collect()
}

/// Only an id [`applications`] lists: a plugin chooses among the user's
/// installed applications, and never names a program this way.
pub(crate) fn open_with(id: &str, host: &Path) -> Result<(), String> {
    if !sicompass_sdk::platform::get_applications()
        .iter()
        .any(|a| a.exec == id)
    {
        return Err("that is not an installed application".to_owned());
    }
    if no_open() {
        if let Ok(mut r) = RECORDED.lock() {
            r.push(format!("open-with:{id}:{}", host.display()));
        }
        return Ok(());
    }
    if sicompass_sdk::platform::open_with(id, &host.to_string_lossy()) {
        Ok(())
    } else {
        Err("the application could not be started".to_owned())
    }
}

/// Move the entry itself to the trash: for a symlink, the link.
pub(crate) fn trash(host: &Path) -> Result<(), String> {
    std::fs::symlink_metadata(host).map_err(|e| format!("{}: {e}", host.display()))?;
    trash_delete(host)
}

/// Undo [`trash`] for `host`.
pub(crate) fn restore(host: &Path) -> Result<(), String> {
    trash_restore(host)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The URL the sign-in opened, from what the test mode recorded.
    fn opened_sign_in(needle: &str) -> String {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            // Only this test's entry: tests run in parallel and share the record.
            let found = SIGN_INS.lock().ok().and_then(|mut r| {
                let at = r.iter().position(|e| e.contains(needle))?;
                Some(r.remove(at))
            });
            if let Some(u) = found {
                return u;
            }
            assert!(std::time::Instant::now() < deadline, "nothing was opened");
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn a_sign_in_hands_back_what_the_browser_came_back_with() {
        use std::io::{Read, Write};
        _set_test_no_open(true);
        let waiter = std::thread::spawn(move || {
            sign_in(
                "https://auth.example.org/o?client_id=x&redirect_uri={redirect-uri}&z=signin1",
                30,
                &AtomicBool::new(false),
            )
        });
        let url = opened_sign_in("z=signin1");
        let redirect = url
            .split("redirect_uri=")
            .nth(1)
            .and_then(|r| r.split('&').next())
            .unwrap()
            .replace("%3A", ":")
            .replace("%2F", "/");
        assert!(redirect.starts_with("http://127.0.0.1:"), "{url}");
        let port: u16 = redirect.rsplit(':').next().unwrap().parse().unwrap();
        // A favicon first, as a browser does: answered and waited past.
        let mut ico = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        ico.write_all(b"GET /favicon.ico HTTP/1.1\r\n\r\n").unwrap();
        let mut sink = String::new();
        let _ = ico.read_to_string(&mut sink);
        let mut back = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        back.write_all(b"GET /?code=4%2Fabc&state=s1 HTTP/1.1\r\nHost: x\r\n\r\n")
            .unwrap();
        let mut page = String::new();
        let _ = back.read_to_string(&mut page);
        assert!(page.starts_with("HTTP/1.1 200"), "{page}");
        let (redirect_uri, query) = waiter.join().unwrap().unwrap();
        assert_eq!(redirect_uri, redirect);
        assert_eq!(query, "code=4%2Fabc&state=s1");
    }

    #[test]
    fn a_sign_in_ends_when_it_is_cancelled() {
        _set_test_no_open(true);
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let waiter = std::thread::spawn(move || {
            sign_in(
                "https://auth.example.org/o?r={redirect-uri}&z=signin2",
                600,
                &flag,
            )
        });
        opened_sign_in("z=signin2");
        cancel.store(true, Ordering::Release);
        let err = waiter.join().unwrap().unwrap_err();
        assert!(err.contains("cancelled"), "{err}");
    }

    #[test]
    fn a_sign_in_is_https() {
        let err = sign_in("http://a.example/", 5, &AtomicBool::new(false)).unwrap_err();
        assert!(err.contains("https"), "{err}");
    }

    /// Only an application the host listed can be asked for.
    #[test]
    fn open_with_takes_only_a_listed_application() {
        _set_test_no_open(true);
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "x").unwrap();
        assert!(open_with("rm -rf ~", &file).is_err());
        if let Some((_, id)) = applications().into_iter().next() {
            open_with(&id, &file).unwrap();
        }
    }

    /// Trashing a symlink takes the link, never its target: deleting a link
    /// to a folder in the file browser must not throw the folder away.
    #[cfg(unix)]
    #[test]
    fn trash_takes_a_link_and_leaves_its_target() {
        _set_test_no_trash(true);
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir(root.join("docs")).unwrap();
        std::fs::write(root.join("docs/a.txt"), "keep me").unwrap();
        std::os::unix::fs::symlink(root.join("docs"), root.join("link")).unwrap();

        trash(&root.join("link")).unwrap();
        assert!(
            std::fs::symlink_metadata(root.join("link")).is_err(),
            "the link is gone"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("docs/a.txt")).unwrap(),
            "keep me",
            "and what it pointed to is untouched"
        );
        assert!(trash(&root.join("nothing")).is_err());
        restore(&root.join("link")).unwrap();
        assert!(
            std::fs::symlink_metadata(root.join("link")).is_ok(),
            "and it comes back"
        );
    }

    #[test]
    fn only_web_and_mail_urls_open() {
        _set_test_no_open(true);
        assert!(open_url("https://example.com/only-web-and-mail").is_ok());
        assert!(open_url("file:///etc/passwd").is_err());
        assert!(open_url("javascript:alert(1)").is_err());
        assert!(
            _take_recorded().contains(&"open-url:https://example.com/only-web-and-mail".to_owned())
        );
    }
}
