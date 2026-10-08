//! Moving an uninstalled plugin's data folder to the OS trash, which the user
//! can empty or restore from. The Store does it itself, so it works the same
//! in the app and in the desicompass superkey.
//!
//! # Tests never touch the real trash
//!
//! `TEST_NO_TRASH` ([`_set_test_no_trash`]) "trashes" into a private temp
//! directory instead, on by default in this crate's unit tests, and switched on
//! by every test harness that can reach a Store (the app's `ensure_builtins()`
//! and the superkey's tests), following the pattern the app's
//! `tests/hygiene.rs` enforces for every crate that can trash.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

static TEST_NO_TRASH: AtomicBool = AtomicBool::new(cfg!(test));

/// Test hook: trash into a private temp directory instead of the OS trash.
pub fn _set_test_no_trash(on: bool) {
    TEST_NO_TRASH.store(on, Ordering::Release);
}

fn no_trash() -> bool {
    TEST_NO_TRASH.load(Ordering::Acquire)
}

/// Move `path` to the trash: the OS trash, or under `TEST_NO_TRASH` a private
/// temp directory.
pub(crate) fn trash_delete(path: &Path) -> Result<(), String> {
    if !no_trash() {
        return os_trash_delete(path);
    }
    static MOVED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = std::env::temp_dir()
        .join("sicompass-store-test-trash")
        .join(format!(
            "{}-{}",
            std::process::id(),
            MOVED.fetch_add(1, Ordering::Relaxed)
        ));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    std::fs::rename(path, dir.join(path.file_name().unwrap_or_default())).map_err(|e| e.to_string())
}

/// The one place this crate reaches the `trash` crate (the app's
/// tests/hygiene.rs), in two variants: on macOS the crate's default goes
/// through Finder over AppleScript (slow, needs Automation permission and a
/// running Finder), so `NsFileManager` is used instead.
#[cfg(target_os = "macos")]
fn os_trash_delete(path: &Path) -> Result<(), String> {
    use trash::macos::{DeleteMethod, TrashContextExtMacos};
    let mut ctx = trash::TrashContext::default();
    ctx.set_delete_method(DeleteMethod::NsFileManager);
    ctx.delete(path).map_err(trash_error)
}

/// See the macOS variant above; everywhere else the crate default is fine.
#[cfg(not(target_os = "macos"))]
fn os_trash_delete(path: &Path) -> Result<(), String> {
    trash::delete(path).map_err(trash_error)
}

/// The trash's refusal as the system put it ("Permission denied (os error
/// 13)"). The crate's own `Display` is its `Debug` dump, which would be read
/// out as is.
fn trash_error(e: trash::Error) -> String {
    match e {
        #[cfg(all(
            unix,
            not(target_os = "macos"),
            not(target_os = "ios"),
            not(target_os = "android")
        ))]
        trash::Error::FileSystem { source, .. } => source.to_string(),
        trash::Error::Os { code, description } => format!("{description} (os error {code})"),
        trash::Error::Unknown { description } => description,
        other => other.to_string(),
    }
}
