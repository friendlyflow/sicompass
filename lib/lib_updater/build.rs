//! Embed an `asInvoker` application manifest in this crate's test binary on
//! Windows MSVC, so the test runner can launch without UAC auto-elevation.
//!
//! Background: cargo names a `[lib]` crate's unit-test binary after the crate
//! (`sicompass_updater-<hash>.exe`). Windows runs an "installer detection"
//! heuristic on `.exe` files whose name contains `update`, `setup`, `install`,
//! or `patch` and silently requests elevation — which fails in any non-
//! elevated shell, CI runner, or test harness. The symptom is cargo reporting
//! `error: test failed` with no test output, because the binary never reached
//! `main()`.
//!
//! The manifest sits at `lib/lib_updater/manifest.xml` and declares
//! `requestedExecutionLevel level="asInvoker"`, which suppresses the heuristic.
//! We attach it only to the test binary (`rustc-link-arg-tests`) — the library
//! rlib itself doesn't link a binary, and we have no `[[bin]]` here.
//!
//! Off Windows-MSVC this script is a no-op.

use std::path::PathBuf;

fn main() {
    println!("cargo::rerun-if-changed=manifest.xml");
    println!("cargo::rerun-if-changed=build.rs");

    let target = std::env::var("TARGET").unwrap_or_default();
    if !(target.contains("windows") && target.contains("msvc")) {
        return;
    }

    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("manifest.xml");

    // /MANIFEST:EMBED — embed the manifest in the .exe rather than emitting a
    // sidecar `.exe.manifest`. /MANIFESTINPUT names the file. Both are MSVC
    // link.exe switches.
    //
    // `rustc-link-arg` (no `-tests` suffix) scopes to this package's linkable
    // artifacts. Since lib_updater has no `[[bin]]` or `[[test]]` targets, the
    // only thing this affects is the unit-test binary cargo builds from the
    // library when running `cargo test -p sicompass-updater`. The rlib itself
    // is just an archive — link args are inert there. Per Cargo's rules, this
    // does *not* propagate to dependents, so the main `sicompass.exe` build is
    // untouched (it embeds its own manifest via winres for the .exe icon).
    println!("cargo::rustc-link-arg=/MANIFEST:EMBED");
    println!(
        "cargo::rustc-link-arg=/MANIFESTINPUT:{}",
        manifest.display()
    );
}
