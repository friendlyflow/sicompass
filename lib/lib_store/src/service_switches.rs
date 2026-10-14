//! A plugin's service switch: the checkbox its `plugin.json` names as
//! `service.switch` ("enable cloud sync").
//!
//! The app shows that row only while the user holds the service's tier
//! (`programs::follow_services`). The Store ticks it, in `settings.json`, when
//! the user pays (redeems a token) and when it installs the plugin while the
//! tier is held. A renewal never ticks it, so a switch the user turned off
//! stays off, and nothing unticks it: after expiry the row is hidden and its
//! value kept, which is what the plugin's own row ("expired N days ago") reads.
//!
//! The Store writes the file rather than telling the app, because in a
//! desicompass session the Store runs in the superkey, another process. The
//! app follows the file either way.

use std::path::Path;

use serde_json::Value;
use sicompass_sdk::plugin_manifest::PluginManifest;

use crate::approvals::{edit, object_at};
use crate::payments::cert;

/// The plugins among `plugins` whose service the tier `held` pays for (a
/// Commercial license pays for Sicompass Cloud too) and that name a switch.
pub fn covered<'a>(
    held: &str,
    plugins: impl IntoIterator<Item = &'a PluginManifest>,
) -> Vec<&'a PluginManifest> {
    plugins
        .into_iter()
        .filter(|m| {
            m.service
                .as_ref()
                .is_some_and(|s| s.switch.is_some() && cert::tier_includes(held, &s.tier))
        })
        .collect()
}

/// Tick `m`'s switch in its own section of `settings` (keyed by its
/// `displayName`, as the settings page keeps it). Returns whether it has one.
pub fn tick(settings: &Path, m: &PluginManifest) -> Result<bool, String> {
    let Some(switch) = m.service.as_ref().and_then(|s| s.switch.as_deref()) else {
        return Ok(false);
    };
    edit(settings, |root| {
        object_at(root, &m.display_name).insert(switch.to_owned(), Value::Bool(true));
    })?;
    Ok(true)
}

/// The user paid for `held`: tick the switch of every plugin in `plugins` it
/// pays for. Returns the display names of the ones ticked.
pub fn tick_covered<'a>(
    settings: &Path,
    held: &str,
    plugins: impl IntoIterator<Item = &'a PluginManifest>,
) -> Vec<String> {
    covered(held, plugins)
        .into_iter()
        .filter(|m| match tick(settings, m) {
            Ok(ticked) => ticked,
            Err(e) => {
                eprintln!("sicompass: store: could not turn on {}'s service: {e}", m.name);
                false
            }
        })
        .map(|m| m.display_name.clone())
        .collect()
}

/// `m` was just installed (not updated): tick its switch when the user holds
/// its service's tier, as paying would have if it had been there then.
pub fn tick_if_held(settings: &Path, m: &PluginManifest) -> Result<bool, String> {
    let Some(service) = &m.service else {
        return Ok(false);
    };
    use sicompass_sdk::license::LicenseStatus;
    match crate::license_standing(&service.tier).status {
        LicenseStatus::Active | LicenseStatus::Grace => tick(settings, m),
        LicenseStatus::Expired | LicenseStatus::Missing => Ok(false),
    }
}

/// The tier a just-redeemed certificate pays for, if it is on (active or in
/// its grace period). An expired one redeemed is not a payment.
pub fn paid_tier(c: &cert::Certificate) -> Option<String> {
    cert::with_grace(&cert::verify(c))
        .is_on()
        .then(|| cert::tier_of_scope(&c.payload.scope).map(str::to_owned))
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sicompass_sdk::plugin_manifest::parse_manifest;

    fn plugin(display: &str, service: &str) -> PluginManifest {
        parse_manifest(&format!(
            r#"{{ "name": "{}", "displayName": "{display}", "type": "process",
                 "entry": "p" {service} }}"#,
            display.replace(' ', "")
        ))
        .unwrap()
    }

    fn cloud(display: &str, switch: &str) -> PluginManifest {
        plugin(
            display,
            &format!(r#", "service": {{ "tier": "friendlyflow/cloud", "switch": "{switch}" }}"#),
        )
    }

    fn read(settings: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(settings).unwrap()).unwrap()
    }

    #[test]
    fn paying_ticks_the_switch_of_every_plugin_the_tier_pays_for() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        std::fs::write(
            &settings,
            r#"{ "notes": { "other": 1 }, "Store": { "licenseRedeemToken": "t" } }"#,
        )
        .unwrap();
        let plugins = [
            cloud("notes", "notesCloudBackup"),
            cloud("project management", "kanbanCloudBackup"),
            // A service without a switch, another tier, and no service.
            plugin("sync less", r#", "service": { "tier": "friendlyflow/cloud" }"#),
            plugin(
                "acme",
                r#", "service": { "tier": "acme/pro", "switch": "acmeOn" }"#,
            ),
            plugin("plain", ""),
        ];

        let ticked = tick_covered(&settings, cert::tier::CLOUD, &plugins);

        assert_eq!(ticked, ["notes", "project management"]);
        let v = read(&settings);
        assert_eq!(v["notes"]["notesCloudBackup"], Value::Bool(true));
        assert_eq!(v["notes"]["other"], 1, "the rest of the section is kept");
        assert_eq!(v["project management"]["kanbanCloudBackup"], Value::Bool(true));
        assert_eq!(v["Store"]["licenseRedeemToken"], "t");
        assert!(v.get("acme").is_none());
        assert!(v.get("sync less").is_none());
    }

    #[test]
    fn commercial_pays_for_cloud_but_cloud_not_for_commercial() {
        let plugins = [
            cloud("notes", "notesCloudBackup"),
            plugin(
                "pro",
                r#", "service": { "tier": "friendlyflow/commercial", "switch": "proOn" }"#,
            ),
        ];
        let names = |held| -> Vec<String> {
            covered(held, &plugins)
                .iter()
                .map(|m| m.display_name.clone())
                .collect()
        };
        assert_eq!(names(cert::tier::COMMERCIAL), ["notes", "pro"]);
        assert_eq!(names(cert::tier::CLOUD), ["notes"]);
        assert!(names(cert::tier::SUPPORT).is_empty());
    }

    #[test]
    fn a_settings_file_that_does_not_parse_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        std::fs::write(&settings, "{ half").unwrap();
        assert!(tick(&settings, &cloud("notes", "notesCloudBackup")).is_err());
        assert_eq!(std::fs::read_to_string(&settings).unwrap(), "{ half");
    }

    #[test]
    fn an_install_without_the_tier_ticks_nothing() {
        // Under test no certificate is ours to read for an unknown tier.
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings.json");
        let m = plugin(
            "acme",
            r#", "service": { "tier": "nobody/tier", "switch": "acmeOn" }"#,
        );
        assert_eq!(tick_if_held(&settings, &m), Ok(false));
        assert!(!settings.exists());
    }

    #[test]
    fn a_certificate_that_does_not_verify_pays_for_nothing() {
        let c: cert::Certificate = serde_json::from_value(serde_json::json!({
            "payload": {
                "product": "sicompass", "license_id": "x", "licensee": "Acme",
                "scope": "friendlyflow/cloud", "issued_at": 0,
                "expires_at": 4_000_000_000i64, "version_coverage": "*",
                "payment_provider": "dev"
            },
            "signature": "AAAA"
        }))
        .unwrap();
        assert_eq!(paid_tier(&c), None);
    }
}
