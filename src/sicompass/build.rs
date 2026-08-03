//! Build script for the `sicompass` binary.
//!
//! Its only job is embedding the application icon and version resource into
//! the `.exe` on Windows. It is a no-op everywhere else.
//!
//! Shaders are deliberately *not* built here. See `scripts/gen-shaders.sh` for
//! why they are compiled by hand and committed.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");

    #[cfg(target_os = "windows")]
    embed_windows_resources();
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
