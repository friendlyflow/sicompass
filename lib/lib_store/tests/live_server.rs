//! End-to-end checks of the certificates against a real license server.
//!
//! Ignored by default: they need a server running, which `cargo test` must not
//! depend on. Run them by hand after changing either side of the wire. The
//! backup half of these checks moved with the backup protocol to the
//! `sicompass-payments` guest library (`sicompass-payments/` in the SDK repo).
//!
//! ```sh
//! # in the server repo
//! DATABASE_URL=sqlite:///tmp/e2e.db BIND_ADDR=127.0.0.1:8799 \
//!   SICOMPASS_DEV_ISSUE=1 cargo run
//!
//! TOKEN=$(curl -s -XPOST localhost:8799/dev/issue \
//!   -H 'content-type: application/json' \
//!   -d '{"licensee":"Acme Corp","email":"acme@example.com"}' | jq -r .redeem_token)
//!
//! # in this repo
//! SICOMPASS_TEST_SERVER=http://127.0.0.1:8799 SICOMPASS_TEST_TOKEN=$TOKEN \
//!   cargo test -p sicompass-store --test live_server -- --ignored --test-threads=1
//! ```
//!
//! Nothing here touches the user's config directory: the certificates are
//! verified in memory rather than redeemed.

use sicompass_store::payments::{cert, usage};

fn server() -> String {
    std::env::var("SICOMPASS_TEST_SERVER").expect("set SICOMPASS_TEST_SERVER")
}

fn token() -> String {
    std::env::var("SICOMPASS_TEST_TOKEN").expect("set SICOMPASS_TEST_TOKEN")
}

/// The contract that decides whether anyone can ever be "paid": a certificate
/// this server signs has to verify against the public key compiled into the
/// client. If the two keys drift, every subscription reads as invalid.
#[test]
#[ignore = "needs a running license server"]
fn a_certificate_from_the_server_verifies_against_the_embedded_key() {
    let url = format!("{}/license/{}", server().trim_end_matches('/'), token());
    let body = reqwest::blocking::get(&url)
        .expect("server unreachable")
        .text()
        .expect("no body");
    let certificate: cert::Certificate =
        serde_json::from_str(&body).expect("server returned something that is not a certificate");

    match cert::verify(&certificate) {
        cert::LicenseStatus::Active { licensee, .. } => {
            assert!(!licensee.is_empty());
        }
        other => panic!(
            "the server's signature did not verify against LICENSE_PUBLIC_KEY_B64: {other:?}\n\
             Run `cargo run --bin pubkey` in the server repo and paste the result into cert.rs."
        ),
    }
}

// ---- Tiers, grace and usage (4.9) ---------------------------------------------
//
// These mint their own licenses through `POST /dev/issue`, so the server must
// run with SICOMPASS_DEV_ISSUE=1 (as in the recipe above).

/// Mint a license for checkout `item`, lasting `term_secs` (negative: already
/// expired that long ago). Returns the redeem token.
fn issue(item: &str, term_secs: i64) -> String {
    let reply: serde_json::Value = reqwest::blocking::Client::new()
        .post(format!("{}/dev/issue", server().trim_end_matches('/')))
        .json(&serde_json::json!({
            "licensee": "Acme Corp", "email": "acme@example.com",
            "item": item, "term_secs": term_secs
        }))
        .send()
        .expect("server unreachable")
        .json()
        .expect("dev issue reply");
    reply["redeem_token"]
        .as_str()
        .expect("dev issue is off: run the server with SICOMPASS_DEV_ISSUE=1")
        .to_owned()
}

fn certificate(token: &str) -> cert::Certificate {
    let url = format!("{}/license/{token}", server().trim_end_matches('/'));
    reqwest::blocking::get(&url)
        .expect("server unreachable")
        .json()
        .expect("not a certificate")
}

/// What the server sells verifies here and lands in the right tier, and
/// Commercial includes Cloud.
#[test]
#[ignore = "needs a running license server"]
fn cloud_and_commercial_certificates_map_onto_their_tiers() {
    let key = cert::LICENSE_PUBLIC_KEY_B64;
    let cloud = certificate(&issue("cloud-monthly", 31 * 86_400));
    assert_eq!(cloud.payload.scope, cert::tier::CLOUD);
    let one = std::slice::from_ref(&cloud);
    assert!(cert::tier_status_among(one, cert::tier::CLOUD, key).is_on());
    assert_eq!(
        cert::tier_status_among(one, cert::tier::COMMERCIAL, key),
        cert::TierStatus::Missing
    );

    let commercial = certificate(&issue("commercial-yearly", 365 * 86_400));
    let one = std::slice::from_ref(&commercial);
    assert!(cert::tier_status_among(one, cert::tier::COMMERCIAL, key).is_on());
    assert!(cert::tier_status_among(one, cert::tier::CLOUD, key).is_on());
}

/// Three days past expiry the client counts a Cloud certificate as in grace;
/// fifteen days past, as expired. (The server side of the same rule is checked
/// with the backup protocol, in the SDK repo's `sicompass-payments/`.)
#[test]
#[ignore = "needs a running license server"]
fn the_grace_period_holds_on_the_client() {
    let key = cert::LICENSE_PUBLIC_KEY_B64;

    let late = issue("cloud-monthly", -3 * 86_400);
    let status = cert::tier_status_among(&[certificate(&late)], cert::tier::CLOUD, key);
    assert!(
        matches!(status, cert::TierStatus::Grace { .. }),
        "{status:?}"
    );

    let gone = issue("cloud-monthly", -15 * 86_400);
    let status = cert::tier_status_among(&[certificate(&gone)], cert::tier::CLOUD, key);
    assert!(
        matches!(status, cert::TierStatus::Expired { .. }),
        "{status:?}"
    );
}

/// The Store asks the server what the backup uses, with the Cloud token.
#[test]
#[ignore = "needs a running license server"]
fn the_store_reads_usage_from_the_server() {
    let token = issue("cloud-yearly", 365 * 86_400);
    let usage = usage::fetch(&server(), &token).expect("usage reported");
    assert!(usage.storage_cap > 0 && usage.transfer_cap > 0, "{usage:?}");
}

/// A token the server does not know is refused in words the user can act on.
#[test]
#[ignore = "needs a running license server"]
fn usage_for_an_unknown_token_is_refused_with_a_readable_reason() {
    let err = usage::fetch(&server(), "not-a-real-token").unwrap_err();
    assert!(err.contains("token"), "{err}");
}
