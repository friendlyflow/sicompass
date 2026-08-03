//! Locating the `assets/` tree at runtime.
//!
//! Shaders and fonts are compiled into the binary (see [`crate::shaders`] and
//! [`crate::fonts`]), so `assets/` is the only runtime tree left. It cannot be
//! embedded as cheaply: it holds the tutorial content and demo images, it is
//! reached through the plugin SDK rather than directly, and providers expect
//! real paths they can hand to an image decoder.
//!
//! The SDK's [`sicompass_sdk::platform::resolve_repo_asset`] does the actual
//! lookup, but its first candidate is built from the *SDK's* own
//! `CARGO_MANIFEST_DIR`, which on an end-user machine points into
//! `~/.cargo/registry`. Its remaining candidates are the executable directory,
//! its parent, and the process working directory.
//!
//! So this module finds the right directory using the layouts the packages
//! actually produce, and `main` then sets the process working directory to it.
//! That makes the SDK's last candidate land correctly without needing an SDK
//! release. Once `resolve_repo_asset` grows an environment-variable override,
//! the `set_current_dir` call in `main` can go and callers can use
//! [`resource_root`] directly.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Environment variable that overrides the search entirely.
///
/// The Nix derivation sets this with `wrapProgram` rather than relying on any
/// layout guess.
pub const RESOURCE_DIR_ENV: &str = "SICOMPASS_RESOURCE_DIR";

/// Directory that runtime resources sit under, i.e. the directory *containing*
/// `assets/`. Probed once and cached.
///
/// Falls back to the current directory when nothing matches, which keeps the
/// behaviour of a developer running `cargo run` from the repository root even
/// if the probe order somehow misses.
pub fn resource_root() -> &'static Path {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf));
        candidates(exe_dir.as_deref())
            .into_iter()
            .find(|c| is_resource_root(c))
            .unwrap_or_else(|| PathBuf::from("."))
    })
}

/// A directory qualifies if `assets/` is under it. That is the one thing every
/// caller of this is ultimately looking for.
fn is_resource_root(dir: &Path) -> bool {
    dir.join("assets").is_dir()
}

/// Candidate roots, most specific first.
///
/// Split out from [`resource_root`] so the ordering can be tested without a
/// real executable.
fn candidates(exe_dir: Option<&Path>) -> Vec<PathBuf> {
    let mut out = Vec::new();

    // 1. Explicit override. Used by the Nix wrapper, and the escape hatch for
    //    anyone with an unusual layout.
    if let Some(dir) = std::env::var_os(RESOURCE_DIR_ENV) {
        out.push(PathBuf::from(dir));
    }

    if let Some(exe) = exe_dir {
        // 2. Beside the executable: release archives, a plain unzip, and the
        //    MSI, which installs resources next to sicompass.exe.
        out.push(exe.to_path_buf());
        // 3. One above: `target/debug/sicompass` during development, and any
        //    install layout with resources above `bin/`.
        out.push(exe.join(".."));
        // 4. macOS .app: Contents/MacOS/sicompass -> Contents/Resources.
        out.push(exe.join("../Resources"));
        // 5. .deb, .rpm and AppImage all put resources in
        //    `<prefix>/lib/sicompass` with the binary in `<prefix>/bin`.
        //    `current_exe` resolves symlinks on Linux, so this holds even when
        //    /usr/bin/sicompass is a link.
        out.push(exe.join("../lib/sicompass"));
        // 6. FHS share layout, which the Nix derivation uses.
        out.push(exe.join("../share/sicompass"));
    }

    // 7. The repository root, so `cargo run` and `cargo test` work from
    //    anywhere without any of the above matching.
    out.push(PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")));

    // 8. Legacy last resort: whatever the process was launched from.
    out.push(PathBuf::from("."));

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The probe must prefer the executable directory over every install
    /// layout, otherwise a developer build inside a checkout would silently
    /// read a system-installed `assets/` tree.
    #[test]
    fn candidate_order_puts_exe_dir_before_install_layouts() {
        let exe = Path::new("/opt/sicompass/bin");
        let c = candidates(Some(exe));

        let pos = |suffix: &str| {
            c.iter()
                .position(|p| p.ends_with(suffix))
                .unwrap_or_else(|| panic!("no candidate ending in {suffix:?} in {c:?}"))
        };

        assert!(pos("bin") < pos("lib/sicompass"));
        assert!(pos("bin") < pos("share/sicompass"));
        assert!(pos("bin") < pos("Resources"));
    }

    #[test]
    fn candidates_cover_every_package_layout() {
        let c = candidates(Some(Path::new("/opt/sicompass/bin")));
        for suffix in ["Resources", "lib/sicompass", "share/sicompass"] {
            assert!(
                c.iter().any(|p| p.ends_with(suffix)),
                "no candidate for {suffix}"
            );
        }
        // The repository root must always be offered, so development builds
        // work regardless of where the binary sits.
        assert!(c.iter().any(|p| p.ends_with("../..")));
    }

    /// Without an executable path there is nothing layout-specific to try, but
    /// the development and legacy fallbacks must still be present.
    #[test]
    fn candidates_without_exe_dir_still_have_fallbacks() {
        let c = candidates(None);
        assert!(c.iter().any(|p| p.ends_with("../..")));
        assert!(c.iter().any(|p| p == Path::new(".")));
    }

    #[test]
    fn resource_root_finds_a_directory_containing_assets() {
        let tmp = std::env::temp_dir().join(format!(
            "sicompass-resources-test-{}-{}",
            std::process::id(),
            line!()
        ));
        let nested = tmp.join("bin");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::create_dir_all(tmp.join("lib/sicompass/assets")).unwrap();

        let c = candidates(Some(&nested));
        let found = c.into_iter().find(|p| is_resource_root(p));

        assert!(
            found.is_some_and(|p| p.ends_with("lib/sicompass")),
            "the .deb/.rpm/AppImage layout was not detected"
        );

        std::fs::remove_dir_all(&tmp).ok();
    }

    /// A directory without `assets/` must not be accepted, or the probe would
    /// stop at the first candidate every time.
    #[test]
    fn directory_without_assets_is_not_a_resource_root() {
        let tmp = std::env::temp_dir().join(format!(
            "sicompass-resources-empty-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        assert!(!is_resource_root(&tmp));
        std::fs::remove_dir_all(&tmp).ok();
    }

    /// The repository itself has an `assets/` directory, so the development
    /// fallback must resolve during tests. This is what keeps `cargo run` from
    /// the repository root working after the change.
    #[test]
    fn repository_root_is_a_valid_resource_root() {
        let repo = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
        assert!(
            is_resource_root(repo),
            "expected assets/ under the repository root"
        );
    }
}
