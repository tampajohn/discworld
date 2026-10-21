// SPDX-License-Identifier: GPL-3.0-or-later
//! V1 ButtonToggle coroutine. Unlike ButtonPress, it changes the setting inside
//! the animation, and losing the window/overlay before that step prevents it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    Remove,
    Down,
    FlipDown,
    Highlight,
    Finished,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Toggle {
    phase: Option<u8>,
}
impl Toggle {
    pub fn start(&mut self) -> bool {
        if self.phase.is_some() {
            return false;
        }
        self.phase = Some(0);
        true
    }
    pub fn busy(&self) -> bool {
        self.phase.is_some()
    }
    pub fn step(&mut self, inventory_active: bool, overlay_exists: bool) -> Effect {
        let Some(phase) = self.phase else {
            return Effect::Finished;
        };
        if (matches!(phase, 1 | 4) && !inventory_active)
            || (matches!(phase, 2 | 3) && !overlay_exists)
        {
            self.phase = None;
            return Effect::Finished;
        }
        self.phase = if phase == 4 { None } else { Some(phase + 1) };
        match phase {
            0 | 3 => Effect::Remove,
            1 => Effect::Down,
            2 => Effect::FlipDown,
            4 => Effect::Highlight,
            _ => unreachable!("private toggle phase"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn state_changes_mid_animation_once_and_respects_reference_abort_checks() {
        let mut toggle = Toggle::default();
        assert!(toggle.start());
        assert!(!toggle.start());
        for expected in [
            Effect::Remove,
            Effect::Down,
            Effect::FlipDown,
            Effect::Remove,
            Effect::Highlight,
        ] {
            assert_eq!(toggle.step(true, true), expected);
        }
        assert!(!toggle.busy());
        assert_eq!(toggle.step(true, true), Effect::Finished);
        for (steps, active, overlay) in [
            (1, false, true),
            (2, true, false),
            (3, true, false),
            (4, false, true),
        ] {
            let mut toggle = Toggle::default();
            toggle.start();
            let mut flips = 0;
            for _ in 0..steps {
                flips += usize::from(toggle.step(true, true) == Effect::FlipDown);
            }
            assert_eq!(toggle.step(active, overlay), Effect::Finished);
            assert_eq!(flips, usize::from(steps >= 3));
            assert!(!toggle.busy());
        }
    }
}
