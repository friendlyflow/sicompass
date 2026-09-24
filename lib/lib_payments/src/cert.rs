//! License certificate: schema, offline Ed25519 verification, load/save.
//!
//! A certificate is a small JSON document signed by the license server's
//! private key. The client embeds only the matching **public** key (below)
//! and verifies signatures offline — no network call, no server dependency
//! at runtime.
//!
//! Model A licensing: verification result is for **display / proof only**. It
//! never gates a feature. The full app is free under GPLv3 regardless of what
//! `verify()` returns. See memory `project_licensing_model`.
//!
//! The one caller that acts on the result rather than displaying it is
//! [`crate::entitlement`], and it decides a single thing: whether to send the
//! user's files to our server. Storage is a service we are paid for. What the
//! user may do with their own copy is never in question.
//!
//! ## Schema contract
//!
//! [`Payload`] is signed by serializing it with `serde_json` (field order is
//! the struct declaration order, so the bytes are deterministic). The license
//! server MUST define a byte-identical `Payload` struct, or signatures will
//! not verify. Keep the two definitions in sync.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Ed25519 public key (base64 of 32 raw bytes) that certificates are verified
/// against. The matching **private** key lives only on the license server.
///
/// This is a real key, not a placeholder: it matches the signing key in the
/// `server/` repo's `.env`, and `lib/lib_payments/tests/live_server.rs` checks
/// that a certificate that server signs verifies here.
///
/// To confirm, or after moving the server: run `cargo run --bin pubkey` there
/// and paste the result here. Do **not** run `cargo run --bin keygen` against
/// a server that already has a key, because it mints a new keypair and
/// invalidates every license already issued.
pub const LICENSE_PUBLIC_KEY_B64: &str = "BXZk+tykjgJhV/TJeW8vnf7BVXrGdQEyELMvOxLAoJ4=";

/// The signed portion of a certificate. Field order is the signing order —
/// it must stay identical to the server's `Payload`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Payload {
    /// Always `"sicompass"`. Guards against a certificate minted for a
    /// different product being accepted here.
    pub product: String,
    /// Unique license identifier (UUID).
    pub license_id: String,
    /// Human-readable licensee — person or organisation.
    pub licensee: String,
    /// License scope. Currently always `"commercial"`.
    pub scope: String,
    /// Issue time, Unix seconds.
    pub issued_at: i64,
    /// Expiry time, Unix seconds. Annual subscription, so this is ~1 year
    /// after `issued_at` and is refreshed by the server on renewal.
    pub expires_at: i64,
    /// Which versions the license covers. Currently `"*"` (all).
    pub version_coverage: String,
    /// Payment provider that processed the sale: `lemonsqueezy` / `paddle` /
    /// `polar`. Informational only.
    pub payment_provider: String,
}

/// A full certificate: the signed [`Payload`] plus its detached signature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Certificate {
    pub payload: Payload,
    /// base64 of the 64-byte Ed25519 signature over [`signing_message`].
    pub signature: String,
}

/// Outcome of verifying a certificate. Display-only — never a feature gate.
#[derive(Debug, Clone, PartialEq)]
pub enum LicenseStatus {
    /// No certificate file present.
    None,
    /// Valid signature, not yet expired.
    Active {
        licensee: String,
        renews_in_days: i64,
    },
    /// Valid signature, but past `expires_at`.
    Expired {
        licensee: String,
        expired_days_ago: i64,
    },
    /// Signature, key, product, or encoding check failed.
    Invalid(String),
}

impl LicenseStatus {
    /// One-line summary suffixed onto a tier link title. `label` names the
    /// license ("Cloud and store" / "Support"). Kept plain (no em dash) so a
    /// screen reader reads it cleanly.
    pub fn summary_line(&self, label: &str) -> String {
        match self {
            LicenseStatus::None => {
                format!("{label} license: none. sicompass is free under GPLv3.")
            }
            LicenseStatus::Active {
                licensee,
                renews_in_days,
            } => format!("{label} license: active, {licensee}, renews in {renews_in_days} days"),
            LicenseStatus::Expired {
                licensee,
                expired_days_ago,
            } => format!("{label} license: expired {expired_days_ago} days ago, {licensee}"),
            LicenseStatus::Invalid(why) => {
                format!("{label} license: invalid certificate ({why})")
            }
        }
    }
}

/// The exact bytes that are signed / verified for a payload.
///
/// `serde_json` serializes struct fields in declaration order, so this is
/// deterministic given a fixed [`Payload`] definition.
pub fn signing_message(payload: &Payload) -> Vec<u8> {
    serde_json::to_vec(payload).expect("Payload always serializes")
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Verify `cert` against an explicit base64 public key. The public entry point
/// [`verify`] calls this with [`LICENSE_PUBLIC_KEY_B64`]; tests pass their own.
pub fn verify_against(cert: &Certificate, public_key_b64: &str) -> LicenseStatus {
    let key_bytes = match STANDARD.decode(public_key_b64) {
        Ok(b) => b,
        Err(_) => return LicenseStatus::Invalid("public key is not valid base64".to_owned()),
    };
    let key_arr: [u8; 32] = match key_bytes.try_into() {
        Ok(a) => a,
        Err(_) => return LicenseStatus::Invalid("public key is not 32 bytes".to_owned()),
    };
    let verifying_key = match VerifyingKey::from_bytes(&key_arr) {
        Ok(k) => k,
        Err(_) => {
            return LicenseStatus::Invalid("public key is not a valid Ed25519 key".to_owned());
        }
    };

    let sig_bytes = match STANDARD.decode(&cert.signature) {
        Ok(b) => b,
        Err(_) => return LicenseStatus::Invalid("signature is not valid base64".to_owned()),
    };
    let sig_arr: [u8; 64] = match sig_bytes.try_into() {
        Ok(a) => a,
        Err(_) => return LicenseStatus::Invalid("signature is not 64 bytes".to_owned()),
    };
    let signature = Signature::from_bytes(&sig_arr);

    if verifying_key
        .verify(&signing_message(&cert.payload), &signature)
        .is_err()
    {
        return LicenseStatus::Invalid("signature does not match".to_owned());
    }

    if cert.payload.product != "sicompass" {
        return LicenseStatus::Invalid("certificate is not for sicompass".to_owned());
    }

    let now = now_unix();
    let licensee = cert.payload.licensee.clone();
    if cert.payload.expires_at < now {
        LicenseStatus::Expired {
            licensee,
            expired_days_ago: (now - cert.payload.expires_at) / 86_400,
        }
    } else {
        LicenseStatus::Active {
            licensee,
            renews_in_days: (cert.payload.expires_at - now) / 86_400,
        }
    }
}

/// Verify a certificate against the embedded production public key.
pub fn verify(cert: &Certificate) -> LicenseStatus {
    verify_against(cert, LICENSE_PUBLIC_KEY_B64)
}

// ---------------------------------------------------------------------------
// Tiers
// ---------------------------------------------------------------------------

/// The tiers sold (docs/plugin-platform.md §1 and §10). The server's
/// `cert::tier` holds the same ids.
///
/// A certificate names its tier in `scope`: a new `Payload` field would make
/// every new certificate fail to verify on 0.1.x, which re-serializes its own
/// `Payload` to check the signature and would drop a field it does not know.
pub mod tier {
    pub const CLOUD: &str = "friendlyflow/cloud";
    pub const COMMERCIAL: &str = "friendlyflow/commercial";
    pub const SPONSOR: &str = "friendlyflow/sponsor";
    pub const SUPPORT: &str = "friendlyflow/support";
}

/// Days a service stays on after its certificate expires, with a notice, so a
/// late renewal payment does not switch cloud backup off. The server allows
/// the same (`GRACE_SECS` there).
pub const GRACE_DAYS: i64 = 14;

/// The tier a certificate's `scope` names. Pre-tier scopes map onto tiers:
/// `commercial` (what "cloud and store" sold) is Sicompass Commercial, which
/// includes Sicompass Cloud, so nobody who paid before loses anything.
pub fn tier_of_scope(scope: &str) -> Option<&str> {
    match scope {
        "commercial" => Some(tier::COMMERCIAL),
        "support" => Some(tier::SUPPORT),
        "sponsor" => Some(tier::SPONSOR),
        s if s.contains('/') => Some(s),
        _ => None,
    }
}

/// Whether holding tier `held` gives what tier `wanted` gives. Commercial is
/// everything in Cloud plus the commercial licence.
pub fn tier_includes(held: &str, wanted: &str) -> bool {
    held == wanted || (held == tier::COMMERCIAL && wanted == tier::CLOUD)
}

/// The key a tier's certificates are signed with, for the tiers this client
/// knows itself. A third party's tier is signed by the issuer the store list
/// names for it, which the Store passes in.
pub fn known_issuer(tier_id: &str) -> Option<&'static str> {
    tier_id
        .starts_with("friendlyflow/")
        .then_some(LICENSE_PUBLIC_KEY_B64)
}

/// Where the user stands with one tier.
#[derive(Debug, Clone, PartialEq)]
pub enum TierStatus {
    /// No valid certificate for it.
    Missing,
    Active {
        licensee: String,
        renews_in_days: i64,
    },
    /// Expired, but within [`GRACE_DAYS`]: the service is still on.
    Grace { licensee: String, days_left: i64 },
    Expired {
        licensee: String,
        expired_days_ago: i64,
    },
}

impl TierStatus {
    /// Whether the tier's service is on: active, or in its grace period.
    pub fn is_on(&self) -> bool {
        matches!(self, TierStatus::Active { .. } | TierStatus::Grace { .. })
    }

    fn rank(&self) -> u8 {
        match self {
            TierStatus::Active { .. } => 3,
            TierStatus::Grace { .. } => 2,
            TierStatus::Expired { .. } => 1,
            TierStatus::Missing => 0,
        }
    }
}

/// A verified certificate's standing for the service it pays for: expiry
/// becomes grace for [`GRACE_DAYS`]. `None` for a status with no licensee.
pub fn with_grace(status: &LicenseStatus) -> TierStatus {
    match status {
        LicenseStatus::Active {
            licensee,
            renews_in_days,
        } => TierStatus::Active {
            licensee: licensee.clone(),
            renews_in_days: *renews_in_days,
        },
        LicenseStatus::Expired {
            licensee,
            expired_days_ago,
        } if *expired_days_ago < GRACE_DAYS => TierStatus::Grace {
            licensee: licensee.clone(),
            days_left: GRACE_DAYS - expired_days_ago,
        },
        LicenseStatus::Expired {
            licensee,
            expired_days_ago,
        } => TierStatus::Expired {
            licensee: licensee.clone(),
            expired_days_ago: *expired_days_ago,
        },
        LicenseStatus::None | LicenseStatus::Invalid(_) => TierStatus::Missing,
    }
}

/// The best standing any of `certs` gives for `tier_id`, counting only those
/// whose own tier includes it and that verify against `issuer`.
pub fn tier_status_among(certs: &[Certificate], tier_id: &str, issuer: &str) -> TierStatus {
    certs
        .iter()
        .filter(|c| {
            tier_of_scope(&c.payload.scope).is_some_and(|held| tier_includes(held, tier_id))
        })
        .map(|c| with_grace(&verify_against(c, issuer)))
        .max_by_key(TierStatus::rank)
        .unwrap_or(TierStatus::Missing)
}

/// The slugs certificates are saved under.
pub const SLUGS: [&str; 2] = [crate::CLOUD_SLUG, crate::SUPPORT_SLUG];

/// Every saved certificate, unverified: ours under [`SLUGS`], and a third
/// party's as `providers/license-<anything>.json`.
pub fn saved() -> Vec<Certificate> {
    let mut out: Vec<Certificate> = SLUGS.iter().filter_map(|slug| load(slug)).collect();
    if let Some(dir) = sicompass_sdk::platform::provider_config_dir()
        && let Ok(entries) = std::fs::read_dir(dir)
    {
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("license-") && n.ends_with(".json"))
            })
            .collect();
        paths.sort();
        out.extend(paths.iter().filter_map(|p| load_from(p)));
    }
    out
}

/// Where the user stands with `tier_id`, from the saved certificates, checked
/// against `issuer`.
pub fn tier_status(tier_id: &str, issuer: &str) -> TierStatus {
    tier_status_among(&saved(), tier_id, issuer)
}

/// On-disk location of the saved certificate for license `slug`
/// (`"store-license"` for cloud and store, `"support-license"` for support).
pub fn cert_path(slug: &str) -> Option<PathBuf> {
    sicompass_sdk::platform::provider_config_path(slug)
}

/// Load and parse the certificate from `path` (no verification).
pub fn load_from(path: &Path) -> Option<Certificate> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Persist a certificate to `path`, atomically. Returns `false` on failure.
pub fn save_to(path: &Path, cert: &Certificate) -> bool {
    if let Some(dir) = path.parent() {
        sicompass_sdk::platform::make_dirs(dir);
    }
    match serde_json::to_string_pretty(cert) {
        Ok(json) => sicompass_sdk::platform::atomic_write(path, &json),
        Err(_) => false,
    }
}

/// Load the saved certificate for license `slug`, if present.
pub fn load(slug: &str) -> Option<Certificate> {
    load_from(&cert_path(slug)?)
}

/// Save a certificate for license `slug`.
pub fn save(slug: &str, cert: &Certificate) -> bool {
    match cert_path(slug) {
        Some(p) => save_to(&p, cert),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    /// Deterministic test keypair (fixed seed — no RNG needed).
    fn test_keypair() -> (SigningKey, String) {
        let signing = SigningKey::from_bytes(&[7u8; 32]);
        let public_b64 = STANDARD.encode(signing.verifying_key().to_bytes());
        (signing, public_b64)
    }

    fn sample_payload(expires_at: i64) -> Payload {
        Payload {
            product: "sicompass".to_owned(),
            license_id: "11111111-2222-3333-4444-555555555555".to_owned(),
            licensee: "Acme Corp".to_owned(),
            scope: "commercial".to_owned(),
            issued_at: 1_700_000_000,
            expires_at,
            version_coverage: "*".to_owned(),
            payment_provider: "polar".to_owned(),
        }
    }

    fn sign(signing: &SigningKey, payload: Payload) -> Certificate {
        let signature = signing.sign(&signing_message(&payload));
        Certificate {
            payload,
            signature: STANDARD.encode(signature.to_bytes()),
        }
    }

    #[test]
    fn valid_future_certificate_is_active() {
        let (signing, pubkey) = test_keypair();
        let cert = sign(&signing, sample_payload(now_unix() + 200 * 86_400));
        match verify_against(&cert, &pubkey) {
            LicenseStatus::Active {
                licensee,
                renews_in_days,
            } => {
                assert_eq!(licensee, "Acme Corp");
                assert!(renews_in_days > 190 && renews_in_days <= 200);
            }
            other => panic!("expected Active, got {other:?}"),
        }
    }

    #[test]
    fn past_expiry_certificate_is_expired() {
        let (signing, pubkey) = test_keypair();
        let cert = sign(&signing, sample_payload(now_unix() - 10 * 86_400));
        match verify_against(&cert, &pubkey) {
            LicenseStatus::Expired {
                expired_days_ago, ..
            } => {
                assert!((9..=11).contains(&expired_days_ago));
            }
            other => panic!("expected Expired, got {other:?}"),
        }
    }

    #[test]
    fn tampered_payload_is_invalid() {
        let (signing, pubkey) = test_keypair();
        let mut cert = sign(&signing, sample_payload(now_unix() + 86_400));
        cert.payload.licensee = "Someone Else".to_owned();
        assert!(matches!(
            verify_against(&cert, &pubkey),
            LicenseStatus::Invalid(_)
        ));
    }

    #[test]
    fn wrong_public_key_is_invalid() {
        let (signing, _) = test_keypair();
        let cert = sign(&signing, sample_payload(now_unix() + 86_400));
        let other_pubkey = STANDARD.encode(
            SigningKey::from_bytes(&[9u8; 32])
                .verifying_key()
                .to_bytes(),
        );
        assert!(matches!(
            verify_against(&cert, &other_pubkey),
            LicenseStatus::Invalid(_)
        ));
    }

    #[test]
    fn wrong_product_is_invalid() {
        let (signing, pubkey) = test_keypair();
        let mut payload = sample_payload(now_unix() + 86_400);
        payload.product = "not-sicompass".to_owned();
        let cert = sign(&signing, payload);
        assert!(matches!(
            verify_against(&cert, &pubkey),
            LicenseStatus::Invalid(_)
        ));
    }

    #[test]
    fn save_then_load_round_trips() {
        let (signing, _) = test_keypair();
        let cert = sign(&signing, sample_payload(now_unix() + 86_400));
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("providers").join("store-license.json");
        assert!(save_to(&path, &cert));
        assert_eq!(load_from(&path), Some(cert));
    }

    #[test]
    fn load_missing_file_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_from(&dir.path().join("absent.json")).is_none());
    }

    #[test]
    fn summary_lines_have_no_em_dash() {
        // Keep status text plain for screen readers.
        for s in [
            LicenseStatus::None,
            LicenseStatus::Active {
                licensee: "X".into(),
                renews_in_days: 1,
            },
            LicenseStatus::Expired {
                licensee: "X".into(),
                expired_days_ago: 1,
            },
            LicenseStatus::Invalid("why".into()),
        ] {
            for label in ["Cloud and store", "Support"] {
                let line = s.summary_line(label);
                assert!(!line.contains('\u{2014}'));
                assert!(line.starts_with(label), "label prefix missing: {line}");
            }
        }
    }

    #[test]
    fn cert_path_is_per_slug() {
        let cloud = cert_path("store-license");
        let support = cert_path("support-license");
        assert!(cloud.is_some() && support.is_some());
        assert_ne!(cloud, support);
    }

    // ---- tiers ------------------------------------------------------------

    fn with_scope(signing: &SigningKey, scope: &str, expires_at: i64) -> Certificate {
        let mut p = sample_payload(expires_at);
        p.scope = scope.to_owned();
        sign(signing, p)
    }

    #[test]
    fn old_scopes_map_onto_tiers_and_commercial_includes_cloud() {
        assert_eq!(tier_of_scope("commercial"), Some(tier::COMMERCIAL));
        assert_eq!(tier_of_scope("support"), Some(tier::SUPPORT));
        assert_eq!(tier_of_scope("sponsor"), Some(tier::SPONSOR));
        assert_eq!(tier_of_scope("acme/pro"), Some("acme/pro"));
        assert_eq!(tier_of_scope("weird"), None);
        assert!(tier_includes(tier::COMMERCIAL, tier::CLOUD));
        assert!(!tier_includes(tier::CLOUD, tier::COMMERCIAL));
        assert!(!tier_includes(tier::SUPPORT, tier::CLOUD));
    }

    /// A certificate from before tiers still verifies, and still pays for
    /// cloud backup: its bytes are unchanged, since the tier is in `scope`.
    #[test]
    fn a_pre_tier_commercial_certificate_counts_as_cloud_and_commercial() {
        let (signing, pubkey) = test_keypair();
        let old = sign(&signing, sample_payload(now_unix() + 30 * 86_400));
        assert_eq!(old.payload.scope, "commercial");
        let json = serde_json::to_string(&old).unwrap();
        let reread: Certificate = serde_json::from_str(&json).unwrap();
        for t in [tier::CLOUD, tier::COMMERCIAL] {
            assert!(matches!(
                tier_status_among(std::slice::from_ref(&reread), t, &pubkey),
                TierStatus::Active { .. }
            ));
        }
        assert_eq!(
            tier_status_among(&[reread], tier::SUPPORT, &pubkey),
            TierStatus::Missing
        );
    }

    #[test]
    fn fourteen_days_of_grace_then_expired() {
        let (signing, pubkey) = test_keypair();
        let status = |days_ago: i64| {
            tier_status_among(
                &[with_scope(
                    &signing,
                    tier::CLOUD,
                    now_unix() - days_ago * 86_400 - 60,
                )],
                tier::CLOUD,
                &pubkey,
            )
        };
        assert!(matches!(status(0), TierStatus::Grace { days_left: 14, .. }));
        assert!(matches!(status(13), TierStatus::Grace { days_left: 1, .. }));
        assert!(status(13).is_on());
        assert!(matches!(status(14), TierStatus::Expired { .. }));
        assert!(!status(14).is_on());
    }

    #[test]
    fn the_best_certificate_for_a_tier_wins_and_others_do_not_count() {
        let (signing, pubkey) = test_keypair();
        let certs = [
            with_scope(&signing, tier::CLOUD, now_unix() - 30 * 86_400),
            with_scope(&signing, tier::SUPPORT, now_unix() + 90 * 86_400),
            with_scope(&signing, tier::COMMERCIAL, now_unix() + 10 * 86_400),
        ];
        assert!(matches!(
            tier_status_among(&certs, tier::CLOUD, &pubkey),
            TierStatus::Active {
                renews_in_days: 9..=10,
                ..
            }
        ));
        // Signed by someone else: not ours to believe.
        let (other, _) = (SigningKey::from_bytes(&[9u8; 32]), ());
        let forged = [with_scope(&other, tier::CLOUD, now_unix() + 86_400)];
        assert_eq!(
            tier_status_among(&forged, tier::CLOUD, &pubkey),
            TierStatus::Missing
        );
    }

    #[test]
    fn our_tiers_are_ours_to_verify_and_a_third_partys_are_not() {
        assert_eq!(known_issuer(tier::CLOUD), Some(LICENSE_PUBLIC_KEY_B64));
        assert_eq!(known_issuer("acme/pro"), None);
    }
}
