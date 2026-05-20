//! Sicompass store provider.
//!
//! This crate is the foundation for the sicompass app store (see PLAN.md). Its
//! first job is the **sicompass commercial license**: it renders the
//! server-hosted store catalog, offers a checkout link, and verifies signed
//! license certificates offline.
//!
//! Licensing model (Model A — see memory `project_licensing_model`): the
//! commercial license sells *rights*, never features. Certificate verification
//! here is for display and proof only. Nothing in sicompass is gated behind a
//! license, and nothing in this crate must ever become a gate.
//!
//! ## Settings consumed via `on_setting_change`
//!
//! - `storeUrl` — base URL of the store / license server.
//! - `licenseRedeemToken` — paste a redeem token here after purchase; the
//!   provider fetches the signed certificate, verifies it, and saves it.

mod catalog;
pub mod cert;

use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::provider::Provider;

/// Default store / license server. Overridable via the `storeUrl` setting.
const DEFAULT_STORE_URL: &str = "https://store.sicompass.org";

pub struct StoreProvider {
    store_url: String,
    current_path: String,
    /// Cached root catalog (cleared when `storeUrl` changes).
    cached_catalog: Option<Vec<FfonElement>>,
    error: Option<String>,
}

impl StoreProvider {
    pub fn new() -> Self {
        StoreProvider {
            store_url: DEFAULT_STORE_URL.to_owned(),
            current_path: "/".to_owned(),
            cached_catalog: None,
            error: None,
        }
    }

    /// Fetch a signed certificate from `{store_url}/license/{token}`, verify
    /// it, and save it on success. Sets `self.error` on any failure.
    fn redeem(&mut self, token: &str) {
        if self.store_url.is_empty() || token.is_empty() {
            return;
        }
        let url = format!("{}/license/{}", self.store_url.trim_end_matches('/'), token);

        let client = match reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                self.error = Some(format!("Could not redeem license: {e}"));
                return;
            }
        };

        let response = match client.get(&url).header("Accept", "application/json").send() {
            Ok(r) => r,
            Err(e) => {
                self.error = Some(format!("Could not reach the store: {e}"));
                return;
            }
        };
        if !response.status().is_success() {
            self.error = Some(format!(
                "Redeem failed: the store returned {}",
                response.status().as_u16()
            ));
            return;
        }

        let body = match response.text() {
            Ok(t) => t,
            Err(e) => {
                self.error = Some(format!("Could not read the store reply: {e}"));
                return;
            }
        };
        let certificate: cert::Certificate = match serde_json::from_str(&body) {
            Ok(c) => c,
            Err(e) => {
                self.error = Some(format!("Store returned an invalid certificate: {e}"));
                return;
            }
        };

        match cert::verify(&certificate) {
            cert::LicenseStatus::Invalid(why) => {
                self.error = Some(format!("License certificate rejected: {why}"));
            }
            _ => {
                if !cert::save(&certificate) {
                    self.error = Some("Could not save the license file".to_owned());
                }
            }
        }
    }
}

impl Default for StoreProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl Provider for StoreProvider {
    fn name(&self) -> &str {
        "store"
    }

    fn display_name(&self) -> String {
        "Store".to_owned()
    }

    fn fetch(&mut self) -> Vec<FfonElement> {
        if self.current_path != "/" {
            // Sub-navigation into catalog pages is handled by the app's
            // <link> resolver, matching RemoteProvider.
            return Vec::new();
        }

        let mut out = Vec::new();

        // 1. Commercial-license status (display only — never a feature gate).
        let status = cert::load()
            .map(|c| cert::verify(&c))
            .unwrap_or(cert::LicenseStatus::None);
        out.push(FfonElement::new_str(status.summary_line()));

        // 2. The server-hosted store catalog (cached after the first fetch).
        if self.cached_catalog.is_none() {
            self.cached_catalog = Some(catalog::fetch_catalog(&self.store_url));
        }
        if let Some(items) = &self.cached_catalog {
            out.extend(items.iter().cloned());
        }

        // 3. Checkout link.
        let base = self.store_url.trim_end_matches('/');
        out.push(FfonElement::new_obj(format!(
            "<link>{base}/checkout?item=commercial-license</link>\
             Buy or manage your commercial license"
        )));

        out
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
        if self.current_path == "/" {
            return;
        }
        if let Some(slash) = self.current_path.rfind('/') {
            if slash == 0 {
                self.current_path = "/".to_owned();
            } else {
                self.current_path.truncate(slash);
            }
        }
    }

    fn current_path(&self) -> &str {
        &self.current_path
    }

    fn on_setting_change(&mut self, key: &str, value: &str) {
        match key {
            "storeUrl" => {
                if self.store_url != value {
                    self.store_url = value.to_owned();
                    self.cached_catalog = None; // invalidate cache
                }
            }
            "licenseRedeemToken" => {
                let token = value.trim();
                if !token.is_empty() {
                    self.redeem(token);
                }
            }
            _ => {}
        }
    }

    fn take_error(&mut self) -> Option<String> {
        self.error.take()
    }
}

// ---------------------------------------------------------------------------
// SDK registration
// ---------------------------------------------------------------------------

/// Register the store provider with the SDK factory and manifest registries.
pub fn register() {
    sicompass_sdk::register_provider_factory("store", || Box::new(StoreProvider::new()));
    sicompass_sdk::register_builtin_manifest(
        sicompass_sdk::BuiltinManifest::new("store", "Store")
            .enable_by_default()
            .with_settings(vec![
                sicompass_sdk::SettingDecl::text(
                    "Store",
                    "Store server URL",
                    "storeUrl",
                    DEFAULT_STORE_URL,
                ),
                sicompass_sdk::SettingDecl::text(
                    "Store",
                    "License redeem token",
                    "licenseRedeemToken",
                    "",
                ),
            ]),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // Same strategy as the lib_remote tests: use a tokio runtime only to start
    // the mock server, then call blocking reqwest from sync context.
    fn start_mock_server() -> (tokio::runtime::Runtime, MockServer) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let server = rt.block_on(MockServer::start());
        (rt, server)
    }

    fn mount(rt: &tokio::runtime::Runtime, server: &MockServer, mock: Mock) {
        rt.block_on(mock.mount(server));
    }

    #[test]
    fn provider_identity() {
        let p = StoreProvider::new();
        assert_eq!(p.name(), "store");
        assert_eq!(p.display_name(), "Store");
    }

    #[test]
    fn non_root_path_returns_empty() {
        let mut p = StoreProvider::new();
        p.push_path("Themes");
        assert!(p.fetch().is_empty());
    }

    #[test]
    fn store_url_change_invalidates_catalog_cache() {
        let mut p = StoreProvider::new();
        p.cached_catalog = Some(vec![FfonElement::new_str("stale")]);
        p.on_setting_change("storeUrl", "https://new.example");
        assert!(p.cached_catalog.is_none());
        assert_eq!(p.store_url, "https://new.example");
    }

    #[test]
    fn fetch_root_shows_status_catalog_and_checkout() {
        let (rt, server) = start_mock_server();
        mount(
            &rt,
            &server,
            Mock::given(method("GET")).and(path("/root")).respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!([{ "Themes": [] }])),
            ),
        );

        let mut p = StoreProvider::new();
        p.on_setting_change("storeUrl", &server.uri());
        let items = p.fetch();

        // status line + catalog entry + checkout link
        assert!(items.len() >= 3, "expected >=3 items, got: {items:?}");
        assert!(items[0].is_str(), "first item should be the license-status line");
        assert!(
            items.iter().any(|e| e
                .as_obj()
                .map(|o| o.key.contains("Themes") && o.key.contains("<link>"))
                .unwrap_or(false)),
            "catalog entry should be link-wrapped, got: {items:?}"
        );
        let last = items.last().unwrap().as_obj().expect("checkout should be an Obj");
        assert!(
            last.key.contains("checkout") && last.key.contains("<link>"),
            "last item should be the checkout link, got: {}",
            last.key
        );
    }

    #[test]
    fn redeem_rejects_invalid_certificate() {
        let (rt, server) = start_mock_server();
        // A syntactically valid certificate, but the placeholder public key
        // will reject the signature.
        mount(
            &rt,
            &server,
            Mock::given(method("GET")).and(path("/license/tok123")).respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "payload": {
                        "product": "sicompass",
                        "license_id": "id",
                        "licensee": "Test",
                        "scope": "commercial",
                        "issued_at": 1_700_000_000_i64,
                        "expires_at": 1_900_000_000_i64,
                        "version_coverage": "*",
                        "payment_provider": "polar"
                    },
                    "signature": "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
                })),
            ),
        );

        let mut p = StoreProvider::new();
        p.on_setting_change("storeUrl", &server.uri());
        p.on_setting_change("licenseRedeemToken", "tok123");
        let err = p.take_error();
        assert!(err.is_some(), "an unverifiable certificate should set an error");
        assert!(err.unwrap().contains("rejected"));
    }
}
