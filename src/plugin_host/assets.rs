//! A plugin's own files: `<plugin_dir>/assets/`, reachable as
//! `asset:<plugin-name>/<file>` in an `<image>` or `<link>` tag and from
//! `dashboard_image_path`, the same URI shape a built-in's compiled-in assets
//! have.

use std::path::{Path, PathBuf};

/// Where a plugin's own files live, relative to its install directory.
pub const ASSET_SUBDIR: &str = "assets";

/// The lexical half of confinement: join `rel` onto `root`, refusing anything that
/// names an absolute location or climbs out.
pub fn confine_in(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let candidate = Path::new(rel);
    // `is_absolute()` alone is not enough, because it is platform-dependent:
    // on Windows `/etc/passwd` is *not* absolute (it carries no drive prefix)
    // and would otherwise fall through. A prefix or root component means the
    // plugin named an absolute location whichever platform we are on, so check
    // the components too and report both the same way.
    let rooted = candidate.components().any(|c| {
        matches!(
            c,
            std::path::Component::Prefix(_) | std::path::Component::RootDir
        )
    });
    if candidate.is_absolute() || rooted {
        return Err(format!(
            "`{rel}` is absolute; plugin paths must be relative"
        ));
    }
    if candidate
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(format!("`{rel}` escapes the plugin directory"));
    }
    Ok(root.join(candidate))
}

/// Largest asset the app reads for a plugin, which is an image it decodes.
pub const MAX_ASSET_BYTES: u64 = 16 * 1024 * 1024;

/// Read `name` from `root`, refusing anything that does not stay inside it.
///
/// Two layers, because [`confine_in`] is purely lexical and cannot see a
/// symlink: the lexical check refuses the obvious attempts with a message worth
/// logging, then canonicalizing both sides and requiring containment closes the
/// symlink hole, including a symlinked *intermediate* directory, which inspecting
/// the leaf alone would miss. The app reads these for whichever plugin's `asset:`
/// URI appears in a tag, so a URI cannot name a file outside that plugin's
/// `assets/`.
pub fn read_confined_asset(root: &Path, name: &str) -> Option<Vec<u8>> {
    // Check the plugin's string as given. Joining first would be wrong: `assets/`
    // prefixed onto `/etc/passwd` yields `assets//etc/passwd`, whose components
    // silently drop the root, turning a refusal into a quiet acceptance.
    let candidate = match confine_in(root, name) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(target: "plugin", root = %root.display(), "asset `{name}`: {e}");
            return None;
        }
    };

    // `canonicalize` needs the path to exist, so a missing file lands here and is
    // reported the same way a refused one is.
    let (real_root, real) = match (root.canonicalize(), candidate.canonicalize()) {
        (Ok(r), Ok(c)) => (r, c),
        _ => {
            tracing::warn!(target: "plugin", root = %root.display(),
                           "asset `{name}` is missing or unreadable");
            return None;
        }
    };
    if !real.starts_with(&real_root) {
        tracing::warn!(target: "plugin", root = %real_root.display(),
                       "asset `{name}` resolves to {} — outside the asset directory",
                       real.display());
        return None;
    }

    match std::fs::metadata(&real) {
        Ok(m) if !m.is_file() => {
            tracing::warn!(target: "plugin", "asset `{name}` is not a regular file");
            None
        }
        Ok(m) if m.len() > MAX_ASSET_BYTES => {
            tracing::warn!(target: "plugin", "asset `{name}` is {} bytes, over the {MAX_ASSET_BYTES} cap",
                           m.len());
            None
        }
        Ok(_) => std::fs::read(&real).ok(),
        Err(_) => None,
    }
}

/// Make `<plugin_dir>/assets/` reachable as `asset:<plugin-name>/<file>`.
///
/// Called once per plugin at load time. This is what gives a plugin the same
/// asset story a built-in has: same URI shape in an `<image>` or `<link>` tag,
/// same call site in the app, different byte source.
///
/// `plugin_name` is the **manifest** name, never the plugin's self-reported
/// `describe().name`, so a plugin cannot claim another one's namespace.
pub fn register_plugin_assets(plugin_name: &str, plugin_dir: &Path) {
    let root = plugin_dir.join(ASSET_SUBDIR);
    sicompass_sdk::assets::register_resolver(
        plugin_name,
        Box::new(move |name| read_confined_asset(&root, name)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "/tmp/sicompass-test-plugin";

    #[test]
    fn confine_accepts_a_plain_relative_path() {
        assert_eq!(
            confine_in(Path::new(ROOT), "assets/logo.png").unwrap(),
            Path::new("/tmp/sicompass-test-plugin/assets/logo.png")
        );
    }

    #[test]
    fn confine_rejects_absolute_paths() {
        let err = confine_in(Path::new(ROOT), "/etc/passwd").unwrap_err();
        assert!(err.contains("absolute"), "unexpected error: {err}");
    }

    #[test]
    fn confine_rejects_parent_traversal() {
        for attempt in ["../secrets", "assets/../../etc/passwd", "a/b/../../../c"] {
            let err = confine_in(Path::new(ROOT), attempt)
                .expect_err(&format!("`{attempt}` should have been rejected"));
            assert!(
                err.contains("escapes"),
                "unexpected error for {attempt}: {err}"
            );
        }
    }

    #[test]
    fn confine_rejects_a_bare_root() {
        assert!(confine_in(Path::new(ROOT), "/").is_err());
    }

    // --- read-asset ---

    /// A plugin directory with `assets/greeting.txt` in it.
    fn plugin_with_asset(body: &[u8]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let assets = dir.path().join(ASSET_SUBDIR);
        std::fs::create_dir_all(&assets).unwrap();
        std::fs::write(assets.join("greeting.txt"), body).unwrap();
        dir
    }

    #[test]
    fn read_confined_asset_returns_the_bytes() {
        let dir = plugin_with_asset(b"hello");
        let root = dir.path().join(ASSET_SUBDIR);
        assert_eq!(
            read_confined_asset(&root, "greeting.txt").as_deref(),
            Some(&b"hello"[..])
        );
    }

    #[test]
    fn read_confined_asset_refuses_an_absolute_path() {
        // The regression test for checking the guest's string *before* joining: with
        // the naive `assets/{rel}` prefix, `/etc/passwd` becomes `assets//etc/passwd`,
        // whose components drop the root, and the refusal turns into an acceptance.
        let dir = plugin_with_asset(b"hello");
        let root = dir.path().join(ASSET_SUBDIR);
        for attempt in ["/etc/passwd", "/"] {
            assert!(
                read_confined_asset(&root, attempt).is_none(),
                "`{attempt}` should have been refused"
            );
        }
    }

    #[test]
    fn read_confined_asset_refuses_parent_traversal() {
        let dir = plugin_with_asset(b"hello");
        std::fs::write(dir.path().join("plugin.json"), b"{}").unwrap();
        let root = dir.path().join(ASSET_SUBDIR);
        for attempt in ["../plugin.json", "a/../../plugin.json", "../../etc/passwd"] {
            assert!(
                read_confined_asset(&root, attempt).is_none(),
                "`{attempt}` should have been refused"
            );
        }
    }

    #[test]
    fn read_confined_asset_refuses_a_missing_file() {
        let dir = plugin_with_asset(b"hello");
        let root = dir.path().join(ASSET_SUBDIR);
        assert!(read_confined_asset(&root, "nope.txt").is_none());
    }

    #[test]
    fn read_confined_asset_refuses_a_directory() {
        let dir = plugin_with_asset(b"hello");
        let root = dir.path().join(ASSET_SUBDIR);
        std::fs::create_dir(root.join("sub")).unwrap();
        assert!(read_confined_asset(&root, "sub").is_none());
    }

    #[test]
    fn read_confined_asset_refuses_a_file_over_the_cap() {
        let dir = plugin_with_asset(b"hello");
        let root = dir.path().join(ASSET_SUBDIR);
        let big = root.join("big.bin");
        let f = std::fs::File::create(&big).unwrap();
        // Sparse: sets the length without writing 16 MiB.
        f.set_len(MAX_ASSET_BYTES + 1).unwrap();
        drop(f);
        assert!(read_confined_asset(&root, "big.bin").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn read_confined_asset_refuses_a_symlink_out_of_the_asset_dir() {
        // `confine_in` is lexical and cannot see this, which is why the read also
        // canonicalizes and requires containment.
        let dir = plugin_with_asset(b"hello");
        let root = dir.path().join(ASSET_SUBDIR);
        std::fs::write(dir.path().join("secret.txt"), b"secret").unwrap();
        std::os::unix::fs::symlink(dir.path().join("secret.txt"), root.join("link.txt")).unwrap();
        assert!(read_confined_asset(&root, "link.txt").is_none());
    }

    #[cfg(unix)]
    #[test]
    fn read_confined_asset_refuses_a_symlinked_parent_directory() {
        // The case inspecting only the leaf would miss: the file itself is a regular
        // file inside the named directory, but the directory leaves `assets/`.
        let dir = plugin_with_asset(b"hello");
        let root = dir.path().join(ASSET_SUBDIR);
        let outside = dir.path().join("outside");
        std::fs::create_dir(&outside).unwrap();
        std::fs::write(outside.join("secret.txt"), b"secret").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("sub")).unwrap();
        assert!(read_confined_asset(&root, "sub/secret.txt").is_none());
    }

    #[test]
    fn register_plugin_assets_serves_the_plugin_directory_as_asset_uris() {
        let dir = plugin_with_asset(b"hello");
        register_plugin_assets("__plugin_asset_test", dir.path());
        assert_eq!(
            sicompass_sdk::assets::resolve("asset:__plugin_asset_test/greeting.txt").as_deref(),
            Some(&b"hello"[..])
        );
        // And the resolver is confined, not merely a path join.
        assert!(
            sicompass_sdk::assets::resolve("asset:__plugin_asset_test/../plugin.json").is_none()
        );
    }
}
