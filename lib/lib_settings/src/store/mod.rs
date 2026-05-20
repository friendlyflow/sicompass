//! Store sub-node helpers used by `SettingsProvider`.
//!
//! The store renders as a fixed, always-top child of the "Available programs:"
//! settings section. Its UI is exactly: a license-status line, a server-hosted
//! catalog (cached on first fetch), a checkout link, and the two text inputs
//! that drive it. License verification is offline, display-only — never a
//! feature gate (see memory `project_licensing_model`).

pub(crate) mod catalog;
pub(crate) mod cert;

/// Default store / license server. Overridable via the Store server URL input.
pub(crate) const DEFAULT_STORE_URL: &str = "https://store.sicompass.org";

/// Port of the redeem flow that previously lived on `StoreProvider`.
///
/// On any failure, returns `Err(message)` so the caller can stash it as the
/// provider's pending error. On success the verified certificate has been
/// written to disk.
pub(crate) fn redeem_license(store_url: &str, token: &str) -> Result<(), String> {
    if store_url.is_empty() || token.is_empty() {
        return Ok(());
    }
    let url = format!("{}/license/{}", store_url.trim_end_matches('/'), token);

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("Could not redeem license: {e}"))?;

    let response = client
        .get(&url)
        .header("Accept", "application/json")
        .send()
        .map_err(|e| format!("Could not reach the store: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "Redeem failed: the store returned {}",
            response.status().as_u16()
        ));
    }

    let body = response
        .text()
        .map_err(|e| format!("Could not read the store reply: {e}"))?;

    let certificate: cert::Certificate = serde_json::from_str(&body)
        .map_err(|e| format!("Store returned an invalid certificate: {e}"))?;

    match cert::verify(&certificate) {
        cert::LicenseStatus::Invalid(why) => Err(format!("License certificate rejected: {why}")),
        _ => {
            if !cert::save(&certificate) {
                Err("Could not save the license file".to_owned())
            } else {
                Ok(())
            }
        }
    }
}
