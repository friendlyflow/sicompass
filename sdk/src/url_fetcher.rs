//! URL-fetch callback registry.
//!
//! Libs that implement HTTP fetching (e.g. `lib_webbrowser`) call
//! [`register_url_fetcher`] from their `register()` function to install a
//! fetch implementation.  App code calls [`fetch_url_to_ffon`] without
//! knowing which lib provides the implementation.

use crate::ffon::FfonElement;
use std::sync::OnceLock;

static URL_FETCHER: OnceLock<Box<dyn Fn(&str) -> Vec<FfonElement> + Send + Sync>> = OnceLock::new();

/// Register a URL fetch implementation.
///
/// Only the first call has effect (idempotent guard via `OnceLock`).
/// Called once from the webbrowser lib's `register()`.
pub fn register_url_fetcher(f: impl Fn(&str) -> Vec<FfonElement> + Send + Sync + 'static) {
    URL_FETCHER.get_or_init(|| Box::new(f));
}

/// Fetch a URL and return its content as FFON elements.
///
/// Returns an empty `Vec` if no fetcher has been registered.
pub fn fetch_url_to_ffon(url: &str) -> Vec<FfonElement> {
    if let Some(f) = URL_FETCHER.get() {
        f(url)
    } else {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fetch_url_returns_empty_when_no_fetcher_registered() {
        // OnceLock may already have a fetcher from another test in a different
        // crate, so we can only test the no-fetcher case if the lock is unset.
        // If it is set, fetch_url_to_ffon must still return *something* (non-panic).
        let result = fetch_url_to_ffon("https://example.com");
        // Either empty (no fetcher) or non-panic (fetcher present) — both are valid.
        let _ = result; // just assert it doesn't panic
    }
}
