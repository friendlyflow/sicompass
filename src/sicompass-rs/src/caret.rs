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
    #[test] fn new_not_visible() { assert!(!CaretState::new().visible); }
    #[test] fn reset_makes_visible() { let mut c = CaretState::new(); c.reset(0); assert!(c.visible); }
    #[test] fn no_toggle_before_interval() { let mut c = CaretState::new(); c.reset(0); c.update(CaretState::BLINK_INTERVAL_MS - 1); assert!(c.visible); }
    #[test] fn toggles_at_interval() { let mut c = CaretState::new(); c.reset(0); c.update(CaretState::BLINK_INTERVAL_MS); assert!(!c.visible); }
    #[test] fn toggles_twice() { let mut c = CaretState::new(); c.reset(0); c.update(CaretState::BLINK_INTERVAL_MS); c.update(CaretState::BLINK_INTERVAL_MS * 2); assert!(c.visible); }
}
