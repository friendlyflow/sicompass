//! `ProcessProvider` against real plugin processes: `examples/process_fixture.rs`
//! and `examples/process_fixture_future.rs`, which `cargo test` builds.
//!
//! Each test installs the fixture into a plugin directory of its own (its
//! executable, `plugin.json`, `locales/`) and drives it through the `Provider`
//! trait, the way the app does.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sicompass::plugin_host::Grants;
use sicompass::plugin_host::{ProcessProvider, Spec};
use sicompass_sdk::{DashboardKey, DashboardKeysym, FfonElement, Provider, TimelineEntry};

/// `target/<profile>/examples/<name>`, building it when `cargo test --test
/// process_plugin` did not.
fn example(name: &str) -> PathBuf {
    let exe = std::env::current_exe().unwrap();
    let profile = exe.parent().unwrap().parent().unwrap();
    let path = profile
        .join("examples")
        .join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    if !path.exists() {
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let ok = std::process::Command::new(cargo)
            .args(["build", "-p", "sicompass", "--example", name])
            .status()
            .unwrap()
            .success();
        assert!(ok, "building {name} failed");
    }
    path
}

/// A plugin directory holding `example` as its `plugin` executable.
fn install(example_name: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let exe = dir
        .path()
        .join(format!("plugin{}", std::env::consts::EXE_SUFFIX));
    std::fs::copy(example(example_name), &exe).unwrap();
    std::fs::write(
        dir.path().join("plugin.json"),
        r#"{ "name": "fixture", "displayName": "fixture", "type": "process", "entry": "plugin" }"#,
    )
    .unwrap();
    std::fs::create_dir(dir.path().join("locales")).unwrap();
    std::fs::write(
        dir.path().join("locales/en-US.ftl"),
        "fixture-name = Fixture\nfixture-hello = Hello through the app\n",
    )
    .unwrap();
    dir
}

fn grants(storage: Option<&Path>) -> Grants {
    Grants {
        settings: vec!["greeting".into()],
        setting_defaults: vec![("greeting".into(), "hi".into())],
        service_tier: Some("acme/pro".into()),
        storage_dir: storage.map(Path::to_path_buf),
        renders_pages: true,
        ..Default::default()
    }
}

fn open_in(dir: &Path, grants: Grants) -> Result<ProcessProvider, String> {
    ProcessProvider::open(Spec {
        entry_path: &dir.join("plugin"),
        plugin_name: "fixture",
        settings_section: "fixture",
        plugin_dir: dir,
        grants,
        env: Vec::new(),
    })
}

fn texts(elements: &[FfonElement]) -> Vec<String> {
    elements
        .iter()
        .filter_map(|e| e.as_str().map(str::to_owned))
        .collect()
}

/// Whether a process is still there.
#[cfg(target_os = "linux")]
fn alive(pid: u32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        // A zombie has exited; only its parent has not looked yet.
        Ok(stat) => !stat.contains(") Z "),
        Err(_) => false,
    }
}

#[cfg(target_os = "linux")]
fn gone_within(pid: u32, limit: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < limit {
        if !alive(pid) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    false
}

#[test]
fn a_plugin_process_is_a_provider() {
    let dir = install("process_fixture");
    let mut p = open_in(dir.path(), grants(None)).unwrap();

    // `describe` and `fetch` translated through the app's bundles, which
    // loaded the plugin's own locales.
    assert_eq!(p.display_name(), "Fixture");
    let rows = texts(&p.fetch());
    assert_eq!(rows[0], "Hello through the app");
    // The declared setting's default arrived with `init`.
    assert_eq!(rows[1], "greeting: hi");

    p.on_setting_change("greeting", "hey");
    p.on_setting_change("emailPassword", "secret");
    assert_eq!(texts(&p.fetch())[1], "greeting: hey");

    // Navigation keeps the path, and the poll after it answers for the new level.
    assert!(p.at_root());
    p.push_path("a");
    assert_eq!(p.current_path(), "/a");
    assert!(!p.at_root());
    p.pop_path();
    assert_eq!(p.current_path(), "/");

    // A command that moves the plugin is noticed straight away.
    assert!(p.execute_command("jump", ""));
    assert_eq!(p.current_path(), "/elsewhere");

    // stdout is not the channel.
    assert!(p.execute_command("print", ""));
    assert_eq!(texts(&p.fetch()).len(), 4);
    assert!(p.take_error().is_none());
    p.cleanup();
}

#[test]
fn a_token_only_for_its_own_service() {
    sicompass_sdk::license::register_token_source(|tier| Some(format!("token-for-{tier}")));
    let dir = install("process_fixture");
    let mut p = open_in(dir.path(), grants(None)).unwrap();
    p.execute_command("token", "acme/pro");
    assert_eq!(texts(&p.fetch())[2], "note: token-for-acme/pro");
    p.execute_command("token", "friendlyflow/cloud");
    assert_eq!(texts(&p.fetch())[2], "note: none");
}

#[test]
fn its_storage_folder_is_created_and_handed_over() {
    let dir = install("process_fixture");
    let data = tempfile::tempdir().unwrap();
    let storage = data.path().join("fixture");
    let mut p = open_in(dir.path(), grants(Some(&storage))).unwrap();
    assert!(storage.is_dir());
    assert_eq!(
        texts(&p.fetch())[3],
        format!("storage: {}", storage.display())
    );
    p.execute_command("write-storage", "kept");
    assert_eq!(
        std::fs::read_to_string(storage.join("written.txt")).unwrap(),
        "kept"
    );
}

#[test]
fn config_timeline_and_dashboard_cross_whole() {
    let dir = install("process_fixture");
    let mut p = open_in(dir.path(), grants(None)).unwrap();

    let cfg = dir.path().join("cfg.txt");
    std::fs::write(&cfg, "loaded").unwrap();
    assert!(p.load_config(&cfg));
    assert_eq!(texts(&p.fetch())[2], "note: loaded");
    assert!(p.save_config(&cfg));
    assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "saved loaded");

    p.execute_command("edit", "x");
    let entries = p.take_timeline_entries();
    assert_eq!(entries.len(), 1);
    let mut error = String::new();
    sicompass_sdk::block_on(p.undo(&entries[0], &mut error));
    assert!(error.is_empty(), "{error}");
    assert_eq!(texts(&p.fetch())[2], "note: undid edit x");
    match &entries[0] {
        TimelineEntry::ProviderOp { payload, .. } => {
            assert_eq!(payload, &FfonElement::new_str("x"))
        }
        other => panic!("{other:?}"),
    }

    p.dashboard_key(DashboardKey {
        keysym: DashboardKeysym::Char('a'),
        ctrl: false,
        shift: false,
        alt: false,
    });
    let frame = p.dashboard_render(10, 2);
    assert_eq!((frame.cols, frame.rows, frame.cells.len()), (10, 2, 20));
    let first: String = frame.cells[..6].iter().map(|c| c.ch).collect();
    assert_eq!(first, "keys 1");
    assert_eq!(frame.cursor, Some((0, 0)));
}

#[test]
fn pages_are_rendered_for_the_links_that_asked() {
    let dir = install("process_fixture");
    let mut p = open_in(dir.path(), grants(None)).unwrap();
    assert!(sicompass_sdk::url_fetcher::renderer_available());
    let wanted = |u: &str| u.contains("process-plugin-test");
    sicompass_sdk::url_fetcher::request_render("https://process-plugin-test.example/a");
    sicompass_sdk::url_fetcher::request_render("https://process-plugin-test.example/decline");
    // One tick asks; the plugin finishes in the poll that follows.
    p.tick();
    p.tick();
    let mut got = sicompass_sdk::url_fetcher::take_rendered_where(wanted);
    got.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(got.len(), 2, "{got:?}");
    assert_eq!(
        got[0].1,
        vec![FfonElement::new_str(
            "rendered https://process-plugin-test.example/a"
        )]
    );
    assert_eq!(
        got[1].1,
        vec![FfonElement::new_str(
            "https://process-plugin-test.example/decline could not be rendered"
        )]
    );
}

#[cfg(target_os = "linux")]
#[test]
fn the_tab_switcher_sees_its_program_and_letting_go_stops_both() {
    let dir = install("process_fixture");
    let mut p = open_in(dir.path(), grants(None)).unwrap();
    assert!(p.execute_command("spawn-child", ""));
    p.tick();
    let shell = p.process_id().expect("the child it runs");
    let plugin = p.plugin_pid();
    assert_ne!(shell, plugin);
    assert!(alive(shell) && alive(plugin));
    // A tab closed without `cleanup`: the plugin cleans up on its own.
    drop(p);
    assert!(
        gone_within(plugin, Duration::from_secs(5)),
        "the plugin exited"
    );
    assert!(
        gone_within(shell, Duration::from_secs(5)),
        "its child was stopped"
    );
}

#[test]
fn a_panic_disables_the_plugin_and_says_so_once() {
    let dir = install("process_fixture");
    let mut p = open_in(dir.path(), grants(None)).unwrap();
    assert!(!p.execute_command("boom", ""));
    let err = p.take_error().expect("an error row");
    assert!(err.contains("panicked"), "{err}");
    assert!(err.contains("the fixture was told to panic"), "{err}");
    assert!(p.is_poisoned());
    assert!(p.fetch().is_empty());
    p.tick();
    assert_eq!(p.take_error(), None, "once, not every frame");
}

#[test]
fn a_hung_plugin_is_stopped_at_the_deadline() {
    let dir = install("process_fixture");
    let mut p = open_in(dir.path(), grants(None)).unwrap();
    p.set_call_deadline(Some(Duration::from_millis(300)));
    let start = Instant::now();
    assert!(!p.execute_command("hang", ""));
    assert!(start.elapsed() < Duration::from_secs(5));
    let err = p.take_error().expect("an error row");
    assert!(err.contains("took too long"), "{err}");
    assert!(p.is_poisoned());
    #[cfg(target_os = "linux")]
    assert!(gone_within(p.plugin_pid(), Duration::from_secs(2)));
}

#[test]
fn a_plugin_from_a_newer_protocol_is_refused_with_a_reason() {
    let dir = install("process_fixture_future");
    let err = open_in(dir.path(), grants(None)).err().expect("refused");
    assert!(err.contains("protocol 99.0"), "{err}");
    assert!(err.contains("update it from the Store"), "{err}");
}

#[test]
fn a_missing_executable_is_an_error_not_a_panic() {
    let dir = tempfile::tempdir().unwrap();
    let err = open_in(dir.path(), grants(None)).err().expect("refused");
    assert!(err.contains("cannot start"), "{err}");
}

/// A real plugin, by hand: `SICOMPASS_TEST_PLUGIN=<plugin dir> cargo test -p
/// sicompass --test process_plugin -- --ignored a_real_plugin`, where the
/// directory holds its `plugin.json`, executable, `assets/` and `locales/`
/// (an unpacked release, or a checkout with the built executable copied in).
/// It must start, describe itself, fetch its root and stop again, without an
/// error row.
#[test]
#[ignore = "needs SICOMPASS_TEST_PLUGIN"]
fn a_real_plugin_starts_fetches_and_stops() {
    let dir = PathBuf::from(std::env::var("SICOMPASS_TEST_PLUGIN").expect("SICOMPASS_TEST_PLUGIN"));
    let m = sicompass::plugin_manifest::parse_manifest(
        &std::fs::read_to_string(dir.join("plugin.json")).unwrap(),
    )
    .unwrap();
    let data = tempfile::tempdir().unwrap();
    let grants = Grants {
        settings: m.settings.iter().map(|s| s.key.clone()).collect(),
        // As `plugin_manifest::grants_for` builds them for the app.
        setting_defaults: m
            .settings
            .iter()
            .filter(|s| !s.default.is_empty())
            .map(|s| {
                (
                    s.key.clone(),
                    sicompass::plugin_manifest::expand_home(&s.default),
                )
            })
            .collect(),
        storage_dir: m.permissions.storage.then(|| data.path().join(&m.name)),
        service_tier: m.service.as_ref().map(|s| s.tier.clone()),
        renders_pages: m.renders_pages,
        ..Default::default()
    };
    let mut p = ProcessProvider::open(Spec {
        entry_path: &dir.join(&m.entry),
        plugin_name: &m.name,
        settings_section: &m.display_name,
        plugin_dir: &dir,
        grants,
        env: Vec::new(),
    })
    .unwrap();
    println!("{} ({})", p.display_name(), p.name());
    let root = p.fetch();
    for e in &root {
        println!(
            "  {}",
            e.as_str()
                .map(str::to_owned)
                .unwrap_or_else(|| format!("{e:?}"))
        );
    }
    assert!(!root.is_empty(), "an empty root");
    p.tick();
    assert_eq!(p.take_error(), None);
    let pid = p.plugin_pid();
    p.cleanup();
    drop(p);
    #[cfg(target_os = "linux")]
    assert!(gone_within(pid, Duration::from_secs(5)));
    let _ = pid;
}
