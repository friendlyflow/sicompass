//! Password entry state machine.
//!
//! Handles buffering typed characters, backspace, clear, and submission.
//! Mirrors the `entry` struct and keyboard handling in `src/loginsicompass/`.
//!
//! This module is pure logic — no Wayland or rendering dependencies.

/// Maximum number of characters accepted in a single password field.
pub const MAX_PASSWORD_LENGTH: usize = 64;

/// Whether the input should be shown or hidden (set by greetd's
/// `auth_message_type`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputMode {
    /// Input is shown as the masking character (e.g. `•`).
    Secret,
    /// Input is shown as plain text (e.g. for a visible OTP prompt).
    Visible,
}

/// Password / response entry buffer.
///
/// Accumulates Unicode characters, supports backspace, clear, and UTF-8
/// extraction for submission to greetd.
#[derive(Debug)]
pub struct PasswordEntry {
    chars: Vec<char>,
    pub mode: InputMode,
    /// The masking character drawn in secret mode (default `•`).
    pub mask_char: char,
}

impl Default for PasswordEntry {
    fn default() -> Self {
        PasswordEntry {
            chars: Vec::new(),
            mode: InputMode::Secret,
            mask_char: '•',
        }
    }
}

impl PasswordEntry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Push a printable character into the buffer.  Ignores the push if
    /// the buffer is already at `MAX_PASSWORD_LENGTH`.
    pub fn push(&mut self, ch: char) {
        if self.chars.len() < MAX_PASSWORD_LENGTH {
            self.chars.push(ch);
        }
    }

    /// Remove the last character (backspace).  No-op on an empty buffer.
    pub fn backspace(&mut self) {
        self.chars.pop();
    }

    /// Clear the entire buffer (Escape / Ctrl+C).
    pub fn clear(&mut self) {
        self.chars.clear();
    }

    /// Current number of characters in the buffer.
    pub fn len(&self) -> usize {
        self.chars.len()
    }

    pub fn is_empty(&self) -> bool {
        self.chars.is_empty()
    }

    /// Extract the buffer as a UTF-8 `String` for submission to greetd.
    pub fn as_string(&self) -> String {
        self.chars.iter().collect()
    }

    /// Iterator over mask characters for rendering (one per buffered char).
    ///
    /// In `Secret` mode yields `mask_char` for each position.
    /// In `Visible` mode yields the actual character.
    pub fn display_chars(&self) -> impl Iterator<Item = char> + '_ {
        self.chars.iter().map(move |&ch| match self.mode {
            InputMode::Secret => self.mask_char,
            InputMode::Visible => ch,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_on_creation() {
        let e = PasswordEntry::new();
        assert!(e.is_empty());
        assert_eq!(e.len(), 0);
        assert_eq!(e.as_string(), "");
    }

    #[test]
    fn push_single_char() {
        let mut e = PasswordEntry::new();
        e.push('a');
        assert_eq!(e.len(), 1);
        assert_eq!(e.as_string(), "a");
    }

    #[test]
    fn push_multiple_chars() {
        let mut e = PasswordEntry::new();
        for ch in "hello".chars() {
            e.push(ch);
        }
        assert_eq!(e.as_string(), "hello");
    }

    #[test]
    fn push_unicode_chars() {
        let mut e = PasswordEntry::new();
        e.push('é');
        e.push('🔑');
        assert_eq!(e.len(), 2);
        assert_eq!(e.as_string(), "é🔑");
    }

    #[test]
    fn backspace_removes_last() {
        let mut e = PasswordEntry::new();
        e.push('a');
        e.push('b');
        e.backspace();
        assert_eq!(e.as_string(), "a");
    }

    #[test]
    fn backspace_on_empty_is_noop() {
        let mut e = PasswordEntry::new();
        e.backspace(); // should not panic
        assert!(e.is_empty());
    }

    #[test]
    fn clear_empties_buffer() {
        let mut e = PasswordEntry::new();
        for ch in "secret".chars() {
            e.push(ch);
        }
        e.clear();
        assert!(e.is_empty());
    }

    #[test]
    fn max_length_enforced() {
        let mut e = PasswordEntry::new();
        for _ in 0..MAX_PASSWORD_LENGTH + 10 {
            e.push('x');
        }
        assert_eq!(e.len(), MAX_PASSWORD_LENGTH);
    }

    #[test]
    fn display_chars_secret_mode() {
        let mut e = PasswordEntry::new();
        e.mode = InputMode::Secret;
        e.mask_char = '•';
        e.push('s');
        e.push('e');
        let display: String = e.display_chars().collect();
        assert_eq!(display, "••");
    }

    #[test]
    fn display_chars_visible_mode() {
        let mut e = PasswordEntry::new();
        e.mode = InputMode::Visible;
        e.push('O');
        e.push('T');
        e.push('P');
        let display: String = e.display_chars().collect();
        assert_eq!(display, "OTP");
    }

    #[test]
    fn secret_mode_by_default() {
        let e = PasswordEntry::new();
        assert_eq!(e.mode, InputMode::Secret);
    }

    #[test]
    fn backspace_then_push() {
        let mut e = PasswordEntry::new();
        e.push('a');
        e.push('b');
        e.backspace();
        e.push('c');
        assert_eq!(e.as_string(), "ac");
    }

    #[test]
    fn clear_then_push() {
        let mut e = PasswordEntry::new();
        e.push('x');
        e.clear();
        e.push('y');
        assert_eq!(e.as_string(), "y");
    }
}
