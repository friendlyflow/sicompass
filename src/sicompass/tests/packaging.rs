//! Drift guards for the packaging that no compiler checks.
//!
//! The packaging metadata in `Cargo.toml` names files by relative path and the
//! CI workflow names scripts by relative path, and none of it is compiled, so
//! a rename or a typo shows up only as a broken package weeks later. That is
//! how the app shipped for several releases with correct icons on disk that no
//! desktop ever displayed, and how 0.1.10 shipped a macOS .dmg that could not
//! launch at all.
//!
//! These tests are deliberately string-level. Pulling in a TOML parser as a
//! dev-dependency to check that a path exists would cost more than it buys.

use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../")
}

fn read(relative: &str) -> String {
    let path = workspace_root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {}: {e}", path.display()))
}

/// Pull the values out of a `key = [ "a", "b" ]` array in `Cargo.toml`.
fn toml_string_array(manifest: &str, key: &str) -> Vec<String> {
    let start = manifest
        .find(&format!("\n{key} = ["))
        .unwrap_or_else(|| panic!("{key} is missing from src/sicompass/Cargo.toml"));
    let rest = &manifest[start..];
    let end = rest
        .find(']')
        .unwrap_or_else(|| panic!("{key} array is unterminated"));
    rest[..end]
        .lines()
        .skip(1)
        .filter_map(|line| {
            let line = line.trim().trim_end_matches(',');
            line.strip_prefix('"')?.strip_suffix('"').map(str::to_owned)
        })
        .collect()
}

/// Every PNG in cargo-packager's `icons` list has to exist, or the `.deb` and
/// the AppImage silently ship one size fewer.
#[test]
fn packager_icon_paths_all_exist() {
    let manifest = read("src/sicompass/Cargo.toml");
    let icons = toml_string_array(&manifest, "icons");
    assert!(
        icons.len() >= 9,
        "the icons list shrank unexpectedly: {icons:?}"
    );

    for icon in &icons {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(icon);
        assert!(
            path.exists(),
            "{icon} is listed in [package.metadata.packager] but does not exist. \
             Run scripts/gen-icons.sh and commit the result."
        );
    }
}

/// The `.desktop` entry reaches the `.deb` *and* the AppImage through
/// cargo-packager's `desktop-template`. If the path drifts, cargo-packager
/// falls back to its own four-key template without warning, and the AppImage
/// ends up with `Name=sicompass` and no categories.
#[test]
fn desktop_template_points_at_the_real_entry() {
    let manifest = read("src/sicompass/Cargo.toml");
    let line = manifest
        .lines()
        .find(|l| l.trim_start().starts_with("desktop-template"))
        .expect("desktop-template is missing from [package.metadata.packager.deb]");
    let target = line
        .split('=')
        .nth(1)
        .expect("malformed desktop-template line")
        .trim()
        .trim_matches('"');

    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(target);
    assert!(
        path.exists(),
        "desktop-template points at {target}, which does not exist"
    );

    // cargo-packager renders the file through Handlebars. Ours has no
    // placeholders, and if someone adds one it would be rendered against a
    // struct that only knows name/comment/exec/icon/categories/mime_type.
    let contents = std::fs::read_to_string(&path).expect("reading the desktop entry");
    assert!(
        !contents.contains("{{"),
        "the desktop entry gained a Handlebars placeholder; \
         cargo-packager will substitute its own values for it"
    );
}

/// `Icon=` is a *name*, looked up against `hicolor`. It has to match the
/// basename of the installed icon files, which cargo-packager derives from the
/// binary name and the `.rpm` asset list spells out by hand.
#[test]
fn desktop_icon_name_matches_the_installed_icon_files() {
    let entry = read("assets/sicompass.desktop");
    let icon = entry
        .lines()
        .find_map(|l| l.strip_prefix("Icon="))
        .expect("the desktop entry has no Icon= key");
    assert_eq!(
        icon, "sicompass",
        "Icon= must equal the binary name, which is what cargo-packager names \
         the files under /usr/share/icons/hicolor/*/apps/"
    );

    let manifest = read("src/sicompass/Cargo.toml");
    let mut hicolor_dests = 0;
    for line in manifest.lines() {
        let Some((_, dest)) = line.split_once("dest = \"/usr/share/icons/hicolor/") else {
            continue;
        };
        let dest = dest.split('"').next().unwrap_or_default();
        assert!(
            dest.contains(&format!("/apps/{icon}.")),
            "the .rpm installs {dest}, which hicolor will not resolve as {icon:?}"
        );
        hicolor_dests += 1;
    }
    assert!(
        hicolor_dests >= 10,
        "the .rpm icon asset list shrank to {hicolor_dests} entries"
    );
}

/// Without these the `.deb` installs its icons and nothing tells the desktop
/// to re-read them, which is exactly the bug they were added to fix.
#[test]
fn deb_maintainer_scripts_are_present_and_wired_up() {
    for script in ["postinst", "postrm"] {
        let contents = read(&format!("src/sicompass/deb/{script}"));
        assert!(
            contents.starts_with("#!/bin/sh"),
            "src/sicompass/deb/{script} needs a POSIX sh shebang: dpkg runs it directly"
        );
        assert!(
            contents.contains("/usr/share/icons/hicolor"),
            "src/sicompass/deb/{script} no longer touches the icon cache"
        );
    }

    // cargo-packager cannot emit maintainer scripts, so the only thing that
    // puts them in the package is this call in the release workflow.
    let workflow = read(".github/workflows/native-packages.yml");
    assert!(
        workflow.contains("scripts/deb-add-maintainer-scripts.sh"),
        "the release workflow no longer injects the .deb maintainer scripts, \
         so the shipped .deb would have none"
    );
}

/// 0.1.10's macOS build linked Homebrew's `libfreetype.6.dylib` by absolute
/// path, so it aborted in dyld, before `main`, on every Mac without that keg.
/// Nothing in the pipeline noticed, because on the runner the path existed.
///
/// Two things stop that recurring, and both are lines in files no compiler
/// reads: the static link forced in `setup-freetype.sh`, and the load-path
/// audit wired into the two workflows that produce a macOS binary. Deleting
/// either brings the bug back silently, which is what this test is for.
#[test]
fn macos_builds_are_checked_for_machine_specific_load_paths() {
    for workflow in [
        ".github/workflows/native-packages.yml",
        ".github/workflows/ci.yml",
    ] {
        assert!(
            read(workflow).contains("scripts/check-macos-standalone.sh"),
            "{workflow} no longer audits the macOS binary's load paths, so a build \
             against a Homebrew dylib would ship and only fail on a user's machine"
        );
    }

    let setup = read("scripts/setup-freetype.sh");
    assert!(
        setup.contains("FREETYPE2_STATIC=1"),
        "setup-freetype.sh no longer forces a static FreeType link on macOS, so the \
         binary would record an absolute Homebrew path for libfreetype.6.dylib"
    );
    assert!(
        setup.contains("libfreetype.a"),
        "setup-freetype.sh no longer checks that the Homebrew keg has a static \
         archive, without which FREETYPE2_STATIC=1 silently changes nothing"
    );
}

/// The `.rpm` cannot borrow the `.deb`'s maintainer scripts, so it carries its
/// own copy of the same refresh in `[package.metadata.generate-rpm]`.
#[test]
fn rpm_scriptlets_refresh_the_icon_cache() {
    let manifest = read("src/sicompass/Cargo.toml");
    for key in ["post_install_script", "post_uninstall_script"] {
        let start = manifest
            .find(&format!("\n{key} = \"\"\""))
            .unwrap_or_else(|| panic!("{key} is missing from [package.metadata.generate-rpm]"));
        let body = &manifest[start..];
        let open = body.find("\"\"\"").expect("opening delimiter") + 3;
        let body = &body[open..];
        let body = &body[..body.find("\"\"\"").expect("unterminated scriptlet")];

        assert!(
            body.contains("/usr/share/icons/hicolor"),
            "{key} no longer refreshes the icon cache"
        );
        // A scriptlet that exits non-zero fails the whole rpm transaction.
        for line in body.lines().filter(|l| !l.trim().is_empty()) {
            assert!(
                line.trim_end().ends_with("|| :"),
                "{key} has an unguarded line, which would fail the install: {line:?}"
            );
        }
    }
}
