//! End-to-end checks against a real license server.
//!
//! Ignored by default: they need a server running, which `cargo test` must not
//! depend on. Run them by hand after changing either side of the wire.
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
//!   cargo test -p sicompass-payments --test live_server -- --ignored --test-threads=1
//! ```
//!
//! Nothing here touches the user's config directory: the certificate is
//! verified in memory rather than redeemed, and the restore writes to a
//! tempdir.

use sicompass_payments::{backup, cert};
use std::collections::BTreeMap;

fn server() -> String {
    std::env::var("SICOMPASS_TEST_SERVER").expect("set SICOMPASS_TEST_SERVER")
}

fn token() -> String {
    std::env::var("SICOMPASS_TEST_TOKEN").expect("set SICOMPASS_TEST_TOKEN")
}

fn files() -> BTreeMap<String, String> {
    BTreeMap::from([
        (".listmeta".to_owned(), r#"{"sha256":"x"}"#.to_owned()),
        ("0001".to_owned(), "Groceries".to_owned()),
        (
            "0001.d/.listmeta".to_owned(),
            r#"{"sha256":"y"}"#.to_owned(),
        ),
        ("0001.d/0001".to_owned(), "milk".to_owned()),
    ])
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

#[test]
#[ignore = "needs a running license server"]
fn a_store_round_trips_through_the_server() {
    let snapshot = backup::Snapshot::new("notes", files());
    assert!(
        backup::put_snapshot(&server(), &token(), &snapshot).expect("upload failed"),
        "a snapshot the server has not seen must be stored"
    );

    let fetched = backup::get_snapshot(&server(), &token(), "notes")
        .expect("download failed")
        .expect("the snapshot just uploaded is missing");
    assert_eq!(fetched.files, snapshot.files);

    // An unchanged store costs nothing.
    assert!(
        !backup::put_snapshot(&server(), &token(), &snapshot).expect("re-upload failed"),
        "an unchanged store must not be stored again"
    );
}

#[test]
#[ignore = "needs a running license server"]
fn a_restore_rebuilds_the_store_on_disk() {
    let snapshot = backup::Snapshot::new("kanban", files());
    backup::put_snapshot(&server(), &token(), &snapshot).expect("upload failed");

    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("projectmanagement");
    assert!(backup::restore(&server(), &token(), &root, "kanban").expect("restore failed"));

    assert_eq!(
        std::fs::read_to_string(root.join("0001")).unwrap(),
        "Groceries"
    );
    assert_eq!(
        std::fs::read_to_string(root.join("0001.d/0001")).unwrap(),
        "milk"
    );

    // And a second restore refuses, because the store is no longer empty.
    assert!(backup::restore(&server(), &token(), &root, "kanban").is_err());
}

/// A token the server does not know must be refused in words the user can act
/// on, not a bare status code.
#[test]
#[ignore = "needs a running license server"]
fn an_unknown_token_is_refused_with_a_readable_reason() {
    let err = backup::get_snapshot(&server(), "not-a-real-token", "notes").unwrap_err();
    assert!(err.contains("token"), "{err}");
}
