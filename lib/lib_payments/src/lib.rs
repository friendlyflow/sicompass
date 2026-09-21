//! The client half of the sicompass commercial offering.
//!
//! Everything a provider needs to take part in the sponsor / cloud / store /
//! support tiers lives here, so the three crates that need it — the settings
//! provider, notes and project management — share one implementation:
//!
//! - [`cert`] — the license certificate: schema, offline Ed25519 verification,
//!   load and save. Display only; it never gates anything in the client.
//! - [`checkout`] — ask the server for a hosted checkout and open it in the
//!   user's real browser.
//! - [`tier_input`] — the button, radio and redeem-token handling for a
//!   server-served tier tree. A tier tree is grafted into *whichever* provider
//!   the user followed the link from, and the app dispatches a `<button>` press
//!   to that provider, so any provider that shows a tier link needs these.
//! - [`entitlement`] — is the cloud and store subscription currently good?
//! - [`row`] — the one `<link>` row a cloud-backed provider puts at the top of
//!   its first layer.
//! - [`backup`] — read a provider's store directory, mirror it to the server,
//!   and write it back.
//!
//! None of this is a feature gate. The app is free under GPLv3 and a user's
//! notes and board stay fully readable and editable whether or not they have
//! paid. What the subscription buys is a *service* — storage on our server —
//! which is why only `backup` talks about being paid at all.

pub mod backup;
pub mod cert;
pub mod checkout;
pub mod cloud;
pub mod config;
pub mod entitlement;
pub mod row;
pub mod tier_input;

use sicompass_sdk::localize;
use std::sync::OnceLock;

/// Default server. Overridable via the "Store server URL" input.
pub const DEFAULT_STORE_URL: &str = "https://store.sicompass.org";

/// Certificate slug for the cloud and store subscription.
pub const CLOUD_SLUG: &str = "store-license";

/// Certificate slug for the support subscription.
pub const SUPPORT_SLUG: &str = "support-license";

/// Make this crate's strings available. Cheap and idempotent; call it from any
/// entry point that renders one.
///
/// The bundles live here rather than in each provider so the cloud row reads
/// identically in notes, in the board and anywhere else it is shown, and so a
/// translation is written once.
pub fn register_translations() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let _ = localize::register_bundle("en-US", include_str!("../locales/en-US.ftl"));
        let _ = localize::register_bundle("nl-BE", include_str!("../locales/nl-BE.ftl"));
        let _ = localize::register_bundle("fr-BE", include_str!("../locales/fr-BE.ftl"));
        let _ = localize::register_bundle("de-BE", include_str!("../locales/de-BE.ftl"));
    });
}

/// Redeem a license token and persist the verified certificate under `slug`
/// ([`CLOUD_SLUG`] for cloud and store, [`SUPPORT_SLUG`] for support).
///
/// On any failure returns `Err(message)` so the caller can stash it as the
/// provider's pending error. On success the verified certificate has been
/// written to disk.
pub fn redeem_license(store_url: &str, token: &str, slug: &str) -> Result<(), String> {
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
