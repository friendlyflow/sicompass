//! Build script for the `sicompass` binary.
//!
//! Its only job is embedding the application icon and version resource into
//! the `.exe` on Windows. It is a no-op everywhere else.
//!
//! Shaders are deliberately *not* built here. See `scripts/gen-shaders.sh` for
//! why they are compiled by hand and committed.

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
        println!("cargo::warning=clang runtime dir {dir:?} does not exist; not linking compiler-rt");
        return;
    }

    println!("cargo::rustc-link-search=native={dir}");
    println!("cargo::rustc-link-lib=static=clang_rt.osx");
}

#[cfg(target_os = "windows")]
fn embed_windows_resources() {
    let manifest_dir = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    // src/sicompass -> workspace root.
    let icon = manifest_dir.join("../../assets/icons/sicompass.ico");
    println!("cargo::rerun-if-changed={}", icon.display());

    let mut res = winresource::WindowsResource::new();
    res.set_icon(icon.to_str().expect("icon path is not valid UTF-8"));
    res.set("ProductName", "sicompass");
    res.set("FileDescription", "Silicon's Compass");
    res.set("CompanyName", "friendlyflow");
    res.set("LegalCopyright", "GPL-3.0-only");

    // A missing rc.exe / windres is not worth failing the build over: it only
    // costs the icon, and it happens on perfectly reasonable setups such as a
    // cross `cargo check --target x86_64-pc-windows-msvc` from Linux.
    if let Err(e) = res.compile() {
        println!("cargo::warning=Windows resources not embedded: {e}");
    }
}
