//! When a freeze is allowed to start, and when it must end.
//!
//! These rules are pure so they can be tested without a window. The Windows
//! hook and the egui loop both consult them.

use std::time::{Duration, Instant};

/// Ignore another capture for this long after the overlay closes.
/// Print Screen auto-repeats faster than this.
pub const CAPTURE_DEBOUNCE: Duration = Duration::from_millis(500);

/// The overlay closes itself after this long even if no input arrives.
pub const OVERLAY_TIMEOUT: Duration = Duration::from_secs(60);

/// Low-level keyboard hook flag. Bit 0 is "extended key", not key-up.
pub const LLKHF_EXTENDED: u32 = 0x01;
pub const LLKHF_UP: u32 = 0x80;

#[derive(Clone, Debug)]
pub struct CaptureGate {
    overlay_open: bool,
    suppress_until: Option<Instant>,
}

impl Default for CaptureGate {
    fn default() -> Self {
        Self::new()
    }
}

impl CaptureGate {
    pub fn new() -> Self {
        Self {
            overlay_open: false,
            suppress_until: None,
        }
    }

    pub fn overlay_open(&self) -> bool {
        self.overlay_open
    }

    /// An explicit hotkey or tray click. Launch, key-repeat, and a second
    /// process starting up are not explicit once the caller has filtered them.
    pub fn allow(&self, now: Instant) -> bool {
        if self.overlay_open {
            return false;
        }
        if let Some(until) = self.suppress_until {
            if now < until {
                return false;
            }
        }
        true
    }

    pub fn on_opened(&mut self) {
        self.overlay_open = true;
        self.suppress_until = None;
    }

    pub fn on_closed(&mut self, now: Instant) {
        self.overlay_open = false;
        self.suppress_until = Some(now + CAPTURE_DEBOUNCE);
    }
}

pub fn overlay_expired(opened: Instant, now: Instant) -> bool {
    now.checked_duration_since(opened)
        .is_some_and(|age| age >= OVERLAY_TIMEOUT)
}

/// True only for the key-up transition. `flags & LLKHF_UP != 0` is the wrong
/// expression: `!=` binds tighter than `&`, so that checks the extended-key bit.
pub fn low_level_key_is_release(flags: u32) -> bool {
    (flags & LLKHF_UP) != 0
}

/// `GetAsyncKeyState` reports "down now" in the high bit, not the low bit.
pub fn async_key_is_down(state: i16) -> bool {
    (state as u16 & 0x8000) != 0
}

pub fn rising_edge(was_down: bool, is_down: bool) -> bool {
    is_down && !was_down
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_does_not_imply_a_capture_and_the_first_hotkey_does() {
        let gate = CaptureGate::new();
        let now = Instant::now();
        assert!(gate.allow(now));
    }

    #[test]
    fn repeats_are_ignored_while_the_overlay_is_open() {
        let mut gate = CaptureGate::new();
        let now = Instant::now();
        assert!(gate.allow(now));
        gate.on_opened();
        assert!(!gate.allow(now));
        assert!(!gate.allow(now + Duration::from_secs(10)));
    }

    #[test]
    fn repeats_are_ignored_for_500ms_after_close() {
        let mut gate = CaptureGate::new();
        let opened = Instant::now();
        gate.on_opened();
        let closed = opened + Duration::from_secs(2);
        gate.on_closed(closed);
        assert!(!gate.allow(closed));
        assert!(!gate.allow(closed + Duration::from_millis(499)));
        assert!(gate.allow(closed + CAPTURE_DEBOUNCE));
        assert!(gate.allow(closed + Duration::from_millis(501)));
    }

    #[test]
    fn timeout_is_60_seconds() {
        let opened = Instant::now();
        assert!(!overlay_expired(opened, opened + Duration::from_secs(59)));
        assert!(overlay_expired(opened, opened + OVERLAY_TIMEOUT));
        assert!(overlay_expired(opened, opened + Duration::from_secs(61)));
    }

    #[test]
    fn key_up_flag_is_not_the_extended_bit() {
        assert!(!low_level_key_is_release(0));
        assert!(!low_level_key_is_release(LLKHF_EXTENDED));
        assert!(low_level_key_is_release(LLKHF_UP));
        assert!(low_level_key_is_release(LLKHF_EXTENDED | LLKHF_UP));
    }

    #[test]
    fn async_key_state_uses_the_high_bit() {
        assert!(!async_key_is_down(0));
        assert!(!async_key_is_down(1));
        assert!(async_key_is_down(i16::MIN));
        assert!(async_key_is_down(-32767));
    }

    #[test]
    fn rising_edge_fires_once_per_press() {
        assert!(!rising_edge(false, false));
        assert!(rising_edge(false, true));
        assert!(!rising_edge(true, true));
        assert!(!rising_edge(true, false));
    }
}
