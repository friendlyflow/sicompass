//! `sicompass:plugin/license`: whether the user holds a paid tier.
//!
//! Always linked, and it hands out nothing but one of four words: the key and
//! the certificate stay on the host side. The answer comes from the check the
//! Store registers (`sicompass_sdk::license`), which verifies the user's
//! certificate against the issuer key the signed store list names for the
//! tier. Unregistered, every tier is `missing`.

use sicompass_sdk::license::LicenseStatus;

use super::HostState;
use super::sicompass::plugin as wit;

use wit::license::TierStatus;

/// The WIT answer for an SDK status.
pub fn to_wit(status: LicenseStatus) -> TierStatus {
    match status {
        LicenseStatus::Active => TierStatus::Active,
        LicenseStatus::Grace => TierStatus::Grace,
        LicenseStatus::Expired => TierStatus::Expired,
        LicenseStatus::Missing => TierStatus::Missing,
    }
}

impl wit::license::Host for HostState {
    fn status(&mut self, tier: String) -> TierStatus {
        to_wit(sicompass_sdk::license::status(&tier))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registered_check_is_what_a_plugin_hears() {
        let mut s = HostState::new("demo", "demo", "/tmp/sicompass-test-plugin", Vec::new());
        sicompass_sdk::license::register_checker(|tier| match tier {
            "acme/pro" => LicenseStatus::Active,
            "acme/late" => LicenseStatus::Grace,
            "acme/old" => LicenseStatus::Expired,
            _ => LicenseStatus::Missing,
        });
        let ask = |s: &mut HostState, t: &str| wit::license::Host::status(s, t.to_owned());
        assert_eq!(ask(&mut s, "acme/pro"), TierStatus::Active);
        assert_eq!(ask(&mut s, "acme/late"), TierStatus::Grace);
        assert_eq!(ask(&mut s, "acme/old"), TierStatus::Expired);
        assert_eq!(ask(&mut s, "someone/else"), TierStatus::Missing);
    }
}
