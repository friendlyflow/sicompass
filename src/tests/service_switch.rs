//! A plugin's service switch (`service.switch` in its `plugin.json`), end to
//! end in the app: the row is on the settings page only while the user holds
//! the service's tier, and a value the Store writes into `settings.json` (in
//! this process or, in a desicompass session, the superkey's) shows on it.
//!
//! lib_store's own tests cover what the Store writes, when the user pays and
//! when it installs a plugin while the tier is held. This one covers the app's
//! half, `programs::follow_services`.
//!
//! # Why this is its own test binary
//!
//! It sets `SICOMPASS_PLUGIN_PATH`, points `XDG_CONFIG_HOME` (or its
//! macOS/Windows equivalent) at a temporary directory, and registers its own
//! license checker, all process-global. Cargo runs each test file in its own
//! process, and this file holds one test, so the overrides reach nothing else.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use sicompass::programs;
use sicompass_sdk::FfonElement;
use sicompass_sdk::license::{LicenseStatus, Standing};
use sicompass_ui::app_state::AppRenderer;

/// What the fake license checker answers for [`TIER`].
static STANDING: Mutex<LicenseStatus> = Mutex::new(LicenseStatus::Missing);

const TIER: &str = "acme/cloud";

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

/// The fixture program under `root`, with a checkbox that is its service's
/// switch and one that is not. Provided by the configuration, so it runs
/// without an approval.
fn place_fixture(root: &Path) {
    let dir = root.join("fixture");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("plugin.json"),
        format!(
            r#"{{ "name": "fixture", "displayName": "fixture demo", "type": "process",
                 "entry": "plugin", "version": "0.2.0", "minAppVersion": "0.1.0",
                 "settings": [
                   {{ "type": "checkbox", "label": "sync", "key": "fixtureSync" }},
                   {{ "type": "checkbox", "label": "other", "key": "fixtureOther" }}
                 ],
                 "service": {{ "tier": "{TIER}", "switch": "fixtureSync" }} }}"#
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

fn settings_path() -> PathBuf {
    sicompass_sdk::platform::main_config_path().unwrap()
}

fn settings_json() -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(settings_path()).unwrap_or_else(|_| "{}".into()))
        .unwrap()
}

/// Write `settings.json` the way the Store does when the user pays: the
/// switch ticked in the plugin's own section, everything else kept.
fn tick_as_the_store_does() {
    let mut v = settings_json();
    v["fixture demo"]["fixtureSync"] = serde_json::Value::Bool(true);
    std::fs::write(settings_path(), serde_json::to_string_pretty(&v).unwrap()).unwrap();
}

/// The lines under the settings section whose title starts with `name`.
fn settings_section(renderer: &AppRenderer, name: &str) -> Vec<String> {
    let root = renderer.ffon.last().unwrap().as_obj().unwrap();
    root.children
        .iter()
        .find_map(|c| match c {
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
        .unwrap_or_else(|| panic!("no {name} section"))
}

fn set_standing(status: LicenseStatus) {
    *STANDING.lock().unwrap() = status;
}

#[test]
fn a_service_switch_shows_while_the_tier_is_held_with_what_the_store_wrote() {
    let home = tempfile::tempdir().unwrap();
    let system = home.path().join("system-plugins");
    sandbox(home.path(), &system);
    sicompass_builtins::register_all();
    // After the Store's own registration: the last one wins.
    sicompass_sdk::license::register_checker(|tier| {
        if tier == TIER {
            Standing {
                status: *STANDING.lock().unwrap(),
                days: 3,
            }
        } else {
            Standing::MISSING
        }
    });
    place_fixture(&system);

    let mut renderer = AppRenderer::new();
    renderer.hooks = Box::new(sicompass::boot::ProgramsHooks::default());
    let queue = programs::load_programs(&mut renderer);
    programs::apply_pending_settings(&mut renderer, &queue, true);
    renderer.settings_queue = Some(queue.clone());

    // ---- Not paid: no switch, from the first frame; other settings stay ----
    let rows = settings_section(&renderer, "fixture demo");
    assert!(!rows.iter().any(|r| r.ends_with("sync")), "{rows:?}");
    assert!(rows.contains(&"<checkbox>other".to_owned()), "{rows:?}");

    // ---- Paid (the certificate arrived): the switch appears ----
    set_standing(LicenseStatus::Active);
    programs::follow_services(&mut renderer, true);
    let rows = settings_section(&renderer, "fixture demo");
    assert!(rows.contains(&"<checkbox>sync".to_owned()), "{rows:?}");

    // ---- The Store ticked it, as at payment: the row follows the file ----
    tick_as_the_store_does();
    programs::follow_services(&mut renderer, true);
    let rows = settings_section(&renderer, "fixture demo");
    assert!(rows.contains(&"<checkbox checked>sync".to_owned()), "{rows:?}");

    // ---- Grace still counts as paid ----
    set_standing(LicenseStatus::Grace);
    programs::follow_services(&mut renderer, true);
    let rows = settings_section(&renderer, "fixture demo");
    assert!(rows.contains(&"<checkbox checked>sync".to_owned()), "{rows:?}");

    // ---- Expired: the switch goes, its value stays for a later payment ----
    set_standing(LicenseStatus::Expired);
    programs::follow_services(&mut renderer, true);
    let rows = settings_section(&renderer, "fixture demo");
    assert!(!rows.iter().any(|r| r.ends_with("sync")), "{rows:?}");
    assert!(rows.contains(&"<checkbox>other".to_owned()), "{rows:?}");
    assert_eq!(
        settings_json()["fixture demo"]["fixtureSync"],
        serde_json::Value::Bool(true)
    );

    // ---- Unticked by the user while paid: kept across later looks ----
    set_standing(LicenseStatus::Active);
    programs::follow_services(&mut renderer, true);
    let mut v = settings_json();
    v["fixture demo"]["fixtureSync"] = serde_json::Value::Bool(false);
    std::fs::write(settings_path(), serde_json::to_string_pretty(&v).unwrap()).unwrap();
    programs::follow_services(&mut renderer, true);
    programs::follow_services(&mut renderer, true);
    let rows = settings_section(&renderer, "fixture demo");
    assert!(rows.contains(&"<checkbox>sync".to_owned()), "{rows:?}");
}
