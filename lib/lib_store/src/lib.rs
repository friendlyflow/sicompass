//! The Store: the built-in program that installs, updates and removes plugins.
//!
//! It is always present, like Settings and the tutorial, and it is not a plugin
//! itself because it is what installs plugins. Its list comes from the signed
//! store list ([`source`]), and each plugin's version, access and service from
//! that plugin's signed `release.json` ([`install`]). The design is
//! `docs/plugin-platform.md` §8 and §9.
//!
//! Network work runs on one worker thread at a time and is picked up in
//! [`Provider::tick`]. The app is told about an install, update or removal
//! through the apply callback, the same queue Settings uses (the SDK boundary
//! forbids a direct call): `pluginInstalled`, `pluginUpdated` or
//! `pluginRemoved`, with the plugin's name as the value. The app then rescans,
//! records the approval, and loads or unloads the plugin, with no restart.
//!
//! Pressing Install or Update is the approval: the entry lists the access the
//! release asks for, and only that exact release is installed.
//!
//! Plugins installed by hand are listed too. One with an `updateUrl` and a
//! `pubkey` in its `plugin.json` updates here in the same release format,
//! trusting the key it was installed with. After an uninstall the entry offers,
//! separately and never by default, to move the plugin's data folder to the
//! trash; the app does that (`pluginDataTrash`), with its guarded trash.

pub mod http;
pub mod payments;
pub mod install;
pub mod source;
pub mod tiers;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::mpsc;

use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::localize;
use sicompass_sdk::package::ReleaseInfo;
use sicompass_sdk::provider::Provider;
use sicompass_sdk::store::StoreEntry;

use crate::http::Fetch;
use crate::install::{Installed, Source};
use crate::source::Loaded;

/// Register this crate's translation bundles with the SDK localizer.
/// Idempotent.
pub fn register_translations() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let _ = localize::register_bundle("en-US", include_str!("../locales/en-US.ftl"));
        let _ = localize::register_bundle("nl-BE", include_str!("../locales/nl-BE.ftl"));
        let _ = localize::register_bundle("fr-BE", include_str!("../locales/fr-BE.ftl"));
        let _ = localize::register_bundle("de-BE", include_str!("../locales/de-BE.ftl"));
    });
}

/// The apply-callback keys the app reacts to. The value is the plugin name.
pub const PLUGIN_INSTALLED: &str = "pluginInstalled";
pub const PLUGIN_UPDATED: &str = "pluginUpdated";
pub const PLUGIN_REMOVED: &str = "pluginRemoved";
pub const PLUGIN_DATA_TRASH: &str = "pluginDataTrash";

type ApplyFn = Box<dyn Fn(&str, &str) + Send + 'static>;

/// A plugin the Store shows, with what its latest release says.
#[derive(Debug, Clone)]
struct Offer {
    name: String,
    /// Where its releases come from. `None` for a plugin installed by hand
    /// without an update address.
    source: Option<Source>,
    /// Its line in the store list, when it is listed there.
    entry: Option<StoreEntry>,
    release: Result<ReleaseInfo, String>,
}

impl Offer {
    fn from_source(fetch: &Fetch, source: Source, entry: Option<StoreEntry>) -> Self {
        Offer {
            name: source.name.clone(),
            release: install::fetch_release(fetch, &source),
            source: Some(source),
            entry,
        }
    }

    fn by_hand(fetch: &Fetch, m: &sicompass_sdk::plugin_manifest::PluginManifest) -> Self {
        match Source::by_hand(m) {
            Some(Ok(source)) => Offer::from_source(fetch, source, None),
            Some(Err(e)) => Offer {
                name: m.name.clone(),
                source: None,
                entry: None,
                release: Err(e),
            },
            None => Offer {
                name: m.name.clone(),
                source: None,
                entry: None,
                release: Err(String::new()),
            },
        }
    }
}

enum Done {
    Loaded {
        store: Result<Loaded, String>,
        offers: Vec<Offer>,
    },
    Installed {
        name: String,
        update: bool,
        result: Result<String, String>,
    },
}

enum Job {
    Loading,
    Installing(String),
}

/// The programs that came with sicompass until 0.2.0 and are plugins in the
/// store now, with the settings section each one had (its `displayName`).
const CAME_WITH_THE_APP: [(&str, &str); 4] = [
    ("filebrowser", "file browser"),
    ("texteditor", "text editor"),
    ("notes", "notes"),
    ("projectmanagement", "project management"),
];

pub struct StoreProvider {
    current_path: String,
    fetch: Fetch,
    store_url: String,
    releases_url: String,
    trusted: Vec<String>,
    plugins_dir: Option<PathBuf>,
    /// Where plugins keep their data (`app_data_dir()`), to offer the folder
    /// of an uninstalled one for the trash.
    data_dir: Option<PathBuf>,

    loaded: Option<Result<Loaded, String>>,
    offers: Vec<Offer>,
    job: Option<(Job, mpsc::Receiver<Done>)>,
    /// The last result per plugin, shown in its entry.
    notes: HashMap<String, String>,
    /// Uninstalled in this session: their entries offer the data folder.
    uninstalled: HashSet<String>,
    /// Data folders the app was asked to move to the trash.
    trash_asked: HashSet<String>,
    apply_fn: Option<ApplyFn>,
    announcement: Option<String>,
    refresh: bool,
    /// Store > tiers.
    tiers: tiers::Tiers,
}

impl Default for StoreProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl StoreProvider {
    pub fn new() -> Self {
        register_translations();
        StoreProvider {
            current_path: "/".to_owned(),
            fetch: http::http_fetch(),
            store_url: source::STORE_URL.to_owned(),
            releases_url: install::RELEASES_URL.to_owned(),
            trusted: source::TRUSTED_KEYS
                .iter()
                .map(|k| (*k).to_owned())
                .collect(),
            plugins_dir: sicompass_sdk::platform::plugins_dir(),
            data_dir: sicompass_sdk::platform::app_data_dir(),
            loaded: None,
            offers: Vec::new(),
            job: None,
            notes: HashMap::new(),
            uninstalled: HashSet::new(),
            trash_asked: HashSet::new(),
            apply_fn: None,
            announcement: None,
            refresh: false,
            tiers: tiers::Tiers::new(http::http_fetch()),
        }
    }

    /// Point the Store somewhere else: a test server, other keys, another
    /// plugins folder.
    pub fn with_sources(
        mut self,
        fetch: Fetch,
        store_url: &str,
        releases_url: &str,
        trusted: &[&str],
        plugins_dir: PathBuf,
    ) -> Self {
        self.fetch = fetch;
        self.store_url = store_url.to_owned();
        self.releases_url = releases_url.to_owned();
        self.trusted = trusted.iter().map(|k| (*k).to_owned()).collect();
        self.plugins_dir = Some(plugins_dir);
        self.tiers = tiers::Tiers::new(self.fetch.clone());
        self
    }

    /// Keep the Store's settings (server URL, redeem tokens) in another
    /// `settings.json` (tests).
    pub fn with_settings_path(mut self, path: PathBuf) -> Self {
        self.tiers.set_settings_path(path);
        self
    }

    /// The tiers section, for tests.
    pub fn tiers_mut(&mut self) -> &mut tiers::Tiers {
        &mut self.tiers
    }

    /// Whether `segment` (the first one of a path) is the tiers section.
    fn is_tiers(segment: &str) -> bool {
        segment == localize::t("store-tiers")
    }

    /// Tell the app about Store settings the tiers section changed.
    fn announce_tier_settings(&mut self) {
        for (k, v) in self.tiers.take_changed() {
            self.fire(&k, &v);
        }
    }

    /// Point the data-folder check somewhere else (tests).
    pub fn with_data_dir(mut self, data_dir: PathBuf) -> Self {
        self.data_dir = Some(data_dir);
        self
    }

    /// Whether a load, install or update is running.
    pub fn is_working(&self) -> bool {
        self.job.is_some()
    }

    fn installed(&self) -> BTreeMap<String, Installed> {
        self.plugins_dir
            .as_deref()
            .map(install::installed)
            .unwrap_or_default()
    }

    fn fire(&self, key: &str, value: &str) {
        if let Some(f) = &self.apply_fn {
            f(key, value);
        }
    }

    fn start_load(&mut self) {
        if self.job.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let fetch = self.fetch.clone();
        let store_url = self.store_url.clone();
        let releases_url = self.releases_url.clone();
        let trusted = self.trusted.clone();
        let plugins_dir = self.plugins_dir.clone();
        spawn("store:load", move || {
            let keys: Vec<&str> = trusted.iter().map(String::as_str).collect();
            let store = source::load(&fetch, &store_url, &keys);
            let mut offers: Vec<Offer> = match &store {
                Ok(loaded) => loaded
                    .store
                    .plugins
                    .iter()
                    .map(|entry| {
                        Offer::from_source(
                            &fetch,
                            Source::listed(entry, &releases_url),
                            Some(entry.clone()),
                        )
                    })
                    .collect(),
                Err(_) => Vec::new(),
            };
            // Everything else in the plugins folder was installed by hand.
            if let Some(dir) = &plugins_dir {
                for (name, i) in install::installed(dir) {
                    if !offers.iter().any(|o| o.name == name) {
                        offers.push(Offer::by_hand(&fetch, &i.manifest));
                    }
                }
            }
            let _ = tx.send(Done::Loaded { store, offers });
        });
        self.job = Some((Job::Loading, rx));
    }

    fn start_install(&mut self, name: &str, update: bool) {
        if self.job.is_some() {
            self.notes
                .insert(name.to_owned(), localize::t("store-busy"));
            return;
        }
        let Some(plugins_dir) = self.plugins_dir.clone() else {
            return;
        };
        let Some(offer) = self.offers.iter().find(|o| o.name == name) else {
            return;
        };
        let (Some(source), Ok(shown)) = (offer.source.clone(), offer.release.clone()) else {
            return;
        };
        let (tx, rx) = mpsc::channel();
        let fetch = self.fetch.clone();
        let name = name.to_owned();
        let job_name = name.clone();
        spawn("store:install", move || {
            let result = install::install(&fetch, &source, &shown, &plugins_dir)
                .map(|m| m.version.unwrap_or_default());
            let _ = tx.send(Done::Installed {
                name,
                update,
                result,
            });
        });
        self.notes.remove(&job_name);
        self.uninstalled.remove(&job_name);
        self.job = Some((Job::Installing(job_name), rx));
    }

    fn uninstall(&mut self, name: &str) {
        let Some(plugins_dir) = self.plugins_dir.clone() else {
            return;
        };
        let Some(current) = self.installed().remove(name) else {
            return;
        };
        let mut args = localize::Args::new();
        args.set("name", name.to_owned());
        match install::uninstall(&plugins_dir, &current) {
            Ok(()) => {
                self.fire(PLUGIN_REMOVED, name);
                self.uninstalled.insert(name.to_owned());
                let line = localize::t_args("store-uninstalled", &args);
                self.notes.insert(name.to_owned(), line.clone());
                self.announcement = Some(line);
            }
            Err(e) => {
                args.set("err", e);
                let line = localize::t_args("store-failed", &args);
                self.notes.insert(name.to_owned(), line.clone());
                self.announcement = Some(line);
            }
        }
        self.refresh = true;
    }

    /// An uninstalled plugin's data folder, when there is one to offer. Never
    /// one a built-in program of the same name also uses.
    fn data_folder(&self, name: &str) -> Option<PathBuf> {
        let shared = sicompass_sdk::builtin_manifests().iter().any(|m| {
            m.name == name || m.display_name == name || m.display_name.replace(' ', "") == name
        });
        if shared || !self.uninstalled.contains(name) {
            return None;
        }
        self.data_dir.as_ref().map(|d| d.join(name))
    }

    fn trash_data(&mut self, name: &str) {
        if self.installed().contains_key(name) {
            return;
        }
        if let Some(dir) = self.data_folder(name)
            && dir.is_dir()
        {
            self.fire(PLUGIN_DATA_TRASH, name);
            self.trash_asked.insert(name.to_owned());
        }
    }

    fn finish(&mut self, done: Done) {
        match done {
            Done::Loaded { store, offers } => {
                if let Ok(loaded) = &store {
                    remember_issuers(&loaded.store);
                }
                self.offers = offers;
                self.loaded = Some(store);
            }
            Done::Installed {
                name,
                update,
                result,
            } => {
                let mut args = localize::Args::new();
                args.set("name", name.clone());
                let line = match result {
                    Ok(version) => {
                        self.fire(
                            if update {
                                PLUGIN_UPDATED
                            } else {
                                PLUGIN_INSTALLED
                            },
                            &name,
                        );
                        args.set("version", version);
                        localize::t_args(
                            if update {
                                "store-updated"
                            } else {
                                "store-installed"
                            },
                            &args,
                        )
                    }
                    Err(e) => {
                        args.set("err", e);
                        localize::t_args("store-failed", &args)
                    }
                };
                self.notes.insert(name, line.clone());
                self.announcement = Some(line);
            }
        }
        self.refresh = true;
    }

    // ---- The tree -----------------------------------------------------------

    fn segments(&self) -> Vec<&str> {
        self.current_path
            .split('/')
            .filter(|s| !s.is_empty())
            .collect()
    }

    /// Whether a program's data folder is here and holds anything. Never for a
    /// built-in's name: that folder is the built-in's own.
    fn has_data(&self, name: &str) -> bool {
        let builtin = sicompass_sdk::builtin_manifests().iter().any(|m| {
            m.name == name || m.display_name == name || m.display_name.replace(' ', "") == name
        });
        !builtin
            && self
                .data_dir
                .as_ref()
                .and_then(|d| std::fs::read_dir(d.join(name)).ok())
                .is_some_and(|mut entries| entries.next().is_some())
    }

    /// Whether the user had `name` when it came with the app: its settings
    /// section is still in `settings.json`. The file browser and the text
    /// editor keep no data folder, so this is how an upgrade finds them.
    fn used_before(&self, name: &str) -> bool {
        let Some((_, section)) = CAME_WITH_THE_APP.iter().find(|(n, _)| *n == name) else {
            return false;
        };
        self.tiers
            .settings_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .is_some_and(|v| v.get(*section).is_some())
    }

    /// Worth pointing out: listed, not installed, and the user's data or old
    /// settings for it are here.
    fn waiting(&self, name: &str) -> bool {
        self.has_data(name) || self.used_before(name)
    }

    /// Listed programs that are not installed but that the user evidently had.
    ///
    /// Mostly the programs that came with the app until 0.2.0, on a machine
    /// that used them: their plugin keeps its data in the same folder and its
    /// settings in the same section, so installing it picks up where the user
    /// left off. This is how the user learns where they went.
    fn waiting_data(&self, installed: &BTreeMap<String, Installed>) -> Vec<String> {
        self.offers
            .iter()
            .filter(|o| o.entry.is_some() && !installed.contains_key(&o.name))
            .filter(|o| self.waiting(&o.name))
            .map(|o| o.name.clone())
            .collect()
    }

    /// The names the store lists, without the network: the loaded list, or
    /// before it is loaded the copy compiled into this version (a hint only,
    /// so its signature is not the point here, and nothing is installed from
    /// it).
    fn listed_names(&self) -> Vec<String> {
        match &self.loaded {
            Some(Ok(l)) => l.store.plugins.iter().map(|e| e.name.clone()).collect(),
            _ => serde_json::from_slice::<sicompass_sdk::store::Store>(source::COMPILED_STORE)
                .map(|s| s.plugins.into_iter().map(|e| e.name).collect())
                .unwrap_or_default(),
        }
    }

    /// The top of the Store. A listed program with data here but not
    /// installed is said above everything else. Still no network: the root
    /// is what every tab shows.
    fn root(&self) -> Vec<FfonElement> {
        let mut out = Vec::new();
        let installed = self.installed();
        let waiting: Vec<String> = self
            .listed_names()
            .into_iter()
            .filter(|n| !installed.contains_key(n) && self.waiting(n))
            .collect();
        if !waiting.is_empty() {
            let mut args = localize::Args::new();
            args.set("names", waiting.join(", "));
            out.push(FfonElement::new_str(localize::t_args(
                "store-data-waiting-root",
                &args,
            )));
        }
        out.push(FfonElement::new_obj(localize::t("store-programs")));
        out.push(FfonElement::new_obj(localize::t("store-tiers")));
        out
    }

    fn programs(&mut self) -> Vec<FfonElement> {
        if self.loaded.is_none() {
            self.start_load();
            return vec![FfonElement::new_str(localize::t("store-loading"))];
        }
        let check_again = FfonElement::new_str(format!(
            "<button>refresh</button>{}",
            localize::t("store-check-again")
        ));
        let mut out = Vec::new();
        match &self.loaded {
            Some(Err(e)) => {
                let mut args = localize::Args::new();
                args.set("err", e.clone());
                out.push(FfonElement::new_str(localize::t_args(
                    "store-load-failed",
                    &args,
                )));
            }
            Some(Ok(loaded)) if loaded.offline.is_some() => {
                out.push(FfonElement::new_str(localize::t("store-offline")));
            }
            _ => {}
        }
        let installed = self.installed();
        let waiting = self.waiting_data(&installed);
        // A plugin installed by hand is shown while it is installed, and after
        // an uninstall for as long as its data folder is on offer.
        let mut shown: Vec<&Offer> = self
            .offers
            .iter()
            .filter(|o| {
                o.entry.is_some()
                    || installed.contains_key(&o.name)
                    || self.uninstalled.contains(&o.name)
            })
            .collect();
        // Programs with data here but not installed come first, the ones the
        // user is most likely looking for. Stable, so the rest keep their order.
        shown.sort_by_key(|o| !waiting.contains(&o.name));
        if shown.is_empty() {
            out.push(FfonElement::new_str(localize::t("store-no-programs")));
        }
        for offer in shown {
            let mut obj = FfonElement::new_obj(self.offer_key(offer, &installed));
            for line in self.offer_lines(offer, installed.get(&offer.name)) {
                obj.as_obj_mut().unwrap().push(line);
            }
            out.push(obj);
        }
        if self.job.is_none() {
            out.push(check_again);
        }
        out
    }

    /// `notes, installed 0.2.0`: the name, then a comma, then the state, so the
    /// list says what is installed without entering each entry.
    fn offer_key(&self, offer: &Offer, installed: &BTreeMap<String, Installed>) -> String {
        let mut args = localize::Args::new();
        let state = match installed.get(&offer.name) {
            Some(i) => {
                args.set("version", i.manifest.version.clone().unwrap_or_default());
                match &offer.release {
                    Ok(r) if install::is_newer(r, &i.manifest) => {
                        args.set("new", r.version.clone());
                        localize::t_args("store-state-update", &args)
                    }
                    _ => localize::t_args("store-state-installed", &args),
                }
            }
            None if offer.entry.is_some() && self.has_data(&offer.name) => {
                localize::t("store-state-data-waiting")
            }
            None if offer.entry.is_some() && self.used_before(&offer.name) => {
                localize::t("store-state-used-before")
            }
            None => localize::t("store-state-not-installed"),
        };
        format!("{}, {state}", offer.name)
    }

    fn offer_lines(&self, offer: &Offer, installed: Option<&Installed>) -> Vec<FfonElement> {
        let name = &offer.name;
        let line = |key: &str, args: &[(&str, String)]| {
            let mut a = localize::Args::new();
            for (k, v) in args {
                a.set(*k, v.clone());
            }
            FfonElement::new_str(localize::t_args(key, &a))
        };
        let mut out = Vec::new();
        if let Some(note) = self.notes.get(name) {
            out.push(FfonElement::new_str(note.clone()));
        }
        if installed.is_none() {
            // Data here, from a built-in of an earlier version or an install
            // before this session: installing opens it again.
            if offer.entry.is_some()
                && self.data_folder(name).is_none()
                && self.has_data(name)
                && let Some(dir) = self.data_dir.as_ref().map(|d| d.join(name))
            {
                out.push(line(
                    "store-data-waiting",
                    &[("path", dir.display().to_string())],
                ));
            }
            if let Some(dir) = self.data_folder(name) {
                if dir.is_dir() {
                    out.push(line(
                        "store-data-kept",
                        &[("path", dir.display().to_string())],
                    ));
                    out.push(button("trashdata", name, localize::t("store-trash-data")));
                } else if self.trash_asked.contains(name) {
                    out.push(line("store-data-trashed", &[]));
                }
            }
            // A hand install cannot be installed again from here.
            if offer.entry.is_none() {
                return out;
            }
        }
        match (&offer.source, &offer.entry) {
            (Some(s), None) => out.push(line("store-by-hand", &[("url", s.folder.clone())])),
            (None, _) => {
                out.push(line("store-by-hand-no-updates", &[]));
                if let Err(e) = &offer.release
                    && !e.is_empty()
                {
                    out.push(line("store-release-unavailable", &[("err", e.clone())]));
                }
                out.push(button("uninstall", name, localize::t("store-uninstall")));
                return out;
            }
            _ => {}
        }
        if let Some(category) = offer.entry.as_ref().and_then(|e| e.category.as_ref()) {
            out.push(line("store-category", &[("category", category.clone())]));
        }
        let release = match &offer.release {
            Ok(r) => r,
            Err(e) => {
                out.push(line("store-release-unavailable", &[("err", e.clone())]));
                if installed.is_some() {
                    out.push(button("uninstall", name, localize::t("store-uninstall")));
                }
                return out;
            }
        };
        out.push(line(
            "store-version",
            &[("version", release.version.clone())],
        ));
        out.extend(access_lines(release).into_iter().map(FfonElement::new_str));
        if let Some(tier) = offer.entry.as_ref().and_then(|e| e.service.as_ref()) {
            let title = self
                .loaded
                .as_ref()
                .and_then(|l| l.as_ref().ok())
                .and_then(|l| l.store.tiers.get(tier))
                .map(|t| localize::t(&t.title))
                .unwrap_or_else(|| tier.clone());
            out.push(line("store-uses-service", &[("tier", title)]));
        }
        if offer.entry.as_ref().is_some_and(|e| e.paid_features) {
            out.push(line("store-paid-features", &[]));
        }

        let revoked = offer
            .entry
            .as_ref()
            .is_some_and(|e| e.is_revoked(&release.archive_sha256));
        let too_new = install::needs_newer_app(release);
        if revoked {
            out.push(line("store-revoked", &[]));
        }
        if let Some(min) = &too_new {
            out.push(line("store-needs-app", &[("version", min.clone())]));
        }
        let installable = !revoked && too_new.is_none();
        let working = matches!(&self.job, Some((Job::Installing(n), _)) if n == name);
        if working {
            out.push(line("store-working", &[]));
            return out;
        }
        match installed {
            None if installable => {
                out.push(button("install", name, localize::t("store-install")));
            }
            None => {}
            Some(current) => {
                if installable && install::is_newer(release, &current.manifest) {
                    let more = release.asks_for_more_than(&current.manifest);
                    if more {
                        out.push(line("store-more-access", &[]));
                    }
                    let label = localize::t_args(
                        if more {
                            "store-approve-update"
                        } else {
                            "store-update"
                        },
                        &{
                            let mut a = localize::Args::new();
                            a.set("version", release.version.clone());
                            a
                        },
                    );
                    out.push(button("update", name, label));
                }
                out.push(button("uninstall", name, localize::t("store-uninstall")));
            }
        }
        out
    }
}

fn button(action: &str, name: &str, label: String) -> FfonElement {
    FfonElement::new_str(format!("<button>{action}:{name}</button>{label}"))
}

/// One line per kind of access, in plain words, or one line saying there is none.
fn access_lines(release: &ReleaseInfo) -> Vec<String> {
    let p = &release.permissions;
    let mut out = Vec::new();
    let any_server = sicompass_sdk::plugin_abi::reaches_any_server(&release.allowed_hosts);
    if any_server {
        out.push(localize::t("store-access-any-server"));
    }
    let named: Vec<String> = release
        .allowed_hosts
        .iter()
        .filter(|h| h.trim() != sicompass_sdk::plugin_abi::ANY_SERVER)
        .cloned()
        .collect();
    let mut add = |key: &str, items: &[String]| {
        if !items.is_empty() {
            let mut a = localize::Args::new();
            a.set("list", items.join(", "));
            out.push(localize::t_args(key, &a));
        }
    };
    add("store-access-hosts", &named);
    add("store-access-files", &p.filesystem);
    add("store-access-programs", &p.process);
    add("store-access-sockets", &p.sockets);
    if out.is_empty() {
        out.push(localize::t(if p.storage {
            "store-access-own-folder"
        } else {
            "store-access-none"
        }));
    }
    out
}

fn spawn(name: &str, f: impl FnOnce() + Send + 'static) {
    if let Err(e) = std::thread::Builder::new().name(name.to_owned()).spawn(f) {
        eprintln!("sicompass: store: no worker thread: {e}");
    }
}

impl Provider for StoreProvider {
    fn name(&self) -> &str {
        "store"
    }

    fn display_name(&self) -> String {
        register_translations();
        localize::t("store-display-name")
    }

    fn fetch(&mut self) -> Vec<FfonElement> {
        let segments: Vec<String> = self.segments().into_iter().map(str::to_owned).collect();
        match segments.as_slice() {
            [] => self.root(),
            [first, rest @ ..] if Self::is_tiers(first) => self.tiers.fetch_at(rest),
            [_programs] => self.programs(),
            [_programs, entry] => {
                // The entry's key carries its state after the name, which changes
                // on install, so it is found by the name alone.
                let name = entry.split_once(", ").map_or(entry.as_str(), |(n, _)| n);
                let installed = self.installed();
                self.offers
                    .iter()
                    .find(|o| o.name == name)
                    .map(|o| self.offer_lines(o, installed.get(name)))
                    .unwrap_or_default()
            }
            _ => Vec::new(),
        }
    }

    fn push_path(&mut self, segment: &str) {
        if self.current_path == "/" {
            self.current_path = format!("/{segment}");
        } else {
            self.current_path.push('/');
            self.current_path.push_str(segment);
        }
    }

    fn pop_path(&mut self) {
        match self.current_path.rfind('/') {
            Some(0) | None => self.current_path = "/".to_owned(),
            Some(i) => self.current_path.truncate(i),
        }
    }

    fn current_path(&self) -> &str {
        &self.current_path
    }

    fn set_current_path(&mut self, path: &str) {
        self.current_path = path.to_owned();
    }

    fn on_button_press(&mut self, function_name: &str) {
        if self.tiers.on_button_press(function_name).is_some() {
            self.refresh = true;
            return;
        }
        match function_name.split_once(':') {
            Some(("install", name)) => self.start_install(name, false),
            Some(("update", name)) => self.start_install(name, true),
            Some(("uninstall", name)) => self.uninstall(name),
            Some(("trashdata", name)) => self.trash_data(name),
            _ if function_name == "refresh" && self.job.is_none() => {
                self.loaded = None;
                self.start_load();
            }
            _ => {}
        }
        self.refresh = true;
    }

    fn tick(&mut self) -> bool {
        if self.tiers.tick() {
            self.refresh = true;
            return true;
        }
        let Some((_, rx)) = &self.job else {
            return false;
        };
        match rx.try_recv() {
            Ok(done) => {
                self.job = None;
                self.finish(done);
                true
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
                // The worker died without an answer (a panic).
                self.job = None;
                if self.loaded.is_none() {
                    self.loaded = Some(Err("the store stopped unexpectedly".to_owned()));
                }
                self.refresh = true;
                true
            }
        }
    }

    fn set_apply_callback(&mut self, cb: Box<dyn Fn(&str, &str) + Send + 'static>) {
        self.apply_fn = Some(cb);
    }

    fn needs_refresh(&self) -> bool {
        self.refresh
    }

    fn clear_needs_refresh(&mut self) {
        self.refresh = false;
    }

    fn take_announcement(&mut self) -> Option<String> {
        self.announcement.take()
    }

    fn take_error(&mut self) -> Option<String> {
        self.tiers.take_error()
    }

    fn on_radio_change(&mut self, group: &str, value: &str) {
        self.tiers.on_radio_change(group, value);
    }

    /// Only the tiers section has inputs: the server URL and the ones on the
    /// tier pages. The app pushes the input's label as the last segment.
    fn commit_edit(&mut self, _old: &str, new: &str) -> bool {
        let segments: Vec<String> = self.segments().into_iter().map(str::to_owned).collect();
        let (Some(first), Some(label)) = (segments.first(), segments.last()) else {
            return false;
        };
        if !Self::is_tiers(first) || segments.len() < 2 {
            return false;
        }
        let handled = self.tiers.commit(label, new);
        self.announce_tier_settings();
        handled
    }

    fn on_setting_change(&mut self, key: &str, value: &str) {
        self.tiers.on_setting_change(key, value);
    }
}

/// Issuer keys of the tiers in the store list, for third-party tiers: the
/// compiled-in list at startup, then the live one once it is loaded.
static ISSUERS: std::sync::RwLock<BTreeMap<String, String>> =
    std::sync::RwLock::new(BTreeMap::new());

fn remember_issuers(store: &sicompass_sdk::store::Store) {
    if let Ok(mut issuers) = ISSUERS.write() {
        for (id, tier) in &store.tiers {
            issuers.insert(id.clone(), tier.issuer.clone());
        }
    }
}

/// What a plugin's `license.standing(tier)` hears: the user's certificates for
/// `tier`, verified against its issuer. Ours are always checked against the
/// built-in key; a store list cannot name another issuer for them.
pub fn license_standing(tier_id: &str) -> sicompass_sdk::license::Standing {
    use crate::payments::cert;
    use sicompass_sdk::license::{LicenseStatus, Standing};
    let issuer = cert::known_issuer(tier_id)
        .map(str::to_owned)
        .or_else(|| ISSUERS.read().ok().and_then(|i| i.get(tier_id).cloned()));
    let Some(issuer) = issuer else {
        return Standing::MISSING;
    };
    let (status, days) = match cert::tier_status(tier_id, &issuer) {
        cert::TierStatus::Active { renews_in_days, .. } => (LicenseStatus::Active, renews_in_days),
        cert::TierStatus::Grace { days_left, .. } => (LicenseStatus::Grace, days_left),
        cert::TierStatus::Expired {
            expired_days_ago, ..
        } => (LicenseStatus::Expired, expired_days_ago),
        cert::TierStatus::Missing => (LicenseStatus::Missing, 0),
    };
    Standing {
        status,
        days: i32::try_from(days).unwrap_or(i32::MAX),
    }
}

/// [`license_standing`] without the days: `license.status(tier)`.
pub fn license_status(tier_id: &str) -> sicompass_sdk::license::LicenseStatus {
    license_standing(tier_id).status
}

/// The redeem token for one of our tiers, which the host gives a plugin only
/// for the tier its manifest names as its service. Cloud and Commercial share
/// the licence token (one certificate slot); a third party's tiers have none
/// here yet.
pub fn license_token(tier_id: &str) -> Option<String> {
    use crate::payments::cert::tier;
    let token = match tier_id {
        t if t == tier::CLOUD || t == tier::COMMERCIAL => {
            crate::payments::config::redeem_token()
        }
        t if t == tier::SUPPORT => crate::payments::config::support_redeem_token(),
        _ => return None,
    };
    (!token.is_empty()).then_some(token)
}

/// Register the Store with the SDK: always present, never in "Available
/// programs:".
pub fn register() {
    register_translations();
    let keys: Vec<&str> = source::TRUSTED_KEYS.to_vec();
    if let Ok(store) = sicompass_sdk::store::verify_store(
        source::COMPILED_STORE,
        source::COMPILED_STORE_SIGNATURE,
        &keys,
    ) {
        remember_issuers(&store);
    }
    sicompass_sdk::license::register_checker(license_standing);
    sicompass_sdk::license::register_token_source(license_token);
    sicompass_sdk::register_provider_factory("store", || Box::new(StoreProvider::new()));
    sicompass_sdk::register_builtin_manifest(
        sicompass_sdk::BuiltinManifest::new("store", "store").always_enabled(),
    );
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_tiers;
