//! Build script for the `sicompass` binary.
//!
//! It embeds the application icon and version resource into the `.exe` on
//! Windows, and in a debug build it builds the plugins of the checkouts beside
//! this one ([`build_sibling_plugins`]).
//!
//! Shaders are deliberately *not* built here. They live with the renderer in
//! the sicompass-ui repo, whose `scripts/gen-shaders.sh` explains why they are
//! compiled by hand and committed.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");

    // Target, not host: build scripts are compiled for the host, so `cfg!` here
    // would answer the wrong question the moment anything cross-compiles.
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "macos" {
        link_clang_runtime();
    }

    #[cfg(target_os = "windows")]
    embed_windows_resources();

    build_sibling_plugins();
}

/// What a plugin checkout's folder is called: `<x>-plugin-sicompass`, beside
/// this workspace (the same rule as `dev_plugins::CHECKOUT_SUFFIX`).
const CHECKOUT_SUFFIX: &str = "-plugin-sicompass";

/// `cargo build` in every sibling plugin checkout, so `cargo build` here is all
/// a plugin edit needs: a debug `target/debug/sicompass` runs those builds
/// (`dev_plugins.rs`), and they are never older than their sources.
///
/// Debug builds only, and only where the checkouts exist, so CI, Nix and
/// release builds never do this. It reruns when a checkout's sources change,
/// not on every build. A checkout that does not build fails this build, rather
/// than leaving the app running the plugin's previous build unnoticed. Set
/// `SICOMPASS_PLUGIN_PATH` (even to nothing) to skip it, the same variable that
/// turns off running them.
fn build_sibling_plugins() {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    println!("cargo::rerun-if-env-changed=SICOMPASS_PLUGIN_PATH");
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    if env("PROFILE") != "debug"
        || std::env::var_os("SICOMPASS_PLUGIN_PATH").is_some()
        || env("TARGET") != env("HOST")
    {
        return;
    }
    // This crate is `<workspace>/src`, and the checkouts sit beside the
    // workspace.
    let manifest_dir = PathBuf::from(env("CARGO_MANIFEST_DIR"));
    let Some(siblings) = manifest_dir.parent().and_then(Path::parent) else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(siblings) else {
        return;
    };
    let mut checkouts: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(CHECKOUT_SUFFIX))
                && p.join("Cargo.toml").is_file()
                && p.join("plugin.json").is_file()
        })
        .collect();
    checkouts.sort();

    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    for checkout in &checkouts {
        watch_sources(checkout);
        let mut build = Command::new(&cargo);
        // In the checkout, so its own `.cargo/config.toml` applies.
        build.arg("build").current_dir(checkout);
        for (key, _) in std::env::vars_os() {
            if key.to_str().is_some_and(is_build_script_var) {
                build.env_remove(&key);
            }
        }
        let name = checkout.file_name().unwrap_or_default().to_string_lossy();
        match build.output() {
            Ok(out) if out.status.success() => {}
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                let why: Vec<&str> = stderr
                    .lines()
                    .filter(|l| l.starts_with("error"))
                    .take(5)
                    .collect();
                println!(
                    "cargo::error=../{name} does not build ({}). Run `cargo build` there for \
                     the details, or set SICOMPASS_PLUGIN_PATH= to build without the sibling \
                     plugins.",
                    why.join(" / ")
                );
            }
            Err(e) => println!("cargo::error=could not run cargo for ../{name}: {e}"),
        }
    }
}

/// Rerun when anything a plugin's build reads changes: everything in its
/// checkout but its `target`, its git data and its editor settings.
fn watch_sources(checkout: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(checkout) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name == "target" || (name.starts_with('.') && name != ".cargo") {
            continue;
        }
        println!("cargo::rerun-if-changed={}", entry.path().display());
    }
}

/// What cargo sets for this build script about *this* package. Passed on, it
/// would make the plugin's build differ from a plain `cargo build` in its
/// checkout (clippy's wrapper, these rustflags, this crate's features), which
/// would rebuild it from scratch each time the two alternate.
fn is_build_script_var(key: &str) -> bool {
    const EXACT: [&str; 16] = [
        "OUT_DIR",
        "TARGET",
        "HOST",
        "PROFILE",
        "OPT_LEVEL",
        "DEBUG",
        "NUM_JOBS",
        "RUSTC_LINKER",
        "RUSTC_WORKSPACE_WRAPPER",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_TARGET_DIR",
        "CARGO_PRIMARY_PACKAGE",
        "CARGO_CRATE_NAME",
        "CARGO_MANIFEST_DIR",
        "CARGO_MANIFEST_PATH",
        "CARGO_MANIFEST_LINKS",
    ];
    const PREFIXES: [&str; 3] = ["CARGO_FEATURE_", "CARGO_CFG_", "CARGO_PKG_"];
    EXACT.contains(&key) || PREFIXES.iter().any(|p| key.starts_with(p))
}

/// Put clang's compiler-rt on the link line for macOS builds.
///
/// SDL3's Objective-C (`SDL_cocoawindow.m`, `SDL_camera_coremedia.m` and
/// friends, all compiled from source by the `bundled-sdl3` feature) uses
/// `@available()`. clang lowers that to a call to `___isPlatformVersionAtLeast`,
/// which lives in `libclang_rt.osx.a`. rustc does not reliably add clang's
/// runtime to its link line, so the symbol comes up undefined:
///
///     Undefined symbols for architecture arm64:
///       "___isPlatformVersionAtLeast", referenced from:
///         -[SDL3Cocoa_WindowListener mouseMoved:] in libsdl3_sys...
///
/// Naming it explicitly is harmless when it would have been linked anyway.
/// Failure to locate it is a warning rather than an error, because a build
/// that does not need it should not be blocked by a missing toolchain query.
fn link_clang_runtime() {
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".to_string());
    let Ok(out) = std::process::Command::new(&cc)
        .arg("-print-runtime-dir")
        .output()
    else {
        println!("cargo::warning=could not run `{cc} -print-runtime-dir`; not linking compiler-rt");
        return;
    };
    if !out.status.success() {
        println!("cargo::warning=`{cc} -print-runtime-dir` failed; not linking compiler-rt");
        return;
    }

    let dir = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if dir.is_empty() || !std::path::Path::new(&dir).is_dir() {
        println!(
            "cargo::warning=clang runtime dir {dir:?} does not exist; not linking compiler-rt"
        );
        return;
    }

    println!("cargo::rustc-link-search=native={dir}");
    println!("cargo::rustc-link-lib=static=clang_rt.osx");
}

#[cfg(target_os = "windows")]
fn embed_windows_resources() {
    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    // src -> workspace root.
    let icon = manifest_dir.join("../assets/icons/sicompass.ico");
    println!("cargo::rerun-if-changed={}", icon.display());

    let mut res = winresource::WindowsResource::new();
    res.set_icon(icon.to_str().expect("icon path is not valid UTF-8"));
    res.set("ProductName", "sicompass");
    res.set("FileDescription", "Sicompass");
    res.set("CompanyName", "friendlyflow");
    res.set("LegalCopyright", "GPL-3.0-only");

    // A missing rc.exe / windres is not worth failing the build over: it only
    // costs the icon, and it happens on perfectly reasonable setups such as a
    // cross `cargo check --target x86_64-pc-windows-msvc` from Linux.
    if let Err(e) = res.compile() {
        println!("cargo::warning=Windows resources not embedded: {e}");
    }
}
