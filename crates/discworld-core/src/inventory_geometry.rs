// SPDX-License-Identifier: GPL-3.0-or-later
//! V1 invAction header maximization, per-window bMax and alternate positions.
use crate::{iso::Error, startup::Window};

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub(crate) struct Saved {
    pub maximized: bool,
    size: [i32; 2],
    other: [i32; 2],
}
impl Default for Saved {
    fn default() -> Self {
        Self {
            maximized: false,
            size: [0, 0],
            other: [21, 15],
        }
    }
}
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct State {
    pub windows: [Saved; 3],
    pub active_maximized: bool,
}
impl State {
    pub fn open(&mut self, slot: usize) {
        self.active_maximized = self.windows[slot].maximized;
    }
    pub fn close(&mut self, slot: usize) {
        self.windows[slot].maximized = self.active_maximized;
    }
    pub fn toggle(&mut self, slot: usize, window: &mut Window) -> Result<bool, Error> {
        // V1 requires BOTH dimensions to vary, not either one.
        if window.max[0] == window.min[0] || window.max[1] == window.min[1] {
            return Ok(false);
        }
        let saved = &mut self.windows[slot];
        let size = if self.active_maximized {
            saved.size
        } else {
            window.max
        };
        if !(1..=10).contains(&size[0]) || !(1..=6).contains(&size[1]) {
            return Err(Error("invalid maximized inventory dimensions"));
        }
        if !self.active_maximized {
            saved.size = window.size;
        }
        window.size = size;
        std::mem::swap(&mut window.position, &mut saved.other);
        self.active_maximized = !self.active_maximized;
        Ok(true)
    }
    pub fn validate(
        &self,
        windows: &[Option<Window>; 3],
        active: Option<i32>,
    ) -> Result<(), Error> {
        for (slot, saved) in self.windows.iter().enumerate() {
            if saved != &Saved::default() && windows[slot].is_none() {
                return Err(Error("missing maximized inventory declaration"));
            }
            let needs_size =
                saved.maximized || (self.active_maximized && active == Some(slot as i32));
            let valid = (1..=10).contains(&saved.size[0]) && (1..=6).contains(&saved.size[1]);
            if !valid && (needs_size || saved.size != [0, 0]) {
                return Err(Error("invalid saved inventory dimensions"));
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn window() -> Window {
        Window {
            title: crate::graphics::Handle(0),
            capacity: 100,
            min: [2, 1],
            size: [3, 2],
            max: [10, 6],
            position: [100, 100],
            first_display: 0,
            contents: vec![],
        }
    }
    #[test]
    fn toggles_keep_separate_positions_and_survive_close_reopen() {
        let mut state = State::default();
        let mut w = window();
        state.open(1);
        assert!(state.toggle(1, &mut w).unwrap());
        assert_eq!((w.size, w.position), ([10, 6], [21, 15]));
        w.position = [31, 25];
        state.close(1);
        state.open(2);
        assert!(!state.active_maximized);
        state.open(1);
        assert!(state.active_maximized);
        state.toggle(1, &mut w).unwrap();
        assert_eq!((w.size, w.position), ([3, 2], [100, 100]));
        state.toggle(1, &mut w).unwrap();
        assert_eq!((w.size, w.position), ([10, 6], [31, 25]));
        let mut windows = [None, None, None];
        windows[1] = Some(w);
        state.validate(&windows, Some(1)).unwrap();
        state.windows[1].size = [0, 2];
        assert!(state.validate(&windows, Some(1)).is_err());
    }
    #[test]
    fn either_fixed_dimension_prevents_maximizing() {
        for dim in 0..2 {
            let mut w = window();
            w.max[dim] = w.min[dim];
            let original = w.clone();
            let mut state = State::default();
            assert!(!state.toggle(1, &mut w).unwrap());
            assert_eq!(w, original);
            assert_eq!(state, State::default());
        }
    }
}
