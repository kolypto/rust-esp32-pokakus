use crate::led::{set_led_state, LedState};

// Report status to LED:
// - In Progress
// - Outcome = Success
// - Outcome = Failure
pub struct Status;

impl Status {
    pub fn new() -> Self {
        set_led_state(LedState::RapidBlink);
        Self
    }

    fn set_result(self, state: LedState) {
        // `self` is *moved* here so it can be run only once
        set_led_state(state);
        // Prevent `.drop()` from running.
        // The guard is leaked, but it's zero-sized so who cares.
        core::mem::forget(self);
    }

    // Blink "success".
    // Can only be called once.
    pub fn success(self) {
        self.set_result(LedState::Success);
    }

    // Blink "failure".
    // Can only be called once.
    pub fn failure(self) {
        self.set_result(LedState::Failure);
    }
}

impl Drop for Status {
    fn drop(&mut self) {
        // Dropped without outcome? Assume failure.
        set_led_state(LedState::Failure);
    }
}