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

    // Write the new plugin.json into staging by **mutating the installed one**,
    // rather than rebuilding it from this crate's partial view.
    //
    // Rebuilding was silently destructive: the updater deserializes only the fields
    // it needs, so every other field was dropped on update. `displayName` was
    // replaced by `name`, and `allowedHosts` and `settings` disappeared entirely —
    // meaning a plugin that used the network lost the capability the moment it
    // updated, and its settings section changed name underneath it. `type` was
    // even written as a hardcoded placeholder.
    //
    // Editing the original keeps everything the updater has no opinion about, and
    // only touches what an update actually changes. The pubkey is deliberately left
    // as-is: it is the trust root, so the next update verifies against the same key
    // (rotation is a follow-up).
    let mut new_disk_manifest: serde_json::Value = serde_json::from_str(&data)
        .map_err(|e| format!("re-parse installed manifest: {e}"))?;
    {
        let obj = new_disk_manifest
            .as_object_mut()
            .ok_or("installed plugin.json is not a JSON object")?;
        obj.insert(
            "entry".to_owned(),
            serde_json::Value::String(entry_filename.to_string_lossy().into_owned()),
        );
        obj.insert(
            "version".to_owned(),
            serde_json::Value::String(new_manifest.version.clone()),
        );
        obj.insert("updateUrl".to_owned(), serde_json::Value::String(update_url.clone()));
        match &new_manifest.min_app_version {
            Some(min) => {
                obj.insert("minAppVersion".to_owned(), serde_json::Value::String(min.clone()));
            }
            // Absent upstream means the constraint was lifted, so drop it rather
            // than leaving a stale floor in place.
            None => {
                obj.remove("minAppVersion");
            }
        }
    }
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


    // Validate the new entry before it replaces anything. Failure leaves the
    // installed plugin completely untouched.
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

/// Check that a staged plugin is a well-formed WASM component before it replaces
/// the installed one.
///
/// This used to be a real `dlopen` on Linux and macOS and a bare file-exists check
/// on Windows — so the one platform where a broken artifact was most likely to slip
/// through was the one that checked least. Validating bytecode is platform-agnostic,
/// so the same real check now runs everywhere.
///
/// Uses `wasmparser` rather than `wasmtime`: this only needs to know the bytes are a
/// valid component, and validation is far cheaper than compiling one. The updater
/// runs on a background thread and has no business spending a second in Cranelift
/// to answer a yes/no question. The app compiles it later, once, and caches it.
fn test_load(entry: &Path) -> Result<(), String> {
    let bytes = std::fs::read(entry).map_err(|e| format!("read staged entry: {e}"))?;
    if bytes.is_empty() {
        return Err("staged entry is empty".to_string());
    }
    // Component-model validation is on by default in `WasmFeatures`.
    wasmparser::Validator::new()
        .validate_all(&bytes)
        .map(|_| ())
        .map_err(|e| format!("not a valid WASM component: {e}"))
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
                "type": "wasm",
                "entry": "p.wasm",
                "version": version,
                "updateUrl": update_url,
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(dir.join("p.wasm"), minimal_component()).unwrap();
        dir
    }


    /// The smallest thing `test_load` will accept: an empty but valid WASM
    /// component — `\0asm`, version, layer. Fixtures have to be real components
    /// now that a staged plugin is genuinely validated before it replaces the
    /// installed one; the old check only looked at whether the file existed.
    fn minimal_component() -> Vec<u8> {
        vec![0x00, 0x61, 0x73, 0x6d, 0x0d, 0x00, 0x01, 0x00]
    }

    #[test]
    fn a_staged_entry_that_is_not_a_component_is_rejected() {
        // The old check only confirmed the file existed unless it ended in .so or
        // .dylib, so a corrupt or truncated download reached the live directory and
        // failed later, at load time. Validation is now real and platform-agnostic.
        let dir = tempfile::tempdir().unwrap();

        let junk = dir.path().join("junk.wasm");
        std::fs::write(&junk, b"// not wasm at all").unwrap();
        let err = test_load(&junk).unwrap_err();
        assert!(err.contains("not a valid WASM component"), "got {err}");

        let empty = dir.path().join("empty.wasm");
        std::fs::write(&empty, b"").unwrap();
        assert!(test_load(&empty).unwrap_err().contains("empty"));

        let missing = dir.path().join("gone.wasm");
        assert!(test_load(&missing).unwrap_err().contains("read staged entry"));

        let good = dir.path().join("good.wasm");
        std::fs::write(&good, minimal_component()).unwrap();
        assert!(test_load(&good).is_ok(), "a valid component must pass");
    }

    #[test]
    fn skips_plugin_without_update_url() {
        let plugins = tempfile::tempdir().unwrap();
        let dir = plugins.path().join("foo");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("plugin.json"),
            r#"{"name":"foo","displayName":"foo","type":"wasm","entry":"p.wasm","version":"0.1.0"}"#,
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

        // Must be a real component: the staged entry is validated before the swap.
        let new_component = minimal_component();
        let new_body: &[u8] = &new_component;
        let sig_b64 = B64.encode(sk.sign(new_body).to_bytes());
        let sha = {
            use sha2::Digest;
            let mut h = sha2::Sha256::new();
            h.update(new_body);
            hex(&h.finalize())
        };

        let server = MockServer::start().await;
        let entry_url = format!("{}/entry.wasm", server.uri());
        let manifest_url = format!("{}/manifest", server.uri());

        let dir = plugins.path().join("foo");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("plugin.json"),
            serde_json::json!({
                "name": "foo",
                // Deliberately different from `name`, and carrying fields the
                // updater knows nothing about, so the assertions below can prove an
                // update preserves rather than reconstructs the manifest.
                "displayName": "Foo Display",
                "type": "wasm",
                "entry": "p.wasm",
                "version": "1.0.0",
                "updateUrl": manifest_url,
                "pubkey": pk_b64,
                "allowedHosts": ["api.example.com"],
                "supportsConfigFiles": true,
            })
            .to_string(),
        )
        .unwrap();
        std::fs::write(dir.join("p.wasm"), minimal_component()).unwrap();

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
            .and(path("/entry.wasm"))
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

        let live = std::fs::read(plugins.path().join("foo/p.wasm")).unwrap();
        assert_eq!(live, new_body);

        // An update must not quietly strip fields the updater has no opinion about.
        // Rebuilding the manifest from this crate's partial view used to drop
        // `allowedHosts` — so a plugin lost its network capability on update — and
        // overwrite `displayName` with `name`, which also renames the settings
        // section its own `get_setting` reads from.
        let updated: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(plugins.path().join("foo/plugin.json")).unwrap(),
        )
        .unwrap();

        assert_eq!(updated["version"], "1.0.1", "version should be bumped");
        assert_eq!(updated["entry"], "p.wasm");
        assert_eq!(updated["type"], "wasm", "the plugin type must survive an update");
        assert_eq!(
            updated["displayName"], "Foo Display",
            "displayName must not be replaced by name"
        );
        assert_eq!(
            updated["allowedHosts"],
            serde_json::json!(["api.example.com"]),
            "allowedHosts must survive, or the plugin silently loses network access"
        );
        assert_eq!(
            updated["supportsConfigFiles"], true,
            "unrelated fields must be preserved verbatim"
        );
        // The trust root stays put so the next update verifies against the same key.
        assert_eq!(updated["pubkey"], pk_b64);

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
        let entry_url = format!("{}/entry.wasm", server.uri());
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
            .and(path("/entry.wasm"))
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

        let live = std::fs::read(plugins.path().join("foo/p.wasm")).unwrap();
        // Still the originally installed component: a rejected update must leave
        // the live directory untouched.
        assert_eq!(live, minimal_component());
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
