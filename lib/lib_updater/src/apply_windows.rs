//! Windows apply path — spawn `msiexec` on the staged MSI and let the
//! installer take over. The running sicompass process exits immediately
//! after spawn so MSI can replace its `.exe` cleanly.
//!
//! Why MSI rather than swapping the binary: cargo-dist already produces a
//! code-signed MSI with a WiX upgrade-guid, and re-running it preserves
//! Programs & Features tracking, the signature chain, and SmartScreen
//! reputation. An in-place `.exe` swap would diverge from the installer
//! database and leave P&F showing the old version.

use std::path::Path;

#[cfg(target_os = "windows")]
pub fn run_msi(installer: &Path) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    // From winbase.h. We avoid pulling the `windows` crate's constants
    // just for one numeric value.
    const DETACHED_PROCESS: u32 = 0x00000008;

    let installer = installer
        .to_str()
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "installer path is not UTF-8")
        })?;

    // /passive shows a progress dialog without prompting; /norestart keeps
    // us from triggering a reboot.
    std::process::Command::new("msiexec")
        .args(["/i", installer, "/passive", "/norestart"])
        .creation_flags(DETACHED_PROCESS)
        .spawn()?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
pub fn run_msi(_installer: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "MSI install only supported on Windows",
    ))
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_utf8_path() {
        // On Windows OsStr can hold non-UTF-8; build a path that is.
        use std::ffi::OsString;
        use std::os::windows::ffi::OsStringExt;
        let raw: [u16; 2] = [0xD800, 0x0041]; // unpaired surrogate
        let s = OsString::from_wide(&raw);
        let p = std::path::PathBuf::from(s);
        assert!(run_msi(&p).is_err());
    }
}

#[cfg(all(test, not(target_os = "windows")))]
mod tests {
    use super::*;

    #[test]
    fn non_windows_returns_unsupported() {
        let r = run_msi(Path::new("/tmp/x.msi"));
        assert_eq!(r.unwrap_err().kind(), std::io::ErrorKind::Unsupported);
    }
}
