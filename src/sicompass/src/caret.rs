//! Cursor blink state. Equivalent to `caret.c`.

#[derive(Debug, Default)]
pub struct CaretState {
    pub visible: bool,
    last_toggle_ms: u64,
}

impl CaretState {
    pub const BLINK_INTERVAL_MS: u64 = 400;

    pub fn new() -> Self { CaretState::default() }

    pub fn update(&mut self, now_ms: u64) {
        if now_ms.saturating_sub(self.last_toggle_ms) >= Self::BLINK_INTERVAL_MS {
            self.visible = !self.visible;
            self.last_toggle_ms = now_ms;
        }
    }

    pub fn reset(&mut self, now_ms: u64) {
        self.visible = true;
        self.last_toggle_ms = now_ms;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const I: u64 = CaretState::BLINK_INTERVAL_MS;

    #[test] fn blink_interval_ms_is_400() { assert_eq!(CaretState::BLINK_INTERVAL_MS, 400); }
    #[test] fn new_not_visible() { assert!(!CaretState::new().visible); }
    /// Mirrors C `test_caretCreate_stores_initial_time`: the new caret has last_toggle_ms = 0,
    /// so update(0) should not trigger a toggle.
    #[test] fn new_caret_update_at_zero_does_not_toggle() { let mut c = CaretState::new(); c.update(0); assert!(!c.visible); }
    #[test] fn reset_makes_visible() { let mut c = CaretState::new(); c.reset(0); assert!(c.visible); }
    #[test] fn no_toggle_before_interval() { let mut c = CaretState::new(); c.reset(0); c.update(I - 1); assert!(c.visible); }
    #[test] fn toggles_at_interval() { let mut c = CaretState::new(); c.reset(0); c.update(I); assert!(!c.visible); }
    #[test] fn toggles_past_interval() { let mut c = CaretState::new(); c.reset(0); c.update(I + 100); assert!(!c.visible); }
    #[test] fn toggles_twice() { let mut c = CaretState::new(); c.reset(0); c.update(I); c.update(I * 2); assert!(c.visible); }

    #[test]
    fn update_updates_last_toggle_time() {
        let mut c = CaretState::new();
        c.reset(0);
        c.update(I); // toggles, sets last_toggle_ms = I
        // No second toggle until I more ms have passed
        c.update(I + I - 1);
        assert!(!c.visible); // still invisible — not enough time
    }

    #[test]
    fn update_no_toggle_keeps_time() {
        let mut c = CaretState::new();
        c.reset(1000);
        c.update(1000 + I - 1); // too early — no toggle
        assert!(c.visible);
        c.update(1000 + I); // exactly at interval — now toggles
        assert!(!c.visible);
    }

    #[test]
    fn reset_updates_last_blink_time() {
        let mut c = CaretState::new();
        c.reset(5000);
        // Should not toggle until 5000 + I
        c.update(5000 + I - 1);
        assert!(c.visible);
        c.update(5000 + I);
        assert!(!c.visible);
    }

    #[test]
    fn reset_already_visible_stays_visible() {
        let mut c = CaretState::new();
        c.reset(0);
        assert!(c.visible);
        c.reset(100);
        assert!(c.visible);
    }

    #[test]
    fn reset_restarts_blink_cycle() {
        let mut c = CaretState::new();
        c.reset(0);
        c.update(I); // goes invisible
        assert!(!c.visible);
        c.reset(1000); // reset at t=1000
        assert!(c.visible);
        c.update(1000 + I - 1); // just before next toggle
        assert!(c.visible);
        c.update(1000 + I); // at next toggle
        assert!(!c.visible);
    }
}
