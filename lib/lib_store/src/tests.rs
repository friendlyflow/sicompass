//! The Store against a local HTTP server serving a signed store list and
//! signed releases, the way GitHub does.
//!
//! wiremock needs an async runtime to start and program the server, while the
//! Store downloads with blocking reqwest on its own threads. So each test owns
//! a runtime used only for the server, and drives the
//! provider from plain synchronous code.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sicompass_sdk::package::{self, ARCHIVE_FILE, RELEASE_FILE, ReleaseInfo, SIGNATURE_FILE};
use sicompass_sdk::plugin_manifest::parse_manifest;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;

const REPO: &str = "friendlyflow/demo_plugin_sicompass";

struct Keys {
    store_secret: String,
    store_public: String,
    plugin_secret: String,
    plugin_public: String,
}

fn keys() -> Keys {
    let (store_secret, store_public) = package::generate_keypair().unwrap();
    let (plugin_secret, plugin_public) = package::generate_keypair().unwrap();
    Keys {
        store_secret,
        store_public,
        plugin_secret,
        plugin_public,
    }
}

/// A signed release: `(release.json, release.json.sig, plugin.tar.gz, info)`.
struct Release {
    json: Vec<u8>,
    sig: String,
    archive: Vec<u8>,
    info: ReleaseInfo,
}

fn plugin_json(version: &str, permissions: &str, min_app: &str) -> String {
    plugin_json_with(version, permissions, min_app, "")
}

/// `extra` is more top-level fields, each starting with a comma.
fn plugin_json_with(version: &str, permissions: &str, min_app: &str, extra: &str) -> String {
    format!(
        r#"{{ "name": "demo", "displayName": "demo", "entry": "plugin.wasm",
             "version": "{version}", "minAppVersion": "{min_app}",
             "permissions": {{ {permissions} }} {extra} }}"#
    )
}

/// What the tests' fake components contain, and the marker the test auditor
/// refuses (standing in for an import the permissions do not grant).
const COMPONENT: &[u8] = b"\0asm, not really";
const FORBIDDEN: &[u8] = b"forbidden import";

/// The app registers wasmtime's audit; these tests register a stand-in that
/// refuses [`FORBIDDEN`] (and anything that is not "\0asm").
fn register_test_auditor() {
    package::register_component_auditor(|wasm, _m| {
        if !wasm.starts_with(b"\0asm") {
            return Err("not a component".to_owned());
        }
        if wasm.windows(FORBIDDEN.len()).any(|w| w == FORBIDDEN) {
            return Err("imports something its permissions do not grant".to_owned());
        }
        Ok(())
    });
}

/// Build and sign a release of `demo`. `archived_manifest` is what goes into
/// the archive, when it should differ from the one `release.json` describes.
fn release_with(keys: &Keys, manifest: &str, archived_manifest: Option<&str>) -> Release {
    release_full(keys, manifest, archived_manifest, COMPONENT)
}

fn release_full(
    keys: &Keys,
    manifest: &str,
    archived_manifest: Option<&str>,
    wasm: &[u8],
) -> Release {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("plugin.json"),
        archived_manifest.unwrap_or(manifest),
    )
    .unwrap();
    std::fs::write(dir.path().join("plugin.wasm"), wasm).unwrap();
    let m = parse_manifest(manifest).unwrap();
    let files = package::collect_files(dir.path(), &m).unwrap();
    let archive = package::build_archive(dir.path(), &files).unwrap();
    let info = ReleaseInfo::new(&m, &archive).unwrap();
    let json = serde_json::to_vec_pretty(&info).unwrap();
    let sig = package::sign(&json, &keys.plugin_secret).unwrap();
    Release {
        json,
        sig,
        archive,
        info,
    }
}

fn release(keys: &Keys, version: &str, permissions: &str) -> Release {
    release_with(keys, &plugin_json(version, permissions, "0.1.0"), None)
}

struct Server {
    rt: tokio::runtime::Runtime,
    server: MockServer,
}

impl Server {
    fn start() -> Self {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let server = rt.block_on(MockServer::start());
        Server { rt, server }
    }

    fn uri(&self) -> String {
        self.server.uri()
    }

    fn serve(&self, at: &str, body: Vec<u8>) {
        self.rt.block_on(
            Mock::given(method("GET"))
                .and(path(at))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
                .mount(&self.server),
        );
    }

    fn reset(&self) {
        self.rt.block_on(self.server.reset());
    }

    /// A store list offering `demo`, signed by `signer`.
    fn serve_store(&self, keys: &Keys, signer: &str, revoked: &[&str]) {
        let store = serde_json::json!({
            "version": 1,
            "tiers": {},
            "plugins": [{
                "name": "demo",
                "repo": REPO,
                "pubkey": keys.plugin_public,
                "category": "examples",
                "revoked": revoked,
            }]
        });
        let json = serde_json::to_vec_pretty(&store).unwrap();
        let sig = package::sign(&json, signer).unwrap();
        self.serve("/store/store.json", json);
        self.serve("/store/store.json.sig", sig.into_bytes());
    }

    /// A store list offering nothing.
    fn serve_empty_store(&self, keys: &Keys) {
        let json = br#"{ "version": 1, "plugins": [] }"#.to_vec();
        let sig = package::sign(&json, &keys.store_secret).unwrap();
        self.serve("/store/store.json", json);
        self.serve("/store/store.json.sig", sig.into_bytes());
    }

    fn serve_release(&self, r: &Release) {
        self.serve_release_at(&format!("/{REPO}/releases/latest/download/"), r);
    }

    /// The three release files under `folder` (starting and ending in `/`).
    fn serve_release_at(&self, folder: &str, r: &Release) {
        let at = |file: &str| format!("{folder}{file}");
        self.serve(&at(RELEASE_FILE), r.json.clone());
        self.serve(&at(SIGNATURE_FILE), r.sig.clone().into_bytes());
        self.serve(&at(ARCHIVE_FILE), r.archive.clone());
    }
}

type Fired = Arc<Mutex<Vec<(String, String)>>>;

struct Harness {
    store: StoreProvider,
    fired: Fired,
    plugins: tempfile::TempDir,
    data: tempfile::TempDir,
}

fn harness(server: &Server, keys: &Keys) -> Harness {
    register_test_auditor();
    let plugins = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let mut store = StoreProvider::new()
        .with_sources(
            http::http_fetch(),
            &format!("{}/store/", server.uri()),
            &server.uri(),
            &[keys.store_public.as_str()],
            plugins.path().to_path_buf(),
        )
        .with_data_dir(data.path().to_path_buf());
    let fired: Fired = Arc::default();
    let sink = fired.clone();
    store.set_apply_callback(Box::new(move |k, v| {
        sink.lock().unwrap().push((k.to_owned(), v.to_owned()));
    }));
    Harness {
        store,
        fired,
        plugins,
        data,
    }
}

impl Harness {
    /// Tick until the running job is done.
    fn settle(&mut self) {
        let start = Instant::now();
        while self.store.is_working() {
            self.store.tick();
            assert!(start.elapsed() < Duration::from_secs(20), "the Store hung");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Open `programs` and wait for the list: the lines of the list itself.
    fn open_programs(&mut self) -> Vec<String> {
        self.store.set_current_path("/");
        self.store.push_path("programs");
        self.store.fetch();
        self.settle();
        lines(self.store.fetch())
    }

    /// The lines inside `demo`'s entry.
    fn entry(&mut self) -> Vec<String> {
        self.store.set_current_path("/programs/demo");
        let out = lines(self.store.fetch());
        self.store.set_current_path("/programs");
        out
    }

    fn press(&mut self, button: &str) {
        self.store.on_button_press(button);
        self.settle();
    }

    fn fired(&self) -> Vec<(String, String)> {
        self.fired.lock().unwrap().clone()
    }

    fn installed_version(&self) -> Option<String> {
        install::installed(self.plugins.path())
            .get("demo")
            .and_then(|i| i.manifest.version.clone())
    }

    /// Nothing may linger in staging after an install, whatever its outcome.
    fn assert_staging_clean(&self) {
        let staging = self.plugins.path().join(install::STAGING_DIR);
        let left: Vec<_> = std::fs::read_dir(&staging)
            .map(|d| d.flatten().map(|e| e.file_name()).collect())
            .unwrap_or_default();
        assert!(left.is_empty(), "left in staging: {left:?}");
    }
}

/// Every element as text: a string as is, an object by its key.
fn lines(elements: Vec<FfonElement>) -> Vec<String> {
    elements
        .into_iter()
        .map(|e| match e {
            FfonElement::Str(s) => s,
            FfonElement::Obj(o) => o.key,
        })
        .collect()
}

fn has(lines: &[String], needle: &str) -> bool {
    lines.iter().any(|l| l.contains(needle))
}

fn t_with(key: &str, args: &[(&str, &str)]) -> String {
    let mut a = localize::Args::new();
    for (k, v) in args {
        a.set(*k, v.to_string());
    }
    localize::t_args(key, &a)
}

#[test]
fn a_listed_plugin_installs_after_its_access_is_shown() {
    let (server, keys) = (Server::start(), keys());
    server.serve_store(&keys, &keys.store_secret, &[]);
    server.serve_release(&release(
        &keys,
        "1.0.0",
        r#""allowedHosts": ["example.com"]"#,
    ));
    let mut h = harness(&server, &keys);

    let list = h.open_programs();
    let not_installed = localize::t("store-state-not-installed");
    assert!(has(&list, &format!("demo, {not_installed}")), "{list:?}");

    let entry = h.entry();
    assert!(
        has(
            &entry,
            &t_with("store-access-hosts", &[("list", "example.com")])
        ),
        "{entry:?}"
    );
    assert!(has(&entry, "<button>install:demo</button>"), "{entry:?}");

    h.press("install:demo");
    assert_eq!(h.installed_version().as_deref(), Some("1.0.0"));
    assert!(h.plugins.path().join("demo/plugin.wasm").is_file());
    assert_eq!(h.fired(), vec![(PLUGIN_INSTALLED.into(), "demo".into())]);
    h.assert_staging_clean();

    // The list now says so, and the entry offers only to uninstall.
    let list = lines(h.store.fetch());
    let installed = t_with("store-state-installed", &[("version", "1.0.0")]);
    assert!(has(&list, &format!("demo, {installed}")), "{list:?}");
    let entry = h.entry();
    assert!(!has(&entry, "<button>install:"), "{entry:?}");
    assert!(has(&entry, "<button>uninstall:demo</button>"), "{entry:?}");
}

#[test]
fn a_store_list_signed_by_an_unknown_key_is_not_believed() {
    let (server, keys) = (Server::start(), keys());
    let (stranger, _) = package::generate_keypair().unwrap();
    server.serve_store(&keys, &stranger, &[]);
    server.serve_release(&release(&keys, "1.0.0", ""));
    let mut h = harness(&server, &keys);

    // The test keys do not verify the compiled-in copy either, so nothing is
    // offered at all.
    let list = h.open_programs();
    assert!(!has(&list, "demo"), "{list:?}");
    assert!(
        list.iter().any(|l| l.contains("no trusted store key")),
        "{list:?}"
    );
}

#[test]
fn a_release_signed_by_another_key_is_not_offered() {
    let (server, keys) = (Server::start(), keys());
    let mut forged = release(&keys, "1.0.0", "");
    let (other, _) = package::generate_keypair().unwrap();
    forged.sig = package::sign(&forged.json, &other).unwrap();
    server.serve_store(&keys, &keys.store_secret, &[]);
    server.serve_release(&forged);
    let mut h = harness(&server, &keys);

    h.open_programs();
    let entry = h.entry();
    assert!(!has(&entry, "<button>install:"), "{entry:?}");
    assert!(has(&entry, "signature"), "{entry:?}");

    // Pressing it anyway does nothing.
    h.press("install:demo");
    assert_eq!(h.installed_version(), None);
    assert!(h.fired().is_empty());
}

#[test]
fn a_tampered_archive_is_refused_and_nothing_is_installed() {
    let (server, keys) = (Server::start(), keys());
    let mut r = release(&keys, "1.0.0", "");
    r.archive.push(0);
    server.serve_store(&keys, &keys.store_secret, &[]);
    server.serve_release(&r);
    let mut h = harness(&server, &keys);

    h.open_programs();
    h.press("install:demo");
    assert_eq!(h.installed_version(), None);
    assert!(h.fired().is_empty());
    assert!(has(&h.entry(), "SHA-256"), "{:?}", h.entry());
    h.assert_staging_clean();
}

#[test]
fn an_archive_asking_for_more_than_its_release_says_is_refused() {
    let (server, keys) = (Server::start(), keys());
    // release.json (signed) says no access, the plugin.json inside says a host.
    let r = release_with(
        &keys,
        &plugin_json("1.0.0", "", "0.1.0"),
        Some(&plugin_json(
            "1.0.0",
            r#""allowedHosts": ["evil.example"]"#,
            "0.1.0",
        )),
    );
    server.serve_store(&keys, &keys.store_secret, &[]);
    server.serve_release(&r);
    let mut h = harness(&server, &keys);

    h.open_programs();
    h.press("install:demo");
    assert_eq!(h.installed_version(), None);
    assert!(h.fired().is_empty());
    assert!(has(&h.entry(), "permissions"), "{:?}", h.entry());
    h.assert_staging_clean();
}

#[test]
fn a_release_published_after_it_was_shown_must_be_shown_again() {
    let (server, keys) = (Server::start(), keys());
    server.serve_store(&keys, &keys.store_secret, &[]);
    server.serve_release(&release(&keys, "1.0.0", ""));
    let mut h = harness(&server, &keys);
    h.open_programs();

    // A new release, asking for a host, appears before the user presses Install.
    server.reset();
    server.serve_store(&keys, &keys.store_secret, &[]);
    server.serve_release(&release(
        &keys,
        "1.1.0",
        r#""allowedHosts": ["example.com"]"#,
    ));
    h.press("install:demo");
    assert_eq!(h.installed_version(), None);
    assert!(h.fired().is_empty());

    // Checking again shows it, and then it installs.
    h.press("refresh");
    let entry = h.entry();
    assert!(
        has(
            &entry,
            &t_with("store-access-hosts", &[("list", "example.com")])
        ),
        "{entry:?}"
    );
    h.press("install:demo");
    assert_eq!(h.installed_version().as_deref(), Some("1.1.0"));
}

#[test]
fn an_update_asking_for_more_access_says_so_and_needs_the_press() {
    let (server, keys) = (Server::start(), keys());
    server.serve_store(&keys, &keys.store_secret, &[]);
    server.serve_release(&release(
        &keys,
        "1.0.0",
        r#""allowedHosts": ["example.com"]"#,
    ));
    let mut h = harness(&server, &keys);
    h.open_programs();
    h.press("install:demo");
    assert_eq!(h.installed_version().as_deref(), Some("1.0.0"));

    server.reset();
    server.serve_store(&keys, &keys.store_secret, &[]);
    server.serve_release(&release(
        &keys,
        "1.1.0",
        r#""allowedHosts": ["example.com", "second.example"]"#,
    ));
    h.press("refresh");
    let list = lines(h.store.fetch());
    let update = t_with(
        "store-state-update",
        &[("version", "1.0.0"), ("new", "1.1.0")],
    );
    assert!(has(&list, &format!("demo, {update}")), "{list:?}");
    let entry = h.entry();
    assert!(has(&entry, &localize::t("store-more-access")), "{entry:?}");
    assert!(
        has(
            &entry,
            &format!(
                "<button>update:demo</button>{}",
                t_with("store-approve-update", &[("version", "1.1.0")])
            )
        ),
        "{entry:?}"
    );
    // Nothing changed on disk until the press.
    assert_eq!(h.installed_version().as_deref(), Some("1.0.0"));

    h.press("update:demo");
    assert_eq!(h.installed_version().as_deref(), Some("1.1.0"));
    assert_eq!(
        h.fired(),
        vec![
            (PLUGIN_INSTALLED.into(), "demo".into()),
            (PLUGIN_UPDATED.into(), "demo".into())
        ]
    );
    h.assert_staging_clean();
}

#[test]
fn an_update_asking_for_less_is_offered_plainly() {
    let (server, keys) = (Server::start(), keys());
    server.serve_store(&keys, &keys.store_secret, &[]);
    server.serve_release(&release(
        &keys,
        "1.0.0",
        r#""allowedHosts": ["example.com"]"#,
    ));
    let mut h = harness(&server, &keys);
    h.open_programs();
    h.press("install:demo");

    server.reset();
    server.serve_store(&keys, &keys.store_secret, &[]);
    server.serve_release(&release(&keys, "1.1.0", ""));
    h.press("refresh");
    let entry = h.entry();
    assert!(!has(&entry, &localize::t("store-more-access")), "{entry:?}");
    assert!(
        has(
            &entry,
            &format!(
                "<button>update:demo</button>{}",
                t_with("store-update", &[("version", "1.1.0")])
            )
        ),
        "{entry:?}"
    );
}

#[test]
fn a_revoked_release_cannot_be_installed() {
    let (server, keys) = (Server::start(), keys());
    let r = release(&keys, "1.0.0", "");
    server.serve_store(&keys, &keys.store_secret, &[r.info.archive_sha256.as_str()]);
    server.serve_release(&r);
    let mut h = harness(&server, &keys);

    h.open_programs();
    let entry = h.entry();
    assert!(has(&entry, &localize::t("store-revoked")), "{entry:?}");
    assert!(!has(&entry, "<button>install:"), "{entry:?}");

    // Even when asked directly, the install path refuses it.
    let offer = h.store.offers[0].clone();
    let err = install::install(
        &h.store.fetch,
        offer.source.as_ref().unwrap(),
        offer.release.as_ref().unwrap(),
        h.plugins.path(),
    )
    .unwrap_err();
    assert!(err.contains("withdrawn"), "{err}");
    assert_eq!(h.installed_version(), None);
}

#[test]
fn an_older_release_never_replaces_a_newer_install() {
    let (server, keys) = (Server::start(), keys());
    server.serve_store(&keys, &keys.store_secret, &[]);
    server.serve_release(&release(&keys, "1.0.0", ""));
    let mut h = harness(&server, &keys);
    let dir = h.plugins.path().join("demo");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("plugin.json"), plugin_json("2.0.0", "", "0.1.0")).unwrap();

    h.open_programs();
    let entry = h.entry();
    assert!(!has(&entry, "<button>update:"), "{entry:?}");
    assert!(!has(&entry, "<button>install:"), "{entry:?}");

    let offer = h.store.offers[0].clone();
    let err = install::install(
        &h.store.fetch,
        offer.source.as_ref().unwrap(),
        offer.release.as_ref().unwrap(),
        h.plugins.path(),
    )
    .unwrap_err();
    assert!(err.contains("not newer"), "{err}");
    assert_eq!(h.installed_version().as_deref(), Some("2.0.0"));
}

#[test]
fn a_release_for_a_newer_sicompass_is_shown_but_not_installable() {
    let (server, keys) = (Server::start(), keys());
    server.serve_store(&keys, &keys.store_secret, &[]);
    server.serve_release(&release_with(
        &keys,
        &plugin_json("1.0.0", "", "99.0.0"),
        None,
    ));
    let mut h = harness(&server, &keys);

    h.open_programs();
    let entry = h.entry();
    assert!(
        has(&entry, &t_with("store-needs-app", &[("version", "99.0.0")])),
        "{entry:?}"
    );
    assert!(!has(&entry, "<button>install:"), "{entry:?}");
}

#[test]
fn uninstall_removes_the_program_and_tells_the_app() {
    let (server, keys) = (Server::start(), keys());
    server.serve_store(&keys, &keys.store_secret, &[]);
    server.serve_release(&release(&keys, "1.0.0", r#""storage": true"#));
    let mut h = harness(&server, &keys);
    // Something else in the plugins folder must survive.
    let neighbour = h.plugins.path().join("neighbour");
    std::fs::create_dir_all(&neighbour).unwrap();
    std::fs::write(neighbour.join("keep.txt"), "x").unwrap();

    h.open_programs();
    assert!(has(&h.entry(), &localize::t("store-access-own-folder")));
    h.press("install:demo");
    h.press("uninstall:demo");

    assert!(!h.plugins.path().join("demo").exists());
    assert!(neighbour.join("keep.txt").is_file());
    assert_eq!(
        h.fired(),
        vec![
            (PLUGIN_INSTALLED.into(), "demo".into()),
            (PLUGIN_REMOVED.into(), "demo".into())
        ]
    );
    let entry = h.entry();
    assert!(
        has(&entry, &t_with("store-uninstalled", &[("name", "demo")])),
        "{entry:?}"
    );
    assert!(has(&entry, "<button>install:demo</button>"), "{entry:?}");
    h.assert_staging_clean();
}

#[test]
fn the_root_does_not_touch_the_network() {
    let (server, keys) = (Server::start(), keys());
    let mut h = harness(&server, &keys);
    let root = lines(h.store.fetch());
    assert_eq!(
        root,
        vec![localize::t("store-programs"), localize::t("store-tiers")]
    );
    assert!(!h.store.is_working());
    let received = server.rt.block_on(server.server.received_requests());
    assert_eq!(received.map(|r| r.len()), Some(0));
}

/// Notes and project management used to come with the app. A user who had
/// them has their data folder but not the plugin, and the Store is where they
/// learn that installing opens it again: at the top, before anything loads.
#[test]
fn the_root_points_out_programs_whose_data_is_here_without_the_network() {
    let (server, keys) = (Server::start(), keys());
    let mut h = harness(&server, &keys);
    std::fs::create_dir_all(h.data.path().join("notes")).unwrap();
    std::fs::write(h.data.path().join("notes/0001"), "Groceries").unwrap();

    let root = lines(h.store.fetch());
    assert!(root[0].contains("notes"), "{root:?}");
    assert_eq!(
        root[1..],
        [localize::t("store-programs"), localize::t("store-tiers")]
    );
    // Only the one with data: project management was never used here.
    assert!(!root[0].contains("projectmanagement"), "{root:?}");
    let received = server.rt.block_on(server.server.received_requests());
    assert_eq!(received.map(|r| r.len()), Some(0));
}

/// An empty data folder is nothing to point out.
#[test]
fn an_empty_data_folder_is_not_pointed_out() {
    let (server, keys) = (Server::start(), keys());
    let mut h = harness(&server, &keys);
    std::fs::create_dir_all(h.data.path().join("notes")).unwrap();
    let root = lines(h.store.fetch());
    assert_eq!(
        root,
        vec![localize::t("store-programs"), localize::t("store-tiers")]
    );
}

/// In the list, the program with data here says so in its row, comes first,
/// and says inside that installing opens the data again. Once installed, the
/// Store stops pointing.
#[test]
fn a_program_whose_data_is_here_says_so_until_it_is_installed() {
    let (server, keys) = (Server::start(), keys());
    server.serve_store(&keys, &keys.store_secret, &[]);
    server.serve_release(&release(&keys, "1.0.0", r#""storage": true"#));
    let mut h = harness(&server, &keys);
    std::fs::create_dir_all(h.data.path().join("demo")).unwrap();
    std::fs::write(h.data.path().join("demo/0001"), "kept").unwrap();

    let list = h.open_programs();
    let waiting = localize::t("store-state-data-waiting");
    assert!(has(&list, &format!("demo, {waiting}")), "{list:?}");
    let entry = h.entry();
    let path = h.data.path().join("demo").display().to_string();
    assert!(
        has(&entry, &t_with("store-data-waiting", &[("path", &path)])),
        "{entry:?}"
    );
    // Never offered for the trash: nothing was uninstalled.
    assert!(!has(&entry, "<button>trashdata:"), "{entry:?}");
    h.store.set_current_path("/");
    assert!(lines(h.store.fetch())[0].contains("demo"));

    h.store.set_current_path("/programs");
    h.press("install:demo");
    assert_eq!(h.installed_version().as_deref(), Some("1.0.0"));
    h.store.set_current_path("/");
    assert_eq!(
        lines(h.store.fetch()),
        vec![localize::t("store-programs"), localize::t("store-tiers")]
    );
}

#[test]
fn every_locale_has_every_key() {
    let keys_of = |src: &str| -> std::collections::BTreeSet<String> {
        src.lines()
            .filter_map(|l| l.split_once(" = ").map(|(k, _)| k.trim().to_owned()))
            .filter(|k| !k.starts_with('#'))
            .collect()
    };
    let en = keys_of(include_str!("../locales/en-US.ftl"));
    for (lang, src) in [
        ("nl-BE", include_str!("../locales/nl-BE.ftl")),
        ("fr-BE", include_str!("../locales/fr-BE.ftl")),
        ("de-BE", include_str!("../locales/de-BE.ftl")),
    ] {
        assert_eq!(keys_of(src), en, "{lang}");
    }
}

#[test]
fn a_component_that_fails_the_import_audit_is_not_installed() {
    let (server, keys) = (Server::start(), keys());
    let mut wasm = COMPONENT.to_vec();
    wasm.extend_from_slice(FORBIDDEN);
    server.serve_store(&keys, &keys.store_secret, &[]);
    server.serve_release(&release_full(
        &keys,
        &plugin_json("1.0.0", "", "0.1.0"),
        None,
        &wasm,
    ));
    let mut h = harness(&server, &keys);

    h.open_programs();
    h.press("install:demo");
    assert_eq!(h.installed_version(), None);
    assert!(h.fired().is_empty());
    assert!(
        has(&h.entry(), "permissions do not grant"),
        "{:?}",
        h.entry()
    );
    h.assert_staging_clean();
}

/// Put `demo` in the plugins folder the way a user installing by hand would.
fn install_by_hand(h: &Harness, manifest: &str) {
    let dir = h.plugins.path().join("demo");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("plugin.json"), manifest).unwrap();
    std::fs::write(dir.join("plugin.wasm"), COMPONENT).unwrap();
}

#[test]
fn a_plugin_installed_by_hand_updates_from_its_update_address() {
    let (server, keys) = (Server::start(), keys());
    server.serve_empty_store(&keys);
    let trust = format!(
        r#", "updateUrl": "{}/hand/demo", "pubkey": "{}""#,
        server.uri(),
        keys.plugin_public
    );
    server.serve_release_at(
        "/hand/demo/",
        &release_with(&keys, &plugin_json_with("1.1.0", "", "0.1.0", &trust), None),
    );
    let mut h = harness(&server, &keys);
    install_by_hand(&h, &plugin_json_with("1.0.0", "", "0.1.0", &trust));

    let list = h.open_programs();
    let update = t_with(
        "store-state-update",
        &[("version", "1.0.0"), ("new", "1.1.0")],
    );
    assert!(has(&list, &format!("demo, {update}")), "{list:?}");
    let entry = h.entry();
    assert!(
        has(
            &entry,
            &t_with(
                "store-by-hand",
                &[("url", &format!("{}/hand/demo/", server.uri()))]
            )
        ),
        "{entry:?}"
    );
    h.press("update:demo");
    assert_eq!(h.installed_version().as_deref(), Some("1.1.0"));
    assert_eq!(h.fired(), vec![(PLUGIN_UPDATED.into(), "demo".into())]);
}

#[test]
fn an_update_of_a_hand_install_signed_with_a_new_key_is_refused() {
    let (server, keys) = (Server::start(), keys());
    server.serve_empty_store(&keys);
    let installed_with = format!(
        r#", "updateUrl": "{}/hand/demo", "pubkey": "{}""#,
        server.uri(),
        keys.plugin_public
    );
    // Signed with the installed key, but naming another one for the future.
    let (_, other_public) = package::generate_keypair().unwrap();
    let rotating = format!(
        r#", "updateUrl": "{}/hand/demo", "pubkey": "{other_public}""#,
        server.uri()
    );
    server.serve_release_at(
        "/hand/demo/",
        &release_with(
            &keys,
            &plugin_json_with("1.1.0", "", "0.1.0", &rotating),
            None,
        ),
    );
    let mut h = harness(&server, &keys);
    install_by_hand(&h, &plugin_json_with("1.0.0", "", "0.1.0", &installed_with));

    h.open_programs();
    h.press("update:demo");
    assert_eq!(h.installed_version().as_deref(), Some("1.0.0"));
    assert!(h.fired().is_empty());
    assert!(has(&h.entry(), "another signing key"), "{:?}", h.entry());
    h.assert_staging_clean();
}

#[test]
fn a_hand_install_without_an_update_address_can_only_be_uninstalled() {
    let (server, keys) = (Server::start(), keys());
    server.serve_empty_store(&keys);
    let mut h = harness(&server, &keys);
    install_by_hand(&h, &plugin_json("1.0.0", "", "0.1.0"));

    let list = h.open_programs();
    assert!(has(&list, "demo, "), "{list:?}");
    let entry = h.entry();
    assert!(
        has(&entry, &localize::t("store-by-hand-no-updates")),
        "{entry:?}"
    );
    assert!(!has(&entry, "<button>update:"), "{entry:?}");
    assert!(has(&entry, "<button>uninstall:demo</button>"), "{entry:?}");

    h.press("uninstall:demo");
    assert_eq!(h.installed_version(), None);
    assert_eq!(h.fired(), vec![(PLUGIN_REMOVED.into(), "demo".into())]);
}

#[test]
fn after_an_uninstall_the_data_folder_is_offered_for_the_trash_never_by_default() {
    let (server, keys) = (Server::start(), keys());
    server.serve_store(&keys, &keys.store_secret, &[]);
    server.serve_release(&release(&keys, "1.0.0", r#""storage": true"#));
    let mut h = harness(&server, &keys);
    let data = h.data.path().join("demo");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("notes.txt"), "mine").unwrap();

    h.open_programs();
    h.press("install:demo");
    // While installed, the data folder is not offered at all.
    assert!(!has(&h.entry(), "<button>trashdata:"), "{:?}", h.entry());
    // Nor may it be asked for.
    h.press("trashdata:demo");
    assert_eq!(h.fired(), vec![(PLUGIN_INSTALLED.into(), "demo".into())]);

    h.press("uninstall:demo");
    // Uninstalling leaves the data where it is, and offers it separately.
    assert!(data.join("notes.txt").is_file());
    let entry = h.entry();
    assert!(
        has(
            &entry,
            &t_with("store-data-kept", &[("path", &data.display().to_string())])
        ),
        "{entry:?}"
    );
    assert!(has(&entry, "<button>trashdata:demo</button>"), "{entry:?}");

    // The Store asks the app, whose trash is the guarded one.
    h.press("trashdata:demo");
    assert_eq!(
        h.fired().last(),
        Some(&(PLUGIN_DATA_TRASH.to_owned(), "demo".to_owned()))
    );
    std::fs::remove_dir_all(&data).unwrap(); // what the app does
    let entry = h.entry();
    assert!(has(&entry, &localize::t("store-data-trashed")), "{entry:?}");
    assert!(!has(&entry, "<button>trashdata:"), "{entry:?}");
}

#[test]
fn a_data_folder_a_built_in_program_shares_is_never_offered() {
    sicompass_sdk::register_builtin_manifest(sicompass_sdk::BuiltinManifest::new(
        "sharedname",
        "shared name",
    ));
    let mut store = StoreProvider::new().with_data_dir(std::env::temp_dir());
    store.uninstalled.insert("sharedname".to_owned());
    store.uninstalled.insert("other".to_owned());
    assert!(store.data_folder("sharedname").is_none());
    assert!(store.data_folder("other").is_some());
}

#[test]
fn any_server_is_said_plainly_and_never_as_a_star() {
    let (server, keys) = (Server::start(), keys());
    server.serve_store(&keys, &keys.store_secret, &[]);
    server.serve_release(&release(&keys, "1.0.0", r#""allowedHosts": ["*"]"#));
    let mut h = harness(&server, &keys);
    h.open_programs();
    let entry = h.entry();
    assert!(
        has(&entry, &localize::t("store-access-any-server")),
        "{entry:?}"
    );
    assert!(
        !entry.iter().any(|l| l.contains("connects to *")),
        "{entry:?}"
    );
}
