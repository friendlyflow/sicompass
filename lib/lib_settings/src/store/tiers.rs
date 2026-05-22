//! Checkout request + external-browser launch for the sponsor / cloud /
//! support tiers.
//!
//! The tier *trees* themselves are server-hosted and fetched by the app's
//! link resolver (a `<link>` Obj per tier). This module only handles the
//! "for payment" button action: ask the server for a hosted checkout and
//! open it in the user's browser.

use std::time::Duration;

/// Request a hosted checkout from the server for `item`.
///
/// `amount` / `recurring` are only meaningful for `sponsor-donation` (pass
/// empty strings otherwise). On success returns the checkout URL. On any
/// failure returns `Err(message)`; for a non-2xx response the server's
/// plain-text body is returned verbatim so the caller can show it in the
/// header.
pub(crate) fn request_checkout(
    base_url: &str,
    item: &str,
    amount: &str,
    recurring: &str,
) -> Result<String, String> {
    let base = base_url.trim_end_matches('/');
    if base.is_empty() {
        return Err("No server URL configured".to_owned());
    }
    let url = format!("{base}/checkout");

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Could not start the checkout: {e}"))?;

    let mut query: Vec<(&str, &str)> = vec![("item", item)];
    if !amount.is_empty() {
        query.push(("amount", amount));
    }
    if !recurring.is_empty() {
        query.push(("recurring", recurring));
    }
    let full_url = reqwest::Url::parse_with_params(&url, &query)
        .map_err(|e| format!("Could not build the checkout URL: {e}"))?;

    let response = client
        .get(full_url)
        .header("Accept", "application/json")
        .send()
        .map_err(|e| format!("Could not reach the server: {e}"))?;

    let status = response.status();
    let body = response
        .text()
        .map_err(|e| format!("Could not read the server reply: {e}"))?;

    if !status.is_success() {
        let msg = body.trim();
        return Err(if msg.is_empty() {
            format!("Checkout failed: the server returned {}", status.as_u16())
        } else {
            msg.to_owned()
        });
    }

    let value: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| format!("Server returned an invalid checkout reply: {e}"))?;
    match value.get("checkout_url").and_then(|v| v.as_str()) {
        Some(u) if !u.is_empty() => Ok(u.to_owned()),
        _ => Err("Server reply did not include a checkout URL".to_owned()),
    }
}

/// Open `url` in the user's external browser. Returns `Err` (with the URL
/// still in the message) when the platform opener cannot be launched.
pub(crate) fn open_url(url: &str) -> Result<(), String> {
    let (program, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
        ("open", vec![url])
    } else if cfg!(target_os = "windows") {
        // `start` treats its first quoted argument as a window title.
        ("cmd", vec!["/C", "start", "", url])
    } else {
        ("xdg-open", vec![url])
    };
    std::process::Command::new(program)
        .args(&args)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("Could not open your browser ({e}). Visit: {url}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_base_url_is_rejected() {
        let err = request_checkout("", "cloud-yearly", "", "").unwrap_err();
        assert!(err.contains("server URL"));
    }

    #[test]
    fn unreachable_server_returns_err() {
        // Port 1 refuses immediately — no hang, no flakiness.
        let err = request_checkout("http://127.0.0.1:1", "support-annual", "", "")
            .unwrap_err();
        assert!(!err.is_empty());
    }
}
