//! Hotkey state machine. Pure logic, fed with key states once per tick.

use std::time::{Duration, Instant};

/// Hold time before a held zoom key starts repeating.
const REPEAT_DELAY: Duration = Duration::from_millis(350);
/// Interval between repeats while a zoom key stays held.
const REPEAT_INTERVAL: Duration = Duration::from_millis(60);

#[derive(Clone, Copy, Debug, Default)]
pub struct Inputs {
    pub trigger: bool,
    pub modifier: bool,
    pub zoom_in: bool,
    pub zoom_out: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Output {
    /// The enabled state flipped during this tick.
    pub toggled: bool,
    /// The magnifier should be visible.
    pub zoom_active: bool,
    /// Net zoom steps requested (positive = zoom in).
    pub zoom_steps: i32,
}

/// Edge-triggered key with auto-repeat, so a tap is exactly one step
/// regardless of the polling rate.
#[derive(Clone, Copy, Debug, Default)]
struct Repeat {
    pressed_at: Option<Instant>,
    last_step: Option<Instant>,
}

impl Repeat {
    fn poll(&mut self, down: bool, now: Instant) -> i32 {
        if !down {
            *self = Self::default();
            return 0;
        }
        match (self.pressed_at, self.last_step) {
            (Some(pressed), Some(last)) => {
                if now - pressed >= REPEAT_DELAY && now - last >= REPEAT_INTERVAL {
                    self.last_step = Some(now);
                    1
                } else {
                    0
                }
            }
            _ => {
                self.pressed_at = Some(now);
                self.last_step = Some(now);
                1
            }
        }
    }
}

#[derive(Debug)]
pub struct InputState {
    pub enabled: bool,
    combo_was_down: bool,
    /// Set by a toggle; keeps the zoom hidden until the trigger is released,
    /// so letting go of the modifier first doesn't pop the magnifier up.
    suppress_until_release: bool,
    zoom_in: Repeat,
    zoom_out: Repeat,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            enabled: true,
            combo_was_down: false,
            suppress_until_release: false,
            zoom_in: Repeat::default(),
            zoom_out: Repeat::default(),
        }
    }
}

impl InputState {
    pub fn update(&mut self, i: Inputs, now: Instant) -> Output {
        let combo = i.trigger && i.modifier;
        let toggled = combo && !self.combo_was_down;
        self.combo_was_down = combo;
        if toggled {
            self.enabled = !self.enabled;
            self.suppress_until_release = true;
        }
        if !i.trigger {
            self.suppress_until_release = false;
        }

        let zoom_active = self.enabled && i.trigger && !i.modifier && !self.suppress_until_release;
        let zoom_steps = if zoom_active {
            self.zoom_in.poll(i.zoom_in, now) - self.zoom_out.poll(i.zoom_out, now)
        } else {
            self.zoom_in = Repeat::default();
            self.zoom_out = Repeat::default();
            0
        };
        Output {
            toggled,
            zoom_active,
            zoom_steps,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(trigger: bool, modifier: bool) -> Inputs {
        Inputs {
            trigger,
            modifier,
            ..Inputs::default()
        }
    }

    #[test]
    fn hold_trigger_zooms() {
        let mut s = InputState::default();
        let t = Instant::now();
        assert!(s.update(keys(true, false), t).zoom_active);
        assert!(!s.update(keys(false, false), t).zoom_active);
    }

    #[test]
    fn combo_toggles_once_per_press() {
        let mut s = InputState::default();
        let t = Instant::now();
        let out = s.update(keys(true, true), t);
        assert!(out.toggled && !out.zoom_active && !s.enabled);
        assert!(!s.update(keys(true, true), t).toggled);
        s.update(keys(false, false), t);
        assert!(
            !s.update(keys(true, false), t).zoom_active,
            "disabled tool must not zoom"
        );
        s.update(keys(false, false), t);
        assert!(s.update(keys(true, true), t).toggled);
        assert!(s.enabled);
    }

    #[test]
    fn releasing_modifier_first_does_not_zoom() {
        let mut s = InputState::default();
        let t = Instant::now();
        s.update(keys(true, true), t); // disable
        s.update(keys(false, false), t);
        s.update(keys(true, true), t); // enable again
        assert!(
            !s.update(keys(true, false), t).zoom_active,
            "trigger still held from the toggle"
        );
        s.update(keys(false, false), t);
        assert!(s.update(keys(true, false), t).zoom_active);
    }

    #[test]
    fn zoom_keys_step_once_per_tap_then_repeat() {
        let mut s = InputState::default();
        let t0 = Instant::now();
        let held = Inputs {
            trigger: true,
            zoom_in: true,
            ..Inputs::default()
        };
        assert_eq!(s.update(held, t0).zoom_steps, 1);
        // Polling every 10 ms must not add steps before the repeat delay.
        for ms in (10..350).step_by(10) {
            assert_eq!(
                s.update(held, t0 + Duration::from_millis(ms)).zoom_steps,
                0,
                "at {ms} ms"
            );
        }
        assert_eq!(
            s.update(held, t0 + Duration::from_millis(350)).zoom_steps,
            1
        );
        assert_eq!(
            s.update(held, t0 + Duration::from_millis(380)).zoom_steps,
            0
        );
        assert_eq!(
            s.update(held, t0 + Duration::from_millis(410)).zoom_steps,
            1
        );
    }

    #[test]
    fn zoom_keys_ignored_while_not_zoomed() {
        let mut s = InputState::default();
        let t = Instant::now();
        let out = s.update(
            Inputs {
                zoom_out: true,
                ..Inputs::default()
            },
            t,
        );
        assert_eq!(out.zoom_steps, 0);
    }
}
