//! Compositor-level keybindings.
//!
//! When the user holds Alt and presses a key, the compositor intercepts it
//! before forwarding it to the focused client.  This mirrors
//! `handle_keybinding()` in `src/desicompass/main.c`.
//!
//! Keeping this logic in its own module makes it trivially unit-testable.

/// What the compositor should do in response to a keybinding.
#[derive(Debug, Clone, PartialEq)]
pub enum BindingAction {
    /// Terminate the compositor (`Alt+Esc`).
    Quit,
    /// Focus the next window in the stack (`Alt+F1`).
    CycleWindows,
    /// No compositor binding matched; pass the key to the focused client.
    PassThrough,
}

/// Evaluate a keysym that was pressed while Alt was held.
///
/// Returns the [`BindingAction`] for the compositor to execute.  Mirrors
/// `handle_keybinding()` in `src/desicompass/main.c`.
///
/// # Arguments
/// * `keysym` — the XKB keysym value (use the constants from `xkbcommon-sys`
///   or the `xkeysym` crate, e.g. `xkbcommon::xkb::KEY_Escape`).
pub fn evaluate(keysym: u32) -> BindingAction {
    match keysym {
        // XKB_KEY_Escape = 0xff1b
        0xff1b => BindingAction::Quit,
        // XKB_KEY_F1 = 0xffbe
        0xffbe => BindingAction::CycleWindows,
        _ => BindingAction::PassThrough,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alt_escape_quits() {
        assert_eq!(evaluate(0xff1b), BindingAction::Quit);
    }

    #[test]
    fn alt_f1_cycles() {
        assert_eq!(evaluate(0xffbe), BindingAction::CycleWindows);
    }

    #[test]
    fn alt_a_passthrough() {
        assert_eq!(evaluate(b'a' as u32), BindingAction::PassThrough);
    }

    #[test]
    fn alt_f2_passthrough() {
        // Only F1 is bound; F2 passes through
        assert_eq!(evaluate(0xffbf), BindingAction::PassThrough);
    }

    #[test]
    fn zero_keysym_passthrough() {
        assert_eq!(evaluate(0), BindingAction::PassThrough);
    }
}
