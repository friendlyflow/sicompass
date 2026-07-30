//! Installed-on-disk to loaded-in-app, through the real discovery path.
//!
//! The other suites start from a known file path. This one starts where a user
//! actually puts a plugin — `<config>/sicompass/plugins/<name>/` — and checks that
//! `discover_user_plugins` finds it, parses its manifest, resolves its entry, and
//! that the result is something the host will instantiate.
//!
//! # Why this is its own test binary
//!
//! It sets `XDG_CONFIG_HOME`, which is process-global. Cargo gives each test *file*
//! its own process, so the override cannot leak into the other suites, and keeping
//! this file to a single test means it cannot race itself either.

use std::path::PathBuf;

use sicompass::plugin_manifest::{PluginType, discover_user_plugins};
use sicompass::wasm_host::WasmProvider;
use sicompass_sdk::Provider;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wasm").join(name)
}

#[test]
fn a_plugin_installed_on_disk_is_discovered_and_loads() {
    let config_home = tempfile::tempdir().expect("temp config home");
    let plugin_dir = config_home.path().join("sicompass/plugins/hello");
    std::fs::create_dir_all(&plugin_dir).unwrap();
    std::fs::copy(fixture("hello.wasm"), plugin_dir.join("plugin.wasm")).unwrap();
    std::fs::write(
        plugin_dir.join("plugin.json"),
        r#"{
            "name": "hello",
            "displayName": "hello demo",
            "type": "wasm",
            "entry": "plugin.wasm",
            "version": "0.1.0"
        }"#,
    )
    .unwrap();

    // Also install something that is not a plugin, so "discovered 1" means the scan
    // is selective rather than counting directories.
    std::fs::create_dir_all(config_home.path().join("sicompass/plugins/not-a-plugin")).unwrap();

    // SAFETY: single-threaded at this point, and this binary contains one test, so
    // nothing else can observe the environment mid-change.
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", config_home.path());
    }

    let discovered = discover_user_plugins();
    let hello = discovered
        .iter()
        .find(|p| p.manifest.name == "hello")
        .unwrap_or_else(|| panic!("installed plugin was not discovered; found {discovered:?}"));

    assert_eq!(hello.manifest.plugin_type, PluginType::Wasm);
    assert_eq!(hello.manifest.display_name, "hello demo");
    assert_eq!(hello.entry_path, plugin_dir.join("plugin.wasm"));
    // Absent `allowedHosts` must mean no network, not unrestricted network.
    assert!(hello.manifest.allowed_hosts.is_empty());
    assert!(
        !discovered.iter().any(|p| p.manifest.name == "not-a-plugin"),
        "a directory without a manifest should be skipped"
    );

    // And what discovery produced is loadable, with the display name driving the
    // settings section exactly as `programs::instantiate_user_plugin` arranges.
    let mut provider = WasmProvider::open(
        &hello.entry_path,
        &hello.manifest.name,
        &hello.manifest.display_name,
        hello.entry_path.parent().unwrap(),
        hello.manifest.allowed_hosts.clone(),
    )
    .expect("the discovered plugin should instantiate");

    assert_eq!(provider.name(), "hello");
    assert!(!provider.fetch().is_empty(), "the loaded plugin should return content");
}
