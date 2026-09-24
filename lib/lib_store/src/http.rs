//! Downloads, behind a function so tests (and a future proxy setting) can
//! swap the client.

use std::io::Read;
use std::sync::Arc;
use std::time::Duration;

/// `GET url` and return the body, or say why not.
pub type Fetch = Arc<dyn Fn(&str) -> Result<Vec<u8>, String> + Send + Sync>;

/// Nothing the Store downloads is bigger than a plugin archive may be.
pub const MAX_DOWNLOAD: u64 = sicompass_sdk::package::MAX_ARCHIVE_BYTES as u64;

/// The real client: blocking reqwest, run on the Store's worker thread.
pub fn http_fetch() -> Fetch {
    Arc::new(get)
}

fn get(url: &str) -> Result<Vec<u8>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent(concat!("sicompass/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client.get(url).send().map_err(|e| format!("{url}: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("{url}: {}", response.status()));
    }
    let mut body = Vec::new();
    response
        .take(MAX_DOWNLOAD + 1)
        .read_to_end(&mut body)
        .map_err(|e| format!("{url}: {e}"))?;
    if body.len() as u64 > MAX_DOWNLOAD {
        return Err(format!("{url}: larger than {MAX_DOWNLOAD} bytes"));
    }
    Ok(body)
}
