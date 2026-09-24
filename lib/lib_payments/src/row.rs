//! The cloud row: the one line a cloud-backed provider puts at the top of its
//! first layer.
//!
//! It is always an `Obj` carrying `<link>{store}/cloud</link>`, paid or not —
//! only the wording changes. Three reasons for that:
//!
//! - The row never moves, so "where do I see whether my notes are backed up"
//!   has one answer.
//! - A paying user can still enter it to check the subscription or change it.
//! - It gives a provider's `reconcile` a single cheap test for the row it
//!   renders but does not store: it is the only row carrying a `<link>`.
//!
//! The row is rendered only when the provider's "enable cloud backup" setting
//! is on. With the setting off, nothing about payment appears anywhere in the
//! provider, because nothing about the provider is paid for.

use crate::cert::LicenseStatus;
use sicompass_sdk::ffon::FfonElement;
use sicompass_sdk::localize;
use sicompass_sdk::localize::Args as FluentArgs;

/// The tier page this row leads to.
pub fn cloud_url(store_url: &str) -> String {
    let base = store_url.trim_end_matches('/');
    let base = if base.is_empty() {
        crate::DEFAULT_STORE_URL
    } else {
        base
    };
    format!("{base}/cloud")
}

/// The row's wording for `status`.
pub fn label(status: &LicenseStatus) -> String {
    crate::register_translations();
    match status {
        LicenseStatus::None => localize::t("payments-cloud-needs-payment"),
        LicenseStatus::Active { renews_in_days, .. } => {
            let mut args = FluentArgs::new();
            args.set("days", *renews_in_days);
            localize::t_args("payments-cloud-active", &args)
        }
        LicenseStatus::Expired {
            expired_days_ago, ..
        } if *expired_days_ago < crate::cert::GRACE_DAYS => {
            let mut args = FluentArgs::new();
            args.set("days", crate::cert::GRACE_DAYS - expired_days_ago);
            localize::t_args("payments-cloud-grace", &args)
        }
        LicenseStatus::Expired {
            expired_days_ago, ..
        } => {
            let mut args = FluentArgs::new();
            args.set("days", *expired_days_ago);
            localize::t_args("payments-cloud-expired", &args)
        }
        LicenseStatus::Invalid(why) => {
            let mut args = FluentArgs::new();
            args.set("why", why.clone());
            localize::t_args("payments-cloud-invalid", &args)
        }
    }
}

/// Build the row. Following it fetches the server's cloud tier tree and grafts
/// it in place, which is where the user picks a plan, pays, and pastes their
/// redeem token — see [`crate::tier_input`] for the handlers that make those
/// controls work once they are sitting in a provider's own tree.
pub fn cloud_row(store_url: &str, status: &LicenseStatus) -> FfonElement {
    FfonElement::new_obj(format!(
        "{} <link>{}</link>",
        label(status),
        cloud_url(store_url)
    ))
}

/// Whether `raw` is a row this module rendered.
///
/// A provider's `reconcile` uses this to drop the cloud row before diffing a
/// list back into its store. The test is "carries a link", not "equals this
/// text", because the wording changes with the subscription's state and with
/// the user's language. It is safe in notes and in project management because
/// both escape `<` and `>` in the text they store, so no note and no card can
/// ever carry a real `<link>`.
pub fn is_cloud_row(raw: &str) -> bool {
    sicompass_sdk::tags::has_link(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_row_links_to_the_cloud_tier() {
        let row = cloud_row("https://srv.example", &LicenseStatus::None);
        let FfonElement::Obj(o) = row else {
            panic!("the cloud row must be an Obj, or the app will not follow it");
        };
        assert!(
            o.key.contains("<link>https://srv.example/cloud</link>"),
            "{}",
            o.key
        );
        // Childless, so the app's link resolver fetches and grafts the tier
        // tree the first time it is entered.
        assert!(o.children.is_empty());
    }

    #[test]
    fn a_trailing_slash_does_not_double_up() {
        assert_eq!(
            cloud_url("https://srv.example/"),
            "https://srv.example/cloud"
        );
    }

    #[test]
    fn an_empty_store_url_falls_back_to_the_default() {
        assert_eq!(cloud_url(""), format!("{}/cloud", crate::DEFAULT_STORE_URL));
    }

    #[test]
    fn the_wording_follows_the_subscription() {
        let unpaid = label(&LicenseStatus::None);
        let active = label(&LicenseStatus::Active {
            licensee: "Acme Corp".to_owned(),
            renews_in_days: 342,
        });
        assert_ne!(unpaid, active);
        assert!(active.contains("342"), "{active}");
        assert!(
            !unpaid.is_empty() && !unpaid.starts_with("payments-"),
            "{unpaid}"
        );
    }

    /// The screen reader reads these out, so they follow the same rule as the
    /// rest of the app's prose.
    #[test]
    fn no_wording_uses_an_em_dash_or_a_semicolon() {
        for status in [
            LicenseStatus::None,
            LicenseStatus::Active {
                licensee: "Acme Corp".to_owned(),
                renews_in_days: 1,
            },
            LicenseStatus::Expired {
                licensee: "Acme Corp".to_owned(),
                expired_days_ago: 1,
            },
            LicenseStatus::Expired {
                licensee: "Acme Corp".to_owned(),
                expired_days_ago: 30,
            },
            LicenseStatus::Invalid("bad".to_owned()),
        ] {
            let text = label(&status);
            assert!(!text.contains('\u{2014}'), "em dash in: {text}");
            assert!(!text.contains(';'), "semicolon in: {text}");
        }
    }

    #[test]
    fn the_cloud_row_is_recognised_and_ordinary_text_is_not() {
        let FfonElement::Obj(o) = cloud_row("https://srv.example", &LicenseStatus::None) else {
            unreachable!()
        };
        assert!(is_cloud_row(&o.key));
        assert!(!is_cloud_row("<id>7</id><input>buy milk</input>"));
        assert!(!is_cloud_row("a note that merely mentions a link"));
    }

    // ---- Locale parity ----------------------------------------------------

    #[test]
    fn all_four_bundles_carry_the_same_keys() {
        fn keys(ftl: &str) -> Vec<String> {
            ftl.lines()
                .filter(|l| !l.trim_start().starts_with('#') && l.contains('='))
                .filter_map(|l| l.split('=').next())
                .map(|k| k.trim().to_owned())
                .filter(|k| !k.is_empty() && !k.contains(' '))
                .collect()
        }
        let en = keys(include_str!("../locales/en-US.ftl"));
        assert!(!en.is_empty());
        for (name, ftl) in [
            ("nl-BE", include_str!("../locales/nl-BE.ftl")),
            ("fr-BE", include_str!("../locales/fr-BE.ftl")),
            ("de-BE", include_str!("../locales/de-BE.ftl")),
        ] {
            assert_eq!(keys(ftl), en, "{name} has drifted from en-US");
        }
    }

    /// `localize::t` hands back the key itself when a string is missing, so a
    /// forgotten entry shows up in the UI as `payments-cloud-active` rather
    /// than failing anywhere. These are the ones a user actually reads.
    #[test]
    fn every_string_resolves_to_real_text() {
        crate::register_translations();
        for key in [
            "payments-cloud-needs-payment",
            "payments-cloud-active",
            "payments-cloud-expired",
            "payments-cloud-grace",
            "payments-cloud-invalid",
            "payments-cloud-needs-subscription",
            "payments-backup-failed",
            "payments-command-restore",
            "payments-restore-done",
            "payments-restore-empty",
            "payments-restore-refused",
        ] {
            assert_ne!(
                sicompass_sdk::localize::t(key),
                key,
                "{key} is missing from the bundles"
            );
        }
    }

    #[test]
    fn in_the_grace_period_the_row_says_how_long_is_left() {
        crate::register_translations();
        let grace = label(&LicenseStatus::Expired {
            licensee: "Acme Corp".to_owned(),
            expired_days_ago: 3,
        });
        assert!(grace.contains("11"), "{grace}");
        let expired = label(&LicenseStatus::Expired {
            licensee: "Acme Corp".to_owned(),
            expired_days_ago: 20,
        });
        assert!(expired.contains("20"), "{expired}");
        assert_ne!(grace, expired);
    }
}
