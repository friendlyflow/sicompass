//! A debug build runs the plugins of the checkouts next to it.
//!
//! sicompass is worked on beside its plugins, each a sibling checkout
//! (`../notes-plugin-sicompass`, `../projectmanagement-plugin-sicompass`, ...).
//! A plugin is a program of its own, and the app would otherwise run the copy
//! the Store installed. Instead, at start-up a debug build links every sibling
//! `<x>-plugin-sicompass` that has a `target/debug/<x>-plugin` into
//! `target/dev-plugins/<name>/` and points `SICOMPASS_PLUGIN_PATH` there, the
//! variable the desicompass dev session uses for the same thing. A debug
//! `cargo build` here builds those checkouts first (`build.rs`), so after it
//! `target/debug/sicompass` runs both as they are on disk.
//!
//! A plugin found there replaces the Store's copy of the same name and runs
//! without asking ([`sicompass_sdk::platform::system_plugin_dirs`]). A sibling
//! with no debug build is left to the Store's copy. A `SICOMPASS_PLUGIN_PATH`
//! set by the caller wins, so `SICOMPASS_PLUGIN_PATH= target/debug/sicompass`
//! runs the Store's copies. Release builds never do any of this.

use std::path::{Path, PathBuf};

/// The name every plugin checkout's folder ends in.
pub const CHECKOUT_SUFFIX: &str = "-plugin-sicompass";

/// Link each checkout in `siblings` that has a debug build into `out`, one
/// folder per plugin, laid out as the Store lays out an install. `out` is
/// rebuilt from scratch. Returns the plugin names, sorted.
pub fn link_checkouts(siblings: &Path, out: &Path) -> std::io::Result<Vec<String>> {
    if out.exists() {
        // Links only: removing them leaves the checkouts alone.
        std::fs::remove_dir_all(out)?;
    }
    std::fs::create_dir_all(out)?;
    let target = sicompass_sdk::plugin_abi::plugin_target()
        .ok_or_else(|| std::io::Error::other("no plugin target for this platform"))?;

    let mut names = Vec::new();
    for entry in std::fs::read_dir(siblings)? {
        let checkout = entry?.path();
        let Some(prefix) = checkout
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.strip_suffix(CHECKOUT_SUFFIX))
        else {
            continue;
        };
        let program = checkout
            .join("target")
            .join("debug")
            .join(format!("{prefix}-plugin{}", std::env::consts::EXE_SUFFIX));
        let manifest = checkout.join("plugin.json");
        if !program.is_file() || !manifest.is_file() {
            continue;
        }
        let Some((name, entry)) = name_and_entry(&manifest) else {
            continue;
        };
        let dir = out.join(&name);
        std::fs::create_dir_all(&dir)?;
        link(&manifest, &dir.join("plugin.json"))?;
        let locales = checkout.join("locales");
        if locales.is_dir() {
            link(&locales, &dir.join("locales"))?;
        }
        link(
            &program,
            &dir.join(sicompass_sdk::plugin_abi::executable_name(&entry, target)),
        )?;
        names.push(name);
    }
    names.sort();
    Ok(names)
}

/// `plugin.json`'s `name` and `entry`.
fn name_and_entry(manifest: &Path) -> Option<(String, String)> {
    let json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(manifest).ok()?).ok()?;
    Some((
        json.get("name")?.as_str()?.to_owned(),
        json.get("entry")?.as_str()?.to_owned(),
    ))
}

#[cfg(unix)]
fn link(from: &Path, to: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(from, to)
}

/// Windows grants symlinks to administrators and developer mode only, so the
/// files are copied instead.
#[cfg(not(unix))]
fn link(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for entry in std::fs::read_dir(from)? {
            let entry = entry?;
            link(&entry.path(), &to.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        std::fs::copy(from, to).map(|_| ())
    }
}

/// In a debug build, when the caller has not set `SICOMPASS_PLUGIN_PATH`:
/// link the sibling checkouts' plugins and set it. Returns what to log: the
/// plugins linked, or why none were. `None` in a release build, or when the
/// caller set the variable.
///
/// # Safety
///
/// Sets an environment variable, so it must run before any other thread
/// exists: first thing in `main`.
pub unsafe fn use_sibling_checkouts() -> Option<Result<Vec<String>, String>> {
    if !cfg!(debug_assertions)
        || std::env::var_os(sicompass_sdk::platform::PLUGIN_PATH_VAR).is_some()
    {
        return None;
    }
    // This crate is `<workspace>/src`, and the checkouts sit beside the
    // workspace.
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR")).parent()?;
    let siblings = workspace.parent()?;
    let out: PathBuf = workspace.join("target").join("dev-plugins");
    Some(match link_checkouts(siblings, &out) {
        Ok(names) => {
            // SAFETY: the caller guarantees no other thread exists yet.
            unsafe { std::env::set_var(sicompass_sdk::platform::PLUGIN_PATH_VAR, &out) };
            Ok(names)
        }
        Err(e) => Err(format!("{}: {e}", out.display())),
    })
}
