//! V1 physical-button classification from the pinned Tinsel MouseProcess.
//!
//! This is the first stage of input handling, not script dispatch. In particular,
//! a WalkTo event targeting a script still needs AllowDclick's token-owned delay;
//! a plain ground walk does not. Hosts currently use development controls.
use serde::{Deserialize, Serialize};

pub const DEFAULT_DOUBLE_CLICK_TICKS: u32 = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
}
impl Button {
    fn index(self) -> usize {
        match self {
            Self::Left => 0,
            Self::Right => 1,
        }
    }
    fn swapped(self, swap: bool) -> Self {
        match (self, swap) {
            (Self::Left, true) => Self::Right,
            (Self::Right, true) => Self::Left,
            _ => self,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerEvent {
    WalkTo,
    Action,
    Look,
    NoEvent,
    DragStart(Button),
    DragEnd(Button),
}

/// Times use the reference's wrapping 32-bit tick clock. The host must supply
/// simulation time, not wall time, and preserve event order within a tick.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MouseInput {
    last_release: [u32; 2],
    was_double: [bool; 2],
    double_click_ticks: u32,
}
impl MouseInput {
    pub fn new(tick: u32, double_click_ticks: u32) -> Self {
        Self {
            // Reference initialization intentionally makes an immediate first
            // press a double. Do not silently replace this with "no prior click".
            last_release: [tick; 2],
            was_double: [false; 2],
            double_click_ticks,
        }
    }

    /// Reference order is drag-start first, then the click classification.
    /// Swapping buttons happens after classification using physical timers.
    pub fn button_down(&mut self, button: Button, tick: u32, swap: bool) -> [PlayerEvent; 2] {
        let index = button.index();
        let double = tick.wrapping_sub(self.last_release[index]) < self.double_click_ticks;
        self.was_double[index] = double;
        let logical = button.swapped(swap);
        let click = match (logical, double) {
            (Button::Left, false) => PlayerEvent::WalkTo,
            (Button::Left, true) => PlayerEvent::Action,
            (Button::Right, false) => PlayerEvent::Look,
            (Button::Right, true) => PlayerEvent::NoEvent,
        };
        [PlayerEvent::DragStart(logical), click]
    }

    pub fn button_up(&mut self, button: Button, tick: u32, swap: bool) -> PlayerEvent {
        let index = button.index();
        if self.was_double[index] {
            // Prevent a third press from becoming another double in the pair.
            self.last_release[index] =
                self.last_release[index].wrapping_sub(self.double_click_ticks);
        } else {
            self.last_release[index] = tick;
        }
        PlayerEvent::DragEnd(button.swapped(swap))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn threshold_is_strict_and_measured_from_release_not_press() {
        for (next_tick, expected) in [(109, PlayerEvent::Action), (110, PlayerEvent::WalkTo)] {
            let mut input = MouseInput::new(0, 10);
            assert_eq!(
                input.button_down(Button::Left, 10, false),
                [PlayerEvent::DragStart(Button::Left), PlayerEvent::WalkTo]
            );
            assert_eq!(
                input.button_up(Button::Left, 100, false),
                PlayerEvent::DragEnd(Button::Left)
            );
            assert_eq!(
                input.button_down(Button::Left, next_tick, false)[1],
                expected
            );
        }
    }

    #[test]
    fn double_release_ends_pair_and_buttons_keep_independent_timers() {
        let mut input = MouseInput::new(0, 10);
        input.button_down(Button::Left, 10, false);
        input.button_up(Button::Left, 11, false);
        assert_eq!(
            input.button_down(Button::Right, 12, false)[1],
            PlayerEvent::Look
        );
        assert_eq!(
            input.button_down(Button::Left, 12, false)[1],
            PlayerEvent::Action
        );
        input.button_up(Button::Left, 13, false);
        assert_eq!(
            input.button_down(Button::Left, 14, false)[1],
            PlayerEvent::WalkTo
        );
        input.button_up(Button::Right, 15, false);
        assert_eq!(
            input.button_down(Button::Right, 16, false)[1],
            PlayerEvent::NoEvent
        );
    }

    #[test]
    fn swap_maps_drag_and_click_after_physical_classification() {
        let mut input = MouseInput::new(0, 10);
        assert_eq!(
            input.button_down(Button::Right, 10, true),
            [PlayerEvent::DragStart(Button::Left), PlayerEvent::WalkTo]
        );
        assert_eq!(
            input.button_up(Button::Right, 11, true),
            PlayerEvent::DragEnd(Button::Left)
        );
        assert_eq!(
            input.button_down(Button::Right, 12, true)[1],
            PlayerEvent::Action
        );
        assert_eq!(
            input.button_down(Button::Left, 12, true)[1],
            PlayerEvent::Look
        );
    }

    #[test]
    fn initialization_wrap_and_disabled_double_click_follow_reference_arithmetic() {
        let mut input = MouseInput::new(u32::MAX - 2, 10);
        assert_eq!(
            input.button_down(Button::Left, u32::MAX - 2, false)[1],
            PlayerEvent::Action
        );
        input.button_up(Button::Left, u32::MAX - 1, false);
        assert_eq!(
            input.button_down(Button::Left, 0, false)[1],
            PlayerEvent::WalkTo
        );
        input.button_up(Button::Left, 1, false);
        assert_eq!(
            input.button_down(Button::Left, 2, false)[1],
            PlayerEvent::Action
        );
        let mut disabled = MouseInput::new(0, 0);
        assert_eq!(
            disabled.button_down(Button::Left, 0, false)[1],
            PlayerEvent::WalkTo
        );
    }

    #[test]
    fn serialized_partial_pair_preserves_next_event() {
        let mut input = MouseInput::new(0, 10);
        input.button_down(Button::Left, 10, false);
        input.button_up(Button::Left, 12, false);
        let bytes = postcard::to_allocvec(&input).unwrap();
        let mut restored: MouseInput = postcard::from_bytes(&bytes).unwrap();
        assert_eq!(
            input.button_down(Button::Left, 15, false),
            restored.button_down(Button::Left, 15, false)
        );
        assert_eq!(input, restored);
    }
}
