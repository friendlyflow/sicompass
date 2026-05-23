//! Per-plugin update flow: read installed manifest, GET updateUrl, compare
//! semver, download to staging, verify, test-load, atomic swap, emit
//! HotReload event.
//!
//! The flow is designed so the installed plugin stays untouched until the
//! new entry has been (a) downloaded, (b) SHA-256 verified, and
//! (c) test-loaded on Linux/macOS (we cannot dlopen on Windows without
//! also being on Windows, so test-load there means file-exists + sha
//! verify only — a real load happens at hot-reload time on the main
//! thread). A failure at any earlier stage leaves the installed directory
//! intact.

use crate::{
    github::download_to, parse_version, signature::verify_entry, staging_path,
    PluginUpdate, PluginUpdateManifest, UpdateEvent,
};
use serde::Deserialize;
use std::path::Path;
use std::sync::mpsc;

const HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
const USER_AGENT: &str = concat!("sicompass-updater/", env!("CARGO_PKG_VERSION"));
const MAX_MANIFEST_BYTES: u64 = 256 * 1024;

/// Subset of `plugin.json` we need to read on disk for update decisions.
/// We deliberately don't import the app's `PluginManifest` here — that
/// would force a dependency on `sicompass` (the app crate), which is
/// out-of-bounds per the SDK boundary rule.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InstalledManifest {
    name: String,
    entry: String,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    update_url: Option<String>,
    /// Embedded pubkey from when this plugin was installed — the trust
    /// root for verifying ed25519 signatures on future updates.
    #[serde(default)]
    pubkey: Option<String>,
    /// Whether this plugin opts out of hot-reload. Default true.
    #[serde(default = "default_true")]
    hot_reload: bool,
}

fn default_true() -> bool {
    true
}

/// Walk every plugin directory in `plugins_dir`, fetch its updateUrl, and
/// stage what's newer. Failures are pushed into `errors` (logged later)
/// and never abort the loop — one busted plugin must not prevent others
/// from updating.
pub fn check_all_plugin_updates(
    plugins_dir: &Path,
    current_app_version: &semver::Version,
    event_tx: Option<&mpsc::Sender<UpdateEvent>>,
    errors: &mut Vec<String>,
) -> Vec<PluginUpdate> {
    let Ok(entries) = std::fs::read_dir(plugins_dir) else {
        return Vec::new();
    };

    let mut results = Vec::new();
    for entry in entries.flatten() {
        let plugin_dir = entry.path();
        if !plugin_dir.is_dir() {
            continue;
        }
        // Skip our own staging dirs.
        if plugin_dir
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.ends_with(".staging"))
            .unwrap_or(false)
        {
            continue;
        }

        match check_one(&plugin_dir, current_app_version, event_tx) {
            Ok(Some(update)) => results.push(update),
            Ok(None) => {}
            Err(e) => {
                tracing::warn!("plugin {}: {e}", plugin_dir.display());
                errors.push(format!("plugin {}: {e}", plugin_dir.display()));
            }
        }
    }
    results
}

fn check_one(
    plugin_dir: &Path,
    current_app_version: &semver::Version,
    event_tx: Option<&mpsc::Sender<UpdateEvent>>,
) -> Result<Option<PluginUpdate>, String> {
    let manifest_path = plugin_dir.join("plugin.json");
    let data = std::fs::read_to_string(&manifest_path)
        .map_err(|e| format!("read manifest: {e}"))?;
    let installed: InstalledManifest =
        serde_json::from_str(&data).map_err(|e| format!("parse manifest: {e}"))?;

    let Some(update_url) = installed.update_url.clone() else {
        return Ok(None); // plugin opted out of auto-update
    };

    let installed_version = match installed.version.as_deref() {
        Some(v) => parse_version(v).map_err(|e| format!("parse installed version: {e}"))?,
        None => semver::Version::new(0, 0, 0),
    };

    let client = reqwest::blocking::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| format!("build client: {e}"))?;

    let new_manifest = fetch_manifest(&client, &update_url)?;
    let new_version = parse_version(&new_manifest.version)
        .map_err(|e| format!("parse new version: {e}"))?;

    if new_version <= installed_version {
        return Ok(None);
    }

    // Compatibility gate: skip if the new plugin demands a newer app
    // than we are.
    if let Some(min) = new_manifest.min_app_version.as_deref() {
        let min = parse_version(min).map_err(|e| format!("parse minAppVersion: {e}"))?;
        if &min > current_app_version {
            return Err(format!(
                "skipped: requires app >= {min} but running {current_app_version}"
            ));
        }
    }

    // Stage: download entry into a fresh `<name>.staging/` dir alongside
    // the live plugin dir. We always reset the staging directory so a
    // crashed previous run can't poison this one.
    let plugins_root = plugin_dir.parent().ok_or("plugin_dir has no parent")?;
    let staging = staging_path(plugins_root, &installed.name);
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| format!("mkdir staging: {e}"))?;

    // Filename in staging mirrors the installed entry path (e.g. plugin.so).
    let entry_filename = Path::new(&installed.entry)
        .file_name()
        .ok_or("installed entry has no filename")?;
    let staged_entry = staging.join(entry_filename);

    download_to(&client, &new_manifest.entry_url, &staged_entry)
        .map_err(|e| format!("download entry: {e}"))?;

    // Verify SHA-256 (required) + optional ed25519 against the trust root
    // embedded in the installed manifest.
    let sig = new_manifest
        .signature
        .as_ref()
        .ok_or("served manifest is missing 'signature.sha256'")?;
    verify_entry(
        &staged_entry,
        &sig.sha256,
        installed.pubkey.as_deref(),
        sig.sig.as_deref(),
    )?;

    // Write the new plugin.json into staging — same fields as installed
    // plus the bumped version/url. We carry through the pubkey from the
    // installed manifest (the trust root) so the next update verifies
    // against the same key; rotation is a follow-up.
    let new_disk_manifest = serde_json::json!({
        "name": installed.name,
        "displayName": serde_json::Value::String(installed.name.clone()), // not stored in updater's view
        "type": "script", // placeholder; the running plugin.json file is what defines this
        "entry": entry_filename.to_string_lossy(),
        "version": new_manifest.version,
        "updateUrl": update_url,
        "minAppVersion": new_manifest.min_app_version,
        "pubkey": installed.pubkey,
        "hotReload": installed.hot_reload,
    });
    // One-shot write: `fs::write` opens, writes, and closes in a single call.
    // The previous `File::create + write_all` pattern kept the handle alive
    // until end-of-scope, which on Windows blocks the directory rename at the
    // swap step below (Windows refuses to rename a directory while any file
    // inside it has an open handle; Linux/macOS allow it, which is why this
    // only surfaced once the lib_updater tests started running on Windows).
    let serialized = serde_json::to_string_pretty(&new_disk_manifest)
        .map_err(|e| format!("serialize staging manifest: {e}"))?;
    std::fs::write(staging.join("plugin.json"), serialized)
        .map_err(|e| format!("write staging manifest: {e}"))?;

    // We deliberately don't copy the existing plugin.json verbatim because
    // we need to bump `version` and `entry` (the staged entry filename may
    // differ in extension). The simpler alternative — load + re-serialize
    // the installed plugin.json with mutations — is left for later when
    // we share the manifest type with the app.

    // Test-load the new entry. On Linux/macOS this is a real dlopen via
    // libloading; on Windows it's a file-exists check (we'd need to
    // load the DLL from the main thread anyway). Test-load failure
    // leaves the installed plugin completely untouched.
    if let Err(e) = test_load(&staged_entry) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(format!("test-load failed: {e}"));
    }

    // Atomic swap: move <name>/ → <name>.old/, <name>.staging/ → <name>/,
    // delete <name>.old/. On any failure mid-rename we try to revert.
    let live = plugins_root.join(&installed.name);
    let backup = plugins_root.join(format!("{}.old", installed.name));
    let _ = std::fs::remove_dir_all(&backup);

    std::fs::rename(&live, &backup).map_err(|e| format!("backup live dir: {e}"))?;
    if let Err(e) = std::fs::rename(&staging, &live) {
        // Revert: put the live dir back.
        let _ = std::fs::rename(&backup, &live);
        return Err(format!("swap staging in: {e}"));
    }
    let _ = std::fs::remove_dir_all(&backup);

    // Tell the main thread to hot-reload, but only if the plugin allows it.
    let applied = installed.hot_reload && {
        if let Some(tx) = event_tx {
            let new_entry = live.join(entry_filename);
            tx.send(UpdateEvent::HotReload {
                plugin_name: installed.name.clone(),
                new_entry_path: new_entry,
            })
            .is_ok()
        } else {
            false
        }
    };

    Ok(Some(PluginUpdate {
        plugin_name: installed.name,
        new_version,
        applied,
    }))
}

fn fetch_manifest(
    client: &reqwest::blocking::Client,
    url: &str,
) -> Result<PluginUpdateManifest, String> {
    use std::io::Read;
    let resp = client
        .get(url)
        .header("Accept", "application/json")
        .send()
        .map_err(|e| format!("GET {url}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {} from {url}", resp.status()));
    }
    let mut buf = Vec::new();
    resp.take(MAX_MANIFEST_BYTES)
        .read_to_end(&mut buf)
        .map_err(|e| format!("read manifest: {e}"))?;
    serde_json::from_slice(&buf).map_err(|e| format!("parse manifest JSON: {e}"))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn test_load(entry: &Path) -> Result<(), String> {
    // Native plugins end in .so / .dylib. For everything else we just
    // check existence — Script plugins are run as a subprocess later.
    let ext = entry.extension().and_then(|s| s.to_str()).unwrap_or("");
    if ext == "so" || ext == "dylib" {
        // SAFETY: we immediately drop the library; we are not calling
        // anything from it.
        unsafe {
            let lib = libloading::Library::new(entry)
                .map_err(|e| format!("dlopen: {e}"))?;
            drop(lib);
        }
    } else if !entry.exists() {
        return Err("staged entry file missing".to_string());
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn test_load(entry: &Path) -> Result<(), String> {
    if !entry.exists() {
        return Err("staged entry file missing".to_string());
    }
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn test_load(entry: &Path) -> Result<(), String> {
    if !entry.exists() {
        return Err("staged entry file missing".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use ed25519_dalek::{Signer, SigningKey};
    use std::path::PathBuf;
    use std::sync::mpsc;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const B64: base64::engine::general_purpose::GeneralPurpose =
        base64::engine::general_purpose::STANDARD;

    fn make_installed_plugin(root: &Path, name: &str, version: &str, update_url: &str) -> PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("plugin.json"),
            serde_json::json!({
                "name": name,
                "displayName": name,
                "type": "script",
                "entry": "p.ts",
                "version": version,
                "updateUrl": update_url,
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(dir.join("p.ts"), b"// old").unwrap();
        dir
    }

    #[test]
    fn skips_plugin_without_update_url() {
        let plugins = tempfile::tempdir().unwrap();
        let dir = plugins.path().join("foo");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("plugin.json"),
            r#"{"name":"foo","displayName":"foo","type":"script","entry":"p.ts","version":"0.1.0"}"#,
        )
        .unwrap();

        let mut errors = Vec::new();
        let results = check_all_plugin_updates(
            plugins.path(),
            &semver::Version::new(0, 1, 0),
            None,
            &mut errors,
        );
        assert!(results.is_empty());
        assert!(errors.is_empty());
    }

    // The blocking reqwest client spawns its own internal tokio runtime;
    // calling it from within `rt.block_on(async { ... })` would nest a
    // runtime drop inside another runtime, which panics. Using
    // `#[tokio::test(flavor = "multi_thread")]` + `block_in_place` keeps
    // the wiremock server alive on async tasks while the blocking client
    // runs on a dedicated blocking worker thread.

    #[tokio::test(flavor = "multi_thread")]
    async fn skips_when_remote_version_not_newer() {
        let plugins = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/manifest"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"name":"foo","version":"1.0.0","entryUrl":"http://nope/","signature":{"sha256":"00"}}"#,
            ))
            .mount(&server)
            .await;
        make_installed_plugin(
            plugins.path(),
            "foo",
            "1.0.0",
            &format!("{}/manifest", server.uri()),
        );

        let results = tokio::task::block_in_place(|| {
            let mut errors = Vec::new();
            let r = check_all_plugin_updates(
                plugins.path(),
                &semver::Version::new(0, 1, 0),
                None,
                &mut errors,
            );
            (r, errors)
        });
        assert!(results.0.is_empty(), "should skip when same version");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn updates_when_newer_version_with_valid_sig() {
        let plugins = tempfile::tempdir().unwrap();

        let sk = SigningKey::from_bytes(&[42u8; 32]);
        let pk = sk.verifying_key();
        let pk_b64 = B64.encode(pk.to_bytes());

        let new_body: &[u8] = b"// new plugin entry";
        let sig_b64 = B64.encode(sk.sign(new_body).to_bytes());
        let sha = {
            use sha2::Digest;
            let mut h = sha2::Sha256::new();
            h.update(new_body);
            hex(&h.finalize())
        };

        let server = MockServer::start().await;
        let entry_url = format!("{}/entry.ts", server.uri());
        let manifest_url = format!("{}/manifest", server.uri());

        let dir = plugins.path().join("foo");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("plugin.json"),
            serde_json::json!({
                "name": "foo",
                "displayName": "foo",
                "type": "script",
                "entry": "p.ts",
                "version": "1.0.0",
                "updateUrl": manifest_url,
                "pubkey": pk_b64,
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(dir.join("p.ts"), b"// old").unwrap();

        Mock::given(method("GET"))
            .and(path("/manifest"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                serde_json::json!({
                    "name": "foo",
                    "version": "1.0.1",
                    "entryUrl": entry_url,
                    "signature": { "sha256": sha, "sig": sig_b64 }
                })
                .to_string(),
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/entry.ts"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(new_body))
            .mount(&server)
            .await;

        let (tx, rx) = mpsc::channel();
        let (results, errors) = tokio::task::block_in_place(|| {
            let mut errors = Vec::new();
            let r = check_all_plugin_updates(
                plugins.path(),
                &semver::Version::new(0, 1, 0),
                Some(&tx),
                &mut errors,
            );
            (r, errors)
        });
        assert!(errors.is_empty(), "expected no errors, got: {:?}", errors);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].plugin_name, "foo");
        assert_eq!(results[0].new_version, semver::Version::new(1, 0, 1));
        assert!(results[0].applied);

        let live = std::fs::read(plugins.path().join("foo/p.ts")).unwrap();
        assert_eq!(live, new_body);

        match rx.try_recv().expect("hot reload event") {
            UpdateEvent::HotReload { plugin_name, .. } => {
                assert_eq!(plugin_name, "foo");
            }
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rejects_when_sha_mismatches() {
        let plugins = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        let entry_url = format!("{}/entry.ts", server.uri());
        let manifest_url = format!("{}/manifest", server.uri());

        make_installed_plugin(plugins.path(), "foo", "1.0.0", &manifest_url);

        Mock::given(method("GET"))
            .and(path("/manifest"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                serde_json::json!({
                    "name": "foo",
                    "version": "1.0.1",
                    "entryUrl": entry_url,
                    "signature": { "sha256": "00".repeat(32) }
                })
                .to_string(),
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/entry.ts"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"any bytes"))
            .mount(&server)
            .await;

        let (results, errors) = tokio::task::block_in_place(|| {
            let mut errors = Vec::new();
            let r = check_all_plugin_updates(
                plugins.path(),
                &semver::Version::new(0, 1, 0),
                None,
                &mut errors,
            );
            (r, errors)
        });
        assert!(results.is_empty());
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("sha256 mismatch"));

        let live = std::fs::read(plugins.path().join("foo/p.ts")).unwrap();
        assert_eq!(live, b"// old");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn rejects_when_min_app_version_exceeds_current() {
        let plugins = tempfile::tempdir().unwrap();
        let server = MockServer::start().await;
        let manifest_url = format!("{}/manifest", server.uri());
        make_installed_plugin(plugins.path(), "foo", "1.0.0", &manifest_url);

        Mock::given(method("GET"))
            .and(path("/manifest"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                serde_json::json!({
                    "name": "foo",
                    "version": "2.0.0",
                    "entryUrl": "http://unused/",
                    "minAppVersion": "99.0.0",
                    "signature": { "sha256": "00" }
                })
                .to_string(),
            ))
            .mount(&server)
            .await;

        let (results, errors) = tokio::task::block_in_place(|| {
            let mut errors = Vec::new();
            let r = check_all_plugin_updates(
                plugins.path(),
                &semver::Version::new(0, 1, 0),
                None,
                &mut errors,
            );
            (r, errors)
        });
        assert!(results.is_empty());
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("requires app"));
    }

    fn hex(b: &[u8]) -> String {
        let mut s = String::with_capacity(b.len() * 2);
        for x in b {
            s.push_str(&format!("{:02x}", x));
        }
        s
    }
}
