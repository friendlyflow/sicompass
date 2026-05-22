//! Sponsor / cloud / support helpers used by `SettingsProvider`.
//!
//! "Available programs:" pins three `<link>` Objs (sponsor, cloud, support).
//! Their tier trees are server-hosted and fetched by the app's link resolver;
//! this module covers the two client-side concerns: redeeming a license token
//! (offline, display-only verification) and the checkout / browser-launch
//! handled by [`tiers`]. License verification is never a feature gate (see
//! memory `project_licensing_model`).

pub(crate) mod cert;
pub(crate) mod tiers;

/// Default server. Overridable via the "Store server URL" input.
pub(crate) const DEFAULT_STORE_URL: &str = "https://store.sicompass.org";

/// Redeem a license token and persist the verified certificate under `slug`
/// (`"store-license"` for cloud and store, `"support-license"` for support).
///
/// On any failure returns `Err(message)` so the caller can stash it as the
/// provider's pending error. On success the verified certificate has been
/// written to disk.
pub(crate) fn redeem_license(store_url: &str, token: &str, slug: &str) -> Result<(), String> {
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
        .map_err(|e| format!("Could not reach the server: {e}"))?;

    if !response.status().is_success() {
        return Err(format!(
            "Redeem failed: the server returned {}",
            response.status().as_u16()
        ));
    }

    let body = response
        .text()
        .map_err(|e| format!("Could not read the server reply: {e}"))?;

    let certificate: cert::Certificate = serde_json::from_str(&body)
        .map_err(|e| format!("Server returned an invalid certificate: {e}"))?;

    match cert::verify(&certificate) {
        cert::LicenseStatus::Invalid(why) => Err(format!("License certificate rejected: {why}")),
        _ => {
            if !cert::save(slug, &certificate) {
                Err("Could not save the license file".to_owned())
            } else {
                Ok(())
            }
        }
    }
}
