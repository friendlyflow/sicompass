//! Checking, installing, updating and removing a plugin.
//!
//! Nothing here trusts the network. A release is believed when its
//! `release.json` is signed by the key its [`Source`] names, and its archive
//! is believed when it hashes to what that `release.json` says. The archive's
//! own `plugin.json` must then agree with `release.json` on everything the
//! user approved, what gets installed must be exactly the release the user was
//! shown, and the component must pass the host's import audit
//! ([`sicompass_sdk::package::audit_component`]) against those permissions.
//!
//! An install is staged in `plugins/.store/` and swapped into
//! `plugins/<name>/` with a rename, so a failure half-way leaves the previous
//! version in place.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sicompass_sdk::package::{self, ARCHIVE_FILE, RELEASE_FILE, ReleaseInfo, SIGNATURE_FILE};
use sicompass_sdk::plugin_abi::ABI_VERSION;
use sicompass_sdk::plugin_manifest::{PluginManifest, parse_manifest};
use sicompass_sdk::store::StoreEntry;

use crate::http::Fetch;

/// Inside the plugins folder, where installs are staged and old versions wait
/// to be deleted. The dot keeps it out of plugin discovery, which looks for
/// `<dir>/plugin.json` and finds none here.
pub const STAGING_DIR: &str = ".store";

/// Where releases of store-listed plugins are downloaded from. GitHub, except
/// in tests.
pub const RELEASES_URL: &str = "https://github.com";

/// Where one plugin's releases come from, and whose key signs them.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub name: String,
    /// The folder holding `release.json`, `release.json.sig` and
    /// `plugin.tar.gz`, ending in `/`.
    pub folder: String,
    /// Ed25519 public key (base64) the releases must be signed with.
    pub pubkey: String,
    /// SHA-256 (hex) of archives never to install.
    pub revoked: Vec<String>,
    /// Installed by hand with an `updateUrl`: trusted on first use, so an
    /// update must keep the key it was installed with.
    pub by_hand: bool,
}

impl Source {
    /// A plugin the store list offers: its GitHub repo's latest release, signed
    /// by the key the (signed) list names.
    pub fn listed(entry: &StoreEntry, releases_url: &str) -> Self {
        Source {
            name: entry.name.clone(),
            folder: format!(
                "{}/{}/releases/latest/download/",
                releases_url.trim_end_matches('/'),
                entry.repo
            ),
            pubkey: entry.pubkey.clone(),
            revoked: entry.revoked.clone(),
            by_hand: false,
        }
    }

    /// A plugin installed by hand: its manifest's `updateUrl` (the folder of
    /// its release files) and `pubkey`. `None` without an `updateUrl`, and an
    /// error with one but no key, since nothing could be verified.
    pub fn by_hand(m: &PluginManifest) -> Option<Result<Self, String>> {
        let url = m.update_url.as_deref()?.trim();
        let Some(pubkey) = m.pubkey.clone() else {
            return Some(Err(
                "its plugin.json has an updateUrl but no pubkey to check updates with".to_owned(),
            ));
        };
        let folder = if url.ends_with('/') {
            url.to_owned()
        } else {
            format!("{url}/")
        };
        Some(Ok(Source {
            name: m.name.clone(),
            folder,
            pubkey,
            revoked: Vec::new(),
            by_hand: true,
        }))
    }

    /// The URL of one of the release's files.
    pub fn url(&self, file: &str) -> String {
        format!("{}{file}", self.folder)
    }

    fn is_revoked(&self, archive_sha256: &str) -> bool {
        self.revoked
            .iter()
            .any(|r| r.eq_ignore_ascii_case(archive_sha256))
    }
}

/// A plugin that is on disk now.
#[derive(Debug, Clone)]
pub struct Installed {
    pub dir: PathBuf,
    pub manifest: PluginManifest,
}

/// Every plugin in `plugins_dir`, by manifest name.
pub fn installed(plugins_dir: &Path) -> BTreeMap<String, Installed> {
    let mut out = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(plugins_dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let dir = entry.path();
        let Ok(text) = std::fs::read_to_string(dir.join("plugin.json")) else {
            continue;
        };
        if let Ok(manifest) = parse_manifest(&text) {
            out.insert(manifest.name.clone(), Installed { dir, manifest });
        }
    }
    out
}

fn version(v: &str) -> Result<semver::Version, String> {
    semver::Version::parse(v.trim_start_matches('v')).map_err(|e| format!("version `{v}`: {e}"))
}

/// The sicompass version a release needs, when it is newer than this one.
pub fn needs_newer_app(info: &ReleaseInfo) -> Option<String> {
    let min = info.min_app_version.as_deref()?;
    let running = version(env!("CARGO_PKG_VERSION")).ok()?;
    match version(min) {
        Ok(min_v) if min_v > running => Some(min.to_owned()),
        _ => None,
    }
}

/// Whether `info` is a newer version than the installed `manifest`.
pub fn is_newer(info: &ReleaseInfo, manifest: &PluginManifest) -> bool {
    match (
        version(&info.version),
        manifest.version.as_deref().map(version),
    ) {
        (Ok(new), Some(Ok(old))) => new > old,
        // An installed plugin without a readable version is replaced by any
        // valid release; an invalid release version never counts as newer.
        (Ok(_), _) => true,
        (Err(_), _) => false,
    }
}

/// What a release has to say about itself before anything else is looked at.
fn check_release_info(source: &Source, info: &ReleaseInfo) -> Result<(), String> {
    if info.name != source.name {
        return Err(format!(
            "the release of `{}` says it is `{}`",
            source.name, info.name
        ));
    }
    if info.abi != ABI_VERSION {
        return Err(format!(
            "it was built for plugin ABI {}, and this sicompass runs {ABI_VERSION}",
            info.abi
        ));
    }
    version(&info.version)?;
    Ok(())
}

fn text(bytes: Vec<u8>, what: &str) -> Result<String, String> {
    String::from_utf8(bytes).map_err(|_| format!("{what} is not text"))
}

/// Download and check a plugin's latest `release.json`: what the Store shows
/// before the user decides. The archive is not downloaded yet.
pub fn fetch_release(fetch: &Fetch, source: &Source) -> Result<ReleaseInfo, String> {
    let json = fetch(&source.url(RELEASE_FILE))?;
    let signature = text(fetch(&source.url(SIGNATURE_FILE))?, SIGNATURE_FILE)?;
    package::verify(&json, &signature, &source.pubkey)
        .map_err(|e| format!("{RELEASE_FILE}: {e}"))?;
    let info: ReleaseInfo =
        serde_json::from_slice(&json).map_err(|e| format!("{RELEASE_FILE} does not parse: {e}"))?;
    check_release_info(source, &info)?;
    Ok(info)
}

/// Install (or update to) the release the user was `shown`.
///
/// Refused when the release on the server is no longer that one, when it is
/// revoked, needs a newer sicompass, is not newer than what is installed, when
/// anything fails to verify, or when the component fails the import audit.
/// Returns the installed manifest.
pub fn install(
    fetch: &Fetch,
    source: &Source,
    shown: &ReleaseInfo,
    plugins_dir: &Path,
) -> Result<PluginManifest, String> {
    let json = fetch(&source.url(RELEASE_FILE))?;
    let signature = text(fetch(&source.url(SIGNATURE_FILE))?, SIGNATURE_FILE)?;
    let archive = fetch(&source.url(ARCHIVE_FILE))?;
    let info = package::verify_release(&json, &signature, &source.pubkey, &archive)?;
    check_release_info(source, &info)?;

    // The user approved what they saw. A release published in between may ask
    // for other access, so it has to be shown again first.
    if info != *shown {
        return Err("a newer release appeared while you were looking, check again".to_owned());
    }
    if source.is_revoked(&info.archive_sha256) {
        return Err("this release was withdrawn".to_owned());
    }
    if let Some(min) = needs_newer_app(&info) {
        return Err(format!("it needs sicompass {min} or newer"));
    }
    if let Some(current) = installed(plugins_dir).get(&source.name)
        && !is_newer(&info, &current.manifest)
    {
        return Err(format!(
            "version {} is not newer than the installed one",
            info.version
        ));
    }

    let staging_root = plugins_dir.join(STAGING_DIR);
    std::fs::create_dir_all(&staging_root)
        .map_err(|e| format!("{}: {e}", staging_root.display()))?;
    let staged = staging_root.join(&source.name);
    remove_if_present(&staged)?;
    let result = stage_and_swap(&archive, &info, source, &staged, plugins_dir);
    // Whatever happened, nothing is left behind in staging.
    let _ = remove_if_present(&staged);
    result
}

fn stage_and_swap(
    archive: &[u8],
    info: &ReleaseInfo,
    source: &Source,
    staged: &Path,
    plugins_dir: &Path,
) -> Result<PluginManifest, String> {
    package::unpack(archive, staged)?;
    let manifest_text = std::fs::read_to_string(staged.join("plugin.json"))
        .map_err(|_| "the archive has no plugin.json".to_owned())?;
    let manifest = parse_manifest(&manifest_text)?;
    info.matches_manifest(&manifest)?;
    if source.by_hand && manifest.pubkey.as_deref() != Some(source.pubkey.as_str()) {
        return Err(
            "the update names another signing key. A plugin installed by hand keeps the key \
             it was installed with, so install the new version by hand to trust a new key"
                .to_owned(),
        );
    }
    let wasm = std::fs::read(staged.join(&manifest.entry))
        .map_err(|_| format!("the archive has no `{}`", manifest.entry))?;
    package::audit_component(&wasm, &manifest)?;
    swap_in(plugins_dir, &source.name, staged)?;
    Ok(manifest)
}

/// Replace `plugins/<name>` by `staged`, keeping the old version until the new
/// one is in place.
fn swap_in(plugins_dir: &Path, name: &str, staged: &Path) -> Result<(), String> {
    let target = plugins_dir.join(name);
    let old = plugins_dir.join(STAGING_DIR).join(format!("{name}.old"));
    remove_if_present(&old)?;
    let had_old = target.exists();
    if had_old {
        std::fs::rename(&target, &old).map_err(|e| format!("{}: {e}", target.display()))?;
    }
    if let Err(e) = std::fs::rename(staged, &target) {
        if had_old {
            let _ = std::fs::rename(&old, &target);
        }
        return Err(format!("{}: {e}", target.display()));
    }
    let _ = remove_if_present(&old);
    Ok(())
}

/// Remove an installed plugin's program files. Its data folder
/// (`app_data_dir()/<name>`) is the user's, and is not touched here.
pub fn uninstall(plugins_dir: &Path, installed: &Installed) -> Result<(), String> {
    // Only ever a direct child of the plugins folder.
    if installed.dir.parent() != Some(plugins_dir) {
        return Err(format!(
            "{} is not in the plugins folder",
            installed.dir.display()
        ));
    }
    let staging_root = plugins_dir.join(STAGING_DIR);
    std::fs::create_dir_all(&staging_root)
        .map_err(|e| format!("{}: {e}", staging_root.display()))?;
    let gone = staging_root.join(format!("{}.removed", installed.manifest.name));
    remove_if_present(&gone)?;
    // A rename first, so the plugin disappears at once even if deleting the
    // files takes a while or fails part-way.
    std::fs::rename(&installed.dir, &gone)
        .map_err(|e| format!("{}: {e}", installed.dir.display()))?;
    let _ = remove_if_present(&gone);
    Ok(())
}

fn remove_if_present(path: &Path) -> Result<(), String> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listed_release_urls_are_the_ones_the_sdk_documents() {
        let entry = StoreEntry {
            name: "notes".into(),
            repo: "friendlyflow/notes-plugin-sicompass".into(),
            pubkey: String::new(),
            category: None,
            service: None,
            paid_features: false,
            revoked: Vec::new(),
        };
        assert_eq!(
            Source::listed(&entry, RELEASES_URL).url(RELEASE_FILE),
            entry.release_url(RELEASE_FILE)
        );
    }

    #[test]
    fn a_hand_install_updates_only_with_an_address_and_a_key() {
        let m = |extra: &str| {
            parse_manifest(&format!(
                r#"{{ "name": "x", "displayName": "x", "entry": "plugin.wasm" {extra} }}"#
            ))
            .unwrap()
        };
        assert!(Source::by_hand(&m("")).is_none());
        assert!(
            Source::by_hand(&m(r#", "updateUrl": "https://e.org/x""#))
                .unwrap()
                .is_err()
        );
        let s = Source::by_hand(&m(r#", "updateUrl": "https://e.org/x", "pubkey": "k""#))
            .unwrap()
            .unwrap();
        assert_eq!(s.url(RELEASE_FILE), "https://e.org/x/release.json");
        assert!(s.by_hand);
    }
}
