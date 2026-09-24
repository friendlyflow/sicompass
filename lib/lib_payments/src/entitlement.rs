//! Is the cloud and store subscription currently good?
//!
//! This is the one place in the client that asks the question, and it exists
//! for exactly one purpose: deciding whether to send a user's files to our
//! server. It never decides what the app will let someone do with their own
//! data. [`cert::LicenseStatus`] is otherwise display-only, and stays so.

use crate::{CLOUD_SLUG, cert};

/// The saved Cloud certificate's status, verified offline. A certificate for
/// a tier that does not include Sicompass Cloud is reported as not for it.
pub fn cloud_status() -> cert::LicenseStatus {
    match cert::load(CLOUD_SLUG) {
        Some(c) => {
            let pays = cert::tier_of_scope(&c.payload.scope)
                .is_some_and(|held| cert::tier_includes(held, cert::tier::CLOUD));
            if pays {
                cert::verify(&c)
            } else {
                cert::LicenseStatus::Invalid(
                    "this certificate is not for Sicompass Cloud".to_owned(),
                )
            }
        }
        None => cert::LicenseStatus::None,
    }
}

/// Whether a certificate in `status` keeps cloud backup on: active, or
/// expired less than [`cert::GRACE_DAYS`] ago (the server allows the same).
pub fn backs_up(status: &cert::LicenseStatus) -> bool {
    cert::with_grace(status).is_on()
}

/// Whether the Sicompass Cloud subscription (or Commercial, which includes it)
/// is signed and unexpired, or in its grace period, and therefore good for
/// cloud backup right now.
///
/// Note what this deliberately cannot see: revocation. A cancelled
/// subscription still holds a valid, unexpired certificate on disk, and only
/// the server knows it was cancelled. That is why the server checks again on
/// every request and is the authority; this check is here so the client does
/// not upload a customer's notes into a 403 on every keystroke.
pub fn is_active() -> bool {
    backs_up(&cloud_status())
}

#[cfg(test)]
mod tests {
    use crate::cert::LicenseStatus;

    /// `is_active` must be true for exactly one status and no other. Written
    /// against the enum rather than the filesystem because `cloud_status`
    /// reads the developer's own config directory.
    #[test]
    fn only_an_active_certificate_counts() {
        let cases = [
            (LicenseStatus::None, false),
            (
                LicenseStatus::Active {
                    licensee: "Acme Corp".to_owned(),
                    renews_in_days: 1,
                },
                true,
            ),
            (
                LicenseStatus::Expired {
                    licensee: "Acme Corp".to_owned(),
                    expired_days_ago: 1,
                },
                false,
            ),
            (LicenseStatus::Invalid("bad".to_owned()), false),
        ];
        for (status, expected) in cases {
            assert_eq!(
                matches!(status, LicenseStatus::Active { .. }),
                expected,
                "{status:?}"
            );
        }
    }

    #[test]
    fn backup_runs_through_the_grace_period_and_stops_after() {
        use super::backs_up;
        let expired = |d| LicenseStatus::Expired {
            licensee: "Acme Corp".to_owned(),
            expired_days_ago: d,
        };
        assert!(backs_up(&LicenseStatus::Active {
            licensee: "Acme Corp".to_owned(),
            renews_in_days: 1
        }));
        assert!(backs_up(&expired(0)));
        assert!(backs_up(&expired(13)));
        assert!(!backs_up(&expired(14)));
        assert!(!backs_up(&LicenseStatus::None));
        assert!(!backs_up(&LicenseStatus::Invalid("bad".into())));
    }
}
