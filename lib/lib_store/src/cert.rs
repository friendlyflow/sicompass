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
//! ## Schema contract
//!
//! [`Payload`] is signed by serializing it with `serde_json` (field order is
//! the struct declaration order, so the bytes are deterministic). The license
//! server MUST define a byte-identical `Payload` struct, or signatures will
//! not verify. Keep the two definitions in sync.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Ed25519 public key (base64 of 32 raw bytes) that certificates are verified
/// against. The matching **private** key lives only on the license server.
///
/// This is a placeholder (32 zero bytes) and will reject every real
/// certificate until replaced. To set up a real key: in the `server/` repo run
/// `cargo run --bin keygen`, paste the printed public key here, and put the
/// printed private key in the server's `.env` as `SICOMPASS_SIGNING_KEY`.
pub const LICENSE_PUBLIC_KEY_B64: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

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
    Active { licensee: String, renews_in_days: i64 },
    /// Valid signature, but past `expires_at`.
    Expired { licensee: String, expired_days_ago: i64 },
    /// Signature, key, product, or encoding check failed.
    Invalid(String),
}

impl LicenseStatus {
    /// One-line summary rendered at the top of the store provider.
    pub fn summary_line(&self) -> String {
        match self {
            LicenseStatus::None => {
                "Commercial license: none. sicompass is free under GPLv3.".to_owned()
            }
            LicenseStatus::Active { licensee, renews_in_days } => format!(
                "Commercial license: active, {licensee}, renews in {renews_in_days} days"
            ),
            LicenseStatus::Expired { licensee, expired_days_ago } => format!(
                "Commercial license: expired {expired_days_ago} days ago, {licensee}"
            ),
            LicenseStatus::Invalid(why) => {
                format!("Commercial license: invalid certificate ({why})")
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
        Err(_) => return LicenseStatus::Invalid("public key is not a valid Ed25519 key".to_owned()),
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

/// On-disk location of the saved certificate.
pub fn cert_path() -> Option<PathBuf> {
    sicompass_sdk::platform::provider_config_path("store-license")
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

/// Load the saved certificate from the standard location, if present.
pub fn load() -> Option<Certificate> {
    load_from(&cert_path()?)
}

/// Save a certificate to the standard location.
pub fn save(cert: &Certificate) -> bool {
    match cert_path() {
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
            LicenseStatus::Active { licensee, renews_in_days } => {
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
            LicenseStatus::Expired { expired_days_ago, .. } => {
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
        let other_pubkey = STANDARD.encode(SigningKey::from_bytes(&[9u8; 32]).verifying_key().to_bytes());
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
            LicenseStatus::Active { licensee: "X".into(), renews_in_days: 1 },
            LicenseStatus::Expired { licensee: "X".into(), expired_days_ago: 1 },
            LicenseStatus::Invalid("why".into()),
        ] {
            assert!(!s.summary_line().contains('\u{2014}'));
        }
    }
}
