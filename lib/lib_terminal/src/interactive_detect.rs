//! Stream parser that flags entry into / exit from a "full-terminal"
//! interactive program by sniffing the DEC private-mode set/reset sequences
//! `CSI ? p1;p2;… {h|l}` in a PTY output stream.
//!
//! Used by `TerminalProvider` to auto-switch into the interactive dashboard
//! when the user runs a TUI from the scrollback view, and back out when the
//! program exits.
//!
//! Two families of program are covered:
//!
//! * **Alternate-screen** TUIs — `vim`, `less`, `htop`, `man` — emit
//!   `CSI ? {1049|47|1047} h` on entry and `… l` on exit.
//! * **Main-screen** TUIs — `claude`, `aider`, `gemini` — never touch the
//!   alternate screen (they keep the scrollback intact). They still announce
//!   themselves by enabling mouse-tracking (`1000/1002/1003/1006`) or
//!   focus-tracking (`1004`) reporting, which a bare shell never does.
//!   (`claude` specifically enables `?1004h`.)
//!
//! Bracketed paste (`2004`) is deliberately *not* a trigger: interactive
//! shells (bash/zsh ≥ 5.1) enable it at every prompt, so it carries no signal
//! about a child program. Cursor visibility (`25`) is excluded for the same
//! reason.
//!
//! The detector tracks the *set* of interactive modes currently enabled and
//! emits `Enter` when that set becomes non-empty and `Leave` when it drains
//! back to empty — so a TUI that enables several modes at once (e.g. alt
//! screen + mouse) still yields a single Enter/Leave pair.
//!
//! Resumes across PTY chunk boundaries — partial sequences are remembered
//! between `feed` calls so the caller may pass arbitrary chunks. Bytes that
//! aren't part of a private-mode sequence are discarded; we only emit
//! transitions, never buffer.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InteractiveEvent {
    /// The set of enabled interactive modes went empty → non-empty: a
    /// full-terminal program started.
    Enter,
    /// The set went non-empty → empty: the program exited.
    Leave,
}

/// DEC private modes that mark a "full-terminal" interactive program.
///
/// Bracketed paste (`2004`) and cursor visibility (`25`) are intentionally
/// absent — both are emitted by ordinary shell prompts and would false-trigger.
const INTERACTIVE_MODES: [u32; 8] = [
    47, 1047, 1049, // alternate screen
    1000, 1002, 1003, 1006, // mouse tracking
    1004, // focus tracking
];

#[derive(Debug)]
pub struct InteractiveDetector {
    state: State,
    /// Decimal parameters accumulated since `CSI ?`, split on `;`.
    params: Vec<u32>,
    /// The parameter currently being accumulated.
    cur: u32,
    /// Whether `cur` has seen any digit yet.
    has_digit: bool,
    /// Interactive modes currently enabled (a subset of `INTERACTIVE_MODES`).
    active: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Plain text or unrelated escape — looking for `ESC`.
    Ground,
    /// Saw `ESC` — looking for `[`.
    Esc,
    /// Saw `ESC [` — looking for `?` (or a final byte that ends a non-private
    /// CSI we don't care about).
    Csi,
    /// Saw `ESC [ ?` — accumulating `;`-separated digits, waiting for `h`/`l`.
    Param,
}

impl InteractiveDetector {
    pub fn new() -> Self {
        InteractiveDetector {
            state: State::Ground,
            params: Vec::new(),
            cur: 0,
            has_digit: false,
            active: Vec::new(),
        }
    }

    /// Feed bytes; invoke `emit` for every Enter/Leave transition seen.
    pub fn feed(&mut self, bytes: &[u8], mut emit: impl FnMut(InteractiveEvent)) {
        for &b in bytes {
            match self.state {
                State::Ground => {
                    if b == 0x1b {
                        self.state = State::Esc;
                    }
                }
                State::Esc => {
                    self.state = if b == b'[' { State::Csi } else { State::Ground };
                }
                State::Csi => {
                    if b == b'?' {
                        self.state = State::Param;
                        self.params.clear();
                        self.cur = 0;
                        self.has_digit = false;
                    } else if matches!(b, 0x40..=0x7E) {
                        // Some other CSI final byte; not a private mode.
                        self.state = State::Ground;
                    }
                    // else: stay in Csi (param / intermediate byte we ignore)
                }
                State::Param => match b {
                    b'0'..=b'9' => {
                        self.cur = self
                            .cur
                            .saturating_mul(10)
                            .saturating_add((b - b'0') as u32);
                        self.has_digit = true;
                    }
                    b';' => {
                        if self.has_digit {
                            self.params.push(self.cur);
                        }
                        self.cur = 0;
                        self.has_digit = false;
                    }
                    b'h' | b'l' => {
                        if self.has_digit {
                            self.params.push(self.cur);
                        }
                        let on = b == b'h';
                        let params = std::mem::take(&mut self.params);
                        for p in params {
                            self.apply_mode(p, on, &mut emit);
                        }
                        self.state = State::Ground;
                        self.cur = 0;
                        self.has_digit = false;
                    }
                    b if matches!(b, 0x40..=0x7E) => {
                        // Any other final byte — done with this sequence.
                        self.state = State::Ground;
                    }
                    _ => {}
                },
            }
        }
    }

    /// Apply a single `mode` set/reset, emitting a transition when the active
    /// set crosses the empty boundary.
    fn apply_mode(&mut self, mode: u32, on: bool, emit: &mut impl FnMut(InteractiveEvent)) {
        if !INTERACTIVE_MODES.contains(&mode) {
            return;
        }
        let was_empty = self.active.is_empty();
        if on {
            if !self.active.contains(&mode) {
                self.active.push(mode);
            }
        } else {
            self.active.retain(|&m| m != mode);
        }
        match (was_empty, self.active.is_empty()) {
            (true, false) => emit(InteractiveEvent::Enter),
            (false, true) => emit(InteractiveEvent::Leave),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(input: &[u8]) -> Vec<InteractiveEvent> {
        let mut d = InteractiveDetector::new();
        let mut out = Vec::new();
        d.feed(input, |e| out.push(e));
        out
    }

    #[test]
    fn detects_alt_screen_enter_leave() {
        assert_eq!(collect(b"\x1b[?1049h"), vec![InteractiveEvent::Enter]);
        assert_eq!(
            collect(b"\x1b[?1049h\x1b[?1049l"),
            vec![InteractiveEvent::Enter, InteractiveEvent::Leave],
        );
        assert_eq!(collect(b"\x1b[?47h"), vec![InteractiveEvent::Enter]);
        assert_eq!(collect(b"\x1b[?1047h"), vec![InteractiveEvent::Enter]);
    }

    #[test]
    fn detects_mouse_tracking() {
        assert_eq!(collect(b"\x1b[?1000h"), vec![InteractiveEvent::Enter]);
        assert_eq!(
            collect(b"\x1b[?1002h\x1b[?1002l"),
            vec![InteractiveEvent::Enter, InteractiveEvent::Leave],
        );
        assert_eq!(collect(b"\x1b[?1006h"), vec![InteractiveEvent::Enter]);
    }

    #[test]
    fn detects_focus_tracking() {
        // This is the signal `claude` emits.
        assert_eq!(collect(b"\x1b[?1004h"), vec![InteractiveEvent::Enter]);
        assert_eq!(
            collect(b"\x1b[?1004h\x1b[?1004l"),
            vec![InteractiveEvent::Enter, InteractiveEvent::Leave],
        );
    }

    #[test]
    fn bracketed_paste_alone_does_not_trigger() {
        // Shells enable ?2004 at every prompt — must not be a trigger.
        assert_eq!(collect(b"\x1b[?2004h"), vec![]);
        assert_eq!(collect(b"\x1b[?2004h\x1b[?2004l"), vec![]);
    }

    #[test]
    fn cursor_visibility_does_not_trigger() {
        assert_eq!(collect(b"\x1b[?25h"), vec![]);
        assert_eq!(collect(b"\x1b[?25l"), vec![]);
    }

    #[test]
    fn claude_style_init_triggers_once() {
        // Sequence captured from a real `claude` startup: sync output, cursor
        // hide, bracketed paste, focus tracking. Only ?1004h is a trigger.
        assert_eq!(
            collect(b"\x1b[?2026h\x1b[?25l\x1b[?2004h\x1b[?1004h\x1b[?2026l"),
            vec![InteractiveEvent::Enter],
        );
    }

    #[test]
    fn multi_param_yields_single_enter() {
        // `CSI ? 1000;1002;1006 h` — three modes, one empty→non-empty cross.
        assert_eq!(collect(b"\x1b[?1000;1002;1006h"), vec![InteractiveEvent::Enter]);
    }

    #[test]
    fn multi_param_alt_screen_with_extra() {
        // The old detector bailed on `;`; now `1049` is still honoured.
        assert_eq!(collect(b"\x1b[?1049;6h"), vec![InteractiveEvent::Enter]);
    }

    #[test]
    fn combined_modes_one_enter_one_leave() {
        // A TUI that enables alt screen + mouse, then disables both on exit.
        assert_eq!(
            collect(b"\x1b[?1049h\x1b[?1002h\x1b[?1002l\x1b[?1049l"),
            vec![InteractiveEvent::Enter, InteractiveEvent::Leave],
        );
    }

    #[test]
    fn idempotent_enable_no_duplicate_enter() {
        assert_eq!(
            collect(b"\x1b[?1004h\x1b[?1004h"),
            vec![InteractiveEvent::Enter],
        );
    }

    #[test]
    fn ignores_plain_csi_and_text() {
        assert_eq!(collect(b"\x1b[31m"), vec![]);
        assert_eq!(collect(b"\x1b[2J"), vec![]);
        assert_eq!(collect(b"\x1b[H"), vec![]);
        assert_eq!(collect(b"hello world\n"), vec![]);
    }

    #[test]
    fn partial_chunks_resume() {
        let mut d = InteractiveDetector::new();
        let mut events = Vec::new();
        for chunk in [&b"\x1b["[..], &b"?10"[..], &b"49h"[..]] {
            d.feed(chunk, |e| events.push(e));
        }
        assert_eq!(events, vec![InteractiveEvent::Enter]);
    }

    #[test]
    fn garbage_recovers_for_next_sequence() {
        assert_eq!(
            collect(b"\x1b[?1049x\x1b[?1049h"),
            vec![InteractiveEvent::Enter],
        );
    }
}
