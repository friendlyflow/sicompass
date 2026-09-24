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
    RECORDED.lock().map(|mut r| std::mem::take(&mut *r)).unwrap_or_default()
}

fn no_open() -> bool {
    TEST_NO_OPEN.load(Ordering::Acquire)
}

fn no_trash() -> bool {
    TEST_NO_TRASH.load(Ordering::Acquire)
}

/// Move `path` to the trash: the OS trash, or under `TEST_NO_TRASH` a private
/// temp directory that [`trash_restore`] can undo.
fn trash_delete(path: &Path) -> Result<(), String> {
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

/// The one place this crate reaches the `trash` crate (tests/hygiene.rs), the
/// same two variants as lib_filebrowser's: on macOS the crate's default goes
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

impl HostState {
    /// The host path for a guest path, if it lies inside a granted directory.
    ///
    /// `must_exist` is false for `restore`, whose target is gone until restored:
    /// then the parent directory is what gets resolved and checked.
    pub fn confine_granted(&self, guest: &str, must_exist: bool) -> Result<PathBuf, String> {
        let refuse = || format!("`{guest}` is outside the folders this plugin was granted");
        let lexical = self.guest_to_host(guest).ok_or_else(refuse)?;

        let resolved = if must_exist {
            lexical.canonicalize().map_err(|e| format!("{guest}: {e}"))?
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
        self.granted_roots.iter().find_map(|(guest_root, host_root)| {
            p.strip_prefix(guest_root).ok().map(|rest| host_root.join(rest))
        })
    }
}

impl wit::desktop::Host for HostState {
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

    fn trash(&mut self, path: String) -> Result<(), String> {
        let host = self.confine_granted(&path, true)?;
        trash_delete(&host)
    }

    fn restore(&mut self, path: String) -> Result<(), String> {
        let host = self.confine_granted(&path, false)?;
        trash_restore(&host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
