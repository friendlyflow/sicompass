//! Is the cloud and store subscription currently good?
//!
//! This is the one place in the client that asks the question, and it exists
//! for exactly one purpose: deciding whether to send a user's files to our
//! server. It never decides what the app will let someone do with their own
//! data. [`cert::LicenseStatus`] is otherwise display-only, and stays so.

use crate::{CLOUD_SLUG, cert};

/// The saved cloud and store certificate's status, verified offline.
pub fn cloud_status() -> cert::LicenseStatus {
    match cert::load(CLOUD_SLUG) {
        Some(c) => cert::verify(&c),
        None => cert::LicenseStatus::None,
    }
}

/// Whether the cloud and store subscription is signed, unexpired and therefore
/// good for cloud backup right now.
///
/// Note what this deliberately cannot see: revocation. A cancelled
/// subscription still holds a valid, unexpired certificate on disk, and only
/// the server knows it was cancelled. That is why the server checks again on
/// every request and is the authority; this check is here so the client does
/// not upload a customer's notes into a 403 on every keystroke.
pub fn is_active() -> bool {
    matches!(cloud_status(), cert::LicenseStatus::Active { .. })
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
}
