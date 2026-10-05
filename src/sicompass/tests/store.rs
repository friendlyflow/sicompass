//! The Store, end to end: a signed store list and a signed release of a real
//! plugin program on a local server, installed by the Store, delivered through
//! the app's settings queue, started without a restart, then uninstalled.
//!
//! lib_store's own tests cover what is refused (bad signatures, tampered
//! archives, more access than approved). This one covers the hand-over to the
//! app: that a finished install ends up as a running program with its settings
//! section, an approval and an enable switch in settings.json, that an
//! uninstall takes all of that away again, and that the data folder goes to the
//! trash only when asked.
//!
//! # Why this is its own test binary
//!
//! It points `XDG_CONFIG_HOME` (or its macOS/Windows equivalent) at a temporary
//! directory, which is process-global. Cargo runs each test file in its own
//! process, and this file holds one test, so the override reaches nothing else.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sicompass::programs;
use sicompass_sdk::FfonElement;
use sicompass_sdk::package::{self, RELEASE_FILE, ReleaseInfo, SIGNATURE_FILE};
use sicompass_ui::app_state::AppRenderer;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const REPO: &str = "friendlyflow/fixture-plugin-sicompass";

/// `examples/process_fixture.rs`, which `cargo test` builds, as this
/// platform's program in the release.
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

/// Point the platform's config and data directories into `dir`; see the
/// module docs. (On macOS and Windows both follow the one variable.)
fn sandbox_config(dir: &Path) {
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
    }
}

fn serve(rt: &tokio::runtime::Runtime, server: &MockServer, at: &str, body: Vec<u8>) {
    rt.block_on(
        Mock::given(method("GET"))
            .and(path(at))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
            .mount(server),
    );
}

/// Tick the provider at `idx` until its background job reports back.
fn settle(renderer: &mut AppRenderer, idx: usize) {
    let start = Instant::now();
    while !renderer.providers[idx].tick() {
        assert!(start.elapsed() < Duration::from_secs(30), "the Store hung");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn settings_json() -> serde_json::Value {
    let path = sicompass_sdk::platform::main_config_path().unwrap();
    serde_json::from_str(&std::fs::read_to_string(path).unwrap_or_else(|_| "{}".into())).unwrap()
}

/// The settings panel's section whose title starts with `name` (the programs
/// section's title is translated and longer than its config key), as the lines
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

fn store_index(renderer: &AppRenderer) -> usize {
    renderer
        .providers
        .iter()
        .position(|p| p.name() == programs::STORE)
        .expect("the Store is always present")
}

fn loaded(renderer: &AppRenderer, name: &str) -> bool {
    renderer.providers.iter().any(|p| p.name() == name)
}

#[test]
fn the_store_installs_a_plugin_into_the_running_app_and_removes_it() {
    // The app spells the Store's keys out itself (the SDK boundary keeps it from
    // importing lib_store), so they have to agree.
    assert_eq!(programs::STORE, "store");
    assert_eq!(
        programs::PLUGIN_INSTALLED,
        sicompass_store::PLUGIN_INSTALLED
    );
    assert_eq!(programs::PLUGIN_UPDATED, sicompass_store::PLUGIN_UPDATED);
    assert_eq!(programs::PLUGIN_REMOVED, sicompass_store::PLUGIN_REMOVED);

    let config_home = tempfile::tempdir().unwrap();
    sandbox_config(config_home.path());
    sicompass_builtins::register_all();
    // The data folder goes to a private temp trash, never the developer's.
    sicompass::plugin_host::desktop::_set_test_no_trash(true);

    // ---- A release of a real plugin program, signed by its own key ----
    let (plugin_secret, plugin_public) = package::generate_keypair().unwrap();
    let (store_secret, store_public) = package::generate_keypair().unwrap();
    let src = tempfile::tempdir().unwrap();
    let manifest_json = r#"{
        "name": "fixture",
        "displayName": "fixture demo",
        "type": "process",
        "entry": "plugin",
        "version": "0.2.0",
        "minAppVersion": "0.1.0",
        "settings": [
            { "type": "text", "label": "greeting", "key": "greeting", "default": "hi" }
        ]
    }"#;
    std::fs::write(src.path().join("plugin.json"), manifest_json).unwrap();
    let manifest = sicompass_sdk::plugin_manifest::parse_manifest(manifest_json).unwrap();
    let files = package::collect_files(src.path(), &manifest).unwrap();
    let target = sicompass_sdk::plugin_abi::plugin_target().unwrap();
    let program = std::fs::read(fixture_program()).unwrap();
    let archive =
        package::build_process_archive(src.path(), &files, &manifest, target, &program).unwrap();
    let release = serde_json::to_vec(
        &ReleaseInfo::new_process(
            &manifest,
            &std::collections::BTreeMap::from([(target.to_owned(), archive.clone())]),
        )
        .unwrap(),
    )
    .unwrap();
    let release_sig = package::sign(&release, &plugin_secret).unwrap();
    let store = serde_json::to_vec(&serde_json::json!({
        "version": 1,
        "plugins": [{ "name": "fixture", "repo": REPO, "pubkey": plugin_public }]
    }))
    .unwrap();
    let store_sig = package::sign(&store, &store_secret).unwrap();

    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = rt.block_on(MockServer::start());
    serve(&rt, &server, "/store/store.json", store);
    serve(
        &rt,
        &server,
        "/store/store.json.sig",
        store_sig.into_bytes(),
    );
    let download = |file: &str| format!("/{REPO}/releases/latest/download/{file}");
    serve(&rt, &server, &download(RELEASE_FILE), release);
    serve(
        &rt,
        &server,
        &download(SIGNATURE_FILE),
        release_sig.into_bytes(),
    );
    serve(&rt, &server, &download(&package::archive_file(target)), archive);

    // ---- The app, with its Store pointed at that server ----
    let mut renderer = AppRenderer::new();
    renderer.hooks = Box::new(sicompass::boot::ProgramsHooks::default());
    let queue = programs::load_programs(&mut renderer);
    programs::apply_pending_settings(&mut renderer, &queue, true);
    renderer.settings_queue = Some(queue.clone());

    let idx = store_index(&renderer);
    let plugins_dir = sicompass_sdk::platform::plugins_dir().unwrap();
    assert!(plugins_dir.starts_with(config_home.path()));
    let data_dir = sicompass_sdk::platform::app_data_dir().unwrap();
    assert!(data_dir.starts_with(config_home.path()));

    renderer.providers[idx] = Box::new(sicompass_store::StoreProvider::new().with_sources(
        sicompass_store::http::http_fetch(),
        &format!("{}/store/", server.uri()),
        &server.uri(),
        &[store_public.as_str()],
        plugins_dir.clone(),
    ));
    programs::wire_store(&mut renderer.providers, &queue);
    assert!(!loaded(&renderer, "fixture"));

    // ---- Install ----
    renderer.providers[idx].set_current_path("/programs");
    renderer.providers[idx].fetch();
    settle(&mut renderer, idx);
    renderer.providers[idx].on_button_press("install:fixture");
    settle(&mut renderer, idx);
    programs::apply_pending_settings(&mut renderer, &queue, false);

    assert!(plugins_dir.join("fixture").join(sicompass_sdk::plugin_abi::executable_name("plugin", target)).is_file());
    assert!(loaded(&renderer, "fixture"), "installed but not loaded");
    let cfg = settings_json();
    assert_eq!(
        cfg["pluginApprovals"]["fixture"].as_str(),
        Some(sicompass_sdk::plugin_abi::approval_fingerprint(&manifest).as_str())
    );
    assert_eq!(cfg["Available programs:"]["enable_fixture"], true);
    let section = settings_section(&renderer, "fixture demo")
        .expect("the plugin's settings section appears without a restart");
    assert!(
        section.iter().any(|l| l.contains("greeting")),
        "its settings are injected on a live install: {section:?}"
    );
    assert!(
        section.iter().any(|l| l.contains("0.2.0")),
        "its version is shown in the same section: {section:?}"
    );
    let sections: Vec<String> = renderer
        .ffon
        .last()
        .unwrap()
        .as_obj()
        .unwrap()
        .children
        .iter()
        .filter_map(|c| c.as_obj().map(|o| o.key.clone()))
        .collect();
    assert_eq!(
        sections.iter().filter(|k| k.contains("fixture")).count(),
        1,
        "one section per plugin, as at startup: {sections:?}"
    );
    let programs_section = settings_section(&renderer, "Available programs").unwrap();
    assert!(
        programs_section.iter().any(|l| l.contains("fixture demo")),
        "{programs_section:?}"
    );

    // The plugin's data folder, as it would have made one.
    let data = data_dir.join("fixture");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("kept.txt"), "mine").unwrap();

    // ---- Uninstall ----
    // `fixture` sorts before `store`, so the Store has moved down one place.
    let idx = store_index(&renderer);
    renderer.providers[idx].on_button_press("uninstall:fixture");
    programs::apply_pending_settings(&mut renderer, &queue, false);

    assert!(!plugins_dir.join("fixture").exists());
    assert!(!loaded(&renderer, "fixture"), "uninstalled but still loaded");
    let cfg = settings_json();
    assert!(cfg["pluginApprovals"].get("fixture").is_none(), "{cfg}");
    assert!(
        cfg["Available programs:"].get("enable_fixture").is_none(),
        "{cfg}"
    );
    assert!(settings_section(&renderer, "fixture demo").is_none());
    let programs_section = settings_section(&renderer, "Available programs").unwrap();
    assert!(
        !programs_section.iter().any(|l| l.contains("fixture demo")),
        "{programs_section:?}"
    );

    // Uninstalling keeps the data; moving it to the trash is a separate press.
    assert!(data.join("kept.txt").is_file());
    let idx = store_index(&renderer);
    renderer.providers[idx].on_button_press("trashdata:fixture");
    programs::apply_pending_settings(&mut renderer, &queue, false);
    assert!(
        !data.exists(),
        "the data folder should be in the (test) trash"
    );
}
