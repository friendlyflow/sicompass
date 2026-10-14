//! Store > publish: a developer submits their plugin for the store.
//!
//! Listing a plugin needs Sicompass Cloud or Sicompass Commercial, for free
//! and proprietary plugins alike (docs/plugin-platform.md §8). The section
//! shows where the user stands with that, the plugins they submitted and how
//! far each got, and a form to submit one.
//!
//! A submission goes to the license server (`POST /developer/plugins`, with
//! the Sicompass Cloud redeem token), which opens a review issue on GitHub and
//! so tells the maintainer. Listing it is the maintainer's signed change to
//! the store list (`/store onboard`), so "listed" is read from the store list
//! the Store already has, and "not accepted" from the review issue being
//! closed without it. When the developer's tier lapses, the maintainer freezes
//! the entry, and it shows here as "updates paused".
//!
//! Like Store > tiers, this is served from the Store's own tree, so a refresh
//! keeps the page and what was typed in it. Everything that touches the
//! network runs on a thread and comes back through [`Publish::tick`].

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use serde::Deserialize;
use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::localize;
use sicompass_sdk::store::Store;

use crate::http::Fetch;
use crate::payments::cert::{self, TierStatus, tier};

/// Where the review issues are opened. The server's
/// `SICOMPASS_SUBMISSIONS_REPO` default.
pub const SUBMISSIONS_REPO: &str = "friendlyflow/sicompass";

/// Buttons: send the form, and open a review issue in the browser.
const SUBMIT_BUTTON: &str = "publish-submit";
const REVIEW_BUTTON: &str = "publish-review:";

/// One plugin this developer submitted, as `GET /developer/mine` says.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Submission {
    pub name: String,
    pub repo: String,
    pub pubkey: String,
    #[serde(default)]
    pub issue: Option<i64>,
}

enum Msg {
    Mine(Result<Vec<Submission>, String>),
    Sent(Result<Submission, String>),
    /// Whether a review issue is closed.
    Issue(i64, Result<bool, String>),
}

/// Where the user stands with Sicompass Cloud (Commercial includes it).
pub type StandingFn = Arc<dyn Fn() -> TierStatus + Send + Sync>;
/// The Sicompass Cloud redeem token, or empty.
pub type TokenFn = Arc<dyn Fn() -> String + Send + Sync>;
/// Open a page in the user's browser.
pub type OpenFn = Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>;

pub struct Publish {
    /// For the review issues' state on GitHub (no token).
    fetch: Fetch,
    github_api: String,
    standing: StandingFn,
    token: TokenFn,
    open: OpenFn,
    tx: mpsc::Sender<Msg>,
    rx: mpsc::Receiver<Msg>,
    pending: usize,
    mine: Option<Result<Vec<Submission>, String>>,
    mine_asked: bool,
    /// Review issue number to whether it is closed.
    issues: HashMap<i64, bool>,
    issues_asked: HashSet<i64>,
    name: String,
    repo: String,
    pubkey: String,
    /// Under the submit button: sending, sent, or why not.
    status: Option<String>,
    error: Option<String>,
}

fn cloud_standing() -> TierStatus {
    match cert::known_issuer(tier::CLOUD) {
        Some(issuer) => cert::tier_status(tier::CLOUD, issuer),
        None => TierStatus::Missing,
    }
}

impl Publish {
    pub fn new(fetch: Fetch) -> Self {
        let (tx, rx) = mpsc::channel();
        Publish {
            fetch,
            github_api: "https://api.github.com".to_owned(),
            standing: Arc::new(cloud_standing),
            token: Arc::new(crate::payments::config::redeem_token),
            open: Arc::new(crate::payments::checkout::open_url),
            tx,
            rx,
            pending: 0,
            mine: None,
            mine_asked: false,
            issues: HashMap::new(),
            issues_asked: HashSet::new(),
            name: String::new(),
            repo: String::new(),
            pubkey: String::new(),
            status: None,
            error: None,
        }
    }

    /// Answer the tier, the token and the browser from elsewhere, and ask
    /// another GitHub (tests).
    pub fn with_hooks(
        mut self,
        standing: StandingFn,
        token: TokenFn,
        open: OpenFn,
        github_api: &str,
    ) -> Self {
        self.standing = standing;
        self.token = token;
        self.open = open;
        self.github_api = github_api.trim_end_matches('/').to_owned();
        self
    }

    pub fn is_loading(&self) -> bool {
        self.pending > 0
    }

    pub fn take_error(&mut self) -> Option<String> {
        self.error.take()
    }

    fn label(key: &str) -> String {
        localize::t(key)
    }

    fn input(key: &str, value: &str) -> FfonElement {
        FfonElement::new_str(format!("{}: <input>{value}</input>", Self::label(key)))
    }

    /// The first line: what listing needs, and where the user stands.
    fn standing_line(&self) -> String {
        let mut a = localize::Args::new();
        let key = match (self.standing)() {
            TierStatus::Active { renews_in_days, .. } => {
                a.set("days", renews_in_days);
                "store-publish-needs-active"
            }
            TierStatus::Grace { days_left, .. } => {
                a.set("days", days_left);
                "store-publish-needs-grace"
            }
            TierStatus::Expired { .. } | TierStatus::Missing => "store-publish-needs",
        };
        localize::t_args(key, &a)
    }

    fn tier_on(&self) -> bool {
        (self.standing)().is_on()
    }

    /// `publish` itself.
    fn list(&self) -> Vec<FfonElement> {
        let mut out = vec![FfonElement::new_str(self.standing_line())];
        if !(self.token)().trim().is_empty() {
            out.push(FfonElement::new_obj(Self::label("store-publish-yours")));
        }
        if self.tier_on() {
            out.push(FfonElement::new_obj(Self::label("store-publish-form")));
        }
        out.push(FfonElement::new_str(Self::label("store-publish-how")));
        out
    }

    /// Below `publish`. `listed` is the store list, when it is loaded.
    pub fn fetch_at(
        &mut self,
        segments: &[String],
        server: &str,
        listed: Option<&Store>,
    ) -> Vec<FfonElement> {
        match segments {
            [] => self.list(),
            [first, rest @ ..] if *first == Self::label("store-publish-yours") => {
                self.ask_mine(server);
                match rest {
                    [] => self.yours(listed),
                    [entry, ..] => {
                        let name = entry.split_once(", ").map_or(entry.as_str(), |(n, _)| n);
                        self.one_of_yours(name, listed)
                    }
                }
            }
            [first] if *first == Self::label("store-publish-form") => self.form(),
            _ => Vec::new(),
        }
    }

    fn form(&self) -> Vec<FfonElement> {
        if !self.tier_on() {
            return vec![FfonElement::new_str(self.standing_line())];
        }
        let mut out = vec![
            Self::input("store-publish-label-name", &self.name),
            // A label cannot hold the `/` of owner/name: it is a path segment.
            FfonElement::new_str(Self::label("store-publish-repo-hint")),
            Self::input("store-publish-label-repo", &self.repo),
            Self::input("store-publish-label-key", &self.pubkey),
            FfonElement::new_str(format!(
                "<button>{SUBMIT_BUTTON}</button>{}",
                Self::label("store-publish-send")
            )),
        ];
        if let Some(status) = &self.status {
            out.push(FfonElement::new_str(status.clone()));
        }
        out
    }

    fn entry_of<'a>(
        listed: Option<&'a Store>,
        name: &str,
    ) -> Option<&'a sicompass_sdk::store::StoreEntry> {
        listed.and_then(|s| s.entry(name))
    }

    /// Where one submission stands: listed (perhaps paused), not accepted, or
    /// waiting. `None` while that cannot be told yet.
    fn state_of(&self, s: &Submission, listed: Option<&Store>) -> Option<&'static str> {
        let store = listed?;
        Some(match store.entry(&s.name) {
            Some(e) if e.frozen.is_some() => "store-publish-state-paused",
            Some(_) => "store-publish-state-listed",
            None if s.issue.and_then(|n| self.issues.get(&n)) == Some(&true) => {
                "store-publish-state-declined"
            }
            None => "store-publish-state-waiting",
        })
    }

    fn key_of(&self, s: &Submission, listed: Option<&Store>) -> String {
        let state = self
            .state_of(s, listed)
            .unwrap_or("store-publish-state-checking");
        format!("{}, {}", s.name, localize::t(state))
    }

    fn yours(&mut self, listed: Option<&Store>) -> Vec<FfonElement> {
        let mine = match &self.mine {
            None => return vec![FfonElement::new_str(Self::label("store-publish-loading"))],
            Some(Err(e)) => {
                let mut a = localize::Args::new();
                a.set("err", e.clone());
                return vec![FfonElement::new_str(localize::t_args(
                    "store-publish-mine-failed",
                    &a,
                ))];
            }
            Some(Ok(mine)) => mine.clone(),
        };
        if mine.is_empty() {
            return vec![FfonElement::new_str(Self::label("store-publish-none"))];
        }
        // Whether a review that is not listed was declined is on GitHub.
        for s in &mine {
            if let Some(n) = s.issue
                && Self::entry_of(listed, &s.name).is_none()
            {
                self.ask_issue(n);
            }
        }
        mine.iter()
            .map(|s| FfonElement::new_obj(self.key_of(s, listed)))
            .collect()
    }

    fn one_of_yours(&mut self, name: &str, listed: Option<&Store>) -> Vec<FfonElement> {
        let Some(Ok(mine)) = &self.mine else {
            return Vec::new();
        };
        let Some(s) = mine.iter().find(|s| s.name == name).cloned() else {
            return Vec::new();
        };
        let line = |key: &str, k: &str, v: &str| {
            let mut a = localize::Args::new();
            a.set(k, v.to_owned());
            FfonElement::new_str(localize::t_args(key, &a))
        };
        let mut out = vec![
            line("store-publish-repo", "repo", &s.repo),
            line("store-publish-key", "key", &s.pubkey),
        ];
        match self.state_of(&s, listed) {
            Some("store-publish-state-paused") => {
                out.push(FfonElement::new_str(Self::label("store-publish-paused")))
            }
            Some("store-publish-state-declined") => {
                out.push(FfonElement::new_str(Self::label("store-publish-declined")))
            }
            Some("store-publish-state-waiting") => {
                out.push(FfonElement::new_str(Self::label("store-publish-waiting")))
            }
            _ => {}
        }
        if let Some(n) = s.issue {
            out.push(FfonElement::new_str(format!(
                "<button>{REVIEW_BUTTON}{n}</button>{}",
                Self::label("store-publish-open-review")
            )));
        }
        out
    }

    /// An `<input>` under `publish` was committed. Returns whether it was ours.
    pub fn commit(&mut self, label: &str, value: &str) -> bool {
        let value = value.trim().to_owned();
        if label == Self::label("store-publish-label-name") {
            self.name = value;
        } else if label == Self::label("store-publish-label-repo") {
            self.repo = value;
        } else if label == Self::label("store-publish-label-key") {
            self.pubkey = value;
        } else {
            return false;
        }
        // A new value is a new submission: the last answer no longer applies.
        self.status = None;
        true
    }

    /// A `<button>` under `publish`. `None` when it is not ours.
    pub fn on_button_press(&mut self, function_name: &str, server: &str) -> Option<()> {
        if function_name == SUBMIT_BUTTON {
            self.submit(server);
            return Some(());
        }
        let n = function_name.strip_prefix(REVIEW_BUTTON)?;
        let url = format!("https://github.com/{SUBMISSIONS_REPO}/issues/{n}");
        if let Err(e) = (self.open)(&url) {
            self.error = Some(e);
        }
        Some(())
    }

    fn submit(&mut self, server: &str) {
        if !self.tier_on() {
            self.status = Some(self.standing_line());
            return;
        }
        let token = (self.token)();
        if token.trim().is_empty() {
            self.status = Some(Self::label("store-publish-no-token"));
            return;
        }
        if let Err(e) =
            sicompass_sdk::store::check_entry_fields(&self.name, &self.repo, &self.pubkey)
        {
            self.status = Some(e);
            return;
        }
        self.status = Some(Self::label("store-publish-sending"));
        let body = serde_json::json!({
            "name": self.name,
            "repo": self.repo,
            "pubkey": self.pubkey,
        });
        let server = server.to_owned();
        self.spawn("store:publish", move || {
            Msg::Sent(post_submission(&server, token.trim(), &body))
        });
    }

    fn ask_mine(&mut self, server: &str) {
        if self.mine_asked {
            return;
        }
        let token = (self.token)();
        if token.trim().is_empty() {
            return;
        }
        self.mine_asked = true;
        let server = server.to_owned();
        self.spawn("store:mine", move || {
            Msg::Mine(get_mine(&server, token.trim()))
        });
    }

    fn ask_issue(&mut self, n: i64) {
        if !self.issues_asked.insert(n) {
            return;
        }
        let url = format!("{}/repos/{SUBMISSIONS_REPO}/issues/{n}", self.github_api);
        let fetch = self.fetch.clone();
        self.spawn("store:review", move || {
            let closed = fetch(&url).and_then(|body| {
                let v: serde_json::Value =
                    serde_json::from_slice(&body).map_err(|e| e.to_string())?;
                Ok(v["state"].as_str() == Some("closed"))
            });
            Msg::Issue(n, closed)
        });
    }

    fn spawn(&mut self, name: &str, work: impl FnOnce() -> Msg + Send + 'static) {
        let tx = self.tx.clone();
        let spawned = std::thread::Builder::new()
            .name(name.to_owned())
            .spawn(move || {
                let _ = tx.send(work());
            });
        if spawned.is_ok() {
            self.pending += 1;
        }
    }

    /// Pick up what the threads brought back. Returns whether something changed.
    pub fn tick(&mut self) -> bool {
        let mut changed = false;
        while let Ok(msg) = self.rx.try_recv() {
            self.pending = self.pending.saturating_sub(1);
            changed = true;
            match msg {
                Msg::Mine(result) => self.mine = Some(result),
                Msg::Sent(Ok(s)) => {
                    let mut a = localize::Args::new();
                    a.set("name", s.name.clone());
                    self.status = Some(localize::t_args("store-publish-sent", &a));
                    if let Some(Ok(mine)) = &mut self.mine {
                        mine.retain(|m| m.name != s.name);
                        mine.push(s);
                        mine.sort_by(|a, b| a.name.cmp(&b.name));
                    } else {
                        // Asked again the next time the list is opened.
                        self.mine = None;
                        self.mine_asked = false;
                    }
                }
                // The server's words, as they are: they say what to do.
                Msg::Sent(Err(e)) => self.status = Some(e),
                // Not knowing a review's state only leaves it "waiting".
                Msg::Issue(n, Ok(closed)) => {
                    self.issues.insert(n, closed);
                }
                Msg::Issue(_, Err(_)) => {}
            }
        }
        changed
    }
}

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(concat!("sicompass/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())
}

/// The reply's body when the server refused, which says why in words meant
/// for the user, or the status when it has none.
fn refusal(response: reqwest::blocking::Response) -> String {
    let status = response.status();
    let text = response.text().unwrap_or_default();
    let text = text.trim();
    if text.is_empty() {
        format!("The server answered {status}")
    } else {
        text.to_owned()
    }
}

/// `POST /developer/plugins`. Blocking: call it from a worker thread.
fn post_submission(
    server: &str,
    token: &str,
    body: &serde_json::Value,
) -> Result<Submission, String> {
    let url = format!("{}/developer/plugins", server.trim_end_matches('/'));
    let response = client()?
        .post(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .body(serde_json::to_vec(body).map_err(|e| e.to_string())?)
        .send()
        .map_err(|e| format!("Could not reach the server: {e}"))?;
    if !response.status().is_success() {
        return Err(refusal(response));
    }
    let v: serde_json::Value =
        serde_json::from_slice(&response.bytes().map_err(|e| e.to_string())?)
            .map_err(|e| format!("The server's answer does not parse: {e}"))?;
    serde_json::from_value(v["plugin"].clone())
        .map_err(|e| format!("The server's answer does not parse: {e}"))
}

/// `GET /developer/mine`. Blocking: call it from a worker thread.
fn get_mine(server: &str, token: &str) -> Result<Vec<Submission>, String> {
    let url = format!("{}/developer/mine", server.trim_end_matches('/'));
    let response = client()?
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/json")
        .send()
        .map_err(|e| format!("Could not reach the server: {e}"))?;
    if !response.status().is_success() {
        return Err(refusal(response));
    }
    let v: serde_json::Value =
        serde_json::from_slice(&response.bytes().map_err(|e| e.to_string())?)
            .map_err(|e| format!("The server's answer does not parse: {e}"))?;
    serde_json::from_value(v["plugins"].clone())
        .map_err(|e| format!("The server's answer does not parse: {e}"))
}
