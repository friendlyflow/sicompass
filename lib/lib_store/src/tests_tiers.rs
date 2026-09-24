//! Store > tiers. The first nine tests are the Settings tier tests, moved here
//! with the tier pages (4.9) and checking the same things through the Store.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use super::*;

type Fired = Arc<Mutex<Vec<(String, String)>>>;

struct T {
    store: StoreProvider,
    fired: Fired,
    settings: tempfile::TempDir,
}

fn store(url: &str) -> T {
    sicompass_payments::config::_set_test_no_persist(true);
    let settings = tempfile::tempdir().unwrap();
    let mut store = StoreProvider::new().with_settings_path(settings.path().join("settings.json"));
    store.tiers_mut().set_store_url(url);
    let fired: Fired = Arc::default();
    let sink = fired.clone();
    store.set_apply_callback(Box::new(move |k, v| {
        sink.lock().unwrap().push((k.to_owned(), v.to_owned()));
    }));
    T {
        store,
        fired,
        settings,
    }
}

impl T {
    fn at(&mut self, path: &str) -> Vec<FfonElement> {
        self.store.set_current_path(path);
        self.store.fetch()
    }

    fn tiers_path(&self) -> String {
        format!("/{}", localize::t("store-tiers"))
    }

    fn saved(&self, key: &str) -> Option<String> {
        let text = std::fs::read_to_string(self.settings.path().join("settings.json")).ok()?;
        let v: serde_json::Value = serde_json::from_str(&text).ok()?;
        v["Store"][key].as_str().map(str::to_owned)
    }

    fn settle(&mut self) {
        let start = Instant::now();
        while self.store.tiers_mut().is_loading() {
            self.store.tick();
            assert!(start.elapsed() < Duration::from_secs(20), "hung");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

fn key(e: &FfonElement) -> String {
    match e {
        FfonElement::Str(s) => s.clone(),
        FfonElement::Obj(o) => o.key.clone(),
    }
}

fn mock_server() -> (tokio::runtime::Runtime, MockServer) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let server = rt.block_on(MockServer::start());
    (rt, server)
}

fn serve_json(
    rt: &tokio::runtime::Runtime,
    server: &MockServer,
    at: &str,
    body: serde_json::Value,
) {
    rt.block_on(
        Mock::given(method("GET"))
            .and(path(at))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(server),
    );
}

/// A well-formed certificate signed by a key that is not the production one,
/// which the embedded public key rejects. Redeeming it must yield a "rejected"
/// error rather than being saved.
fn sample_cert_json() -> serde_json::Value {
    serde_json::json!({
        "payload": {
            "product": "sicompass", "license_id": "id", "licensee": "Test",
            "scope": "commercial", "issued_at": 1_700_000_000_i64,
            "expires_at": 1_900_000_000_i64, "version_coverage": "*",
            "payment_provider": "polar"
        },
        "signature": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
    })
}

/// The server's cloud page, as `/cloud` serves it.
fn cloud_page() -> serde_json::Value {
    serde_json::json!([
        "Sicompass Cloud, monthly or yearly",
        "Lemonsqueezy setup: <input></input>",
        { "<radio>monthly or yearly": ["per month", "<checked>per year"] },
        "<button>checkout:cloud</button>for payment",
        "License redeem token: <input></input>"
    ])
}

// ---- Moved from lib_settings ------------------------------------------------

#[test]
fn the_tier_rows_lead_the_tiers_section() {
    let mut t = store("https://srv.example");
    let p = t.tiers_path();
    let items = t.at(&p);
    for (i, title) in [
        "store-tier-sponsor",
        "store-tier-cloud",
        "store-tier-commercial",
        "store-tier-support",
    ]
    .into_iter()
    .enumerate()
    {
        let FfonElement::Obj(o) = &items[i] else {
            panic!("tier row {i} is an Obj: {items:?}")
        };
        assert!(o.key.starts_with(&localize::t(title)), "{}", o.key);
    }
    // The server URL input follows the tier rows (and any usage lines).
    let last = key(items.last().unwrap());
    assert!(
        last.contains("<input>https://srv.example</input>"),
        "{last}"
    );
}

#[test]
fn cloud_commercial_and_support_rows_carry_a_status_sponsor_does_not() {
    let mut t = store("https://srv.example");
    let p = t.tiers_path();
    let items = t.at(&p);
    assert_eq!(key(&items[0]), localize::t("store-tier-sponsor"));
    for (i, title) in [
        (1, "store-tier-cloud"),
        (2, "store-tier-commercial"),
        (3, "store-tier-support"),
    ] {
        let k = key(&items[i]);
        assert!(k.starts_with(&format!("{}, ", localize::t(title))), "{k}");
        assert!(
            k.len() > localize::t(title).len() + 2,
            "status missing: {k}"
        );
    }
}

#[test]
fn commit_routes_the_server_url() {
    let mut t = store("https://srv.example");
    assert!(
        t.store
            .tiers_mut()
            .commit("Store server URL", "https://new.example")
    );
    assert_eq!(t.store.tiers_mut().store_url(), "https://new.example");
}

/// `commit_edit` must route a tier edit even though `current_path` carries
/// the *localized* section name and label.
#[test]
fn commit_edit_routes_under_the_localized_section_name() {
    let mut t = store("https://srv.example");
    let path = format!(
        "{}/{}",
        t.tiers_path(),
        localize::t("store-label-server-url")
    );
    t.store.set_current_path(&path);
    let ok = t.store.commit_edit("", "http://localhost:8787");
    assert!(ok, "edit under the localized section name must route");
    assert_eq!(t.store.tiers_mut().store_url(), "http://localhost:8787");
    // Kept where lib_payments reads it, and every provider is told.
    assert_eq!(
        t.saved("storeUrl").as_deref(),
        Some("http://localhost:8787")
    );
    assert!(
        t.fired
            .lock()
            .unwrap()
            .contains(&("storeUrl".to_owned(), "http://localhost:8787".to_owned()))
    );
}

#[test]
fn commit_captures_the_donation_amount() {
    let mut t = store("https://srv.example");
    assert!(t.store.tiers_mut().commit("amount in \u{20ac}", "25"));
}

#[test]
fn commit_accepts_provider_setup_inputs() {
    let mut t = store("https://srv.example");
    assert!(t.store.tiers_mut().commit("Paddle setup", "v-2"));
    // Unrecognised labels are rejected.
    assert!(!t.store.tiers_mut().commit("mystery field", "x"));
}

#[test]
fn a_radio_selection_on_a_page_is_kept() {
    let (rt, server) = mock_server();
    serve_json(&rt, &server, "/cloud", cloud_page());
    let mut t = store(&server.uri());
    let page = format!("{}/{}", t.tiers_path(), localize::t("store-tier-cloud"));
    t.at(&page);
    t.settle();

    t.store
        .on_radio_change("monthly or yearly", "per month");
    let items = t.at(&page);
    let FfonElement::Obj(radio) = &items[2] else {
        panic!("{items:?}")
    };
    assert_eq!(
        radio.children.iter().map(key).collect::<Vec<_>>(),
        vec![
            "<checked>per month".to_owned(),
            "per year".to_owned()
        ]
    );
}

#[test]
fn a_checkout_against_an_unreachable_server_sets_one_error() {
    let mut t = store("http://127.0.0.1:1"); // refused immediately
    t.store.on_button_press("checkout:support-annual");
    assert!(
        t.store.take_error().is_some(),
        "an unreachable server must set an error"
    );
    assert!(t.store.take_error().is_none(), "the error is consumed once");
}

#[test]
fn redeem_tokens_route_to_their_own_settings() {
    let (rt, server) = mock_server();
    serve_json(&rt, &server, "/license/tok-c", sample_cert_json());
    serve_json(&rt, &server, "/license/tok-s", sample_cert_json());
    let mut t = store(&server.uri());

    assert!(t.store.tiers_mut().commit("License redeem token", "tok-c"));
    assert_eq!(t.saved("licenseRedeemToken").as_deref(), Some("tok-c"));
    assert!(
        t.store
            .take_error()
            .expect("cloud redeem error")
            .contains("rejected")
    );

    assert!(t.store.tiers_mut().commit("Support redeem token", "tok-s"));
    assert_eq!(t.saved("supportRedeemToken").as_deref(), Some("tok-s"));
    assert!(
        t.store
            .take_error()
            .expect("support redeem error")
            .contains("rejected")
    );
}

// ---- The pages ----------------------------------------------------------------

#[test]
fn a_tier_page_is_loaded_from_the_server_once_and_served_from_the_store() {
    let (rt, server) = mock_server();
    serve_json(&rt, &server, "/cloud", cloud_page());
    let mut t = store(&server.uri());
    // The row's key carries a status after the title; the path does too.
    let p = t.tiers_path();
    let row = key(&t.at(&p)[1]);
    let page = format!("{p}/{row}");

    assert_eq!(
        t.at(&page).iter().map(key).collect::<Vec<_>>(),
        vec![localize::t("store-loading-page")]
    );
    t.settle();
    let items = t.at(&page);
    assert!(
        items
            .iter()
            .any(|e| key(e) == "<button>checkout:cloud</button>for payment")
    );
    assert!(
        items
            .iter()
            .any(|e| key(e) == "License redeem token: <input></input>")
    );

    // Fetched once: a refresh is served from the cache.
    t.at(&page);
    let requests = rt.block_on(server.received_requests()).unwrap();
    assert_eq!(
        requests.iter().filter(|r| r.url.path() == "/cloud").count(),
        1
    );
}

/// The reason the pages are served by the Store rather than grafted: the
/// refresh after a commit re-fetches the page, and it must still be the page,
/// showing what was typed.
#[test]
fn a_page_survives_the_refresh_after_redeeming_with_the_token_in_it() {
    let (rt, server) = mock_server();
    serve_json(&rt, &server, "/cloud", cloud_page());
    serve_json(&rt, &server, "/license/tok-c", sample_cert_json());
    let mut t = store(&server.uri());
    let page = format!("{}/{}", t.tiers_path(), localize::t("store-tier-cloud"));
    t.at(&page);
    t.settle();

    t.store
        .set_current_path(&format!("{page}/License redeem token"));
    assert!(t.store.commit_edit("", "tok-c"));
    let items = t.at(&page);
    assert!(
        items
            .iter()
            .any(|e| key(e) == "License redeem token: <input>tok-c</input>"),
        "{items:?}"
    );
}

#[test]
fn a_page_that_cannot_be_loaded_says_so() {
    let mut t = store("http://127.0.0.1:1");
    let page = format!("{}/{}", t.tiers_path(), localize::t("store-tier-support"));
    t.at(&page);
    t.settle();
    assert!(t.store.take_error().is_some());
}

// ---- license.status for plugins -----------------------------------------------

#[test]
fn a_third_party_tier_is_checked_against_the_issuer_the_store_list_names() {
    use sicompass_sdk::license::LicenseStatus;
    // No certificate saved for it, and the key is known: missing, not an error.
    let (_, public) = sicompass_sdk::package::generate_keypair().unwrap();
    let store = sicompass_sdk::store::Store::parse(
        format!(
            r#"{{ "version": 1, "tiers": {{ "acme/pro": {{ "issuer": "{public}",
                 "checkout": "https://acme.example/buy", "title": "acme-pro" }} }},
               "plugins": [] }}"#
        )
        .as_bytes(),
    )
    .unwrap();
    remember_issuers(&store);
    assert_eq!(license_status("acme/pro"), LicenseStatus::Missing);
    // A tier nobody lists has no issuer to believe.
    assert_eq!(license_status("nobody/tier"), LicenseStatus::Missing);
    // The registration is what the WASM host asks.
    register();
    assert_eq!(
        sicompass_sdk::license::status("nobody/tier"),
        LicenseStatus::Missing
    );
}

#[test]
fn a_store_list_cannot_name_another_issuer_for_our_tiers() {
    let store = sicompass_sdk::store::Store::parse(
        format!(
            r#"{{ "version": 1, "tiers": {{ "friendlyflow/cloud": {{
                 "issuer": "{}", "checkout": "https://x.example", "title": "t" }} }},
               "plugins": [] }}"#,
            sicompass_sdk::package::generate_keypair().unwrap().1
        )
        .as_bytes(),
    )
    .unwrap();
    remember_issuers(&store);
    assert_eq!(
        sicompass_payments::cert::known_issuer(sicompass_payments::cert::tier::CLOUD),
        Some(sicompass_payments::cert::LICENSE_PUBLIC_KEY_B64)
    );
}
