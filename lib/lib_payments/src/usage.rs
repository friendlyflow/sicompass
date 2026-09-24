//! What the cloud backup uses, as the server last reported it.
//!
//! Every backup reply carries a `usage` object: bytes stored against the
//! storage cap, and bytes moved this calendar month against the transfer cap.
//! The server enforces both (an upload over a cap is refused with a reason,
//! and nothing stored is touched). This keeps the last report, in memory and
//! in `providers/cloud-usage.json`, so the Store can show it.

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
        crate::register_translations();
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

/// Keep the `usage` of a backup reply, if it has one.
pub fn record_from(reply: &serde_json::Value) {
    let Some(usage) = reply
        .get("usage")
        .cloned()
        .and_then(|u| serde_json::from_value::<Usage>(u).ok())
    else {
        return;
    };
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
        *last = Some(usage);
    }
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

    #[test]
    fn sizes_read_naturally() {
        assert_eq!(human(0), "0 bytes");
        assert_eq!(human(1_500), "2 kB");
        assert_eq!(human(340_000_000), "340 MB");
        assert_eq!(human(2_100_000_000), "2.1 GB");
    }
}
