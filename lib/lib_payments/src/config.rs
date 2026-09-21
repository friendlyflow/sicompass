//! Read-only access to the Store settings that other providers need.
//!
//! The settings provider owns `settings.json` and is the only writer. A
//! provider that renders a tier link, or backs its store up, needs two of
//! those values before any `on_setting_change` broadcast has arrived — the
//! first `fetch()` happens during startup, ahead of the settings queue drain —
//! so they are read straight off disk here.
//!
//! Both are also delivered live through `Provider::on_setting_change`, and a
//! provider should keep taking them from there. This is the starting value,
//! not a substitute.

use crate::DEFAULT_STORE_URL;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};

// ---------------------------------------------------------------------------
// Test stub: never write to the real config directory from a test.
//
// `save_redeem_token` writes under the user's own config directory, and this
// crate's tests, and the providers' tests, exercise the redeem path. Without
// this, `cargo test` would quietly plant a token in the developer's live
// installation. Same shape as `lib_notes`'s kill switch, and for the same
// reason:
//
// * This crate's own tests get it free from `cfg!(test)`.
// * Another crate's test binary compiles this one *without* `cfg(test)`, so
//   those call `_set_test_no_persist(true)` once per binary instead.
// ---------------------------------------------------------------------------

static TEST_NO_PERSIST: AtomicBool = AtomicBool::new(cfg!(test));

#[doc(hidden)]
pub fn _set_test_no_persist(enabled: bool) {
    TEST_NO_PERSIST.store(enabled, Ordering::Release);
}

#[inline]
fn test_no_persist() -> bool {
    TEST_NO_PERSIST.load(Ordering::Acquire)
}

/// The `settings.json` section the Store bucket lives under.
const SECTION: &str = "Store";

/// Where this crate keeps its own copy of the redeem token, beside the
/// certificate it belongs to.
///
/// `settings.json` is the settings provider's file and it is the only writer,
/// but the token can now be pasted into the cloud tier tree from *inside*
/// notes or the board, and a provider cannot write another provider's file.
/// Without a copy of its own, a token entered there would redeem once and be
/// gone at the next launch, taking the backup with it.
const TOKEN_SLUG: &str = "store-token";

fn read_key(key: &str) -> Option<String> {
    // Under test the developer's own `settings.json` is not an input. Reading
    // it would make a test that never configured a server URL or a token
    // behave one way on their machine and another in CI, and a machine with a
    // real token configured could have a test reach a real server.
    if test_no_persist() {
        return None;
    }
    let path = sicompass_sdk::platform::main_config_path()?;
    let text = std::fs::read_to_string(path).ok()?;
    let root: Value = serde_json::from_str(&text).ok()?;
    root.get(SECTION)?
        .get(key)?
        .as_str()
        .map(str::to_owned)
        .filter(|v| !v.is_empty())
}

/// The configured store server, or [`DEFAULT_STORE_URL`].
///
/// An empty setting means "unset", not "no server": a blank URL would make
/// every tier link a dead end, and the default is what a user who never
/// touched the setting expects.
pub fn store_url() -> String {
    read_key("storeUrl").unwrap_or_else(|| DEFAULT_STORE_URL.to_owned())
}

/// The license redeem token, which doubles as the cloud-backup credential.
/// Empty when the user has not pasted one.
///
/// `settings.json` wins: that is where the user sees and edits it. The local
/// copy is the fallback for a token that was pasted into a tier tree grafted
/// into some other provider.
pub fn redeem_token() -> String {
    read_key("licenseRedeemToken")
        .or_else(saved_redeem_token)
        .unwrap_or_default()
}

fn token_path() -> Option<std::path::PathBuf> {
    sicompass_sdk::platform::provider_config_path(TOKEN_SLUG)
}

fn saved_redeem_token() -> Option<String> {
    if test_no_persist() {
        return None;
    }
    let text = std::fs::read_to_string(token_path()?).ok()?;
    let token = text.trim().to_owned();
    if token.is_empty() { None } else { Some(token) }
}

/// Keep a token that was entered outside the settings screen, so the backup
/// still has a credential after a restart. Returns whether it was written.
pub fn save_redeem_token(token: &str) -> bool {
    let token = token.trim();
    if token.is_empty() || test_no_persist() {
        return false;
    }
    let Some(path) = token_path() else {
        return false;
    };
    if let Some(dir) = path.parent() {
        sicompass_sdk::platform::make_dirs(dir);
    }
    sicompass_sdk::platform::atomic_write(&path, token)
}

/// Whether `key` is one of the Store settings a provider should react to when
/// it arrives through `Provider::on_setting_change`.
pub fn is_store_setting(key: &str) -> bool {
    matches!(key, "storeUrl" | "licenseRedeemToken")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_no_configuration_the_defaults_are_safe() {
        // The test guard makes `settings.json` invisible, so this is the
        // never-configured case: a usable default URL, and no credential.
        assert_eq!(store_url(), DEFAULT_STORE_URL);
        assert!(
            redeem_token().is_empty(),
            "a test must never pick up a real token"
        );
    }

    #[test]
    fn a_token_is_never_written_during_tests() {
        // The guard is on by default here, so this must not touch the disk.
        assert!(!save_redeem_token("tok-42"));
        assert!(token_path().is_some(), "the path itself still resolves");
    }

    #[test]
    fn an_empty_token_is_never_saved() {
        // Deliberately does not flip the kill switch off to test the other
        // branch: the switch is global, the test binary runs in parallel, and
        // one test turning it off would let another write to the real config.
        assert!(!save_redeem_token("   "));
    }

    #[test]
    fn only_the_two_store_keys_are_ours() {
        assert!(is_store_setting("storeUrl"));
        assert!(is_store_setting("licenseRedeemToken"));
        assert!(!is_store_setting("supportRedeemToken"));
        assert!(!is_store_setting("colorScheme"));
    }
}
