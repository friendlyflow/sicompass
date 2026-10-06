//! What a plugin process asks the app, and the app's answers.
//!
//! The same services a WASM guest reaches through its imports, minus the
//! confinement: a plugin process names real paths, and could reach them itself.
//! What stays scoped is what the app holds for the user rather than the
//! plugin: settings (its own section only) and the licence token (its own
//! service's only). Those limits are the app's manners, not a sandbox: the
//! process runs with the user's rights.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use sicompass_sdk::license::{LicenseStatus, Standing};
use sicompass_sdk::plugin_ipc::{
    Application, HostRequest, HostResponse, OauthReply, TierStanding, TierStatus,
};

use super::desktop;

/// The URLs a plugin was asked to render and has not answered yet, shared
/// between the provider that asks and the thread that takes the answer.
#[derive(Default)]
pub struct Renders {
    asked: Mutex<HashSet<String>>,
    answered: AtomicU64,
}

impl Renders {
    pub fn ask(&self, url: &str) {
        if let Ok(mut a) = self.asked.lock() {
            a.insert(url.to_owned());
        }
    }

    pub fn forget(&self, url: &str) {
        if let Ok(mut a) = self.asked.lock() {
            a.remove(url);
        }
    }

    /// How many render requests the plugin has answered so far.
    pub fn answered(&self) -> u64 {
        self.answered.load(Ordering::Acquire)
    }
}

/// One plugin's view of the app.
pub struct Services {
    pub plugin_name: String,
    /// The settings section: the manifest's `displayName`, which is where
    /// `programs::inject_plugin_settings` registers its settings.
    pub settings_section: String,
    /// Each declared setting's default, `~` expanded.
    pub setting_defaults: Vec<(String, String)>,
    /// The keys of the settings `plugin.json` declares.
    pub setting_keys: Vec<String>,
    /// The tier its manifest names as its `service`: the only one it gets a
    /// token for.
    pub service_tier: Option<String>,
    pub renders: Arc<Renders>,
    /// Set when the plugin is gone, which ends a sign-in it waits on.
    pub closed: Arc<AtomicBool>,
}

impl Services {
    /// One of the plugin's own settings: the saved value, or the manifest's
    /// default until there is one. `~` is the home folder either way.
    pub fn setting(&self, key: &str) -> Option<String> {
        if !self.setting_keys.iter().any(|k| k == key) {
            return None;
        }
        read_plugin_setting(&self.settings_section, key)
            .map(|v| crate::plugin_manifest::expand_home(&v))
            .or_else(|| {
                self.setting_defaults
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, v)| v.clone())
            })
    }

    /// The current value of every declared setting that has one, for `init`.
    pub fn settings(&self) -> Vec<(String, String)> {
        self.setting_keys
            .iter()
            .filter_map(|k| self.setting(k).map(|v| (k.clone(), v)))
            .collect()
    }

    pub fn answer(&self, request: HostRequest) -> HostResponse {
        use HostResponse as R;
        match request {
            HostRequest::GetSetting(key) => R::OptStr(self.setting(&key)),
            HostRequest::Translate { key, args } if args.is_empty() => {
                R::Str(sicompass_sdk::localize::t(&key))
            }
            HostRequest::Translate { key, args } => {
                let mut fluent = sicompass_sdk::localize::Args::new();
                for (name, value) in args {
                    fluent.set(name, value);
                }
                R::Str(sicompass_sdk::localize::t_args(&key, &fluent))
            }
            HostRequest::LicenseStatus(tier) => {
                R::Status(tier_status(sicompass_sdk::license::status(&tier)))
            }
            HostRequest::LicenseStanding(tier) => {
                R::Standing(tier_standing(sicompass_sdk::license::standing(&tier)))
            }
            HostRequest::LicenseToken(tier) => R::OptStr(
                (self.service_tier.as_deref() == Some(tier.as_str()))
                    .then(|| sicompass_sdk::license::token(&tier))
                    .flatten(),
            ),
            HostRequest::OpenUrl(url) => R::Done(desktop::open_url(&url)),
            HostRequest::OpenPath(path) => R::Done(real_path(&path).and_then(|p| desktop::open_path(&p))),
            HostRequest::Applications => R::Applications(
                desktop::applications()
                    .into_iter()
                    .map(|(name, id)| Application { name, id })
                    .collect(),
            ),
            HostRequest::OpenWith { id, path } => {
                R::Done(real_path(&path).and_then(|p| desktop::open_with(&id, &p)))
            }
            HostRequest::Trash(path) => R::Done(real_path(&path).and_then(|p| desktop::trash(&p))),
            HostRequest::Restore(path) => {
                R::Done(real_path(&path).and_then(|p| desktop::restore(&p)))
            }
            HostRequest::OauthRedirect {
                auth_url,
                timeout_secs,
            } => R::Oauth(
                desktop::sign_in(&auth_url, timeout_secs, &self.closed)
                    .map(|(redirect_uri, query)| OauthReply { redirect_uri, query }),
            ),
            HostRequest::Rendered { url, page } => {
                let asked = self
                    .renders
                    .asked
                    .lock()
                    .map(|mut a| a.remove(&url))
                    .unwrap_or(false);
                if asked {
                    self.renders.answered.fetch_add(1, Ordering::AcqRel);
                    sicompass_sdk::url_fetcher::deliver_render(
                        &url,
                        sicompass_sdk::ffon::deserialize_binary(&page),
                    );
                } else {
                    tracing::warn!(target: "plugin", plugin = %self.plugin_name, "rendered {url}, which it was not asked for");
                }
                R::Unit
            }
        }
    }
}

/// Read one setting from the plugin's own section of the user's `settings.json`.
///
/// Only the section its own manifest declared: other providers' sections hold
/// API keys, IMAP passwords and licence certificates. (A plugin process could
/// read the file itself, but the app does not hand them over.)
pub(crate) fn read_plugin_setting(section: &str, key: &str) -> Option<String> {
    let path = sicompass_sdk::platform::main_config_path()?;
    let data = std::fs::read_to_string(&path).ok()?;
    let root: serde_json::Value = serde_json::from_str(&data).ok()?;

    // `programs::inject_plugin_settings` registers under the manifest's
    // `displayName`, which is what `section` is. The spaces-stripped fallback
    // matches `programs::instantiate_builtin`'s leniency about names like
    // "chat client" vs "chatclient".
    let compact: String = section.chars().filter(|&c| c != ' ').collect();
    for candidate in [section, compact.as_str()] {
        if let Some(v) = root.get(candidate).and_then(|s| s.get(key)) {
            return Some(match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            });
        }
    }
    None
}

/// A path a plugin names: absolute, as the plugin process sees the
/// filesystem, which is the app's view too.
fn real_path(path: &str) -> Result<PathBuf, String> {
    let p = Path::new(path);
    if p.is_absolute() {
        Ok(p.to_path_buf())
    } else {
        Err(format!("`{path}` is not an absolute path"))
    }
}

fn tier_status(s: LicenseStatus) -> TierStatus {
    match s {
        LicenseStatus::Active => TierStatus::Active,
        LicenseStatus::Grace => TierStatus::Grace,
        LicenseStatus::Expired => TierStatus::Expired,
        LicenseStatus::Missing => TierStatus::Missing,
    }
}

fn tier_standing(s: Standing) -> TierStanding {
    TierStanding {
        status: tier_status(s.status),
        days: s.days,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn services(service_tier: Option<&str>) -> Services {
        Services {
            plugin_name: "notes".into(),
            settings_section: "notes".into(),
            setting_defaults: vec![("folder".into(), "/data/notes".into())],
            setting_keys: vec!["folder".into()],
            service_tier: service_tier.map(str::to_owned),
            renders: Arc::default(),
            closed: Arc::default(),
        }
    }

    #[test]
    fn a_token_only_for_the_plugins_own_service() {
        sicompass_sdk::license::register_token_source(|tier| Some(format!("token-for-{tier}")));
        let ask = |s: &Services, tier: &str| s.answer(HostRequest::LicenseToken(tier.into()));
        assert_eq!(
            ask(&services(None), "friendlyflow/cloud"),
            HostResponse::OptStr(None)
        );
        let own = services(Some("friendlyflow/cloud"));
        assert_eq!(
            ask(&own, "friendlyflow/cloud"),
            HostResponse::OptStr(Some("token-for-friendlyflow/cloud".into()))
        );
        assert_eq!(ask(&own, "acme/pro"), HostResponse::OptStr(None));
    }

    #[test]
    fn only_declared_settings_are_answered_and_defaults_fill_in() {
        let s = services(None);
        assert_eq!(
            s.answer(HostRequest::GetSetting("folder".into())),
            HostResponse::OptStr(Some("/data/notes".into()))
        );
        assert_eq!(
            s.answer(HostRequest::GetSetting("emailPassword".into())),
            HostResponse::OptStr(None)
        );
        assert_eq!(s.settings(), vec![("folder".into(), "/data/notes".into())]);
    }

    #[test]
    fn a_page_is_taken_only_when_it_was_asked_for() {
        let s = services(None);
        let page = sicompass_sdk::ffon::serialize_binary(&[sicompass_sdk::FfonElement::new_str("p")]);
        let rendered = |url: &str| HostRequest::Rendered {
            url: url.into(),
            page: page.clone(),
        };
        s.answer(rendered("https://never.example"));
        assert_eq!(s.renders.answered(), 0);
        s.renders.ask("https://asked.example");
        s.answer(rendered("https://asked.example"));
        assert_eq!(s.renders.answered(), 1);
        // Once.
        s.answer(rendered("https://asked.example"));
        assert_eq!(s.renders.answered(), 1);
    }

    #[test]
    fn desktop_paths_must_be_absolute() {
        let s = services(None);
        match s.answer(HostRequest::OpenPath("relative/file".into())) {
            HostResponse::Done(Err(e)) => assert!(e.contains("not an absolute path"), "{e}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_sign_in_ends_when_the_plugin_is_gone() {
        let s = services(None);
        s.closed.store(true, Ordering::Release);
        match s.answer(HostRequest::OauthRedirect {
            auth_url: "https://accounts.example/auth?r={redirect-uri}".into(),
            timeout_secs: 30,
        }) {
            HostResponse::Oauth(Err(e)) => assert!(e.contains("cancelled"), "{e}"),
            other => panic!("{other:?}"),
        }
    }
}
