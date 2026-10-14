//! Store > publish: a developer's submissions, against a mock license server,
//! a mock GitHub and a signed store list.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::json;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;
use crate::payments::cert::TierStatus;

type Opened = Arc<Mutex<Vec<String>>>;

struct P {
    store: StoreProvider,
    rt: tokio::runtime::Runtime,
    server: MockServer,
    opened: Opened,
    _plugins: tempfile::TempDir,
}

fn active() -> TierStatus {
    TierStatus::Active {
        licensee: "Acme".into(),
        renews_in_days: 212,
    }
}

fn key(byte: u8) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode([byte; 32])
}

/// A Store whose publish section sees `standing` and `token`, talking to one
/// mock server for the store list, the license server and GitHub.
fn publish_with(standing: TierStatus, token: &str) -> P {
    crate::payments::config::_set_test_no_persist(true);
    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = rt.block_on(MockServer::start());
    let plugins = tempfile::tempdir().unwrap();
    let (store_secret, store_public) = sicompass_sdk::package::generate_keypair().unwrap();

    // `acme` listed, `oldthing` frozen; `waiting` and `declined` not listed.
    let list = json!({
        "version": 1,
        "plugins": [
            { "name": "acme", "repo": "acme-dev/acme", "pubkey": key(1) },
            { "name": "oldthing", "repo": "acme-dev/old", "pubkey": key(2), "frozen": "v1.0.0" },
        ]
    });
    let body = serde_json::to_vec_pretty(&list).unwrap();
    let sig = sicompass_sdk::package::sign(&body, &store_secret).unwrap();
    let get = |at: &str, body: Vec<u8>| {
        rt.block_on(
            Mock::given(method("GET"))
                .and(path(at))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
                .mount(&server),
        )
    };
    get("/store/store.json", body);
    get("/store/store.json.sig", sig.into_bytes());

    let opened: Opened = Arc::default();
    let sink = opened.clone();
    let token = token.to_owned();
    let publish = publish::Publish::new(http::http_fetch()).with_hooks(
        Arc::new(move || standing.clone()),
        Arc::new(move || token.clone()),
        Arc::new(move |url: &str| {
            sink.lock().unwrap().push(url.to_owned());
            Ok(())
        }),
        &server.uri(),
    );
    let settings = plugins.path().join("settings.json");
    let mut store = StoreProvider::new()
        .with_sources(
            http::http_fetch(),
            &format!("{}/store/", server.uri()),
            &server.uri(),
            &[store_public.as_str()],
            plugins.path().join("plugins"),
        )
        .with_settings_path(settings)
        .with_publish(publish);
    store.tiers_mut().set_store_url(&server.uri());
    P {
        store,
        rt,
        server,
        opened,
        _plugins: plugins,
    }
}

impl P {
    fn publish_path(&self) -> String {
        format!("/{}", localize::t("store-publish"))
    }

    fn form_path(&self) -> String {
        format!(
            "{}/{}",
            self.publish_path(),
            localize::t("store-publish-form")
        )
    }

    fn yours_path(&self) -> String {
        format!(
            "{}/{}",
            self.publish_path(),
            localize::t("store-publish-yours")
        )
    }

    fn at(&mut self, path: &str) -> Vec<String> {
        self.store.set_current_path(path);
        self.store
            .fetch()
            .into_iter()
            .map(|e| match e {
                FfonElement::Str(s) => s,
                FfonElement::Obj(o) => o.key,
            })
            .collect()
    }

    /// Fetch `path` until nothing is loading any more, and return what it shows.
    fn settled_at(&mut self, path: &str) -> Vec<String> {
        let start = Instant::now();
        loop {
            let lines = self.at(path);
            self.store.tick();
            if !self.store.publish_mut().is_loading() && self.store.job.is_none() {
                let again = self.at(path);
                if !self.store.publish_mut().is_loading() {
                    return again;
                }
            }
            assert!(
                start.elapsed() < Duration::from_secs(20),
                "hung at {path}: {lines:?}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn type_in(&mut self, label_key: &str, value: &str) {
        let at = format!("{}/{}", self.form_path(), localize::t(label_key));
        self.store.set_current_path(&at);
        assert!(self.store.commit_edit("", value), "{label_key} not taken");
    }

    fn press(&mut self, button: &str) {
        self.store.set_current_path(&self.form_path());
        self.store.on_button_press(button);
    }

    fn serve_mine(&self, mine: serde_json::Value) {
        self.rt.block_on(
            Mock::given(method("GET"))
                .and(path("/developer/mine"))
                .and(header("authorization", "Bearer tok"))
                .respond_with(ResponseTemplate::new(200).set_body_json(mine))
                .mount(&self.server),
        );
    }

    fn serve_issue(&self, n: i64, state: &str) {
        self.rt.block_on(
            Mock::given(method("GET"))
                .and(path(format!(
                    "/repos/{}/issues/{n}",
                    publish::SUBMISSIONS_REPO
                )))
                .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "state": state })))
                .mount(&self.server),
        );
    }

    fn submissions_sent(&self) -> Vec<wiremock::Request> {
        self.rt
            .block_on(self.server.received_requests())
            .unwrap()
            .into_iter()
            .filter(|r| r.method.as_str() == "POST")
            .collect()
    }
}

fn has(lines: &[String], needle: &str) -> bool {
    lines.iter().any(|l| l.contains(needle))
}

#[test]
fn publish_follows_tiers_at_the_root() {
    let mut p = publish_with(active(), "tok");
    let root = p.at("/");
    let tiers = root
        .iter()
        .position(|l| *l == localize::t("store-tiers"))
        .unwrap();
    assert_eq!(root[tiers + 1], localize::t("store-publish"));
}

#[test]
fn without_a_tier_the_form_is_hidden_and_tiers_is_pointed_to() {
    let mut p = publish_with(TierStatus::Missing, "");
    let list = p.at(&p.publish_path());
    assert_eq!(list[0], localize::t("store-publish-needs"));
    assert!(!has(&list, &localize::t("store-publish-form")), "{list:?}");
    assert!(!has(&list, &localize::t("store-publish-yours")), "{list:?}");
    assert!(has(&list, &localize::t("store-publish-how")), "{list:?}");

    // Even pressed directly, nothing is sent.
    p.press("publish-submit");
    assert!(p.submissions_sent().is_empty());
}

#[test]
fn an_active_tier_says_when_it_renews_and_shows_the_form() {
    let mut p = publish_with(active(), "tok");
    let list = p.at(&p.publish_path());
    assert!(list[0].contains("212"), "{list:?}");
    assert!(has(&list, &localize::t("store-publish-form")), "{list:?}");
    assert!(has(&list, &localize::t("store-publish-yours")), "{list:?}");

    let form = p.at(&p.form_path());
    for label in [
        "store-publish-label-name",
        "store-publish-label-repo",
        "store-publish-label-key",
    ] {
        assert!(
            has(&form, &format!("{}: <input>", localize::t(label))),
            "{form:?}"
        );
    }
    assert!(has(&form, "<button>publish-submit</button>"), "{form:?}");
}

#[test]
fn in_grace_the_days_left_are_said() {
    let mut p = publish_with(
        TierStatus::Grace {
            licensee: "Acme".into(),
            days_left: 9,
        },
        "tok",
    );
    let list = p.at(&p.publish_path());
    assert!(list[0].contains('9'), "{list:?}");
    assert!(has(&list, &localize::t("store-publish-form")), "{list:?}");
}

#[test]
fn a_malformed_submission_is_named_and_not_sent() {
    let mut p = publish_with(active(), "tok");
    p.type_in("store-publish-label-name", "acme");
    p.type_in("store-publish-label-repo", "no-slash");
    p.type_in("store-publish-label-key", &key(1));
    p.press("publish-submit");
    let form = p.settled_at(&p.form_path());
    assert!(has(&form, "owner/name"), "{form:?}");
    assert!(p.submissions_sent().is_empty());
}

#[test]
fn a_submission_is_sent_with_the_token_and_said_to_be_sent() {
    let mut p = publish_with(active(), "tok");
    p.rt.block_on(
        Mock::given(method("POST"))
            .and(path("/developer/plugins"))
            .and(header("authorization", "Bearer tok"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "plugin": { "name": "newone", "repo": "acme-dev/newone", "pubkey": key(3), "issue": 9 }
            })))
            .expect(1)
            .mount(&p.server),
    );
    p.type_in("store-publish-label-name", "newone");
    p.type_in("store-publish-label-repo", "acme-dev/newone");
    p.type_in("store-publish-label-key", &key(3));
    p.press("publish-submit");
    let form = p.settled_at(&p.form_path());
    assert!(
        has(
            &form,
            &format!(
                "{}: <input>newone</input>",
                localize::t("store-publish-label-name")
            )
        ),
        "{form:?}"
    );
    let sent_line = {
        let mut a = localize::Args::new();
        a.set("name", "newone");
        localize::t_args("store-publish-sent", &a)
    };
    assert!(has(&form, &sent_line), "{form:?}");

    let sent = p.submissions_sent();
    assert_eq!(sent.len(), 1);
    let body: serde_json::Value = serde_json::from_slice(&sent[0].body).unwrap();
    assert_eq!(
        body,
        json!({ "name": "newone", "repo": "acme-dev/newone", "pubkey": key(3) })
    );
}

#[test]
fn the_servers_refusal_is_shown_as_it_is() {
    let mut p = publish_with(active(), "tok");
    p.rt.block_on(
        Mock::given(method("POST"))
            .and(path("/developer/plugins"))
            .respond_with(
                ResponseTemplate::new(409)
                    .set_body_string("The name `acme` is already submitted by another developer."),
            )
            .mount(&p.server),
    );
    p.type_in("store-publish-label-name", "acme");
    p.type_in("store-publish-label-repo", "someone/acme");
    p.type_in("store-publish-label-key", &key(1));
    p.press("publish-submit");
    let form = p.settled_at(&p.form_path());
    assert!(
        has(&form, "already submitted by another developer"),
        "{form:?}"
    );
}

#[test]
fn your_plugins_say_listed_paused_waiting_or_not_accepted() {
    let mut p = publish_with(active(), "tok");
    p.serve_mine(json!({ "plugins": [
        { "name": "acme", "repo": "acme-dev/acme", "pubkey": key(1), "submittedAt": 1, "issue": 3 },
        { "name": "declined", "repo": "acme-dev/declined", "pubkey": key(4), "submittedAt": 1, "issue": 6 },
        { "name": "oldthing", "repo": "acme-dev/old", "pubkey": key(2), "submittedAt": 1, "issue": 4 },
        { "name": "waiting", "repo": "acme-dev/waiting", "pubkey": key(5), "submittedAt": 1, "issue": 5 },
    ]}));
    p.serve_issue(5, "open");
    p.serve_issue(6, "closed");

    let yours = p.yours_path();
    let list = p.settled_at(&yours);
    let state = |k: &str| localize::t(k);
    assert_eq!(
        list,
        vec![
            format!("acme, {}", state("store-publish-state-listed")),
            format!("declined, {}", state("store-publish-state-declined")),
            format!("oldthing, {}", state("store-publish-state-paused")),
            format!("waiting, {}", state("store-publish-state-waiting")),
        ]
    );

    let old = p.at(&format!("{yours}/{}", list[2]));
    assert!(has(&old, &localize::t("store-publish-paused")), "{old:?}");
    assert!(has(&old, "acme-dev/old"), "{old:?}");

    let waiting = p.at(&format!("{yours}/{}", list[3]));
    assert!(
        has(&waiting, &localize::t("store-publish-waiting")),
        "{waiting:?}"
    );
    assert!(
        has(&waiting, "<button>publish-review:5</button>"),
        "{waiting:?}"
    );

    p.store.set_current_path(&format!("{yours}/{}", list[3]));
    p.store.on_button_press("publish-review:5");
    assert_eq!(
        *p.opened.lock().unwrap(),
        vec![format!(
            "https://github.com/{}/issues/5",
            publish::SUBMISSIONS_REPO
        )]
    );
}

#[test]
fn nothing_submitted_yet_is_said() {
    let mut p = publish_with(active(), "tok");
    p.serve_mine(json!({ "plugins": [] }));
    let list = p.settled_at(&p.yours_path());
    assert_eq!(list, vec![localize::t("store-publish-none")]);
}
