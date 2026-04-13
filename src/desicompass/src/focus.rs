//! Window focus and cycle-windows logic.
//!
//! The compositor maintains a focus stack: toplevels are ordered from
//! most-recently-focused (front) to least-recently-focused (back), mirroring
//! the `wl_list toplevels` in `src/desicompass/main.c`.
//!
//! This module provides the pure window-management logic that can be tested
//! without a running Wayland server.

/// A lightweight handle for a window in the focus stack.
///
/// In the real compositor this wraps a `smithay::desktop::Window`.  Here it
/// is a plain `usize` ID so we can unit-test the stack behaviour in isolation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowId(pub usize);

/// A simple ordered focus stack: `windows[0]` is the frontmost/focused window.
///
/// Mirrors the `wl_list toplevels` in the C compositor, where the head of
/// the list is the most recently focused toplevel.
#[derive(Debug, Default)]
pub struct FocusStack {
    windows: Vec<WindowId>,
}

impl FocusStack {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a window to the front of the stack (highest focus priority).
    pub fn push_front(&mut self, id: WindowId) {
        self.windows.retain(|&w| w != id);
        self.windows.insert(0, id);
    }

    /// Remove a window from the stack.
    pub fn remove(&mut self, id: WindowId) {
        self.windows.retain(|&w| w != id);
    }

    /// Currently focused window (frontmost).
    pub fn focused(&self) -> Option<WindowId> {
        self.windows.first().copied()
    }

    /// Number of windows in the stack.
    pub fn len(&self) -> usize {
        self.windows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }

    /// Cycle focus: the last window in the stack (least recently focused)
    /// becomes the new frontmost window.  Mirrors the Alt+F1 logic in
    /// `handle_keybinding()` in `src/desicompass/main.c`.
    ///
    /// Returns the newly focused window, or `None` if the stack has fewer
    /// than 2 windows.
    pub fn cycle(&mut self) -> Option<WindowId> {
        if self.windows.len() < 2 {
            return None;
        }
        let last = self.windows.pop().unwrap();
        self.windows.insert(0, last);
        self.windows.first().copied()
    }

    /// Ordered view of the stack, frontmost first.
    pub fn as_slice(&self) -> &[WindowId] {
        &self.windows
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_stack_has_no_focus() {
        let s = FocusStack::new();
        assert!(s.focused().is_none());
        assert_eq!(s.len(), 0);
        assert!(s.is_empty());
    }

    #[test]
    fn push_front_sets_focus() {
        let mut s = FocusStack::new();
        s.push_front(WindowId(1));
        assert_eq!(s.focused(), Some(WindowId(1)));
    }

    #[test]
    fn push_front_updates_focus() {
        let mut s = FocusStack::new();
        s.push_front(WindowId(1));
        s.push_front(WindowId(2));
        assert_eq!(s.focused(), Some(WindowId(2)));
        assert_eq!(s.len(), 2);
    }

    #[test]
    fn push_front_existing_moves_to_front() {
        let mut s = FocusStack::new();
        s.push_front(WindowId(1));
        s.push_front(WindowId(2));
        s.push_front(WindowId(1)); // re-focus 1
        assert_eq!(s.focused(), Some(WindowId(1)));
        assert_eq!(s.len(), 2);
        assert_eq!(s.as_slice(), &[WindowId(1), WindowId(2)]);
    }

    #[test]
    fn remove_frontmost_focuses_next() {
        let mut s = FocusStack::new();
        s.push_front(WindowId(1));
        s.push_front(WindowId(2));
        s.remove(WindowId(2));
        assert_eq!(s.focused(), Some(WindowId(1)));
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn remove_middle() {
        let mut s = FocusStack::new();
        s.push_front(WindowId(1));
        s.push_front(WindowId(2));
        s.push_front(WindowId(3));
        s.remove(WindowId(2));
        assert_eq!(s.as_slice(), &[WindowId(3), WindowId(1)]);
    }

    #[test]
    fn remove_nonexistent_is_noop() {
        let mut s = FocusStack::new();
        s.push_front(WindowId(1));
        s.remove(WindowId(99));
        assert_eq!(s.len(), 1);
    }

    #[test]
    fn cycle_single_window_returns_none() {
        let mut s = FocusStack::new();
        s.push_front(WindowId(1));
        assert!(s.cycle().is_none());
        assert_eq!(s.focused(), Some(WindowId(1)));
    }

    #[test]
    fn cycle_two_windows() {
        let mut s = FocusStack::new();
        s.push_front(WindowId(1));
        s.push_front(WindowId(2));
        // stack: [2, 1]
        let focused = s.cycle();
        // last (1) moves to front
        assert_eq!(focused, Some(WindowId(1)));
        assert_eq!(s.as_slice(), &[WindowId(1), WindowId(2)]);
    }

    #[test]
    fn cycle_three_windows_rotates() {
        let mut s = FocusStack::new();
        s.push_front(WindowId(1));
        s.push_front(WindowId(2));
        s.push_front(WindowId(3));
        // stack: [3, 2, 1]
        s.cycle(); // [1, 3, 2]
        assert_eq!(s.as_slice(), &[WindowId(1), WindowId(3), WindowId(2)]);
        s.cycle(); // [2, 1, 3]
        assert_eq!(s.as_slice(), &[WindowId(2), WindowId(1), WindowId(3)]);
    }

    #[test]
    fn cycle_empty_returns_none() {
        let mut s = FocusStack::new();
        assert!(s.cycle().is_none());
    }
}
