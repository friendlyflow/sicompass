//! The plugin updates waiting for this computer, found without opening the
//! Store, and installed without it.
//!
//! The app checks once at startup, beside its own update, and lists what it
//! found in one message. Ctrl+U then installs every update that asks for
//! nothing the user has not approved, exactly as the Store's Update button
//! does ([`install::install_and_approve`]). An update that asks for more
//! access, or of a plugin the user never approved, is only ever installed by a
//! press in the Store, where the access is shown.
//!
//! Plugins this computer's configuration provides (`SICOMPASS_PLUGIN_PATH`)
//! are left alone, as the Store leaves them.

use std::path::PathBuf;

use sicompass_sdk::package::ReleaseInfo;

use crate::http::{self, Fetch};
use crate::install::{self, PluginOrigin, Source};
use crate::{approvals, load_offers, source};

/// Where the store list, the releases, the plugins and the approvals are.
#[derive(Clone)]
pub struct Sources {
    pub fetch: Fetch,
    /// The folder holding `store.json` and its signature, ending in `/`.
    pub store_url: String,
    pub releases_url: String,
    /// The keys the store list may be signed with.
    pub trusted: Vec<String>,
    pub plugins_dir: PathBuf,
    /// The folders this computer's configuration provides.
    pub system_plugin_dirs: Vec<PathBuf>,
    /// `settings.json`, where approvals are recorded. `None` when there is no
    /// settings folder: then nothing needing approval is installed.
    pub settings: Option<PathBuf>,
}

impl Sources {
    /// This computer's, as the Store uses them. `None` without a plugins
    /// folder, where there is nothing to update.
    pub fn here() -> Option<Self> {
        Some(Sources {
            fetch: http::http_fetch(),
            store_url: source::STORE_URL.to_owned(),
            releases_url: install::RELEASES_URL.to_owned(),
            trusted: source::TRUSTED_KEYS
                .iter()
                .map(|k| (*k).to_owned())
                .collect(),
            plugins_dir: sicompass_sdk::platform::plugins_dir()?,
            system_plugin_dirs: sicompass_sdk::platform::system_plugin_dirs(),
            settings: sicompass_sdk::platform::main_config_path(),
        })
    }
}

/// A newer release of an installed plugin.
#[derive(Debug, Clone)]
pub struct PendingUpdate {
    pub name: String,
    /// The installed version.
    pub from: String,
    /// The release's version.
    pub to: String,
    /// It asks for more access than the installed version, or the installed
    /// version was never approved: only the Store installs it, after showing
    /// what it asks for.
    pub needs_approval: bool,
    source: Source,
    /// What was found, so an install refuses a release published since.
    shown: ReleaseInfo,
}

/// Every installed plugin with a newer release this sicompass can run.
pub fn check(sources: &Sources) -> Vec<PendingUpdate> {
    let keys: Vec<&str> = sources.trusted.iter().map(String::as_str).collect();
    let (_, offers) = load_offers(
        &sources.fetch,
        &sources.store_url,
        &sources.releases_url,
        &keys,
        &sources.system_plugin_dirs,
        Some(&sources.plugins_dir),
    );
    let installed =
        install::installed_everywhere(&sources.system_plugin_dirs, Some(&sources.plugins_dir));
    offers
        .into_iter()
        .filter_map(|offer| {
            let current = installed.get(&offer.name)?;
            if current.origin == PluginOrigin::System {
                return None;
            }
            let source = offer.source?;
            let release = offer.release.ok()?;
            if !install::is_newer(&release, &current.manifest)
                || install::needs_newer_app(&release).is_some()
            {
                return None;
            }
            let (_, sha) = release
                .archive_for(sicompass_sdk::plugin_abi::plugin_target())
                .ok()?;
            if source.is_revoked(sha) {
                return None;
            }
            let needs_approval = release.asks_for_more_than(&current.manifest)
                || !approvals::is_approved(sources.settings.as_deref(), &current.manifest);
            Some(PendingUpdate {
                name: offer.name,
                from: current.manifest.version.clone().unwrap_or_default(),
                to: release.version.clone(),
                needs_approval,
                source,
                shown: release,
            })
        })
        .collect()
}

/// Install `update` and record its approval. Refused when it needs one the
/// user has not given. Returns the installed version.
pub fn install(sources: &Sources, update: &PendingUpdate) -> Result<String, String> {
    if update.needs_approval {
        return Err("it asks for more access, approve it in the store".to_owned());
    }
    install::install_and_approve(
        &sources.fetch,
        &update.source,
        &update.shown,
        &sources.plugins_dir,
        sources
            .settings
            .clone()
            .ok_or_else(|| "no settings folder on this system".to_owned()),
    )
}
