//! The application icon, embedded in the binary, plus the app identity the
//! desktop uses to find it.
//!
//! There are two entirely separate ways a Linux desktop puts an icon next to
//! this app, and both were broken.
//!
//! **The launcher entry** comes from `/usr/share/applications/sicompass.desktop`
//! and its `Icon=sicompass` key, resolved against the `hicolor` theme. That is
//! the packages' job, and it only works where a package was installed.
//!
//! **The window icon** (taskbar, alt-tab, dock, window list) is the desktop's
//! problem, and it has two possible sources. Either the window carries a real
//! icon, from `_NET_WM_ICON` on X11, or the desktop matches the window's
//! `app_id` (Wayland) / `WM_CLASS` (X11) against an installed `.desktop` file
//! and borrows the icon from there.
//!
//! We were supplying neither:
//!
//!   * No `SDL_SetWindowIcon` call, so no `_NET_WM_ICON`. Nothing to fall back
//!     on when no `.desktop` file is installed, which is the case for `nix
//!     run`, for the release archives, for the `curl | sh` installer and for an
//!     AppImage the user has not integrated.
//!   * No app metadata, so SDL derived the `app_id` from the executable name
//!     via `/proc/self/exe`. That is `sicompass` for most builds but
//!     `.sicompass-wrapped` under Nix, because `wrapProgram` renames the real
//!     binary and puts a shell script in its place. `.sicompass-wrapped`
//!     matches no `.desktop` file on earth, so the Nix package showed a
//!     generic icon even though the derivation installs the entry and the full
//!     `hicolor` tree correctly.
//!
//! [`set_app_metadata`] fixes the second by stating the identity outright, and
//! [`window_icon`] fixes the first, which also makes the icon independent of
//! whether anything was installed at all.
//!
//! Both are needed, and neither is redundant. On X11 the window icon is
//! `_NET_WM_ICON` and always works. On Wayland `SDL_SetWindowIcon` goes
//! through `xdg_toplevel_icon_manager_v1`, which not every compositor
//! advertises, and where it is missing the `app_id` route is the only one
//! left. Conversely the `app_id` route needs an installed `.desktop` file,
//! which is exactly what `nix run` and the archives do not have.

use sdl3::pixels::PixelFormat;
use sdl3::surface::Surface;

/// The identifier the desktop matches against `<name>.desktop`.
///
/// Must stay equal to the basename of `assets/sicompass.desktop` and to the
/// `Icon=` name inside it. Not the reverse-DNS
/// `com.friendlyflow.sicompass` used for the macOS bundle and the MSI:
/// Wayland compositors look for `<app_id>.desktop`, and ours is
/// `sicompass.desktop`.
pub const APP_ID: &str = "sicompass";

/// Human-readable name, shown by some compositors next to the icon.
const APP_NAME: &str = "Sicompass";

/// Source PNG for the window icon.
///
/// 256x256 is the size worth embedding: large enough for a HiDPI dock, small
/// enough (about 8 KB) not to matter, and SDL hands the desktop whatever it
/// gets and lets the compositor scale. Generated from
/// `assets/icons/sicompass.svg` by `scripts/gen-icons.sh`.
///
/// Embedded rather than read from `assets/`, because the whole point is to
/// have an icon on installs that ship nothing but the executable.
const ICON_PNG: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/icons/256x256.png"
));

/// Tell SDL who we are, so the Wayland `app_id` and the X11 `WM_CLASS` are
/// [`APP_ID`] rather than whatever the executable happens to be called.
///
/// Call this before `SDL_Init`: the video backends read the app id when they
/// start up, and setting it afterwards is too late for the window that has
/// already been created.
pub fn set_app_metadata() {
    // `SDL_SetAppMetadata` takes the three common fields at once. The version
    // is `None` rather than a lie; SDL treats a null as "unset".
    let name = std::ffi::CString::new(APP_NAME).expect("APP_NAME has no interior nul");
    let identifier = std::ffi::CString::new(APP_ID).expect("APP_ID has no interior nul");

    // SAFETY: both pointers are valid `CStr`s for the duration of the call,
    // and SDL copies the strings it is given.
    unsafe {
        sdl3_sys::init::SDL_SetAppMetadata(name.as_ptr(), std::ptr::null(), identifier.as_ptr());
    }
}

/// Decode the embedded icon into an SDL surface, ready for
/// `Window::set_icon`.
///
/// Returns the RGBA buffer alongside the surface: [`Surface::from_data`]
/// borrows rather than copies, so the caller has to keep the pixels alive
/// until `set_icon` has run.
pub fn window_icon() -> Result<(Vec<u8>, u32, u32), String> {
    let decoded = image::load_from_memory(ICON_PNG).map_err(|e| e.to_string())?;
    let rgba = decoded.to_rgba8();
    let (width, height) = rgba.dimensions();
    Ok((rgba.into_raw(), width, height))
}

/// Build the surface for `pixels`, which must outlive the returned value.
///
/// `RGBA32` is SDL's byte-order-independent alias for "R, G, B, A in memory",
/// which is exactly what the `image` crate produced. Naming `RGBA8888`
/// instead would swap the channels on little-endian and tint the icon.
pub fn icon_surface(pixels: &mut [u8], width: u32, height: u32) -> Result<Surface<'_>, String> {
    Surface::from_data(pixels, width, height, width * 4, PixelFormat::RGBA32)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The embedded PNG has to decode, and has to be the size the doc comment
    /// claims, or `icon_surface`'s pitch arithmetic is wrong.
    #[test]
    fn embedded_icon_decodes_to_square_rgba() {
        let (pixels, width, height) = window_icon().expect("the embedded icon must decode");
        assert_eq!((width, height), (256, 256));
        assert_eq!(pixels.len(), (width * height * 4) as usize);
    }

    /// A fully transparent icon would look exactly like the missing icon this
    /// module exists to fix, and would pass every other check.
    #[test]
    fn embedded_icon_is_not_blank() {
        let (pixels, _, _) = window_icon().expect("the embedded icon must decode");
        let opaque = pixels.chunks_exact(4).filter(|px| px[3] > 0).count();
        assert!(
            opaque > 1000,
            "only {opaque} non-transparent pixels; the icon is effectively blank"
        );
    }

    /// `APP_ID` is what the compositor turns into `<APP_ID>.desktop`. If it
    /// stops matching the shipped entry, the window silently loses its icon
    /// again on every packaged install.
    #[test]
    fn app_id_matches_the_shipped_desktop_entry() {
        let entry = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/sicompass.desktop"
        ))
        .expect("reading assets/sicompass.desktop");

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
}
