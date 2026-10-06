//! A plugin this computer's configuration provides, end to end: a real plugin
//! program in a folder `SICOMPASS_PLUGIN_PATH` names, started at launch with no
//! approval and without being switched on, in place of the user's own copy of
//! the same plugin.
//!
//! The desicompass NixOS module points that variable at plugins built from
//! local checkouts for its dev session.
//!
//! # Why this is its own test binary
//!
//! It sets `SICOMPASS_PLUGIN_PATH` and points `XDG_CONFIG_HOME` (or its
//! macOS/Windows equivalent) at a temporary directory, both process-global.
//! Cargo runs each test file in its own process, and this file holds one test,
//! so the overrides reach nothing else.

use std::path::{Path, PathBuf};

use sicompass::programs;
use sicompass_sdk::FfonElement;
use sicompass_ui::app_state::AppRenderer;

/// `examples/process_fixture.rs`, which `cargo test` builds.
fn fixture_program() -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let profile = exe.parent().unwrap().parent().unwrap();
    let path = profile
        .join("examples")
        .join(format!("process_fixture{}", std::env::consts::EXE_SUFFIX));
    if !path.exists() {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let ok = std::process::Command::new(cargo)
            .args(["build", "-p", "sicompass", "--example", "process_fixture"])
            .status()
            .unwrap()
            .success();
        assert!(ok, "building the fixture failed");
    }
    path
}

/// Point the platform's config and data directories into `dir`, and the
/// configuration's plugin folder at `system`; see the module docs.
fn sandbox(dir: &Path, system: &Path) {
    // SAFETY: called first thing in the only test of this binary, before any
    // other thread exists.
    unsafe {
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        {
            std::env::set_var("XDG_CONFIG_HOME", dir.join("config"));
            std::env::set_var("XDG_DATA_HOME", dir.join("data"));
        }
        #[cfg(target_os = "windows")]
        std::env::set_var("APPDATA", dir);
        #[cfg(target_os = "macos")]
        std::env::set_var("HOME", dir);
        std::env::set_var(sicompass_sdk::platform::PLUGIN_PATH_VAR, system);
    }
}

/// `fixture` under `root`, as the Store would lay it out, with `display_name`.
fn place_fixture(root: &Path, display_name: &str) {
    let dir = root.join("fixture");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("plugin.json"),
        format!(
            r#"{{ "name": "fixture", "displayName": "{display_name}", "type": "process",
                 "entry": "plugin", "version": "0.2.0", "minAppVersion": "0.1.0",
                 "permissions": {{ "process": ["git"] }} }}"#
        ),
    )
    .unwrap();
    let target = sicompass_sdk::plugin_abi::plugin_target().unwrap();
    let exe = dir.join(sicompass_sdk::plugin_abi::executable_name("plugin", target));
    std::fs::copy(fixture_program(), &exe).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn settings_json() -> serde_json::Value {
    let path = sicompass_sdk::platform::main_config_path().unwrap();
    serde_json::from_str(&std::fs::read_to_string(path).unwrap_or_else(|_| "{}".into())).unwrap()
}

/// The settings panel's section whose title starts with `name`, as the lines
/// under it.
fn settings_section(renderer: &AppRenderer, name: &str) -> Option<Vec<String>> {
    let root = renderer.ffon.last()?.as_obj()?;
    root.children.iter().find_map(|c| match c {
        FfonElement::Obj(o) if o.key.starts_with(name) => Some(
            o.children
                .iter()
                .map(|c| match c {
                    FfonElement::Str(s) => s.clone(),
                    FfonElement::Obj(o) => o.key.clone(),
                })
                .collect(),
        ),
        _ => None,
    })
}

#[test]
fn a_plugin_the_configuration_provides_runs_unasked_in_place_of_the_users_copy() {
    let home = tempfile::tempdir().unwrap();
    let system = home.path().join("system-plugins");
    sandbox(home.path(), &system);
    sicompass_builtins::register_all();

    place_fixture(&system, "system fixture");
    // The user's own copy of the same plugin, which the system one hides.
    let plugins_dir = sicompass_sdk::platform::plugins_dir().unwrap();
    assert!(plugins_dir.starts_with(home.path()));
    place_fixture(&plugins_dir, "user fixture");

    let mut renderer = AppRenderer::new();
    renderer.hooks = Box::new(sicompass::boot::ProgramsHooks::default());
    let queue = programs::load_programs(&mut renderer);
    programs::apply_pending_settings(&mut renderer, &queue, true);

    // Running, though nobody approved it.
    assert!(
        renderer.providers.iter().any(|p| p.name() == "fixture"),
        "the configuration's plugin was not started"
    );
    let cfg = settings_json();
    assert!(cfg["pluginApprovals"].get("fixture").is_none(), "{cfg}");

    // The system copy, once, and not the user's.
    assert!(settings_section(&renderer, "system fixture").is_some());
    assert!(settings_section(&renderer, "user fixture").is_none());
    assert_eq!(
        renderer
            .providers
            .iter()
            .filter(|p| p.name() == "fixture")
            .count(),
        1
    );
}
