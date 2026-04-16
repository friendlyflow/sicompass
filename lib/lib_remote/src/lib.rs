//! Remote FFON provider — Rust port of `lib/lib_remote/remote.ts`.
//!
//! Fetches a JSON FFON tree from a remote HTTP server and exposes it as a
//! sicompass provider. Each top-level object in the server response is wrapped
//! with a `<link>` tag so the main app's existing link-navigation handles
//! sub-navigation (no per-path fetching is needed here, matching the TS script
//! behaviour).
//!
//! ## Settings keys consumed via `on_setting_change`
//!
//! - `"remoteUrl"` — base URL of the remote service (e.g. `https://example.com/api`)
//! - `"apiKey"`    — optional Bearer token; omit or leave empty for unauthenticated access
//!
//! ## Config file schema (compatible with the C build)
//!
//! ```json
//! {
//!   "<provider-name>": {
//!     "remoteUrl": "https://example.com/api",
//!     "apiKey":    "optional-bearer-token"
//!   }
//! }
//! ```

use sicompass_sdk::ffon::{parse_json_value, FfonElement};
use sicompass_sdk::provider::Provider;

// ---------------------------------------------------------------------------
// RemoteProvider
// ---------------------------------------------------------------------------

pub struct RemoteProvider {
    /// Provider name — also used as the settings section name.
    name: String,
    remote_url: String,
    api_key: String,
    current_path: String,
    /// Cached root fetch (cleared when remoteUrl changes).
    cached_root: Option<Vec<FfonElement>>,
}

impl RemoteProvider {
    /// Create a new provider with a known URL and API key.
    ///
    /// Pass empty strings for `remote_url` / `api_key` when neither is known
    /// yet; they will be populated via `on_setting_change` during `init()`.
    pub fn new(name: &str, remote_url: String, api_key: String) -> Self {
        RemoteProvider {
            name: name.to_owned(),
            remote_url,
            api_key,
            current_path: "/".to_owned(),
            cached_root: None,
        }
    }

    /// Perform a blocking GET request and return the parsed FFON elements.
    fn fetch_from_server(&self) -> Vec<FfonElement> {
        if self.remote_url.is_empty() {
            return vec![FfonElement::new_str(format!(
                "No remote URL configured for \"{}\"",
                self.name
            ))];
        }

        let root_url = format!("{}/root", self.remote_url.trim_end_matches('/'));

        let client = match reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                return vec![FfonElement::new_str(format!(
                    "Error building HTTP client: {e}"
                ))]
            }
        };

        let mut req = client.get(&root_url).header("Accept", "application/json");
        if !self.api_key.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", self.api_key));
        }

        let response = match req.send() {
            Ok(r) => r,
            Err(e) => {
                return vec![FfonElement::new_str(format!(
                    "Error connecting to {}: {}",
                    self.remote_url, e
                ))]
            }
        };

        if !response.status().is_success() {
            return vec![FfonElement::new_str(format!(
                "Failed to fetch from {}: {} {}",
                self.remote_url,
                response.status().as_u16(),
                response.status().canonical_reason().unwrap_or("")
            ))];
        }

        let body = match response.text() {
            Ok(t) => t,
            Err(e) => {
                return vec![FfonElement::new_str(format!(
                    "Error reading response from {}: {e}",
                    self.remote_url
                ))]
            }
        };

        let arr = match serde_json::from_str::<serde_json::Value>(&body) {
            Ok(serde_json::Value::Array(a)) => a,
            Ok(_) => {
                return vec![FfonElement::new_str(format!(
                    "Invalid response from {}",
                    self.remote_url
                ))]
            }
            Err(e) => {
                return vec![FfonElement::new_str(format!(
                    "Invalid JSON from {}: {e}",
                    self.remote_url
                ))]
            }
        };

        // Wrap each top-level object with a <link> tag for lazy sub-navigation,
        // matching wrapWithLinks() in remote.ts.
        let base = self.remote_url.trim_end_matches('/');
        arr.iter().map(|v| wrap_with_link(v, base)).collect()
    }
}

/// Wrap a top-level JSON value with a `<link>` tag on its object key,
/// mirroring `wrapWithLinks` in remote.ts.
///
/// - Strings are passed through as-is.
/// - Objects whose key already contains `<link>` are passed through.
/// - Other objects get `<link>{base_url}/{url-encoded key}</link>{key}` as
///   their key, and an empty children list (sub-nav is handled by the link
///   resolver in the main binary, not fetched here).
fn wrap_with_link(v: &serde_json::Value, base_url: &str) -> FfonElement {
    if let serde_json::Value::Object(map) = v {
        if let Some((key, _)) = map.iter().next() {
            if key.contains("<link>") {
                // Already wrapped — delegate to the standard parser.
                return parse_json_value(v);
            }
            let encoded_key = url_encode(key);
            let link_key = format!("<link>{base_url}/{encoded_key}</link>{key}");
            // Return an empty object — children are fetched lazily via the link.
            return FfonElement::new_obj(link_key);
        }
    }
    parse_json_value(v)
}

/// Minimal percent-encoding for path segments (RFC 3986 unreserved chars are
/// left as-is; everything else is %-encoded). Mirrors encodeURIComponent in TS.
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9'
            | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push(char::from_digit((b >> 4) as u32, 16).unwrap().to_ascii_uppercase());
                out.push(char::from_digit((b & 0xf) as u32, 16).unwrap().to_ascii_uppercase());
            }
        }
    }
    out
}

impl Provider for RemoteProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn display_name(&self) -> &str {
        &self.name
    }

    fn fetch(&mut self) -> Vec<FfonElement> {
        if self.current_path != "/" {
            // Sub-navigation is handled by the <link> resolver; we serve nothing here.
            return Vec::new();
        }
        if let Some(cached) = &self.cached_root {
            return cached.clone();
        }
        let result = self.fetch_from_server();
        self.cached_root = Some(result.clone());
        result
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
            "remoteUrl" => {
                if self.remote_url != value {
                    self.remote_url = value.to_owned();
                    self.cached_root = None; // invalidate cache
                }
            }
            "apiKey" => {
                self.api_key = value.to_owned();
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

// Strategy: wiremock requires an async runtime to start the server. We create
// a fresh tokio::runtime::Runtime per test, use it only to start the server
// and register mocks, then drop out to sync context before calling any
// blocking reqwest code. This avoids the "cannot drop runtime in async context"
// panic that occurs when reqwest::blocking runs inside a tokio executor.

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn start_mock_server() -> (tokio::runtime::Runtime, MockServer) {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let server = rt.block_on(MockServer::start());
        (rt, server)
    }

    fn mount(rt: &tokio::runtime::Runtime, server: &MockServer, mock: Mock) {
        rt.block_on(mock.mount(server));
    }

    #[test]
    fn fetch_success_wraps_objects_with_link_tags() {
        let (rt, server) = start_mock_server();
        mount(
            &rt,
            &server,
            Mock::given(method("GET"))
                .and(path("/root"))
                .respond_with(ResponseTemplate::new(200).set_body_json(
                    serde_json::json!([
                        { "Products": [] },
                        "plain string item",
                    ]),
                )),
        );

        let base_url = server.uri();
        let mut p = RemoteProvider::new("mysvc", base_url.clone(), String::new());
        let items = p.fetch();

        // Expect 2 elements
        assert_eq!(items.len(), 2, "should have 2 items, got: {items:?}");

        // First item: "Products" should be wrapped with a <link> tag
        let first_key = match &items[0] {
            FfonElement::Obj(o) => o.key.clone(),
            other => panic!("expected Obj, got {other:?}"),
        };
        assert!(
            first_key.contains("<link>") && first_key.contains("Products"),
            "expected <link> tag wrapping 'Products', got: {first_key}"
        );
        assert!(
            first_key.contains(&base_url),
            "link key should contain server URL, got: {first_key}"
        );

        // Second item: plain string passes through
        assert_eq!(
            items[1],
            FfonElement::Str("plain string item".to_owned())
        );
    }

    #[test]
    fn fetch_bearer_auth_header_sent_when_api_key_set() {
        let (rt, server) = start_mock_server();
        mount(
            &rt,
            &server,
            Mock::given(method("GET"))
                .and(path("/root"))
                .and(header("Authorization", "Bearer secret123"))
                .respond_with(ResponseTemplate::new(200).set_body_json(
                    serde_json::json!(["item"]),
                )),
        );

        let mut p = RemoteProvider::new(
            "mysvc",
            server.uri(),
            "secret123".to_owned(),
        );
        let items = p.fetch();
        assert_eq!(items, vec![FfonElement::Str("item".to_owned())]);
    }

    #[test]
    fn fetch_non_200_returns_error_string() {
        let (rt, server) = start_mock_server();
        mount(
            &rt,
            &server,
            Mock::given(method("GET"))
                .and(path("/root"))
                .respond_with(ResponseTemplate::new(401)),
        );

        let mut p = RemoteProvider::new("mysvc", server.uri(), String::new());
        let items = p.fetch();

        assert_eq!(items.len(), 1);
        let msg = match &items[0] {
            FfonElement::Str(s) => s.clone(),
            other => panic!("expected Str error, got {other:?}"),
        };
        assert!(
            msg.contains("401") || msg.contains("Failed"),
            "expected 401/Failed in error message, got: {msg}"
        );
    }

    #[test]
    fn fetch_no_url_returns_not_configured_message() {
        let mut p = RemoteProvider::new("mysvc", String::new(), String::new());
        let items = p.fetch();
        assert_eq!(items.len(), 1);
        let msg = items[0].as_str().unwrap_or("");
        assert!(
            msg.contains("No remote URL"),
            "expected 'No remote URL' message, got: {msg}"
        );
    }

    #[test]
    fn on_setting_change_remote_url_invalidates_cache() {
        let (rt, server) = start_mock_server();
        mount(
            &rt,
            &server,
            Mock::given(method("GET"))
                .and(path("/root"))
                .respond_with(ResponseTemplate::new(200).set_body_json(
                    serde_json::json!(["item"]),
                )),
        );

        let mut p = RemoteProvider::new("mysvc", server.uri(), String::new());
        let _ = p.fetch(); // populate cache
        assert!(p.cached_root.is_some());

        p.on_setting_change("remoteUrl", "https://other.example.com");
        assert!(p.cached_root.is_none(), "cache should be cleared after remoteUrl change");
        assert_eq!(p.remote_url, "https://other.example.com");
    }

    #[test]
    fn on_setting_change_api_key_stores_value() {
        let mut p = RemoteProvider::new("mysvc", String::new(), String::new());
        p.on_setting_change("apiKey", "newkey");
        assert_eq!(p.api_key, "newkey");
    }

    #[test]
    fn url_encode_spaces_and_slashes() {
        assert_eq!(url_encode("hello world"), "hello%20world");
        assert_eq!(url_encode("a/b"), "a%2Fb");
        assert_eq!(url_encode("abc"), "abc");
    }
}

// ---------------------------------------------------------------------------
// SDK registration
// ---------------------------------------------------------------------------

/// Instantiate a `RemoteProvider` for a named remote service.
///
/// `RemoteProvider` requires `(name, url, api_key)` at construction time and
/// cannot fit the zero-arg factory signature, so this helper is called
/// directly by `sicompass_builtins` and by the app's `load_remote_programs`.
pub fn create_remote(
    name: &str,
    remote_url: String,
    api_key: String,
) -> Box<dyn sicompass_sdk::Provider> {
    Box::new(RemoteProvider::new(name, remote_url, api_key))
}

/// Register the remote provider with the SDK manifest registry (no zero-arg
/// factory — use [`create_remote`] to instantiate).
pub fn register() {
    // RemoteProvider is per-service, not a single named factory; only the
    // manifest type info is registered so the app can recognise remote entries.
    // Instantiation goes through create_remote().
}
