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

/// Nothing may still claim to ship the two asset trees that moved into the crates
/// that own them.
///
/// A stale glob in `generate-rpm`, a stale `src` in the cargo-packager resources or
/// a stale `<File Source=…>` in the MSI all fail at *package build* time — that is,
/// during a release, on a runner, after the tag is pushed. This catches it in
/// `cargo test` instead.
#[test]
fn no_packaging_list_still_references_the_moved_asset_trees() {
    for file in [
        "dist-workspace.toml",
        "src/sicompass/Cargo.toml",
        "src/sicompass/wix/main.wxs",
        "flake.nix",
    ] {
        let contents = read(file);
        for (n, line) in contents.lines().enumerate() {
            // Prose is allowed to explain where they went; a directive is not. Both
            // path separators, since main.wxs uses backslashes.
            let names_a_tree = line.contains("assets/tutorial")
                || line.contains("assets/sales-demo")
                || line.contains(r"assets\tutorial")
                || line.contains(r"assets\sales-demo");
            let is_comment = {
                let t = line.trim_start();
                t.starts_with('#') || t.starts_with("<!--") || t.starts_with("//")
            };
            assert!(
                !names_a_tree || is_comment,
                "{file}:{} still ships a moved asset tree: {line:?}",
                n + 1
            );
        }
    }
}

/// The counterpart: removing the asset entries must not have taken the icons or the
/// desktop entry with them, since those really are installed from the checkout.
#[test]
fn the_desktop_entry_and_icons_are_still_shipped() {
    let manifest = read("src/sicompass/Cargo.toml");
    for source in ["assets/sicompass.desktop", "assets/icons/sicompass.svg"] {
        assert!(
            manifest.contains(&format!("source = \"{source}\"")),
            "the .rpm no longer installs {source}"
        );
    }
    assert!(
        read("flake.nix").contains("assets/icons/sicompass.svg"),
        "the Nix build no longer installs the hicolor icon"
    );
}

/// Everything left under the top-level `assets/` must be a *build-time* input.
///
/// This is the invariant the move established: a file the app reads at runtime
/// belongs in its own crate's `assets/` directory, embedded with `include_bytes!`
/// and published with `sicompass_sdk::assets::register_bytes` — not here, where
/// shipping it means editing four hand-maintained lists that nothing verifies.
#[test]
fn the_top_level_asset_tree_holds_only_packaging_inputs() {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir)
            .expect("assets/ should exist")
            .flatten()
        {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else {
                out.push(path);
            }
        }
    }

    let root = workspace_root().join("assets");
    let mut files = Vec::new();
    walk(&root, &mut files);
    assert!(!files.is_empty(), "assets/ is empty — did the icons move?");

    for path in files {
        let rel = path.strip_prefix(&root).unwrap();
        let ok = rel.starts_with("icons") || rel == Path::new("sicompass.desktop");
        assert!(
            ok,
            "assets/{} is neither an icon nor the desktop entry. If the app reads it \
             at runtime, embed it in the crate that owns it instead.",
            rel.display()
        );
    }
}

/// Every `<Component>` in the MSI needs exactly one `<ComponentRef>`, and every
/// `<ComponentRef>` needs a `<Component>`.
///
/// WiX's `light` fails with LGHT0094 (exit code 94) on a reference to a symbol that
/// does not exist, and that only happens when the MSI is actually built — which is
/// during a release, on a Windows runner, after the tag is pushed. Removing the
/// asset components without their refs failed exactly this way in v0.1.12, so the
/// two lists are pinned against each other here instead.
#[test]
fn every_msi_component_is_referenced_exactly_once() {
    let wxs = read("src/sicompass/wix/main.wxs");

    // Ids inside XML comments do not count: the template ships a commented-out
    // `License` component *and* a commented-out ref for it, and neither reaches WiX.
    let mut live = String::new();
    let mut rest = wxs.as_str();
    while let Some(open) = rest.find("<!--") {
        live.push_str(&rest[..open]);
        rest = match rest[open..].find("-->") {
            Some(close) => &rest[open + close + 3..],
            None => "",
        };
    }
    live.push_str(rest);

    let ids = |tag: &str| -> Vec<String> {
        live.split(tag)
            .skip(1)
            .filter_map(|s| {
                let start = s.find("Id='")? + 4;
                let end = s[start..].find('\'')?;
                Some(s[start..start + end].to_owned())
            })
            .collect()
    };

    let mut defined = ids("<Component ");
    let mut referenced = ids("<ComponentRef ");
    assert!(
        !defined.is_empty() && !referenced.is_empty(),
        "parsed nothing out of main.wxs — did the quoting style change?"
    );

    defined.sort();
    referenced.sort();
    assert_eq!(
        defined, referenced,
        "main.wxs components and refs disagree. A ref without a component fails \
         `light` with LGHT0094; a component without a ref silently ships nothing."
    );
}

/// The renderer (`sicompass-ui`, its own repo) compiles the fonts into this
/// binary, so every package has to ship their license texts, and `fonts/` here
/// keeps copies for dist, cargo-packager, the rpm and the Nix build to install.
/// Those copies have to be the texts of the fonts actually linked.
#[test]
fn shipped_font_licenses_match_the_linked_renderer() {
    for (name, text) in sicompass_ui::fonts::LICENSES {
        assert_eq!(
            read(&format!("fonts/{name}")),
            *text,
            "fonts/{name} differs from the one in the sicompass-ui version this \
             build links. Copy it over from that repo's fonts/."
        );
    }
    assert!(
        !sicompass_ui::fonts::LICENSES.is_empty(),
        "sicompass-ui exposes no font licenses to check"
    );
}

/// `scripts/gen-icons.sh` regenerates `assets/icons/*` here, but the window
/// icon is embedded by the renderer, which keeps its own copy. A regenerated
/// icon that never reached that copy would leave every window on the old one.
#[test]
fn the_window_icon_matches_the_generated_one() {
    let generated = std::fs::read(workspace_root().join("assets/icons/256x256.png"))
        .expect("reading assets/icons/256x256.png");
    assert!(
        generated == sicompass_ui::icon::ICON_PNG,
        "assets/icons/256x256.png differs from sicompass-ui's assets/icon-256x256.png. \
         Copy it into the sicompass-ui repo, tag a release, and move the pin."
    );
}

/// `APP_ID` is what the compositor turns into `<APP_ID>.desktop`. If it
/// stops matching the shipped entry, the window silently loses its icon
/// again on every packaged install.
///
/// Moved here from `sicompass-ui`'s `icon.rs` when the renderer got its own
/// repo: the entry is part of this repo's packages, not of the renderer.
#[test]
fn app_id_matches_the_shipped_desktop_entry() {
    use sicompass_ui::icon::{APP_ID, APP_NAME};
    let entry = read("assets/sicompass.desktop");

    assert_eq!(
        entry.lines().find_map(|l| l.strip_prefix("Icon=")),
        Some(APP_ID),
        "Icon= must equal APP_ID"
    );
    assert_eq!(
        entry
            .lines()
            .find_map(|l| l.strip_prefix("StartupWMClass=")),
        Some(APP_ID),
        "StartupWMClass= is what X11 desktops match WM_CLASS against"
    );
    assert_eq!(
        entry.lines().find_map(|l| l.strip_prefix("Name=")),
        Some(APP_NAME)
    );
}

/// `sicompass --check` is the first thing the post-release smoke test runs, and
/// its first line is how a user reports what they have installed. The check
/// itself lives in the renderer (`sicompass-ui`, its own repo and version), so
/// the version has to be handed in by the app. Right after the split it wasn't,
/// and sicompass 0.1.21 reported itself as the renderer's 0.2.0.
///
/// Only the first line is read: the Vulkan probe that follows may or may not
/// find a device on the machine running the tests, which is not this test's
/// business.
#[test]
fn check_reports_the_apps_own_version() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sicompass"))
        .arg("--check")
        .output()
        .expect("running sicompass --check");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first = stdout.lines().next().unwrap_or_default();
    assert!(
        first.starts_with(&format!("sicompass {} (", env!("CARGO_PKG_VERSION"))),
        "`sicompass --check` opened with {first:?}"
    );
}
