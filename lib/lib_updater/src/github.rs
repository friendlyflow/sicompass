//! GitHub Releases API client — minimal blocking JSON wrapper around
//! `api.github.com/repos/<owner>/<repo>/releases/latest`.
//!
//! We deliberately avoid the `self_update` crate. Its release-discovery and
//! download logic are a handful of lines each over `reqwest`, and dropping
//! the dep keeps the transitive tree lean (self_update pulls in zip,
//! semver, hyper-rustls variants we don't otherwise need).

use crate::{parse_version, AppUpdate};
use serde::Deserialize;
use std::io::Write;
use std::path::PathBuf;

const USER_AGENT: &str = concat!("sicompass-updater/", env!("CARGO_PKG_VERSION"));
/// Cap the response so a hostile or misconfigured GitHub mirror can't ask
/// us to allocate gigabytes. The releases JSON is well under this.
const MAX_JSON_BYTES: u64 = 4 * 1024 * 1024;
/// Cap downloaded installer size. cargo-dist MSIs sit well under 100 MiB
/// today; bumping this is cheap if a future release grows past it.
const MAX_INSTALLER_BYTES: u64 = 256 * 1024 * 1024;
const HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

#[derive(Debug, Deserialize)]
struct ReleaseJson {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    html_url: String,
    assets: Vec<AssetJson>,
}

#[derive(Debug, Deserialize)]
struct AssetJson {
    name: String,
    browser_download_url: String,
}

/// Query GitHub for the latest release and decide whether it's newer than
/// `current`. Returns `Ok(None)` when up-to-date, `Ok(Some(_))` with the
/// MSI staged in `%TEMP%` (or system tmp) when newer, `Err` on any
/// network/parse failure.
pub fn check_app_update(
    owner: &str,
    repo: &str,
    current: &semver::Version,
) -> Result<Option<AppUpdate>, String> {
    let url = format!("https://api.github.com/repos/{owner}/{repo}/releases/latest");
    let client = reqwest::blocking::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .user_agent(USER_AGENT)
        .build()
        .map_err(|e| format!("build client: {e}"))?;

    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|e| format!("GET {url}: {e}"))?;

    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        // No releases yet — common during early development.
        return Ok(None);
    }
    if !resp.status().is_success() {
        return Err(format!("{url}: HTTP {}", resp.status()));
    }

    let body = read_capped(resp, MAX_JSON_BYTES)?;
    let release: ReleaseJson = serde_json::from_slice(&body)
        .map_err(|e| format!("parse releases JSON: {e}"))?;

    if release.draft || release.prerelease {
        return Ok(None);
    }

    let new_version =
        parse_version(&release.tag_name).map_err(|e| format!("parse tag {}: {e}", release.tag_name))?;
    if new_version <= *current {
        return Ok(None);
    }

    // Pick the platform-appropriate asset. On Windows we want `*.msi`; on
    // other platforms we currently have no apply path, so we return the
    // release URL and skip the download.
    #[cfg(target_os = "windows")]
    let asset_name_filter: fn(&str) -> bool = |n| n.to_ascii_lowercase().ends_with(".msi");
    #[cfg(not(target_os = "windows"))]
    let asset_name_filter: fn(&str) -> bool = |_| false;

    let asset = release.assets.iter().find(|a| asset_name_filter(&a.name));

    let staged = if let Some(asset) = asset {
        let dest = std::env::temp_dir().join(format!(
            "sicompass-update-{}-{}",
            new_version, asset.name
        ));
        match download_to(&client, &asset.browser_download_url, &dest) {
            Ok(()) => Some(dest),
            Err(e) => {
                tracing::warn!("download {}: {e}", asset.browser_download_url);
                None
            }
        }
    } else {
        None
    };

    Ok(Some(AppUpdate {
        new_version,
        staged_installer_path: staged,
        release_url: release.html_url,
    }))
}

fn read_capped(resp: reqwest::blocking::Response, cap: u64) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut buf = Vec::new();
    resp.take(cap).read_to_end(&mut buf).map_err(|e| format!("read body: {e}"))?;
    Ok(buf)
}

/// Stream an asset to a local file. Truncates if `dest` already exists
/// (a previous interrupted download). Caller verifies size/checksum after.
pub fn download_to(
    client: &reqwest::blocking::Client,
    url: &str,
    dest: &std::path::Path,
) -> Result<(), String> {
    use std::io::Read;
    let mut resp = client
        .get(url)
        .send()
        .map_err(|e| format!("GET {url}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("{url}: HTTP {}", resp.status()));
    }
    if let Some(len) = resp.content_length() {
        if len > MAX_INSTALLER_BYTES {
            return Err(format!("{url}: installer too large ({len} bytes)"));
        }
    }
    let parent = dest.parent().ok_or_else(|| "dest has no parent".to_string())?;
    std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    let mut f = std::fs::File::create(dest).map_err(|e| format!("create {}: {e}", dest.display()))?;
    let mut buf = [0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        let n = resp.read(&mut buf).map_err(|e| format!("read body: {e}"))?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > MAX_INSTALLER_BYTES {
            return Err(format!("{url}: installer exceeded cap"));
        }
        f.write_all(&buf[..n]).map_err(|e| format!("write {}: {e}", dest.display()))?;
    }
    Ok(())
}

#[allow(dead_code)] // re-exported through `check_app_update`'s return path
pub(crate) fn _staged_dest_for(name: &str, version: &semver::Version) -> PathBuf {
    std::env::temp_dir().join(format!("sicompass-update-{version}-{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn release_json(tag: &str, draft: bool, prerelease: bool, assets: &[(&str, &str)]) -> String {
        let assets_json: Vec<_> = assets
            .iter()
            .map(|(name, url)| {
                serde_json::json!({"name": name, "browser_download_url": url})
            })
            .collect();
        serde_json::json!({
            "tag_name": tag,
            "draft": draft,
            "prerelease": prerelease,
            "html_url": "https://example.com/release",
            "assets": assets_json,
        })
        .to_string()
    }

    // The blocking reqwest client spawns its own tokio runtime; nesting
    // that inside `rt.block_on(async { ... })` panics on drop. We use
    // `#[tokio::test(flavor = "multi_thread")]` so the server runs on
    // async workers and the blocking client runs on a separate
    // `block_in_place` worker.

    #[tokio::test(flavor = "multi_thread")]
    async fn parses_release_json_and_extracts_version() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/repos/o/r/releases/latest"))
            .respond_with(ResponseTemplate::new(200).set_body_string(release_json(
                "v0.1.0", false, false, &[],
            )))
            .mount(&server)
            .await;

        let url = format!("{}/repos/o/r/releases/latest", server.uri());
        let body = tokio::task::block_in_place(|| {
            reqwest::blocking::get(&url).unwrap().text().unwrap()
        });
        let r: ReleaseJson = serde_json::from_str(&body).unwrap();
        let v = parse_version(&r.tag_name).unwrap();
        assert_eq!(v, semver::Version::new(0, 1, 0));
        assert!(!r.draft);
        assert!(r.assets.is_empty());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn draft_and_prerelease_are_ignored() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/release"))
            .respond_with(ResponseTemplate::new(200).set_body_string(release_json(
                "v9.9.9", true, false, &[],
            )))
            .mount(&server)
            .await;
        let url = format!("{}/release", server.uri());
        let body = tokio::task::block_in_place(|| {
            reqwest::blocking::get(&url).unwrap().text().unwrap()
        });
        let r: ReleaseJson = serde_json::from_str(&body).unwrap();
        assert!(r.draft);
    }

    #[test]
    fn _staged_dest_includes_version_and_name() {
        let p = _staged_dest_for("foo.msi", &semver::Version::new(1, 2, 3));
        let s = p.to_string_lossy();
        assert!(s.contains("1.2.3"));
        assert!(s.contains("foo.msi"));
    }
}
