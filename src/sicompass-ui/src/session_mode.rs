//! Session mode: sicompass running *as* the session rather than as a window
//! on someone else's desktop.
//!
//! sicompass is an ordinary application first. It runs under COSMIC, GNOME,
//! KDE, Windows and macOS, and nothing in this module changes that. It can
//! also be the only client of `desicompass`, the keyboard-driven Wayland
//! compositor in `src/desicompass`, in which case a few things that make
//! sense on a desktop stop making sense: there is no pointer to click a
//! titlebar with, no other window to be raised above, and nothing to
//! minimise to.
//!
//! The rules for anything that consults this module:
//!
//! 1. **Off is the default and is today's behaviour, unchanged.** No flag, no
//!    variable, no compositor: nothing here alters a single pixel.
//! 2. **It is read once and asked in as few places as possible.** If session
//!    mode ever needs more than a handful of call sites, it wants its own
//!    module rather than conditionals sprinkled through the renderer.
//! 3. **It is not a dependency on desicompass.** `--session` is a plain flag
//!    that works under any compositor, which is also how it gets tested.
//!
//! The compositor passes `SICOMPASS_SESSION=1` to the client it launches; a
//! person can pass `--session` by hand.

use std::sync::OnceLock;

static SESSION_MODE: OnceLock<bool> = OnceLock::new();

/// Whether this process is running as the session.
///
/// Resolved from the command line and the environment the first time it is
/// asked, then cached: it cannot change while the process is running, and a
/// value that flipped mid-frame would be worse than useless.
pub fn is_session_mode() -> bool {
    *SESSION_MODE.get_or_init(|| detect(std::env::args(), std::env::var("SICOMPASS_SESSION").ok()))
}

/// The decision itself, separated from the process it is normally read from
/// so it can be tested.
fn detect(args: impl IntoIterator<Item = String>, env: Option<String>) -> bool {
    if args.into_iter().any(|a| a == "--session") {
        return true;
    }
    matches!(env.as_deref(), Some("1") | Some("true"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn off_by_default() {
        // The guarantee the whole module rests on: a plain launch is a normal
        // desktop application, with nothing switched on behind the user's
        // back.
        assert!(!detect(args(&["sicompass"]), None));
    }

    #[test]
    fn enabled_by_the_flag() {
        assert!(detect(args(&["sicompass", "--session"]), None));
    }

    #[test]
    fn enabled_by_the_environment() {
        assert!(detect(args(&["sicompass"]), Some("1".into())));
        assert!(detect(args(&["sicompass"]), Some("true".into())));
    }

    #[test]
    fn other_values_do_not_enable_it() {
        for value in ["0", "false", "", "yes", "SESSION"] {
            assert!(
                !detect(args(&["sicompass"]), Some(value.into())),
                "{value:?} should not enable session mode"
            );
        }
    }

    #[test]
    fn unrelated_flags_do_not_enable_it() {
        assert!(!detect(args(&["sicompass", "--check", "--sessions"]), None));
        assert!(!detect(args(&["sicompass", "session"]), None));
    }

    #[test]
    fn the_flag_wins_over_a_disabled_environment() {
        assert!(detect(args(&["sicompass", "--session"]), Some("0".into())));
    }
}
