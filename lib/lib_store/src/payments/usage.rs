//! What the cloud backup uses, as the server last reported it.
//!
//! `GET /usage` (with the Sicompass Cloud redeem token) answers with a `usage`
//! object: bytes stored against the storage cap, and bytes moved this calendar
//! month against the transfer cap. The server enforces both (an upload over a
//! cap is refused with a reason, and nothing stored is touched). The uploads
//! themselves are the plugins', so the Store asks the server rather than
//! hearing it from them. This keeps the last report, in memory and in
//! `providers/cloud-usage.json`, so the Store can show it offline too.

use serde::{Deserialize, Serialize};
use sicompass_sdk::localize;
use std::sync::Mutex;

const SLUG: &str = "cloud-usage";

static LAST: Mutex<Option<Usage>> = Mutex::new(None);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub stored: i64,
    pub storage_cap: i64,
    pub transferred: i64,
    pub transfer_cap: i64,
    /// `YYYY-MM` (UTC) the transfer is counted in.
    pub month: String,
}

impl Usage {
    /// Two lines for the Store: storage, then this month's traffic.
    pub fn lines(&self) -> [String; 2] {
        crate::payments::register_translations();
        let line = |key: &str, used: i64, cap: i64| {
            let mut args = localize::Args::new();
            args.set("used", human(used));
            args.set("cap", human(cap));
            localize::t_args(key, &args)
        };
        [
            line("payments-usage-stored", self.stored, self.storage_cap),
            line(
                "payments-usage-transferred",
                self.transferred,
                self.transfer_cap,
            ),
        ]
    }
}

/// A byte count in decimal units, as the server words its refusals.
pub fn human(bytes: i64) -> String {
    let b = bytes.max(0) as f64;
    if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.0} MB", b / 1e6)
    } else if b >= 1e3 {
        format!("{:.0} kB", b / 1e3)
    } else {
        format!("{bytes} bytes")
    }
}

/// Ask the server what the backup uses. The reply is kept for [`last`].
///
/// Blocking: call it from a worker thread.
pub fn fetch(server: &str, token: &str) -> Result<Usage, String> {
    if server.is_empty() || token.is_empty() {
        return Err("No server or no Sicompass Cloud token".to_owned());
    }
    let url = format!("{}/usage", server.trim_end_matches('/'));
    let response = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?
        .get(&url)
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/json")
        .send()
        .map_err(|e| format!("Could not reach the server: {e}"))?;
    let status = response.status();
    let body = response.text().map_err(|e| e.to_string())?;
    if !status.is_success() {
        // The server words its refusals for a person to read.
        let body = body.trim();
        return Err(if body.is_empty() {
            format!("the server returned {}", status.as_u16())
        } else {
            body.to_owned()
        });
    }
    let reply: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| format!("Server returned an invalid reply: {e}"))?;
    record_from(&reply).ok_or_else(|| "The server's reply has no usage".to_owned())
}

/// Keep the `usage` of a server reply, if it has one, and return it.
pub fn record_from(reply: &serde_json::Value) -> Option<Usage> {
    let usage = reply
        .get("usage")
        .cloned()
        .and_then(|u| serde_json::from_value::<Usage>(u).ok())?;
    // Not written to disk from this crate's own tests, whose replies come from
    // a mock server: the sandbox would catch it, but there is nothing to keep.
    if !cfg!(test)
        && let Some(path) = sicompass_sdk::platform::provider_config_path(SLUG)
        && let Ok(json) = serde_json::to_string_pretty(&usage)
    {
        if let Some(dir) = path.parent() {
            sicompass_sdk::platform::make_dirs(dir);
        }
        sicompass_sdk::platform::atomic_write(&path, &json);
    }
    if let Ok(mut last) = LAST.lock() {
        *last = Some(usage.clone());
    }
    Some(usage)
}

/// The last usage the server reported, this session or an earlier one.
pub fn last() -> Option<Usage> {
    if let Some(u) = LAST.lock().ok().and_then(|l| l.clone()) {
        return Some(u);
    }
    let path = sicompass_sdk::platform::provider_config_path(SLUG)?;
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reply_with_usage_is_kept_and_worded() {
        record_from(&serde_json::json!({ "stored": true }));
        record_from(&serde_json::json!({
            "stored": true,
            "usage": {
                "stored": 2_100_000_000i64, "storage_cap": 10_000_000_000i64,
                "transferred": 340_000_000, "transfer_cap": 5_000_000_000i64,
                "month": "2026-09"
            }
        }));
        let u = last().expect("kept");
        assert_eq!(u.month, "2026-09");
        let [stored, moved] = u.lines();
        assert!(
            stored.contains("2.1 GB") && stored.contains("10.0 GB"),
            "{stored}"
        );
        assert!(
            moved.contains("340 MB") && moved.contains("5.0 GB"),
            "{moved}"
        );
    }

    /// A server answering `GET /usage` for `tok-42`. The runtime is returned
    /// with it: the mock serves only while both are alive.
    fn usage_server(
        status: u16,
        body: serde_json::Value,
    ) -> (tokio::runtime::Runtime, wiremock::MockServer) {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let rt = tokio::runtime::Runtime::new().unwrap();
        let server = rt.block_on(MockServer::start());
        rt.block_on(
            Mock::given(method("GET"))
                .and(path("/usage"))
                .and(header("Authorization", "Bearer tok-42"))
                .respond_with(ResponseTemplate::new(status).set_body_json(body))
                .mount(&server),
        );
        (rt, server)
    }

    #[test]
    fn the_store_asks_the_server_with_the_token() {
        let (_rt, server) = usage_server(
            200,
            serde_json::json!({ "usage": {
                "stored": 1_000, "storage_cap": 10_000_000_000i64,
                "transferred": 2_000, "transfer_cap": 5_000_000_000i64,
                "month": "2026-09"
            }}),
        );
        let u = fetch(&server.uri(), "tok-42").unwrap();
        assert_eq!((u.stored, u.transferred), (1_000, 2_000));
        assert_eq!(last().unwrap().month, "2026-09");
    }

    #[test]
    fn a_refusal_is_passed_on_and_nothing_is_asked_without_a_token() {
        let (_rt, server) = usage_server(401, serde_json::json!("That token is not known"));
        assert!(fetch(&server.uri(), "tok-42").unwrap_err().contains("token"));
        assert!(fetch(&server.uri(), "").is_err());
    }

    #[test]
    fn sizes_read_naturally() {
        assert_eq!(human(0), "0 bytes");
        assert_eq!(human(1_500), "2 kB");
        assert_eq!(human(340_000_000), "340 MB");
        assert_eq!(human(2_100_000_000), "2.1 GB");
    }
}
