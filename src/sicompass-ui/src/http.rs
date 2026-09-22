//! Where an HTTP body comes from.
//!
//! Following a `<link>` to an `http(s)://` URL needs an HTTP client, and an
//! HTTP client means a TLS stack — about 126 crates. A login screen has no
//! links to follow and must not link any of that, so the renderer asks for the
//! body through a callback the embedder installs.
//!
//! This is deliberately the same shape as
//! [`sicompass_sdk::url_fetcher`], which solves the same problem one layer up
//! (there it is `lib_webbrowser` that registers the implementation). What
//! stays here is the interesting half — deciding whether the body is a
//! server-hosted FFON document or an HTML page — because that is a rendering
//! decision, not a networking one.
//!
//! With nothing registered, an HTTP link reports that it cannot be followed and
//! the node still renders. That is the greeter's behaviour, and it is also what
//! a build with no browser provider does.

use std::sync::OnceLock;

type BodyFetcher = Box<dyn Fn(&str) -> Result<Vec<u8>, String> + Send + Sync>;

static BODY_FETCHER: OnceLock<BodyFetcher> = OnceLock::new();

/// Install the HTTP fetcher. Only the first call takes effect, matching
/// [`sicompass_sdk::register_url_fetcher`].
pub fn register_body_fetcher(
    f: impl Fn(&str) -> Result<Vec<u8>, String> + Send + Sync + 'static,
) {
    let _ = BODY_FETCHER.set(Box::new(f));
}

/// Fetch a URL's raw bytes, or say why not.
pub fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    match BODY_FETCHER.get() {
        Some(f) => f(url),
        None => Err(format!("cannot follow {url}: this build has no HTTP client")),
    }
}

/// Fetch a URL and decode it as UTF-8 text.
pub fn fetch_body(url: &str) -> Result<String, String> {
    let raw = fetch_bytes(url)?;
    String::from_utf8(raw).map_err(|e| format!("{url} is not valid UTF-8: {e}"))
}

/// Whether an embedder has installed a fetcher.
pub fn has_body_fetcher() -> bool {
    BODY_FETCHER.get().is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Without a fetcher the call is an error, not a panic and not a hang.
    /// (`OnceLock` is process-wide, so this only asserts the shape of the
    /// answer: another test in the same binary may have registered one.)
    #[test]
    fn an_unregistered_fetch_is_a_reported_error() {
        match fetch_body("https://example.invalid/") {
            Err(e) => assert!(!e.is_empty(), "an error must say something"),
            Ok(_) => assert!(has_body_fetcher(), "a body implies a registered fetcher"),
        }
    }
}
