//! The store list and the releases the Store last downloaded, kept on disk so
//! the programs list shows at once while a fresh copy loads.
//!
//! The cache holds the signed files exactly as they were downloaded, and it is
//! only ever read through a [`Fetch`], so everything in it goes through the
//! same signature checks as a download, every time it is used. A stale or
//! damaged file can at most show an older list, never an unsigned one, and an
//! install always downloads the release again.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sicompass_sdk::package::sha256_hex;

use crate::http::Fetch;

/// Where `url`'s copy is kept.
fn path_for(dir: &Path, url: &str) -> PathBuf {
    dir.join(sha256_hex(url.as_bytes()))
}

/// `fetch`, keeping a copy of everything it downloads in `dir`.
pub fn keeping(fetch: Fetch, dir: PathBuf) -> Fetch {
    Arc::new(move |url: &str| {
        let body = fetch(url)?;
        // Best effort: a cache that cannot be written only means a slower
        // start next time. Written aside and renamed, so a reader never sees
        // half a file.
        if std::fs::create_dir_all(&dir).is_ok() {
            let path = path_for(&dir, url);
            let part = path.with_extension("part");
            if std::fs::write(&part, &body).is_ok() && std::fs::rename(&part, &path).is_err() {
                let _ = std::fs::remove_file(&part);
            }
        }
        Ok(body)
    })
}

/// Answers from the copies in `dir` only, never the network.
pub fn kept(dir: PathBuf) -> Fetch {
    Arc::new(move |url: &str| {
        std::fs::read(path_for(&dir, url)).map_err(|e| format!("{url}: not kept ({e})"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_was_downloaded_is_answered_from_disk_and_nothing_else_is() {
        let dir = tempfile::tempdir().unwrap();
        let network: Fetch = Arc::new(|url: &str| {
            if url.ends_with("missing") {
                Err("404".to_owned())
            } else {
                Ok(format!("body of {url}").into_bytes())
            }
        });
        let fetch = keeping(network, dir.path().join("store"));
        assert_eq!(fetch("https://a/x").unwrap(), b"body of https://a/x");
        assert!(fetch("https://a/missing").is_err());

        let disk = kept(dir.path().join("store"));
        assert_eq!(disk("https://a/x").unwrap(), b"body of https://a/x");
        assert!(disk("https://a/missing").is_err(), "a failure is not kept");
        assert!(disk("https://a/y").is_err(), "never downloaded");
    }
}
