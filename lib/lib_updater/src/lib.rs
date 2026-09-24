//! sicompass-updater — startup self-update for the app.
//!
//! The checker runs on a background thread spawned from `main.rs` so startup
//! never blocks on the network. It hits the GitHub Releases API for the app,
//! downloads what is available, and writes results into an
//! `Arc<Mutex<UpdateStatus>>` that the renderer reads each frame.
//!
//! On Windows the signed MSI is downloaded, then on user consent `msiexec /i
//! ... /passive` is spawned and the app exits so the installer can replace
//! files (preserves WiX upgrade-guid + Programs & Features tracking).
//!
//! Plugins are updated by the Store (`lib_store`), in the signed release
//! format, including plugins installed by hand with an `updateUrl`.

use std::path::PathBuf;

pub mod github;

#[cfg(target_os = "windows")]
mod apply_windows;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Snapshot of what the background checker found. The renderer reads this
/// each frame and surfaces a message in the header when something is pending.
#[derive(Debug, Default, Clone)]
pub struct UpdateStatus {
    /// Newer app version found + a staged installer path ready to apply.
    pub app_update: Option<AppUpdate>,
    /// Non-fatal errors encountered during the check. Surfaced in logs only,
    /// not the UI, so a flaky network never disrupts startup.
    pub errors: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AppUpdate {
    pub new_version: semver::Version,
    /// Local path to the downloaded MSI (Windows) or release archive.
    /// On platforms without an apply path, this is `None` and `release_url`
    /// is shown as a fallback.
    pub staged_installer_path: Option<PathBuf>,
    /// Browser-openable URL to the release page — used on non-Windows.
    pub release_url: String,
}

// ---------------------------------------------------------------------------
// Checker
// ---------------------------------------------------------------------------

/// What the checker looks for. Construct once at startup, then call
/// [`UpdateChecker::check_and_stage`] from a background thread.
pub struct UpdateChecker {
    pub current_app_version: semver::Version,
    pub github_owner: String,
    pub github_repo: String,
}

impl UpdateChecker {
    pub fn new(
        current_app_version: semver::Version,
        github_owner: impl Into<String>,
        github_repo: impl Into<String>,
    ) -> Self {
        Self {
            current_app_version,
            github_owner: github_owner.into(),
            github_repo: github_repo.into(),
        }
    }

    /// Run the full check. Designed to be called from a background thread;
    /// never panics, swallows all I/O errors into `UpdateStatus.errors`.
    pub fn check_and_stage(&self) -> UpdateStatus {
        let mut status = UpdateStatus::default();

        // ---- App update --------------------------------------------------
        match github::check_app_update(
            &self.github_owner,
            &self.github_repo,
            &self.current_app_version,
        ) {
            Ok(Some(app_update)) => {
                tracing::info!(
                    "app update available: {} → {}",
                    self.current_app_version,
                    app_update.new_version
                );
                status.app_update = Some(app_update);
            }
            Ok(None) => {
                tracing::debug!("app is up to date at {}", self.current_app_version);
            }
            Err(e) => {
                tracing::warn!("app update check failed: {e}");
                status.errors.push(format!("app update check: {e}"));
            }
        }

        status
    }

    /// Apply a staged app update. On Windows this spawns `msiexec` and
    /// exits the process; on other platforms it returns an error and the
    /// caller should open `release_url` in the browser.
    ///
    /// The non-Windows path is a deliberate end state, not a gap. Since 0.1.9
    /// there are macOS and Linux packages, but self-applying them is the wrong
    /// thing to do: a `.deb` or `.rpm` belongs to the system package manager,
    /// which owns the dependency graph and the user's trust decisions, and an
    /// AppImage is a file the user placed somewhere themselves. Replacing a
    /// mounted `.app` from inside the process running out of it is a separate
    /// project involving a helper binary and re-signing.
    ///
    /// Opening the release page is the correct behaviour on those platforms,
    /// and it now leads somewhere useful: before 0.1.9 that page had only a
    /// Windows build on it.
    #[allow(unused_variables)] // path arg is unused on non-Windows
    pub fn apply_app_update(&self, update: &AppUpdate) -> std::io::Result<()> {
        #[cfg(target_os = "windows")]
        {
            let Some(ref path) = update.staged_installer_path else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "no staged installer",
                ));
            };
            apply_windows::run_msi(path)?;
            std::process::exit(0);
        }
        #[cfg(not(target_os = "windows"))]
        {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "in-app apply unsupported; open release_url in browser",
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Parse a version string, tolerating a leading `v` (e.g. `"v0.1.1"`).
pub fn parse_version(s: &str) -> Result<semver::Version, semver::Error> {
    let s = s.strip_prefix('v').unwrap_or(s);
    semver::Version::parse(s)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_accepts_leading_v() {
        let v = parse_version("v1.2.3").unwrap();
        assert_eq!(v, semver::Version::new(1, 2, 3));
    }

    #[test]
    fn parse_version_accepts_no_prefix() {
        let v = parse_version("0.4.0").unwrap();
        assert_eq!(v, semver::Version::new(0, 4, 0));
    }

    #[test]
    fn parse_version_rejects_garbage() {
        assert!(parse_version("not-a-version").is_err());
    }
}
