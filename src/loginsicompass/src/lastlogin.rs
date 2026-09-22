//! Remembering which user and session were last used.
//!
//! # Where this lives, and why not the usual place
//!
//! Not `sicompass_sdk::platform::app_state_dir()`. Under greetd the greeter
//! runs as a system user whose home is `/var/empty`
//! (`greeter:x:989:985::/var/empty:…/nologin`), so the usual XDG path resolves
//! somewhere unwritable. The state directory is passed in explicitly instead,
//! defaulting to `/var/lib/loginsicompass`, which the NixOS module creates.
//!
//! Not `/run`, either: that is a tmpfs, so the memory would be lost across
//! exactly the reboot it exists to survive.
//!
//! # Committed only on success
//!
//! [`Store::commit`] is called after greetd has accepted `start_session`, never
//! when the selection changes. Persisting on change would make a mistyped
//! username the remembered default, which is the opposite of the feature.
//!
//! The file holds a login name and a desktop-file id, both of which are already
//! on screen. There is no secret here.

use std::path::{Path, PathBuf};

const FILE_NAME: &str = "last.json";

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Remembered {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    user: Option<String>,
    /// The session's desktop-file *id*, never its `Name=`: the display name is
    /// locale-dependent and would stop matching if the language changed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    session: Option<String>,
}

pub struct Store {
    path: PathBuf,
    current: Remembered,
    loaded: Remembered,
}

impl Store {
    /// Read the remembered selection. A missing, unreadable or corrupt file is
    /// an empty memory, never an error: the greeter must come up regardless.
    pub fn load(dir: &Path) -> Self {
        let path = dir.join(FILE_NAME);
        let loaded = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| match serde_json::from_str::<Remembered>(&s) {
                Ok(v) => Some(v),
                Err(e) => {
                    tracing::warn!("ignoring unreadable {}: {e}", path.display());
                    None
                }
            })
            .unwrap_or_default();
        Self {
            path,
            current: loaded.clone(),
            loaded,
        }
    }

    pub fn user(&self) -> Option<&str> {
        self.loaded.user.as_deref()
    }

    pub fn session(&self) -> Option<&str> {
        self.loaded.session.as_deref()
    }

    pub fn set_user(&mut self, name: &str) {
        self.current.user = Some(name.to_owned());
    }

    /// Takes the desktop-file id, not the display name.
    pub fn set_session(&mut self, id: &str) {
        self.current.session = Some(id.to_owned());
    }

    /// Persist. Called only after `start_session` succeeded.
    pub fn commit(&self) {
        if self.current == self.loaded {
            return;
        }
        let Ok(json) = serde_json::to_string_pretty(&self.current) else {
            return;
        };
        if let Err(e) = write_atomic(&self.path, json.as_bytes()) {
            // A greeter that cannot remember is still a working greeter.
            tracing::warn!("could not save {}: {e}", self.path.display());
        }
    }
}

/// Write via a temporary file in the same directory, then rename.
///
/// A half-written `last.json` would be parsed as an empty memory on the next
/// boot, which is harmless, but a torn write during a power cut is exactly the
/// situation where the file matters most.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let tmp = path.with_extension("json.tmp");
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// Index of `wanted` in `names`, or 0.
///
/// A remembered user who has since been deleted, or a session that is no longer
/// installed, silently falls back to the first entry rather than leaving the
/// greeter pointing at nothing.
pub fn index_of(names: &[String], wanted: Option<&str>) -> usize {
    wanted
        .and_then(|w| names.iter().position(|n| n == w))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn round_trips_through_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::load(dir.path());
        assert_eq!(s.user(), None);
        s.set_user("nico");
        s.set_session("desicompass");
        s.commit();

        let again = Store::load(dir.path());
        assert_eq!(again.user(), Some("nico"));
        assert_eq!(again.session(), Some("desicompass"));
    }

    /// The point of the whole module: a selection that was never committed
    /// must not be remembered.
    #[test]
    fn dropping_without_commit_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut s = Store::load(dir.path());
            s.set_user("typo");
            s.set_session("desicompass");
            // no commit
        }
        assert!(!dir.path().join(FILE_NAME).exists());
        assert_eq!(Store::load(dir.path()).user(), None);
    }

    #[test]
    fn commit_does_not_clobber_when_nothing_changed() {
        let dir = tempfile::tempdir().unwrap();
        let mut s = Store::load(dir.path());
        s.set_user("nico");
        s.commit();
        let first = std::fs::read_to_string(dir.path().join(FILE_NAME)).unwrap();

        let s2 = Store::load(dir.path());
        s2.commit(); // no changes
        let second = std::fs::read_to_string(dir.path().join(FILE_NAME)).unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn corrupt_file_reads_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(FILE_NAME), "{ this is not json").unwrap();
        let s = Store::load(dir.path());
        assert_eq!(s.user(), None);
        assert_eq!(s.session(), None);
    }

    #[test]
    fn missing_directory_is_created_on_commit() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("a/b/c");
        let mut s = Store::load(&nested);
        s.set_user("nico");
        s.commit();
        assert_eq!(Store::load(&nested).user(), Some("nico"));
    }

    #[test]
    fn a_remembered_value_that_no_longer_exists_falls_back_to_the_first() {
        let names = ids(&["alice", "nico"]);
        assert_eq!(index_of(&names, Some("nico")), 1);
        assert_eq!(index_of(&names, Some("deleted-user")), 0);
        assert_eq!(index_of(&names, None), 0);
        assert_eq!(index_of(&[], Some("anyone")), 0);
    }
}
