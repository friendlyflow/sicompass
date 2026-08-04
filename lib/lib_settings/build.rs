//! Bake the resolved `sicompass-sdk` version into this crate as
//! `SICOMPASS_SDK_VERSION`, so the settings panel can show it next to the app
//! version.
//!
//! Background: the settings tree's `sicompass` section shows both the app
//! version and the SDK version. The app version is trivial (`CARGO_PKG_VERSION`
//! of the app crate, injected via `Provider::set_section_version`), but the SDK
//! version is not available at runtime at all: `sicompass-sdk` is an external
//! crates.io dependency that exposes no `VERSION` const, and cargo does not hand
//! a crate the versions of its own dependencies. The only authoritative source
//! is the workspace `Cargo.lock`, which is what this script reads.
//!
//! Reading the *lock* rather than the `sicompass-sdk = "0.3.0"` requirement in
//! `Cargo.toml` matters: the manifest entry is a semver range, and during local
//! SDK development the `[patch.crates-io]` path override resolves to whatever
//! version the sibling checkout carries. The lock always records the one version
//! actually compiled in.
//!
//! Every crate in this workspace is `publish = false`, so the lockfile is always
//! present in-tree — there is no `cargo package` case where it goes missing, and
//! therefore no fallback. If the lookup fails, the build fails loudly: a silent
//! "unknown" shipped in a release binary would be worse than a broken build.

use std::path::{Path, PathBuf};

fn main() {
    println!("cargo::rerun-if-changed=build.rs");

    let manifest_dir = PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is always set by cargo"),
    );
    let lock = find_cargo_lock(&manifest_dir).unwrap_or_else(|| {
        panic!(
            "no Cargo.lock found in any ancestor of {}; \
             lib_settings needs it to resolve the sicompass-sdk version",
            manifest_dir.display()
        )
    });
    println!("cargo::rerun-if-changed={}", lock.display());

    let text = std::fs::read_to_string(&lock)
        .unwrap_or_else(|e| panic!("failed to read {}: {e}", lock.display()));
    let version = sdk_version(&text).unwrap_or_else(|| {
        panic!(
            "no `sicompass-sdk` package entry in {}; \
             run `cargo metadata` or rebuild the lockfile",
            lock.display()
        )
    });

    println!("cargo::rustc-env=SICOMPASS_SDK_VERSION={version}");
}

/// Walk up from `start` until a directory containing `Cargo.lock` is found.
fn find_cargo_lock(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|d| d.join("Cargo.lock"))
        .find(|p| p.is_file())
}

/// Pull the `version` of the `sicompass-sdk` `[[package]]` entry out of a
/// `Cargo.lock`.
///
/// Hand-rolled rather than pulling in a `toml` build-dependency: the lockfile
/// format is a flat, cargo-generated sequence of `[[package]]` tables with one
/// `key = "value"` per line, so a line scan is exact and keeps the build-deps
/// tree empty.
fn sdk_version(lock: &str) -> Option<String> {
    let mut in_sdk = false;
    for line in lock.lines() {
        let line = line.trim();
        if line == "[[package]]" {
            in_sdk = false;
        } else if line == "name = \"sicompass-sdk\"" {
            in_sdk = true;
        } else if in_sdk {
            if let Some(rest) = line.strip_prefix("version = \"") {
                return rest.strip_suffix('"').map(str::to_owned);
            }
        }
    }
    None
}
