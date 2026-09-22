//! The GPU side, abstracted over the two ways desicompass can run.
//!
//! * **winit** — nested inside an existing Wayland or X11 session. This is
//!   the development backend: it opens an ordinary window on the desktop you
//!   are already logged into.
//! * **tty** — the real one. Talks to DRM/KMS directly, takes the display,
//!   and reads input through libinput. Only built with the `tty` feature.
//!
//! The TTY backend drives a single GPU and a single connector. Multi-GPU
//! (`GpuManager` / `GbmGlesBackend`) and multi-output are not wired: on a
//! hybrid laptop, or with a second monitor, this needs the renderer type to
//! change throughout `tty.rs`, so it is a deliberate gap rather than an
//! oversight. See the scope note at the top of that module.
//!
//! Both are wrapped in one enum rather than hidden behind a trait because
//! only one method is ever needed polymorphically: `DmabufHandler::
//! dmabuf_imported` is handed `&mut State` and has to reach the renderer to
//! validate a client's buffer. Everything else about the two backends —
//! their event sources, how a frame is submitted, what owns the output — is
//! different enough that a shared trait would be a fiction.

use smithay::backend::renderer::gles::GlesRenderer;
use smithay::backend::winit::WinitGraphicsBackend;

/// Which backend to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum BackendChoice {
    /// Pick by looking at the environment.
    #[default]
    Auto,
    /// Nested in an existing session.
    Winit,
    /// Take over the display through DRM/KMS.
    Tty,
}

impl BackendChoice {
    /// Resolve `Auto`.
    ///
    /// A session to nest in announces itself: `WAYLAND_DISPLAY` or `DISPLAY`
    /// is set. On a bare TTY neither is, so there is nothing to nest in and
    /// the only thing left to do is take the display.
    ///
    /// Auto-detection is the default because the command line that starts
    /// the session is fixed — a greetd config or a `.desktop` `Exec=` cannot
    /// carry a conditional — and the same line has to work whether it is run
    /// from inside a desktop while developing or from a TTY at boot. The
    /// explicit values stay so that forcing `tty` from inside a session
    /// produces an honest "cannot become DRM master" rather than a confusing
    /// extra window.
    pub fn resolve(self, env: impl Fn(&str) -> bool) -> ResolvedBackend {
        match self {
            BackendChoice::Winit => ResolvedBackend::Winit,
            BackendChoice::Tty => ResolvedBackend::Tty,
            BackendChoice::Auto => {
                if env("WAYLAND_DISPLAY") || env("DISPLAY") {
                    ResolvedBackend::Winit
                } else {
                    ResolvedBackend::Tty
                }
            }
        }
    }
}

/// The outcome of resolving [`BackendChoice`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedBackend {
    Winit,
    Tty,
}

/// The live GPU backend.
pub enum Gpu {
    Winit(WinitGraphicsBackend<GlesRenderer>),
    #[cfg(feature = "tty")]
    Tty(Box<crate::tty::TtyGpu>),
}

impl Gpu {
    /// The renderer, whichever backend is running.
    ///
    /// On the TTY backend this is the primary GPU's renderer from the
    /// multi-GPU manager, which on a single-GPU machine is simply the only
    /// one.
    pub fn renderer(&mut self) -> &mut GlesRenderer {
        match self {
            Gpu::Winit(backend) => backend.renderer(),
            #[cfg(feature = "tty")]
            Gpu::Tty(tty) => tty.renderer(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with<'a>(vars: &'a [&'a str]) -> impl Fn(&str) -> bool + 'a {
        move |name: &str| vars.contains(&name)
    }

    #[test]
    fn explicit_choices_ignore_the_environment() {
        assert_eq!(
            BackendChoice::Winit.resolve(with(&[])),
            ResolvedBackend::Winit
        );
        assert_eq!(
            BackendChoice::Tty.resolve(with(&["WAYLAND_DISPLAY"])),
            ResolvedBackend::Tty
        );
    }

    #[test]
    fn auto_nests_inside_a_wayland_session() {
        assert_eq!(
            BackendChoice::Auto.resolve(with(&["WAYLAND_DISPLAY"])),
            ResolvedBackend::Winit
        );
    }

    #[test]
    fn auto_nests_inside_an_x11_session() {
        assert_eq!(
            BackendChoice::Auto.resolve(with(&["DISPLAY"])),
            ResolvedBackend::Winit
        );
    }

    #[test]
    fn auto_takes_the_display_on_a_bare_tty() {
        // A fresh TTY login has neither variable set, which is the whole
        // signal: there is no session to nest in.
        assert_eq!(BackendChoice::Auto.resolve(with(&[])), ResolvedBackend::Tty);
    }

    #[test]
    fn auto_is_the_default() {
        assert_eq!(BackendChoice::default(), BackendChoice::Auto);
    }
}
