//! What the user approved, per plugin: `settings.json`'s [`APPROVALS_KEY`],
//! `{ "<name>": "<approval fingerprint>" }`.
//!
//! The Store records it itself, whichever process runs it: the app, or the
//! desicompass superkey, which shows the Store in a session. The app only
//! reads it (`plugin_manifest::read_approvals`), on every load, and follows
//! the file and the plugins folder, so an install made in another process
//! starts the plugin there too.
//!
//! [`APPROVALS_KEY`]: crate::APPROVALS_KEY

use std::collections::HashMap;
use std::path::Path;

use serde_json::{Map, Value};
use sicompass_sdk::plugin_manifest::PluginManifest;

use crate::APPROVALS_KEY;

/// The approvals recorded in `settings`, by plugin name.
pub fn read(settings: &Path) -> HashMap<String, String> {
    std::fs::read_to_string(settings)
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .and_then(|v| v.get(APPROVALS_KEY).cloned())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

/// Whether the user approved this version of an installed plugin: the line
/// recorded for it in `settings` is the one its manifest asks for now. A
/// plugin that needs no approval is approved, and with no settings file
/// nothing else is.
pub fn is_approved(settings: Option<&Path>, m: &PluginManifest) -> bool {
    if !sicompass_sdk::plugin_abi::needs_approval(m) {
        return true;
    }
    let want = sicompass_sdk::plugin_abi::approval_fingerprint(m);
    settings.is_some_and(|p| read(p).get(&m.name) == Some(&want))
}

/// Record that the user approved `m` as it is: by pressing Install, Update or
/// approve, for whatever the manifest asks, so a later update asking for more
/// is noticed.
pub fn record(settings: &Path, m: &PluginManifest) -> Result<(), String> {
    edit(settings, |root| {
        object_at(root, APPROVALS_KEY).insert(
            m.name.clone(),
            Value::String(sicompass_sdk::plugin_abi::approval_fingerprint(m)),
        );
    })
}

/// Forget an uninstalled plugin's approval. Its own settings section is kept,
/// like its data folder: reinstalling finds them again.
pub fn forget(settings: &Path, name: &str) -> Result<(), String> {
    edit(settings, |root| {
        object_at(root, APPROVALS_KEY).remove(name);
    })
}

fn object_at<'a>(root: &'a mut Map<String, Value>, key: &str) -> &'a mut Map<String, Value> {
    let slot = root
        .entry(key.to_owned())
        .or_insert_with(|| Value::Object(Default::default()));
    if !slot.is_object() {
        *slot = Value::Object(Default::default());
    }
    slot.as_object_mut().expect("just made an object")
}

/// Read-modify-write `settings.json`, the way the settings provider does: a
/// file that exists but does not parse is left alone (another process may be
/// half-way through writing it), never rebuilt from nothing.
fn edit(path: &Path, f: impl FnOnce(&mut Map<String, Value>)) -> Result<(), String> {
    let mut root = match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(m)) => m,
            _ => return Err(format!("{} does not parse, left as it is", path.display())),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Default::default(),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    f(&mut root);
    if let Some(parent) = path.parent() {
        sicompass_sdk::platform::make_dirs(parent);
    }
    let json = serde_json::to_string_pretty(&Value::Object(root)).map_err(|e| e.to_string())?;
    if sicompass_sdk::platform::atomic_write(path, &json) {
        Ok(())
    } else {
        Err(format!("{} could not be written", path.display()))
    }
}
