//! Where the store list comes from, and why the app believes it.
//!
//! The live copy is fetched from the sicompass repo on GitHub. It is believed
//! only if it is signed by one of [`TRUSTED_KEYS`]: the working store key or
//! the cold backup (docs/plugin-platform.md §8). When it cannot be fetched or
//! does not verify, the copy compiled into this binary is used instead, which
//! passes the same check (a test makes sure of that).

use sicompass_sdk::store::{STORE_FILE, STORE_SIGNATURE_FILE, Store, verify_store};

use crate::http::Fetch;

/// The public halves of the store keys. The secret halves never leave the
/// maintainer's machine (the working key) or offline storage (the backup).
pub const TRUSTED_KEYS: [&str; 2] = [
    // Working key, `~/.config/sicompass/store.key` on the maintainer's machine.
    "0i4sD+M+SILZZdNVpkJI0O1DB7ap1O5QIsHQyM+gV34=",
    // Cold backup, kept offline. Signs only if the working key is lost or leaked.
    "/dOAxi3JRm576kiGvmcxMiGNbfffjEmmT1O9CQgPjmg=",
];

/// The folder the live copy is fetched from (it ends in a slash).
pub const STORE_URL: &str =
    "https://raw.githubusercontent.com/friendlyflow/sicompass/main/lib/lib_store/";

pub const COMPILED_STORE: &[u8] = include_bytes!("../store.json");
pub const COMPILED_STORE_SIGNATURE: &str = include_str!("../store.json.sig");

/// A store list the app believes, and whether it is the live one.
#[derive(Debug, Clone)]
pub struct Loaded {
    pub store: Store,
    /// Why the live copy was not used, when it was not.
    pub offline: Option<String>,
}

/// The live store list, or the compiled-in one when the live one fails.
pub fn load(fetch: &Fetch, base_url: &str, trusted: &[&str]) -> Result<Loaded, String> {
    match load_live(fetch, base_url, trusted) {
        Ok(store) => Ok(Loaded {
            store,
            offline: None,
        }),
        Err(live) => {
            let store = verify_store(COMPILED_STORE, COMPILED_STORE_SIGNATURE, trusted)
                .map_err(|compiled| format!("{live}. The built-in copy: {compiled}"))?;
            Ok(Loaded {
                store,
                offline: Some(live),
            })
        }
    }
}

fn load_live(fetch: &Fetch, base_url: &str, trusted: &[&str]) -> Result<Store, String> {
    let base = if base_url.ends_with('/') {
        base_url.to_owned()
    } else {
        format!("{base_url}/")
    };
    let json = fetch(&format!("{base}{STORE_FILE}"))?;
    let signature = fetch(&format!("{base}{STORE_SIGNATURE_FILE}"))?;
    let signature =
        String::from_utf8(signature).map_err(|_| format!("{STORE_SIGNATURE_FILE} is not text"))?;
    verify_store(&json, &signature, trusted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_compiled_store_list_is_signed_by_a_trusted_key() {
        // If this fails, store.json was edited without re-signing it (the
        // /store skill signs it with ~/.config/sicompass/store.key).
        verify_store(COMPILED_STORE, COMPILED_STORE_SIGNATURE, &TRUSTED_KEYS).unwrap();
    }

    #[test]
    fn no_network_falls_back_to_the_compiled_copy() {
        let fetch: Fetch = std::sync::Arc::new(|_| Err("offline".to_owned()));
        let loaded = load(&fetch, STORE_URL, &TRUSTED_KEYS).unwrap();
        assert_eq!(loaded.offline.as_deref(), Some("offline"));
    }
}
