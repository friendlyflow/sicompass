//! `sicompass:plugin/desktop`: open a URL or a file, trash and restore.
//!
//! Always linked, and harmless because every path is confined: it must lie
//! inside a directory this plugin was granted ([`super::Grants`]), checked here
//! after resolving symlinks. A plugin with no granted directory can only open
//! `http`, `https` and `mailto` URLs.
//!
//! Paths arrive as the guest sees them. The plugin's own storage appears at
//! `/storage` inside the guest and is mapped back to its host directory; a
//! user-granted folder has the same path on both sides.
//!
//! # Tests never touch the real desktop
//!
//! Two stubs, both on by default in this crate's unit tests and switched on by
//! the integration tests (`ensure_builtins()`), following the pattern
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

use super::HostState;
use super::sicompass::plugin as wit;

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

/// `ls -l`'s view of `meta`, for `desktop.stat`.
fn file_info(meta: &std::fs::Metadata) -> wit::desktop::FileInfo {
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs() as i64);
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        wit::desktop::FileInfo {
            size: meta.len(),
            modified,
            utc_offset: utc_offset_at(modified),
            mode: Some(meta.mode()),
            links: Some(meta.nlink()),
            owner: Some(user_name(meta.uid())),
            group: Some(group_name(meta.gid())),
        }
    }
    #[cfg(not(unix))]
    {
        wit::desktop::FileInfo {
            size: meta.len(),
            modified,
            utc_offset: 0,
            mode: None,
            links: None,
            owner: None,
            group: None,
        }
    }
}

/// The local UTC offset at `secs` (seconds east), daylight saving included.
#[cfg(unix)]
fn utc_offset_at(secs: i64) -> i32 {
    let t = secs as libc::time_t;
    // SAFETY: `localtime_r` writes only into `tm`, which lives on this frame.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return 0;
    }
    tm.tm_gmtoff as i32
}

/// A user's name, or the number when there is none (`ls` does the same).
#[cfg(unix)]
fn user_name(uid: u32) -> String {
    let mut buf = vec![0 as libc::c_char; 4096];
    // SAFETY: `pwd` and `buf` outlive the call, and `buf.len()` is its size;
    // the name is read only when the call reports it found an entry.
    unsafe {
        let mut pwd: libc::passwd = std::mem::zeroed();
        let mut out: *mut libc::passwd = std::ptr::null_mut();
        if libc::getpwuid_r(uid, &mut pwd, buf.as_mut_ptr(), buf.len(), &mut out) == 0
            && !out.is_null()
        {
            return std::ffi::CStr::from_ptr(pwd.pw_name).to_string_lossy().into_owned();
        }
    }
    uid.to_string()
}

/// A group's name, or the number when there is none.
#[cfg(unix)]
fn group_name(gid: u32) -> String {
    let mut buf = vec![0 as libc::c_char; 4096];
    // SAFETY: as in `user_name`.
    unsafe {
        let mut grp: libc::group = std::mem::zeroed();
        let mut out: *mut libc::group = std::ptr::null_mut();
        if libc::getgrgid_r(gid, &mut grp, buf.as_mut_ptr(), buf.len(), &mut out) == 0
            && !out.is_null()
        {
            return std::ffi::CStr::from_ptr(grp.gr_name).to_string_lossy().into_owned();
        }
    }
    gid.to_string()
}

impl HostState {
    /// The host path for a guest path, if it lies inside a granted directory.
    ///
    /// `must_exist` is false for `restore`, whose target is gone until restored:
    /// then the parent directory is what gets resolved and checked.
    pub fn confine_granted(&self, guest: &str, must_exist: bool) -> Result<PathBuf, String> {
        let refuse = || format!("`{guest}` is outside the folders this plugin was granted");
        let lexical = self.guest_to_host(guest).ok_or_else(refuse)?;

        let resolved = if must_exist {
            lexical
                .canonicalize()
                .map_err(|e| format!("{guest}: {e}"))?
        } else {
            let parent = lexical.parent().ok_or_else(refuse)?;
            let name = lexical.file_name().ok_or_else(refuse)?;
            parent
                .canonicalize()
                .map_err(|e| format!("{guest}: {e}"))?
                .join(name)
        };
        let inside = self
            .granted_roots
            .iter()
            .filter_map(|(_, host)| host.canonicalize().ok())
            .any(|root| resolved.starts_with(&root) && resolved != root);
        if inside { Ok(resolved) } else { Err(refuse()) }
    }

    /// Map a guest path onto the host, lexically. `None` for a relative path,
    /// one containing `..`, or one under no granted root.
    fn guest_to_host(&self, guest: &str) -> Option<PathBuf> {
        let p = Path::new(guest);
        if !p.is_absolute() || p.components().any(|c| c == std::path::Component::ParentDir) {
            return None;
        }
        self.granted_roots
            .iter()
            .find_map(|(guest_root, host_root)| {
                p.strip_prefix(guest_root)
                    .ok()
                    .map(|rest| host_root.join(rest))
            })
    }
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
                    let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
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

impl wit::desktop::Host for HostState {
    /// See the WIT. The browser comes back to a loopback port only this call
    /// listens on, and only once.
    fn oauth_redirect(
        &mut self,
        auth_url: String,
        timeout_secs: u32,
    ) -> Result<wit::desktop::OauthReply, String> {
        let cancel = match &self.tasks {
            super::tasks::TaskRole::Worker { cancel, .. } => cancel.clone(),
            _ => return Err("a sign-in waits for the browser: start it from a task".to_owned()),
        };
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
            self.open_url(url)?;
        }
        let deadline = std::time::Instant::now()
            + std::time::Duration::from_secs(timeout_secs.clamp(1, OAUTH_MAX_SECS).into());
        let query = await_redirect(&listener, deadline, Some(&cancel))?;
        Ok(wit::desktop::OauthReply {
            redirect_uri,
            query,
        })
    }

    fn open_url(&mut self, url: String) -> Result<(), String> {
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
        if sicompass_sdk::platform::open_with_default(&url) {
            Ok(())
        } else {
            Err("the desktop could not open that URL".to_owned())
        }
    }

    fn open_path(&mut self, path: String) -> Result<(), String> {
        let host = self.confine_granted(&path, true)?;
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

    /// Read from the system each time, so an application installed while
    /// sicompass runs is there. The id is the command the system launches it
    /// with, which `open-with` checks against this same list.
    fn applications(&mut self) -> Vec<wit::desktop::Application> {
        sicompass_sdk::platform::get_applications()
            .into_iter()
            .map(|a| wit::desktop::Application {
                name: a.name,
                id: a.exec,
            })
            .collect()
    }

    /// Only an id `applications` lists: a plugin can choose among the user's
    /// installed applications, never name a program of its own.
    fn open_with(&mut self, id: String, path: String) -> Result<(), String> {
        let host = self.confine_granted(&path, true)?;
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
        if sicompass_sdk::platform::open_with(&id, &host.to_string_lossy()) {
            Ok(())
        } else {
            Err("the application could not be started".to_owned())
        }
    }

    /// The entry itself, like `ls -l`: a symlink is described, not followed.
    fn stat(&mut self, path: String) -> Result<wit::desktop::FileInfo, String> {
        let host = self.confine_granted(&path, false)?;
        let meta = std::fs::symlink_metadata(&host).map_err(|e| format!("{path}: {e}"))?;
        Ok(file_info(&meta))
    }

    /// The item itself goes to the trash: for a symlink, the link, never what
    /// it points to. So its folder is confined (resolved), and the name is not.
    fn trash(&mut self, path: String) -> Result<(), String> {
        let host = self.confine_granted(&path, false)?;
        std::fs::symlink_metadata(&host).map_err(|e| format!("{path}: {e}"))?;
        trash_delete(&host)
    }

    fn restore(&mut self, path: String) -> Result<(), String> {
        let host = self.confine_granted(&path, false)?;
        trash_restore(&host)
    }

    /// The link's own folder is confined (resolved), the link itself is not:
    /// resolving it would answer with its target's target.
    fn read_link(&mut self, path: String) -> Result<String, String> {
        let host = self.confine_granted(&path, false)?;
        std::fs::read_link(&host)
            .map(|t| t.to_string_lossy().into_owned())
            .map_err(|e| format!("{path}: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worker_state(cancel: std::sync::Arc<AtomicBool>) -> HostState {
        let mut s = HostState::new("demo", "demo", "/tmp/sicompass-test-plugin", Vec::new());
        let (events, _rx) = std::sync::mpsc::channel();
        std::mem::forget(_rx);
        s.tasks = super::super::tasks::TaskRole::Worker {
            id: 1,
            cancel,
            events,
        };
        s
    }

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
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let waiter = std::thread::spawn(move || {
            let mut s = worker_state(cancel);
            wit::desktop::Host::oauth_redirect(
                &mut s,
                "https://auth.example.org/o?client_id=x&redirect_uri={redirect-uri}&z=signin1"
                    .to_owned(),
                30,
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
        let reply = waiter.join().unwrap().unwrap();
        assert_eq!(reply.redirect_uri, redirect);
        assert_eq!(reply.query, "code=4%2Fabc&state=s1");
    }

    #[test]
    fn a_sign_in_ends_when_its_task_is_cancelled() {
        _set_test_no_open(true);
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let waiter = std::thread::spawn(move || {
            let mut s = worker_state(flag);
            wit::desktop::Host::oauth_redirect(
                &mut s,
                "https://auth.example.org/o?r={redirect-uri}&z=signin2".to_owned(),
                600,
            )
        });
        opened_sign_in("z=signin2");
        cancel.store(true, Ordering::Release);
        let err = waiter.join().unwrap().unwrap_err();
        assert!(err.contains("cancelled"), "{err}");
    }

    #[test]
    fn a_sign_in_is_https_and_only_from_a_task() {
        let mut ui = HostState::new("demo", "demo", "/tmp/sicompass-test-plugin", Vec::new());
        let err = wit::desktop::Host::oauth_redirect(&mut ui, "https://a.example/".to_owned(), 5)
            .unwrap_err();
        assert!(err.contains("task"), "{err}");
        let mut s = worker_state(std::sync::Arc::new(AtomicBool::new(false)));
        let err = wit::desktop::Host::oauth_redirect(&mut s, "http://a.example/".to_owned(), 5)
            .unwrap_err();
        assert!(err.contains("https"), "{err}");
    }


    fn state(roots: Vec<(PathBuf, PathBuf)>) -> HostState {
        let mut s = HostState::new("demo", "demo", "/tmp/sicompass-test-plugin", Vec::new());
        s.granted_roots = roots;
        s
    }

    #[test]
    fn storage_maps_to_its_host_directory_and_nothing_escapes() {
        let host = tempfile::tempdir().unwrap();
        std::fs::write(host.path().join("a.txt"), "x").unwrap();
        let s = state(vec![(PathBuf::from("/storage"), host.path().to_path_buf())]);

        assert_eq!(
            s.confine_granted("/storage/a.txt", true).unwrap(),
            host.path().canonicalize().unwrap().join("a.txt")
        );
        assert!(s.confine_granted("/storage/../etc/passwd", true).is_err());
        assert!(s.confine_granted("/etc/passwd", true).is_err());
        assert!(s.confine_granted("storage/a.txt", true).is_err());
        // The root itself cannot be trashed or opened.
        assert!(s.confine_granted("/storage", true).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_out_of_a_granted_folder_is_refused() {
        let host = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink("/etc", host.path().join("escape")).unwrap();
        let s = state(vec![(PathBuf::from("/storage"), host.path().to_path_buf())]);
        assert!(s.confine_granted("/storage/escape/passwd", true).is_err());
    }

    /// `stat` describes the entry itself: a link's own mode, and names for
    /// its owner and group; nothing outside the grants.
    #[cfg(unix)]
    #[test]
    fn stat_describes_the_entry_like_ls() {
        use std::os::unix::fs::PermissionsExt;
        use wit::desktop::Host;
        let granted = tempfile::tempdir().unwrap();
        let root = granted.path().canonicalize().unwrap();
        std::fs::write(root.join("a.txt"), "hello").unwrap();
        std::fs::set_permissions(root.join("a.txt"), std::fs::Permissions::from_mode(0o640))
            .unwrap();
        std::os::unix::fs::symlink(root.join("a.txt"), root.join("link")).unwrap();
        let mut s = state(vec![(root.clone(), root.clone())]);

        let f = s.stat(root.join("a.txt").to_string_lossy().into_owned()).unwrap();
        assert_eq!(f.size, 5);
        assert_eq!(f.mode.unwrap() & 0o777, 0o640);
        assert_eq!(f.links, Some(1));
        let me = user_name(unsafe { libc::getuid() });
        assert_eq!(f.owner.as_deref(), Some(me.as_str()));
        assert!(f.group.is_some_and(|g| !g.is_empty()));
        assert!(f.modified > 0);

        let l = s.stat(root.join("link").to_string_lossy().into_owned()).unwrap();
        assert_eq!(l.mode.unwrap() & libc::S_IFMT, libc::S_IFLNK, "the link, not its target");
        assert!(s.stat("/etc/passwd".to_owned()).is_err(), "outside the grants");
    }

    /// Only an application the host listed can be asked for, and only for a
    /// file inside the grants.
    #[test]
    fn open_with_takes_only_a_listed_application() {
        use wit::desktop::Host;
        _set_test_no_open(true);
        let granted = tempfile::tempdir().unwrap();
        let root = granted.path().canonicalize().unwrap();
        std::fs::write(root.join("a.txt"), "x").unwrap();
        let mut s = state(vec![(root.clone(), root.clone())]);
        let file = root.join("a.txt").to_string_lossy().into_owned();

        assert!(s.open_with("rm -rf ~".to_owned(), file.clone()).is_err());
        if let Some(app) = s.applications().into_iter().next() {
            s.open_with(app.id.clone(), file).unwrap();
            assert!(s.open_with(app.id, "/etc/passwd".to_owned()).is_err());
        }
    }

    /// Trashing a symlink takes the link, never its target: deleting a link
    /// to a folder in the file browser must not throw the folder away.
    #[cfg(unix)]
    #[test]
    fn trash_takes_a_link_and_leaves_its_target() {
        use wit::desktop::Host;
        _set_test_no_trash(true);
        let granted = tempfile::tempdir().unwrap();
        let root = granted.path().canonicalize().unwrap();
        std::fs::create_dir(root.join("docs")).unwrap();
        std::fs::write(root.join("docs/a.txt"), "keep me").unwrap();
        std::os::unix::fs::symlink(root.join("docs"), root.join("link")).unwrap();
        let mut s = state(vec![(root.clone(), root.clone())]);

        s.trash(root.join("link").to_string_lossy().into_owned()).unwrap();
        assert!(std::fs::symlink_metadata(root.join("link")).is_err(), "the link is gone");
        assert_eq!(
            std::fs::read_to_string(root.join("docs/a.txt")).unwrap(),
            "keep me",
            "and what it pointed to is untouched"
        );
        assert!(s.trash(root.join("nothing").to_string_lossy().into_owned()).is_err());
    }

    /// A link inside a granted folder reads back as written, an absolute
    /// target included (which WASI itself refuses to read); a link outside,
    /// and a path that is no link, are refused.
    #[cfg(unix)]
    #[test]
    fn read_link_answers_only_inside_a_granted_folder() {
        use wit::desktop::Host;
        let granted = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let root = granted.path().canonicalize().unwrap();
        std::fs::create_dir(root.join("actual")).unwrap();
        std::os::unix::fs::symlink(root.join("actual"), root.join("via")).unwrap();
        std::os::unix::fs::symlink("/etc", outside.path().join("elsewhere")).unwrap();
        let mut s = state(vec![(root.clone(), root.clone())]);

        let via = root.join("via").to_string_lossy().into_owned();
        assert_eq!(s.read_link(via).unwrap(), root.join("actual").to_string_lossy());
        let plain = root.join("actual").to_string_lossy().into_owned();
        assert!(s.read_link(plain).is_err(), "not a link");
        let far = outside.path().join("elsewhere").to_string_lossy().into_owned();
        assert!(s.read_link(far).is_err(), "outside the grants");
    }

    #[test]
    fn nothing_is_reachable_without_a_grant() {
        let s = state(Vec::new());
        assert!(s.confine_granted("/tmp", true).is_err());
    }

    #[test]
    fn only_web_and_mail_urls_open() {
        let mut s = state(Vec::new());
        _take_recorded();
        assert!(wit::desktop::Host::open_url(&mut s, "https://example.com".into()).is_ok());
        assert!(wit::desktop::Host::open_url(&mut s, "file:///etc/passwd".into()).is_err());
        assert!(wit::desktop::Host::open_url(&mut s, "javascript:alert(1)".into()).is_err());
        assert!(_take_recorded().contains(&"open-url:https://example.com".to_owned()));
    }
}
