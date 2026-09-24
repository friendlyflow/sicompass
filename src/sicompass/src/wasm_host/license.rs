//! `sicompass:plugin/license`: where the user stands with a paid tier, and the
//! credential for the plugin's own service.
//!
//! Always linked. It hands out one of four words with a day count, and a redeem
//! token only for the tier the plugin's `plugin.json` names as its `service`:
//! a plugin never gets the token of another plugin's service, and never a key
//! or a certificate. The answers come from what the Store registers
//! (`sicompass_sdk::license`). Unregistered, every tier is `missing`.

use sicompass_sdk::license::{LicenseStatus, Standing};

use super::HostState;
use super::sicompass::plugin as wit;

use wit::license::{TierStanding, TierStatus};

/// The WIT answer for an SDK status.
pub fn to_wit(status: LicenseStatus) -> TierStatus {
    match status {
        LicenseStatus::Active => TierStatus::Active,
        LicenseStatus::Grace => TierStatus::Grace,
        LicenseStatus::Expired => TierStatus::Expired,
        LicenseStatus::Missing => TierStatus::Missing,
    }
}

fn to_wit_standing(s: Standing) -> TierStanding {
    TierStanding {
        status: to_wit(s.status),
        days: s.days,
    }
}

impl wit::license::Host for HostState {
    fn status(&mut self, tier: String) -> TierStatus {
        to_wit(sicompass_sdk::license::status(&tier))
    }

    fn standing(&mut self, tier: String) -> TierStanding {
        to_wit_standing(sicompass_sdk::license::standing(&tier))
    }

    fn token(&mut self, tier: String) -> Option<String> {
        if self.service_tier.as_deref() != Some(tier.as_str()) {
            return None;
        }
        sicompass_sdk::license::token(&tier)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_registered_check_is_what_a_plugin_hears() {
        let mut s = HostState::new("demo", "demo", "/tmp/sicompass-test-plugin", Vec::new());
        sicompass_sdk::license::register_checker(|tier| match tier {
            "acme/pro" => Standing {
                status: LicenseStatus::Active,
                days: 200,
            },
            "acme/late" => Standing {
                status: LicenseStatus::Grace,
                days: 3,
            },
            "acme/old" => Standing {
                status: LicenseStatus::Expired,
                days: 40,
            },
            _ => Standing::MISSING,
        });
        let ask = |s: &mut HostState, t: &str| wit::license::Host::status(s, t.to_owned());
        assert_eq!(ask(&mut s, "acme/pro"), TierStatus::Active);
        assert_eq!(ask(&mut s, "acme/late"), TierStatus::Grace);
        assert_eq!(ask(&mut s, "acme/old"), TierStatus::Expired);
        assert_eq!(ask(&mut s, "someone/else"), TierStatus::Missing);
        let late = wit::license::Host::standing(&mut s, "acme/late".to_owned());
        assert_eq!((late.status, late.days), (TierStatus::Grace, 3));
    }

    #[test]
    fn a_token_only_for_the_plugins_own_service() {
        sicompass_sdk::license::register_token_source(|tier| Some(format!("token-for-{tier}")));
        let mut s = HostState::new("notes", "notes", "/tmp/sicompass-test-plugin", Vec::new());
        // No service declared: no token at all.
        assert_eq!(
            wit::license::Host::token(&mut s, "friendlyflow/cloud".into()),
            None
        );
        s.service_tier = Some("friendlyflow/cloud".to_owned());
        assert_eq!(
            wit::license::Host::token(&mut s, "friendlyflow/cloud".into()).as_deref(),
            Some("token-for-friendlyflow/cloud")
        );
        // Another tier, even a real one: no.
        assert_eq!(
            wit::license::Host::token(&mut s, "friendlyflow/support".into()),
            None
        );
        assert_eq!(wit::license::Host::token(&mut s, "acme/pro".into()), None);
    }
}
