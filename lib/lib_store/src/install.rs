//! Checking, installing, updating and removing a plugin.
//!
//! Nothing here trusts the network. A release is believed when its
//! `release.json` is signed by the key its [`Source`] names, and its archive
//! is believed when it hashes to what that `release.json` says. The archive's
//! own `plugin.json` must then agree with `release.json` on everything the
//! user approved, and what gets installed must be exactly the release the user
//! was shown.
//!
//! A plugin is a program, released once per platform, and only this platform's
//! archive is downloaded ([`ReleaseInfo::archive_for`]). What the user approves
//! is that it runs at all, with their rights. Its executable is made runnable
//! after unpacking.
//!
//! An install is staged in `plugins/.store/` and swapped into
//! `plugins/<name>/` with a rename, so a failure half-way leaves the previous
//! version in place.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sicompass_sdk::package::{self, RELEASE_FILE, ReleaseInfo, SIGNATURE_FILE};
use sicompass_sdk::plugin_abi::{
    PROTOCOL_VERSION, executable_name, plugin_target, process_abi_compatible,
};
use sicompass_sdk::plugin_manifest::{PluginManifest, PluginType, parse_manifest};
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
    /// The folder holding `release.json`, `release.json.sig` and the
    /// `plugin-<target>.tar.gz` archives, ending in `/`.
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
    /// A plugin the store list offers: its GitHub repo's latest release (or
    /// the release a frozen entry stays at), signed by the key the (signed)
    /// list names.
    pub fn listed(entry: &StoreEntry, releases_url: &str) -> Self {
        Source {
            name: entry.name.clone(),
            folder: entry.release_folder(releases_url.trim_end_matches('/')),
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

    pub fn is_revoked(&self, archive_sha256: &str) -> bool {
        self.revoked
            .iter()
            .any(|r| r.eq_ignore_ascii_case(archive_sha256))
    }
}

pub use sicompass_sdk::installed_plugins::PluginOrigin;

/// A plugin that is on disk now.
#[derive(Debug, Clone)]
pub struct Installed {
    pub dir: PathBuf,
    pub manifest: PluginManifest,
    /// [`PluginOrigin::System`] for one this computer's configuration provides,
    /// which the Store neither updates nor removes.
    pub origin: PluginOrigin,
}

/// Every plugin on this computer, by manifest name: the folders the
/// configuration provides first (`SICOMPASS_PLUGIN_PATH`), then
/// `plugins_dir`. One per name, the first found kept, which is the one the app
/// runs ([`sicompass_sdk::installed_plugins::discover_all`]).
pub fn installed_everywhere(
    system: &[PathBuf],
    plugins_dir: Option<&Path>,
) -> BTreeMap<String, Installed> {
    sicompass_sdk::installed_plugins::discover_all_in(system, plugins_dir)
        .into_iter()
        .filter_map(|(dir, origin, manifest)| {
            let manifest = manifest.ok()?;
            Some((
                manifest.name.clone(),
                Installed {
                    dir,
                    manifest,
                    origin,
                },
            ))
        })
        .collect()
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
            out.insert(
                manifest.name.clone(),
                Installed {
                    dir,
                    manifest,
                    origin: PluginOrigin::User,
                },
            );
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
    // A WASM release, from before sicompass 0.3: `archive_for` says why not.
    info.archive_for(plugin_target())?;
    if !process_abi_compatible(&info.abi) {
        return Err(format!(
            "it was built for plugin protocol {}, and this sicompass speaks {PROTOCOL_VERSION}",
            info.abi.trim_start_matches("process/")
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
/// anything fails to verify, or when the archive has no program for this
/// platform.
/// Returns the installed manifest.
pub fn install(
    fetch: &Fetch,
    source: &Source,
    shown: &ReleaseInfo,
    plugins_dir: &Path,
) -> Result<PluginManifest, String> {
    let json = fetch(&source.url(RELEASE_FILE))?;
    let signature = text(fetch(&source.url(SIGNATURE_FILE))?, SIGNATURE_FILE)?;
    // Which archive to download is in the signed release.json: this
    // platform's build.
    let listed = package::verify_release_info(&json, &signature, &source.pubkey)?;
    check_release_info(source, &listed)?;
    let (archive_file, _) = listed.archive_for(plugin_target())?;
    let archive = fetch(&source.url(&archive_file))?;
    let info = package::verify_release(&json, &signature, &source.pubkey, &archive)?;
    let archive_sha256 = package::sha256_hex(&archive);

    // The user approved what they saw. A release published in between may ask
    // for other access, so it has to be shown again first.
    if info != *shown {
        return Err("a newer release appeared while you were looking, check again".to_owned());
    }
    if source.is_revoked(&archive_sha256) {
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

/// [`install`], then record in `settings` that the user approved what was
/// installed: pressing Update (or Ctrl+U for an update asking for nothing new)
/// is the approval. A new install of a plugin whose service the user already
/// pays for has its service switch ticked, as paying would have; an update
/// keeps the user's choice. Returns the installed version.
pub fn install_and_approve(
    fetch: &Fetch,
    source: &Source,
    shown: &ReleaseInfo,
    plugins_dir: &Path,
    settings: Result<PathBuf, String>,
) -> Result<String, String> {
    let new = !installed(plugins_dir).contains_key(&source.name);
    let m = install(fetch, source, shown, plugins_dir)?;
    let path = settings
        .and_then(|path| crate::approvals::record(&path, &m).map(|()| path))
        .map_err(|e| format!("installed, but the approval could not be saved: {e}"))?;
    if new && let Err(e) = crate::service_switches::tick_if_held(&path, &m) {
        eprintln!(
            "sicompass: store: could not turn on {}'s service: {e}",
            m.name
        );
    }
    Ok(m.version.unwrap_or_default())
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
    if manifest.plugin_type != PluginType::Process {
        return Err("plugin.json in the archive does not name a program".to_owned());
    }
    let target = plugin_target().ok_or("sicompass has no plugin builds for this platform")?;
    let exe = staged.join(executable_name(&manifest.entry, target));
    if !exe.is_file() {
        return Err(format!(
            "the archive has no `{}`",
            executable_name(&manifest.entry, target)
        ));
    }
    make_executable(&exe)?;
    swap_in(plugins_dir, &source.name, staged)?;
    Ok(manifest)
}

/// Let a plugin process's executable run: unpacking writes every file 0644.
#[cfg(unix)]
fn make_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
        .map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> Result<(), String> {
    Ok(())
}

/// Replace `plugins/<name>` by `staged`, keeping the old version until the new
/// one is in place.
///
/// On Windows a folder cannot be renamed while a program runs from it, and
/// every open tab of the plugin runs its `plugin.exe` there, in that folder.
/// The files inside can still be renamed, the running program's included, so
/// there the old files move out and the new ones in, one at a time
/// ([`swap_contents`]). The app sees the new `plugin.json` and restarts the
/// plugin. The old program, which cannot be deleted while it runs, goes at
/// the next update ([`old_dir`]).
fn swap_in(plugins_dir: &Path, name: &str, staged: &Path) -> Result<(), String> {
    let target = plugins_dir.join(name);
    if !target.exists() {
        return std::fs::rename(staged, &target).map_err(|e| format!("{}: {e}", target.display()));
    }
    let old = old_dir(plugins_dir, name)?;
    match std::fs::rename(&target, &old) {
        Ok(()) => {
            if let Err(e) = std::fs::rename(staged, &target) {
                let _ = std::fs::rename(&old, &target);
                return Err(format!("{}: {e}", target.display()));
            }
        }
        Err(_) if cfg!(windows) => swap_contents(&target, &old, staged)?,
        Err(e) => return Err(format!("{}: {e}", target.display())),
    }
    let _ = remove_if_present(&old);
    Ok(())
}

/// Where the version being replaced goes: `.store/<name>.old`, once what
/// earlier updates left there is deleted. A program that was still running
/// then cannot be deleted yet (Windows), so then `.store/<name>.old-<n>`.
fn old_dir(plugins_dir: &Path, name: &str) -> Result<PathBuf, String> {
    let staging_root = plugins_dir.join(STAGING_DIR);
    let base = format!("{name}.old");
    let is_old = |file: &str| {
        file == base
            || file
                .strip_prefix(&base)
                .and_then(|rest| rest.strip_prefix('-'))
                .is_some_and(|n| n.parse::<u32>().is_ok())
    };
    for entry in std::fs::read_dir(&staging_root)
        .into_iter()
        .flatten()
        .flatten()
    {
        if is_old(&entry.file_name().to_string_lossy()) {
            let _ = remove_if_present(&entry.path());
        }
    }
    (0..1000)
        .map(|n| match n {
            0 => staging_root.join(&base),
            n => staging_root.join(format!("{base}-{n}")),
        })
        .find(|p| !p.exists())
        .ok_or_else(|| format!("{}: too many old versions", staging_root.display()))
}

/// Move what is in `target` into `old`, then what is in `staged` into
/// `target`, for when `target` itself cannot be renamed. Undone as far as it
/// got when a move fails, so the previous version stays in place.
fn swap_contents(target: &Path, old: &Path, staged: &Path) -> Result<(), String> {
    let names = |dir: &Path| -> Result<Vec<std::ffi::OsString>, String> {
        std::fs::read_dir(dir)
            .and_then(|entries| entries.map(|e| e.map(|e| e.file_name())).collect())
            .map_err(|e| format!("{}: {e}", dir.display()))
    };
    let current = names(target)?;
    let new = names(staged)?;
    std::fs::create_dir_all(old).map_err(|e| format!("{}: {e}", old.display()))?;
    move_entries(&current, target, old)?;
    if let Err(e) = move_entries(&new, staged, target) {
        let _ = move_entries(&current, old, target);
        return Err(e);
    }
    Ok(())
}

/// Rename each of `names` from `from` to `to`, moving back the ones already
/// moved when one fails.
fn move_entries(names: &[std::ffi::OsString], from: &Path, to: &Path) -> Result<(), String> {
    for (i, name) in names.iter().enumerate() {
        if let Err(e) = std::fs::rename(from.join(name), to.join(name)) {
            for done in &names[..i] {
                let _ = std::fs::rename(to.join(done), from.join(done));
            }
            return Err(format!("{}: {e}", from.join(name).display()));
        }
    }
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
            ..Default::default()
        };
        assert_eq!(
            Source::listed(&entry, RELEASES_URL).url(RELEASE_FILE),
            entry.release_url(RELEASE_FILE)
        );
        let frozen = StoreEntry {
            frozen: Some("v1.4.0".into()),
            ..entry
        };
        assert_eq!(
            Source::listed(&frozen, RELEASES_URL).url(RELEASE_FILE),
            frozen.release_url(RELEASE_FILE)
        );
        assert!(
            frozen
                .release_url(RELEASE_FILE)
                .contains("/releases/download/v1.4.0/")
        );
    }

    #[test]
    fn a_hand_install_updates_only_with_an_address_and_a_key() {
        let m = |extra: &str| {
            parse_manifest(&format!(
                r#"{{ "name": "x", "displayName": "x", "type": "process", "entry": "plugin" {extra} }}"#
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

    /// A plugin open in a tab runs from its folder, in it. Windows will not
    /// rename that folder, and the update must still go in, twice in a row
    /// (the first update's old program is still running at the second).
    #[cfg(windows)]
    #[test]
    fn an_update_goes_in_while_the_plugin_runs() {
        use std::process::{Command, Stdio};

        let plugins = tempfile::tempdir().unwrap();
        let target = plugins.path().join("demo");
        std::fs::create_dir_all(target.join("locales")).unwrap();
        std::fs::create_dir_all(plugins.path().join(STAGING_DIR)).unwrap();
        // cmd.exe reading its commands from a pipe stands in for a plugin.
        let exe = target.join("plugin.exe");
        std::fs::copy(r"C:\Windows\System32\cmd.exe", &exe).unwrap();
        std::fs::write(target.join("plugin.json"), "1").unwrap();
        let mut running = Command::new(&exe)
            .args(["/q", "/k"])
            .current_dir(&target)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        // Started: it answers.
        {
            use std::io::{BufRead, Write};
            let stdin = running.stdin.as_mut().unwrap();
            writeln!(stdin, "echo ready").unwrap();
            let mut line = String::new();
            std::io::BufReader::new(running.stdout.as_mut().unwrap())
                .read_line(&mut line)
                .unwrap();
            assert!(line.trim_end().ends_with("ready"), "{line}");
        }
        assert!(
            std::fs::rename(&target, plugins.path().join("moved")).is_err(),
            "the running plugin's folder is expected to be held"
        );

        for version in ["2", "3"] {
            let staged = plugins.path().join(STAGING_DIR).join("demo");
            std::fs::create_dir_all(staged.join("locales")).unwrap();
            std::fs::write(staged.join("plugin.exe"), version).unwrap();
            std::fs::write(staged.join("plugin.json"), version).unwrap();
            std::fs::write(staged.join("locales").join("en-US.ftl"), version).unwrap();

            swap_in(plugins.path(), "demo", &staged).unwrap();
            let _ = remove_if_present(&staged);

            for file in ["plugin.exe", "plugin.json", "locales/en-US.ftl"] {
                assert_eq!(std::fs::read_to_string(target.join(file)).unwrap(), version);
            }
        }

        let _ = running.kill();
        let _ = running.wait();
        // Stopped, the old program goes with the next update.
        old_dir(plugins.path(), "demo").unwrap();
        let left: Vec<_> = std::fs::read_dir(plugins.path().join(STAGING_DIR))
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert!(left.is_empty(), "left in staging: {left:?}");
    }
}
