//! sicompass-updater — startup self-update for the app + user plugins.
//!
//! The checker runs on a background thread spawned from `main.rs` so startup
//! never blocks on the network. It hits the GitHub Releases API for the app
//! and each plugin's `updateUrl` for plugins, downloads what is available,
//! verifies SHA-256, and writes results into an `Arc<Mutex<UpdateStatus>>`
//! that the renderer reads each frame.
//!
//! Apply paths:
//! - **App** on Windows: download the signed MSI, then on user consent
//!   spawn `msiexec /i ... /passive` and exit so the installer can replace
//!   files (preserves WiX upgrade-guid + Programs & Features tracking).
//! - **Plugin**: test-load the new entry, atomically swap
//!   `<plugins_dir>/<name>/`, then send a `HotReload` event back to the main
//!   thread so the running provider can be swapped without restart.

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

pub mod github;
pub mod plugin;
pub mod signature;

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
    /// Plugins that were updated (already swapped on disk + hot-reload sent).
    /// `applied=true` means the swap succeeded and a `HotReload` event was
    /// emitted; `applied=false` means a newer version exists but the download
    /// or test-load failed.
    pub plugin_updates: Vec<PluginUpdate>,
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

#[derive(Debug, Clone)]
pub struct PluginUpdate {
    pub plugin_name: String,
    pub new_version: semver::Version,
    pub applied: bool,
}

/// Events the updater sends to the main thread. Only `HotReload` is
/// consumed today; `AppUpdateReady` is reserved for a future toast.
#[derive(Debug, Clone)]
pub enum UpdateEvent {
    /// A plugin's directory has been atomically swapped; the running
    /// provider must be torn down and re-instantiated from the new files.
    HotReload { plugin_name: String, new_entry_path: PathBuf },
}

// ---------------------------------------------------------------------------
// Manifest types served by a plugin's updateUrl
// ---------------------------------------------------------------------------

/// JSON document returned by a plugin's `updateUrl`.
///
/// Mirrors the on-disk `plugin.json` schema for fields that matter to the
/// updater, plus an `entryUrl` pointing at the new entry file.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginUpdateManifest {
    pub name: String,
    pub version: String,
    pub entry_url: String,
    #[serde(default)]
    pub min_app_version: Option<String>,
    #[serde(default)]
    pub signature: Option<PluginSignature>,
}

/// Optional per-plugin signature block in the served manifest.
///
/// `sha256` is required for any update served over HTTP. `pubkey` + `sig`
/// are optional and, when present, are verified against the embedded
/// ed25519 public key in the currently-installed plugin manifest (trust-on-
/// first-use; the very first install has nothing to compare against).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSignature {
    /// Hex-encoded SHA-256 of the entry file as served by `entryUrl`.
    pub sha256: String,
    /// Base64-encoded ed25519 public key. Optional in v1.
    #[serde(default)]
    pub pubkey: Option<String>,
    /// Base64-encoded ed25519 signature of the entry-file bytes. Optional.
    #[serde(default)]
    pub sig: Option<String>,
}

// ---------------------------------------------------------------------------
// Checker
// ---------------------------------------------------------------------------

/// What the checker scans for + downloads. Construct once at startup,
/// hand it a clone of the `UpdateEvent` sender, then call
/// [`UpdateChecker::check_and_stage`] from a background thread.
pub struct UpdateChecker {
    pub current_app_version: semver::Version,
    pub plugins_dir: PathBuf,
    pub github_owner: String,
    pub github_repo: String,
    pub event_tx: Option<mpsc::Sender<UpdateEvent>>,
}

impl UpdateChecker {
    /// Build a checker with sensible defaults. The caller wires in
    /// `event_tx` to receive hot-reload events.
    pub fn new(
        current_app_version: semver::Version,
        plugins_dir: PathBuf,
        github_owner: impl Into<String>,
        github_repo: impl Into<String>,
    ) -> Self {
        Self {
            current_app_version,
            plugins_dir,
            github_owner: github_owner.into(),
            github_repo: github_repo.into(),
            event_tx: None,
        }
    }

    pub fn with_event_sender(mut self, tx: mpsc::Sender<UpdateEvent>) -> Self {
        self.event_tx = Some(tx);
        self
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

        // ---- Plugin updates ---------------------------------------------
        if self.plugins_dir.as_os_str().is_empty() {
            return status;
        }
        let plugin_updates = plugin::check_all_plugin_updates(
            &self.plugins_dir,
            &self.current_app_version,
            self.event_tx.as_ref(),
            &mut status.errors,
        );
        status.plugin_updates = plugin_updates;

        status
    }

    /// Apply a staged app update. On Windows this spawns `msiexec` and
    /// exits the process; on other platforms it returns an error and the
    /// caller should open `release_url` in the browser.
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

/// `~/.config/sicompass/plugins/<name>.staging/` style helper.
pub fn staging_path(plugins_dir: &Path, name: &str) -> PathBuf {
    plugins_dir.join(format!("{name}.staging"))
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

    #[test]
    fn staging_path_appends_suffix() {
        let p = staging_path(Path::new("/tmp/plugins"), "foo");
        assert_eq!(p, PathBuf::from("/tmp/plugins/foo.staging"));
    }

    #[test]
    fn plugin_update_manifest_parses() {
        let json = r#"{
            "name": "demo",
            "version": "1.2.3",
            "entryUrl": "https://example.com/demo.ts",
            "minAppVersion": "0.1.0",
            "signature": {
                "sha256": "abc123",
                "pubkey": "Pn8="
            }
        }"#;
        let m: PluginUpdateManifest = serde_json::from_str(json).unwrap();
        assert_eq!(m.name, "demo");
        assert_eq!(m.version, "1.2.3");
        assert_eq!(m.min_app_version.as_deref(), Some("0.1.0"));
        let sig = m.signature.unwrap();
        assert_eq!(sig.sha256, "abc123");
        assert_eq!(sig.pubkey.as_deref(), Some("Pn8="));
        assert!(sig.sig.is_none());
    }

    #[test]
    fn plugin_update_manifest_minimal() {
        let json = r#"{"name":"x","version":"0.1.0","entryUrl":"https://e.com/x.so"}"#;
        let m: PluginUpdateManifest = serde_json::from_str(json).unwrap();
        assert!(m.min_app_version.is_none());
        assert!(m.signature.is_none());
    }
}
