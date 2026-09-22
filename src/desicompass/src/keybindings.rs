//! Compositor-level keybindings.
//!
//! When a binding matches, the compositor consumes the key instead of
//! forwarding it to the focused client. This mirrors `handle_keybinding()` in
//! `src/desicompass-c/main.c`, with a different modifier and a much larger
//! map.
//!
//! ## Why Super and not Alt
//!
//! The C original, and the first Rust cut, intercepted Alt. That cannot stay:
//!
//! * sicompass consumes Alt itself. `dispatch_key` in the app's
//!   `shortcuts.rs` folds `LALT | RALT` into its own `alt` flag and hangs a
//!   large part of its shortcut table off it, so a compositor that swallows
//!   Alt eats the app's own keyboard.
//! * On most layouts outside the US, the right Alt is AltGr. On the Belgian
//!   layout this machine uses, AltGr is the only way to type `@ # [ ] { }`.
//!   Grabbing Alt would make those characters untypeable in every client.
//!
//! Super (Logo, Mod4) is consumed by neither: a search for
//! `GUIMOD|LGUI|RGUI|Mod::GUI` across the app and every provider crate finds
//! nothing.
//!
//! ## Why raw keysyms
//!
//! Bindings are matched against the *Latin* keysym for the physical key, via
//! smithay's `raw_latin_sym_or_raw_current_sym`, not the modified symbol.
//! Matching the modified symbol would break the bindings twice over: Shift
//! turns `j` into `J`, and a non-Latin layout turns it into something else
//! entirely. The letters below are therefore always lowercase.

use smithay::input::keyboard::keysyms;

/// The modifier state a binding is evaluated against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Mods {
    /// Super / Logo / Mod4 — the compositor's modifier.
    pub logo: bool,
    pub shift: bool,
}

/// What the compositor should do in response to a keybinding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingAction {
    /// Focus the next / previous window in layout order, wrapping.
    FocusNext,
    FocusPrev,
    /// Focus the window physically to the left / right. Does not wrap.
    FocusLeft,
    FocusRight,
    /// Move the focused window one place along the layout order.
    MoveNext,
    MovePrev,
    /// Swap the focused window with its left / right neighbour.
    MoveLeft,
    MoveRight,
    /// Focus the least recently used window (the Alt-Tab of this compositor).
    CycleRecent,
    /// Switch between Columns and Monocle.
    ToggleLayout,
    /// Ask the focused window to close.
    CloseWindow,
    /// Launch the configured terminal.
    Spawn,
    /// Request to end the session. Confirmed by pressing it twice; see
    /// `State::request_quit`.
    Quit,
    /// No binding matched; the key belongs to the focused client.
    PassThrough,
}

/// Evaluate a key press.
///
/// `keysym` is the raw Latin keysym for the physical key, so it is unaffected
/// by Shift or by the active layout.
pub fn evaluate(mods: Mods, keysym: u32) -> BindingAction {
    // Super is the only compositor modifier. Without it, every key belongs to
    // the client - including plain Escape and F1, which the C original took
    // for itself and which a terminal or an editor very much wants.
    if !mods.logo {
        return BindingAction::PassThrough;
    }

    match (keysym, mods.shift) {
        (keysyms::KEY_j, false) => BindingAction::FocusNext,
        (keysyms::KEY_k, false) => BindingAction::FocusPrev,
        (keysyms::KEY_h, false) => BindingAction::FocusLeft,
        (keysyms::KEY_l, false) => BindingAction::FocusRight,

        (keysyms::KEY_j, true) => BindingAction::MoveNext,
        (keysyms::KEY_k, true) => BindingAction::MovePrev,
        (keysyms::KEY_h, true) => BindingAction::MoveLeft,
        (keysyms::KEY_l, true) => BindingAction::MoveRight,

        (keysyms::KEY_Tab, _) => BindingAction::CycleRecent,
        (keysyms::KEY_m, false) => BindingAction::ToggleLayout,
        (keysyms::KEY_Return, false) => BindingAction::Spawn,

        // Both destructive actions need Shift. Closing a window or ending the
        // session on a single unshifted chord is too easy to hit by accident
        // on a keyboard-only shell, where there is no pointer to undo with.
        (keysyms::KEY_q, true) => BindingAction::CloseWindow,
        (keysyms::KEY_e, true) => BindingAction::Quit,

        _ => BindingAction::PassThrough,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sup() -> Mods {
        Mods {
            logo: true,
            shift: false,
        }
    }

    fn sup_shift() -> Mods {
        Mods {
            logo: true,
            shift: true,
        }
    }

    #[test]
    fn without_super_everything_passes_through() {
        let none = Mods::default();
        for sym in [
            keysyms::KEY_j,
            keysyms::KEY_q,
            keysyms::KEY_Escape,
            keysyms::KEY_F1,
            keysyms::KEY_Return,
        ] {
            assert_eq!(evaluate(none, sym), BindingAction::PassThrough);
        }
    }

    #[test]
    fn shift_alone_does_not_bind() {
        let shift_only = Mods {
            logo: false,
            shift: true,
        };
        assert_eq!(
            evaluate(shift_only, keysyms::KEY_q),
            BindingAction::PassThrough
        );
    }

    #[test]
    fn focus_motions() {
        assert_eq!(evaluate(sup(), keysyms::KEY_j), BindingAction::FocusNext);
        assert_eq!(evaluate(sup(), keysyms::KEY_k), BindingAction::FocusPrev);
        assert_eq!(evaluate(sup(), keysyms::KEY_h), BindingAction::FocusLeft);
        assert_eq!(evaluate(sup(), keysyms::KEY_l), BindingAction::FocusRight);
    }

    #[test]
    fn shift_turns_motion_into_movement() {
        assert_eq!(
            evaluate(sup_shift(), keysyms::KEY_j),
            BindingAction::MoveNext
        );
        assert_eq!(
            evaluate(sup_shift(), keysyms::KEY_k),
            BindingAction::MovePrev
        );
        assert_eq!(
            evaluate(sup_shift(), keysyms::KEY_h),
            BindingAction::MoveLeft
        );
        assert_eq!(
            evaluate(sup_shift(), keysyms::KEY_l),
            BindingAction::MoveRight
        );
    }

    #[test]
    fn recent_cycle_ignores_shift() {
        assert_eq!(
            evaluate(sup(), keysyms::KEY_Tab),
            BindingAction::CycleRecent
        );
        assert_eq!(
            evaluate(sup_shift(), keysyms::KEY_Tab),
            BindingAction::CycleRecent
        );
    }

    #[test]
    fn layout_and_spawn() {
        assert_eq!(
            evaluate(sup(), keysyms::KEY_m),
            BindingAction::ToggleLayout
        );
        assert_eq!(evaluate(sup(), keysyms::KEY_Return), BindingAction::Spawn);
    }

    #[test]
    fn destructive_actions_require_shift() {
        // Unshifted, these must reach the client untouched.
        assert_eq!(evaluate(sup(), keysyms::KEY_q), BindingAction::PassThrough);
        assert_eq!(evaluate(sup(), keysyms::KEY_e), BindingAction::PassThrough);

        assert_eq!(
            evaluate(sup_shift(), keysyms::KEY_q),
            BindingAction::CloseWindow
        );
        assert_eq!(evaluate(sup_shift(), keysyms::KEY_e), BindingAction::Quit);
    }

    #[test]
    fn the_old_alt_bindings_are_gone() {
        // Alt+Esc and Alt+F1 used to quit and cycle. Neither may fire now,
        // whatever the modifier, because the app itself uses Escape and the
        // function keys.
        for mods in [Mods::default(), sup(), sup_shift()] {
            assert_eq!(
                evaluate(mods, keysyms::KEY_Escape),
                BindingAction::PassThrough
            );
            assert_eq!(evaluate(mods, keysyms::KEY_F1), BindingAction::PassThrough);
        }
    }

    #[test]
    fn unbound_letters_pass_through() {
        for sym in [keysyms::KEY_a, keysyms::KEY_z, keysyms::KEY_5] {
            assert_eq!(evaluate(sup(), sym), BindingAction::PassThrough);
        }
    }

    #[test]
    fn bindings_match_the_latin_sym_so_layout_cannot_break_them() {
        // The caller resolves the physical key to its Latin keysym before
        // calling in, so an AZERTY or Cyrillic layout still reaches KEY_j
        // here. Uppercase 'J' is what a *modified* sym would look like, and
        // must not be bound - if it ever matches, the caller is passing the
        // wrong sym.
        assert_eq!(evaluate(sup_shift(), keysyms::KEY_J), BindingAction::PassThrough);
    }
}
