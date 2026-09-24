//! Make a server-served tier tree work wherever it is grafted.
//!
//! The tier trees (`/sponsor`, `/cloud`, `/support`) are FFON served by the
//! license server and fetched by the app's `<link>` resolver. The resolver
//! grafts the fetched tree onto the link row **in the tree of whichever
//! provider the user followed the link from**, and the app dispatches a
//! `<button>` press, a `<radio>` change and an `<input>` commit to that same
//! provider.
//!
//! So a tier tree entered from Settings is Settings' to drive, and the very
//! same tree entered from notes is notes' to drive. That is why this lives in
//! a shared crate instead of in one provider: the alternative is
//! three copies of the checkout, one per provider that shows a tier link.
//!
//! A provider forwards its three callbacks here and stashes the returned error,
//! if any, wherever it keeps its pending error. Nothing here touches the
//! provider's own data.

use crate::{CLOUD_SLUG, SUPPORT_SLUG, cert, checkout, config, redeem_license};
use std::collections::HashMap;

/// The radio group the cloud tier uses to pick a billing period.
const CLOUD_PERIOD_GROUP: &str = "monthly or yearly";

/// Transient state for a tier tree the user is filling in.
///
/// Nothing here is persisted: the selections only have to live long enough to
/// build one checkout request. The Store, notes and the board each hold one.
#[derive(Debug, Default)]
pub struct TierSession {
    /// Base URL of the license server. Kept in step through
    /// `Provider::on_setting_change` and seeded from [`config::store_url`].
    store_url: String,
    /// Radio and input values the server-served tree has collected so far,
    /// keyed by the group or label the server used.
    form_state: HashMap<String, String>,
}

impl TierSession {
    /// Start a session against the configured server.
    pub fn new() -> Self {
        TierSession {
            store_url: config::store_url(),
            form_state: HashMap::new(),
        }
    }

    pub fn store_url(&self) -> &str {
        &self.store_url
    }

    pub fn set_store_url(&mut self, url: &str) {
        self.store_url = if url.is_empty() {
            crate::DEFAULT_STORE_URL.to_owned()
        } else {
            url.to_owned()
        };
    }

    /// Route a setting the app broadcast to every provider. Returns whether it
    /// was one of ours, so a caller can `if !session.on_setting_change(..)`
    /// and carry on with its own keys.
    pub fn on_setting_change(&mut self, key: &str, value: &str) -> bool {
        match key {
            "storeUrl" => {
                self.set_store_url(value);
                true
            }
            // The token is read from disk when it is needed, so there is
            // nothing to cache, but it is still ours and not the caller's.
            "licenseRedeemToken" => true,
            _ => false,
        }
    }

    /// A `<radio>` inside the tier tree changed. Unknown groups are captured
    /// rather than rejected: the server owns these trees and can add a control
    /// without a client release.
    pub fn on_radio_change(&mut self, group: &str, value: &str) {
        self.form_state.insert(group.to_owned(), value.to_owned());
    }

    /// An `<input>` inside the tier tree was committed.
    ///
    /// `label` is the last path segment, which is how the Store
    /// matches these too, so an arbitrarily deep server-served tree works.
    /// Returns `None` when the input is not one of ours.
    pub fn commit_input(&mut self, label: &str, value: &str) -> Option<Result<(), String>> {
        match label {
            "License redeem token" => Some(self.redeem(value, CLOUD_SLUG)),
            "Support redeem token" => Some(self.redeem(value, SUPPORT_SLUG)),
            // The donation amount, and the per-provider operator setup fields.
            // Captured for the checkout, never persisted.
            "amount in \u{20ac}" | "amount" => {
                self.form_state
                    .insert("amount".to_owned(), value.to_owned());
                Some(Ok(()))
            }
            l if l.ends_with("setup") => Some(Ok(())),
            _ => None,
        }
    }

    fn redeem(&self, token: &str, slug: &str) -> Result<(), String> {
        redeem_license(&self.store_url, token.trim(), slug)
    }

    /// A "for payment" button was pressed. `function_name` is `checkout:<item>`.
    ///
    /// Asks the server for a hosted checkout and opens it in the user's real
    /// browser: a payment page belongs in a browser with the user's session and
    /// their password manager, not rendered as FFON inside sicompass.
    ///
    /// Returns `None` when the button is not ours, so a provider can keep its
    /// own buttons.
    pub fn on_button_press(&mut self, function_name: &str) -> Option<Result<(), String>> {
        let item = function_name.strip_prefix("checkout:")?;
        let (resolved, amount, recurring) = self.resolve_item(item);
        Some(
            checkout::request_checkout(&self.store_url, &resolved, &amount, &recurring)
                .and_then(|url| checkout::open_url(&url)),
        )
    }

    /// Turn the server's button id into the concrete checkout item, using
    /// whatever the user picked in the tree.
    fn resolve_item(&self, item: &str) -> (String, String, String) {
        match item {
            // Sicompass Cloud and Commercial each come monthly or yearly.
            "cloud" | "commercial" => {
                // Yearly is the server's default checked option, so monthly
                // only when the user actively picked it.
                let monthly = self
                    .form_state
                    .get(CLOUD_PERIOD_GROUP)
                    .is_some_and(|v| v.contains("month"));
                let period = if monthly { "monthly" } else { "yearly" };
                (format!("{item}-{period}"), String::new(), String::new())
            }
            "sponsor-donation" => (
                "sponsor-donation".to_owned(),
                self.form_state.get("amount").cloned().unwrap_or_default(),
                self.form_state
                    .get("one-time or recurring")
                    .cloned()
                    .unwrap_or_default(),
            ),
            other => (other.to_owned(), String::new(), String::new()),
        }
    }
}

/// A status line for a tier link, from the certificate saved under `slug`.
pub fn status_line(slug: &str, label: &str) -> String {
    match cert::load(slug) {
        Some(c) => cert::verify(&c).summary_line(label),
        None => cert::LicenseStatus::None.summary_line(label),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(url: &str) -> TierSession {
        let mut s = TierSession::default();
        s.set_store_url(url);
        s
    }

    #[test]
    fn the_cloud_button_defaults_to_the_yearly_plan() {
        let s = session("https://srv.example");
        assert_eq!(s.resolve_item("cloud").0, "cloud-yearly");
    }

    #[test]
    fn picking_the_monthly_option_changes_the_item() {
        let mut s = session("https://srv.example");
        s.on_radio_change(CLOUD_PERIOD_GROUP, "per month");
        assert_eq!(s.resolve_item("cloud").0, "cloud-monthly");
    }

    #[test]
    fn picking_yearly_back_again_changes_it_back() {
        let mut s = session("https://srv.example");
        s.on_radio_change(CLOUD_PERIOD_GROUP, "per month");
        s.on_radio_change(CLOUD_PERIOD_GROUP, "per year");
        assert_eq!(s.resolve_item("cloud").0, "cloud-yearly");
    }

    #[test]
    fn commercial_comes_monthly_or_yearly_like_cloud() {
        let mut s = session("https://srv.example");
        assert_eq!(s.resolve_item("commercial").0, "commercial-yearly");
        s.on_radio_change(CLOUD_PERIOD_GROUP, "per month");
        assert_eq!(s.resolve_item("commercial").0, "commercial-monthly");
    }

    #[test]
    fn a_donation_carries_its_amount_and_cadence() {
        let mut s = session("https://srv.example");
        s.commit_input("amount in \u{20ac}", "25").unwrap().unwrap();
        s.on_radio_change("one-time or recurring", "recurring per month");
        let (item, amount, recurring) = s.resolve_item("sponsor-donation");
        assert_eq!(item, "sponsor-donation");
        assert_eq!(amount, "25");
        assert_eq!(recurring, "recurring per month");
    }

    /// The server owns these trees, so an item this client has never heard of
    /// has to pass straight through rather than be dropped.
    #[test]
    fn an_unknown_item_passes_through_untouched() {
        let s = session("https://srv.example");
        assert_eq!(s.resolve_item("sponsor-diamond").0, "sponsor-diamond");
        assert_eq!(
            s.resolve_item("something-new-in-2027").0,
            "something-new-in-2027"
        );
    }

    #[test]
    fn a_button_that_is_not_a_checkout_is_not_ours() {
        let mut s = session("https://srv.example");
        assert!(s.on_button_press("archive").is_none());
        assert!(s.on_button_press("").is_none());
    }

    #[test]
    fn a_checkout_against_an_unreachable_server_reports_one_error() {
        // Port 1 refuses immediately, so this neither hangs nor flakes.
        let mut s = session("http://127.0.0.1:1");
        let err = s.on_button_press("checkout:cloud").unwrap().unwrap_err();
        assert!(!err.is_empty());
    }

    #[test]
    fn provider_setup_inputs_are_accepted_but_not_stored() {
        let mut s = session("https://srv.example");
        assert!(s.commit_input("Lemonsqueezy setup", "acct-1").is_some());
        assert!(s.form_state.is_empty());
    }

    #[test]
    fn an_unrelated_input_is_not_ours() {
        let mut s = session("https://srv.example");
        assert!(s.commit_input("buy milk", "x").is_none());
    }

    /// An empty token is a no-op, not an error: it is what an untouched input
    /// commits, and it must not overwrite a certificate already on disk.
    #[test]
    fn an_empty_redeem_token_does_nothing() {
        let mut s = session("https://srv.example");
        assert!(s.commit_input("License redeem token", "").unwrap().is_ok());
    }

    #[test]
    fn store_url_changes_are_ours_and_other_settings_are_not() {
        let mut s = session("https://srv.example");
        assert!(s.on_setting_change("storeUrl", "https://other.example"));
        assert_eq!(s.store_url(), "https://other.example");
        assert!(s.on_setting_change("licenseRedeemToken", "tok"));
        assert!(!s.on_setting_change("colorScheme", "light"));
    }

    #[test]
    fn clearing_the_store_url_falls_back_to_the_default() {
        let mut s = session("https://srv.example");
        s.on_setting_change("storeUrl", "");
        assert_eq!(s.store_url(), crate::DEFAULT_STORE_URL);
    }
}
