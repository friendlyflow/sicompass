//! Store > tiers: the paid tiers, where the user stands with each, and their
//! pages. Moved here from Settings (docs/plugin-platform.md §9).
//!
//! Each tier row leads to a page the license server serves as FFON (`/sponsor`,
//! `/cloud`, `/commercial`, `/support`): the offer, the monthly or yearly
//! choice, the payment button and the redeem-token input. The Store fetches a
//! page once, on a worker thread, and serves it from its own tree at
//! `/tiers/<tier>`, rather than as a `<link>` the app grafts in. A grafted page
//! has no path of its own, so the refresh after redeeming a token would have
//! put the tiers list where the page was. Served from here, the page survives
//! the refresh with the typed token in it, and the row above it shows the new
//! status.
//!
//! The controls are handled by `crate::payments::tier_input::TierSession`.
//! The server URL and the redeem tokens are kept in `settings.json` under
//! `Store`, where `crate::payments::config` reads them. What the cloud backup
//! uses is asked of the server (`GET /usage`) once a session, when the list is
//! first shown, because the uploads are the plugins' own.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;

use crate::payments::cert::{self, TierStatus, tier};
use crate::payments::tier_input::TierSession;
use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::localize;
use sicompass_sdk::tags;

use crate::http::Fetch;

/// The pages, in the order they are listed: id (the server path), title, and
/// the tier whose status the row shows.
pub const PAGES: [(&str, &str, Option<&str>); 4] = [
    ("sponsor", "store-tier-sponsor", None),
    ("cloud", "store-tier-cloud", Some(tier::CLOUD)),
    (
        "commercial",
        "store-tier-commercial",
        Some(tier::COMMERCIAL),
    ),
    ("support", "store-tier-support", Some(tier::SUPPORT)),
];

/// The server-URL input's label, as rendered.
fn url_label() -> String {
    localize::t("store-label-server-url")
}

pub struct Tiers {
    session: TierSession,
    fetch: Fetch,
    /// `settings.json`, where the server URL and the redeem tokens are kept.
    settings_path: Option<PathBuf>,
    pages: HashMap<&'static str, Vec<FfonElement>>,
    loading: Option<(
        &'static str,
        mpsc::Receiver<Result<Vec<FfonElement>, String>>,
    )>,
    error: Option<String>,
    /// `GET /usage` in flight; it answers whether a report arrived.
    usage_loading: Option<mpsc::Receiver<bool>>,
    usage_asked: bool,
    /// Keys the app should hear about through the apply callback.
    changed: Vec<(String, String)>,
}

impl Tiers {
    pub fn new(fetch: Fetch) -> Self {
        Tiers {
            session: TierSession::new(),
            fetch,
            settings_path: sicompass_sdk::platform::main_config_path(),
            pages: HashMap::new(),
            loading: None,
            error: None,
            usage_loading: None,
            usage_asked: false,
            changed: Vec::new(),
        }
    }

    /// `settings.json`, which the Store also reads to see which programs the
    /// user had before they moved to the store.
    pub fn settings_path(&self) -> Option<&std::path::Path> {
        self.settings_path.as_deref()
    }

    pub fn set_settings_path(&mut self, path: PathBuf) {
        self.settings_path = Some(path);
    }

    pub fn set_store_url(&mut self, url: &str) {
        if self.session.store_url() != url {
            self.session.set_store_url(url);
            self.pages.clear();
        }
    }

    pub fn store_url(&self) -> &str {
        self.session.store_url()
    }

    pub fn is_loading(&self) -> bool {
        self.loading.is_some() || self.usage_loading.is_some()
    }

    pub fn take_error(&mut self) -> Option<String> {
        self.error.take()
    }

    /// Settings changes to announce, for the Store's apply callback.
    pub fn take_changed(&mut self) -> Vec<(String, String)> {
        std::mem::take(&mut self.changed)
    }

    fn status(tier_id: &str) -> TierStatus {
        match cert::known_issuer(tier_id) {
            Some(issuer) => cert::tier_status(tier_id, issuer),
            None => TierStatus::Missing,
        }
    }

    fn status_text(status: &TierStatus) -> String {
        let mut a = localize::Args::new();
        let key = match status {
            TierStatus::Missing => "store-tier-status-missing",
            TierStatus::Active {
                licensee,
                renews_in_days,
            } => {
                a.set("licensee", licensee.clone());
                a.set("days", *renews_in_days);
                "store-tier-status-active"
            }
            TierStatus::Grace { days_left, .. } => {
                a.set("days", *days_left);
                "store-tier-status-grace"
            }
            TierStatus::Expired {
                expired_days_ago, ..
            } => {
                a.set("days", *expired_days_ago);
                "store-tier-status-expired"
            }
        };
        localize::t_args(key, &a)
    }

    /// The row for one page: its title, then where the user stands.
    pub fn row_key(title_key: &str, tier_id: Option<&str>) -> String {
        let title = localize::t(title_key);
        match tier_id {
            Some(t) => format!("{title}, {}", Self::status_text(&Self::status(t))),
            None => title,
        }
    }

    /// `tiers` itself: the four rows, the cloud usage, the server URL.
    pub fn list(&self) -> Vec<FfonElement> {
        let mut out: Vec<FfonElement> = PAGES
            .iter()
            .map(|(_, title, tier_id)| FfonElement::new_obj(Self::row_key(title, *tier_id)))
            .collect();
        if let Some(usage) = crate::payments::usage::last() {
            out.extend(usage.lines().into_iter().map(FfonElement::new_str));
        }
        out.push(FfonElement::new_str(format!(
            "{}: <input>{}</input>",
            url_label(),
            self.session.store_url()
        )));
        out
    }

    /// The page a path segment names, by the title before its status.
    fn page_of(segment: &str) -> Option<&'static str> {
        let title = segment.split_once(", ").map_or(segment, |(t, _)| t);
        PAGES
            .iter()
            .find(|(_, key, _)| localize::t(key) == title)
            .map(|(id, _, _)| *id)
    }

    /// Below `tiers`: a page, or an object inside one.
    pub fn fetch_at(&mut self, segments: &[String]) -> Vec<FfonElement> {
        let Some((first, rest)) = segments.split_first() else {
            self.ask_usage();
            return self.list();
        };
        let Some(page) = Self::page_of(first) else {
            return Vec::new();
        };
        let Some(children) = self.pages.get(page) else {
            self.start_loading(page);
            return vec![FfonElement::new_str(localize::t("store-loading-page"))];
        };
        let mut level = children.clone();
        for segment in rest {
            let next = level.into_iter().find_map(|e| match e {
                FfonElement::Obj(o) if tags::strip_display(&o.key) == segment.as_str() => {
                    Some(o.children)
                }
                _ => None,
            });
            match next {
                Some(c) => level = c,
                None => return Vec::new(),
            }
        }
        level
    }

    fn start_loading(&mut self, page: &'static str) {
        if self.loading.is_some() {
            return;
        }
        let url = format!("{}/{page}", self.session.store_url().trim_end_matches('/'));
        let fetch = self.fetch.clone();
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("store:tier".into())
            .spawn(move || {
                let result = fetch(&url).and_then(|body| {
                    let text = String::from_utf8(body).map_err(|_| "not text".to_owned())?;
                    sicompass_sdk::ffon::parse_json(&text).map_err(|e| e.to_string())
                });
                let _ = tx.send(result);
            });
        if spawned.is_ok() {
            self.loading = Some((page, rx));
        }
    }

    /// Ask the server what the cloud backup uses, once a session, when there
    /// is a Sicompass Cloud token to ask with. A failure is not shown: the
    /// usage lines are information, and the last report stays on screen.
    fn ask_usage(&mut self) {
        if self.usage_asked {
            return;
        }
        self.usage_asked = true;
        let token = crate::payments::config::redeem_token();
        if token.is_empty() {
            return;
        }
        let server = self.session.store_url().to_owned();
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("store:usage".into())
            .spawn(move || {
                let _ = tx.send(crate::payments::usage::fetch(&server, &token).is_ok());
            });
        if spawned.is_ok() {
            self.usage_loading = Some(rx);
        }
    }

    /// Pick up a finished page load or usage report. Returns whether
    /// something changed.
    pub fn tick(&mut self) -> bool {
        let usage_arrived = match self.usage_loading.as_ref().map(|rx| rx.try_recv()) {
            Some(Ok(arrived)) => {
                self.usage_loading = None;
                arrived
            }
            Some(Err(mpsc::TryRecvError::Disconnected)) => {
                self.usage_loading = None;
                false
            }
            _ => false,
        };
        self.tick_page() || usage_arrived
    }

    fn tick_page(&mut self) -> bool {
        let Some((page, rx)) = &self.loading else {
            return false;
        };
        let page = *page;
        match rx.try_recv() {
            Ok(Ok(children)) => {
                self.pages.insert(page, children);
                self.loading = None;
                true
            }
            Ok(Err(e)) => {
                let mut a = localize::Args::new();
                a.set("err", e);
                self.error = Some(localize::t_args("store-tier-page-failed", &a));
                self.loading = None;
                true
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.loading = None;
                true
            }
        }
    }

    /// A `<button>` inside a page. `None` when it is not a tier button.
    pub fn on_button_press(&mut self, function_name: &str) -> Option<()> {
        match self.session.on_button_press(function_name)? {
            Ok(()) => {}
            Err(e) => self.error = Some(e),
        }
        Some(())
    }

    /// A `<radio>` inside a page changed: remember it for the checkout, and
    /// keep the cached page showing the choice.
    pub fn on_radio_change(&mut self, group: &str, value: &str) {
        self.session.on_radio_change(group, value);
        for page in self.pages.values_mut() {
            check_radio(page, group, value);
        }
    }

    /// An `<input>` under `tiers` was committed; `label` is its last path
    /// segment. Returns whether it was one of ours.
    pub fn commit(&mut self, label: &str, value: &str) -> bool {
        if label == url_label() || label == "Store server URL" {
            if self.session.store_url() != value {
                self.set_store_url(value);
                self.persist("storeUrl", value);
            }
            return true;
        }
        let Some(result) = self.session.commit_input(label, value) else {
            return false;
        };
        if let Err(e) = result {
            self.error = Some(e);
        }
        // Kept even when redeeming failed, as Settings did: the user may be
        // offline, and the token is also the cloud-backup credential.
        match label {
            "License redeem token" => self.persist("licenseRedeemToken", value.trim()),
            "Support redeem token" => self.persist("supportRedeemToken", value.trim()),
            _ => {}
        }
        for page in self.pages.values_mut() {
            fill_input(page, label, value);
        }
        true
    }

    /// Another provider (or undo) changed a Store setting.
    pub fn on_setting_change(&mut self, key: &str, value: &str) {
        if key == "storeUrl" {
            self.set_store_url(value);
        }
    }

    /// Write `Store/<key>` into settings.json, preserving everything else, and
    /// tell the app. A file that exists but does not parse is left alone.
    fn persist(&mut self, key: &str, value: &str) {
        self.changed.push((key.to_owned(), value.to_owned()));
        let Some(path) = &self.settings_path else {
            return;
        };
        let mut root = match std::fs::read_to_string(path) {
            Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
                Ok(serde_json::Value::Object(m)) => m,
                _ => return,
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Default::default(),
            Err(_) => return,
        };
        let section = root
            .entry("Store")
            .or_insert_with(|| serde_json::Value::Object(Default::default()));
        if let Some(m) = section.as_object_mut() {
            m.insert(key.to_owned(), serde_json::Value::String(value.to_owned()));
        }
        if let Some(dir) = path.parent() {
            sicompass_sdk::platform::make_dirs(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(&serde_json::Value::Object(root)) {
            sicompass_sdk::platform::atomic_write(path, &json);
        }
    }
}

/// Move `<checked>` to `value` in every `<radio>group` object of `elements`.
fn check_radio(elements: &mut [FfonElement], group: &str, value: &str) {
    for e in elements {
        if let FfonElement::Obj(o) = e {
            if o.key == format!("<radio>{group}") {
                for option in &mut o.children {
                    if let FfonElement::Str(s) = option {
                        let bare = s.strip_prefix("<checked>").unwrap_or(s).to_owned();
                        *s = if bare == value {
                            format!("<checked>{bare}")
                        } else {
                            bare
                        };
                    }
                }
            } else {
                check_radio(&mut o.children, group, value);
            }
        }
    }
}

/// Show `value` in every `label: <input>…</input>` line of `elements`.
fn fill_input(elements: &mut [FfonElement], label: &str, value: &str) {
    let prefix = format!("{label}: <input>");
    for e in elements {
        match e {
            FfonElement::Str(s) if s.starts_with(&prefix) => {
                *s = format!("{prefix}{value}</input>");
            }
            FfonElement::Obj(o) => fill_input(&mut o.children, label, value),
            _ => {}
        }
    }
}
