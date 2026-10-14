//! Bring a renewed certificate in.
//!
//! A renewal re-issues the certificate on the server under the same redeem
//! token (`GET /license/<token>` answers with the new one), but nothing told
//! the client. Without this a monthly subscriber read as expired a month after
//! redeeming, however long they kept paying, and their service switch
//! (`crate::service_switches`) disappeared from the settings page.
//!
//! So whenever a standing is asked (`crate::license_standing`), at most once
//! an hour, a saved certificate that expires within [`DUE_WITHIN_SECS`] (or
//! has) is asked for again, on a thread of its own. A failure is logged, never
//! shown: the certificate on disk stays what it was. A refresh never ticks a
//! service switch, since a renewal is not a new payment.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::{CLOUD_SLUG, SUPPORT_SLUG, cert, config, fetch_certificate};

/// Ask for a renewal this long before the certificate expires.
pub const DUE_WITHIN_SECS: i64 = 3 * 86_400;

/// How often [`refresh_if_due`] looks at all.
const EVERY: Duration = Duration::from_secs(60 * 60);

static LAST_LOOK: Mutex<Option<Instant>> = Mutex::new(None);

/// Whether `c` should be asked for again at `now` (Unix seconds).
pub fn is_due(c: &cert::Certificate, now: i64) -> bool {
    c.payload.expires_at - now < DUE_WITHIN_SECS
}

/// The certificate to keep instead of `current`: `fetched`, when it verifies
/// against `issuer` and lasts longer.
pub fn renewed(
    current: &cert::Certificate,
    fetched: cert::Certificate,
    issuer: &str,
) -> Option<cert::Certificate> {
    if matches!(
        cert::verify_against(&fetched, issuer),
        cert::LicenseStatus::Invalid(_)
    ) {
        return None;
    }
    (fetched.payload.expires_at > current.payload.expires_at).then_some(fetched)
}

/// Ask the server at `store_url` for `token`'s certificate, and return it when
/// it renews `current`.
pub fn renewal_from(
    store_url: &str,
    token: &str,
    current: &cert::Certificate,
    issuer: &str,
) -> Result<Option<cert::Certificate>, String> {
    Ok(renewed(current, fetch_certificate(store_url, token)?, issuer))
}

/// Fetch and keep a renewal of the certificate saved under `slug`, on this
/// thread. Returns whether one was saved.
pub fn refresh_now(store_url: &str, token: &str, slug: &str) -> Result<bool, String> {
    let Some(current) = cert::load(slug) else {
        return Ok(false);
    };
    match renewal_from(store_url, token, &current, cert::LICENSE_PUBLIC_KEY_B64)? {
        Some(c) if cert::save(slug, &c) => Ok(true),
        Some(_) => Err("could not save the renewed license file".to_owned()),
        None => Ok(false),
    }
}

/// At most once an hour: for each of our saved certificates that is due and
/// has a redeem token, ask the server for its renewal on a thread of its own.
pub fn refresh_if_due() {
    {
        let mut last = LAST_LOOK.lock().unwrap_or_else(|e| e.into_inner());
        if last.is_some_and(|t| t.elapsed() < EVERY) {
            return;
        }
        *last = Some(Instant::now());
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let due: Vec<(&'static str, String)> = [
        (CLOUD_SLUG, config::redeem_token()),
        (SUPPORT_SLUG, config::support_redeem_token()),
    ]
    .into_iter()
    .filter(|(slug, token)| !token.is_empty() && cert::load(slug).is_some_and(|c| is_due(&c, now)))
    .collect();
    if due.is_empty() {
        return;
    }
    let store_url = config::store_url();
    let _ = std::thread::Builder::new()
        .name("store:license-refresh".into())
        .spawn(move || {
            for (slug, token) in due {
                match refresh_now(&store_url, &token, slug) {
                    Ok(true) => eprintln!("sicompass: store: {slug} renewed"),
                    Ok(false) => {}
                    Err(e) => eprintln!("sicompass: store: could not renew {slug}: {e}"),
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use ed25519_dalek::{Signer, SigningKey};
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn key() -> (SigningKey, String) {
        let signing = SigningKey::from_bytes(&[11u8; 32]);
        let public = base64::engine::general_purpose::STANDARD
            .encode(signing.verifying_key().to_bytes());
        (signing, public)
    }

    fn signed(signing: &SigningKey, expires_at: i64) -> cert::Certificate {
        let payload = cert::Payload {
            product: "sicompass".to_owned(),
            license_id: "id".to_owned(),
            licensee: "Acme".to_owned(),
            scope: cert::tier::CLOUD.to_owned(),
            issued_at: 1_700_000_000,
            expires_at,
            version_coverage: "*".to_owned(),
            payment_provider: "dev".to_owned(),
        };
        let signature = signing.sign(&cert::signing_message(&payload));
        cert::Certificate {
            payload,
            signature: base64::engine::general_purpose::STANDARD.encode(signature.to_bytes()),
        }
    }

    #[test]
    fn due_three_days_before_expiry_and_after_it() {
        let (s, _) = key();
        let c = signed(&s, 1_000_000);
        assert!(!is_due(&c, 1_000_000 - DUE_WITHIN_SECS - 1));
        assert!(is_due(&c, 1_000_000 - DUE_WITHIN_SECS + 1));
        assert!(is_due(&c, 2_000_000), "an expired one is asked for too");
    }

    #[test]
    fn only_a_later_certificate_that_verifies_is_kept() {
        let (s, public) = key();
        let current = signed(&s, 1_800_000_000);
        let later = signed(&s, 1_900_000_000);
        assert_eq!(renewed(&current, later.clone(), &public), Some(later.clone()));
        assert_eq!(renewed(&current, signed(&s, 1_700_000_000), &public), None);
        assert_eq!(renewed(&current, current.clone(), &public), None);
        let (other, _) = (SigningKey::from_bytes(&[12u8; 32]), ());
        assert_eq!(
            renewed(&current, signed(&other, 1_900_000_000), &public),
            None,
            "signed by another key"
        );
    }

    #[test]
    fn the_renewal_is_asked_of_the_server_under_the_same_token() {
        let (s, public) = key();
        let current = signed(&s, 1_800_000_000);
        let later = signed(&s, 1_900_000_000);
        let rt = tokio::runtime::Runtime::new().unwrap();
        let server = rt.block_on(MockServer::start());
        rt.block_on(
            Mock::given(method("GET"))
                .and(path("/license/tok"))
                .respond_with(ResponseTemplate::new(200).set_body_json(&later))
                .mount(&server),
        );
        assert_eq!(
            renewal_from(&server.uri(), "tok", &current, &public),
            Ok(Some(later))
        );
        assert!(renewal_from(&server.uri(), "gone", &current, &public).is_err());
    }

    #[test]
    fn under_test_there_is_no_token_so_nothing_is_asked() {
        // The payments config reads no settings under test: no token, so
        // `refresh_if_due` never reaches a server from a test.
        assert_eq!(config::redeem_token(), "");
        refresh_if_due();
    }
}
