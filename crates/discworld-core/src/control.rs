// SPDX-License-Identifier: GPL-3.0-or-later
//! V1 player-control modes from tinlib.cpp Control and cursor.cpp.
use crate::iso::Error;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Control {
    pub enabled: bool,
    pub hidden: bool,
    pub frozen: bool,
    pub tags_enabled: bool,
    pub cursor: [i32; 2],
    saved: [i64; 2],
    off_mode: i32,
    last_event: u64,
}
impl Default for Control {
    fn default() -> Self {
        Self {
            enabled: false,
            hidden: true,
            frozen: false,
            tags_enabled: false,
            cursor: [0; 2],
            saved: [0; 2],
            off_mode: 4,
            last_event: 0,
        }
    }
}
impl Control {
    pub fn apply(&mut self, mode: i32, inventory_open: bool) -> Result<(), Error> {
        self.apply_at(mode, inventory_open, [0; 2])
    }
    pub fn apply_at(
        &mut self,
        mode: i32,
        inventory_open: bool,
        origin: [i32; 2],
    ) -> Result<(), Error> {
        match mode {
            4 => {
                self.enabled = false;
                self.tags_enabled = false;
                self.hidden = true;
                self.off_mode = 4;
            }
            0 | 2 | 3 => {
                if self.enabled || self.off_mode == 4 {
                    self.saved =
                        std::array::from_fn(|i| i64::from(self.cursor[i]) + i64::from(origin[i]));
                }
                self.enabled = false;
                self.tags_enabled = false;
                self.off_mode = mode;
                self.hidden = mode == 0;
                if mode == 2 {
                    self.frozen = true;
                }
            }
            1 => {
                if self.off_mode != 3 && self.off_mode != 4 {
                    self.cursor = std::array::from_fn(|i| {
                        (self.saved[i] - i64::from(origin[i])) as i16 as i32
                    });
                }
                self.enabled = true;
                if !inventory_open {
                    self.tags_enabled = true;
                }
                self.hidden = false;
                self.frozen = false;
            }
            _ => return Err(Error("unknown control mode")),
        }
        Ok(())
    }
    /// Script MOVECURSOR bypasses input freezing and updates CONTROL_ON's
    /// saved world position. Common::Point stores driver coordinates as i16.
    pub fn script_cursor(&mut self, world: [i32; 2], origin: [i32; 2], tick: u64) {
        let position =
            std::array::from_fn(|i| (i64::from(world[i]) - i64::from(origin[i])) as i16 as i32);
        if self.cursor != position {
            self.user_event(tick);
        }
        self.cursor = position;
        self.saved = world.map(i64::from);
    }
    pub fn move_cursor(&mut self, position: [i32; 2]) {
        if !self.frozen {
            self.cursor = position;
        }
    }
    pub fn user_event(&mut self, tick: u64) {
        self.last_event = tick;
    }
    pub fn idle_seconds(&mut self, tick: u64) -> Result<i32, Error> {
        if !self.enabled {
            self.last_event = tick;
            return Ok(0);
        }
        let elapsed = tick
            .checked_sub(self.last_event)
            .ok_or(Error("idle clock moved backwards"))?;
        i32::try_from(elapsed / 24).map_err(|_| Error("idle seconds overflow"))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scripted_world_cursor_survives_control_restore_and_camera_changes() {
        let mut c = Control::default();
        c.apply_at(1, false, [320, 0]).unwrap();
        c.move_cursor([12, 30]);
        c.apply_at(2, false, [320, 0]).unwrap();
        c.script_cursor([400, 90], [320, 0], 100);
        assert_eq!(c.cursor, [80, 90]);
        assert!(c.frozen);
        c.apply_at(1, false, [360, 20]).unwrap();
        assert_eq!(c.cursor, [40, 70]);
        assert_eq!(c.idle_seconds(124).unwrap(), 1);
        c.apply_at(0, false, [360, 20]).unwrap();
        c.apply_at(1, false, [380, 30]).unwrap();
        assert_eq!(c.cursor, [20, 60]);
        c.script_cursor([i32::MAX, i32::MIN], [i32::MIN, i32::MAX], 200);
        assert_eq!(c.cursor, [-1, 1]);
    }
    #[test]
    fn hidden_control_restores_position_but_visible2_keeps_it() {
        let mut c = Control::default();
        c.apply(1, false).unwrap();
        c.move_cursor([10, 20]);
        c.apply(0, false).unwrap();
        c.move_cursor([50, 60]);
        c.apply(1, false).unwrap();
        assert_eq!(c.cursor, [10, 20]);
        c.apply(3, false).unwrap();
        c.move_cursor([70, 80]);
        c.apply(1, false).unwrap();
        assert_eq!(c.cursor, [70, 80]);
    }
    #[test]
    fn frozen_cursor_and_idle_reset() {
        let mut c = Control::default();
        assert_eq!(c.idle_seconds(100).unwrap(), 0);
        c.apply(1, false).unwrap();
        assert_eq!(c.idle_seconds(148).unwrap(), 2);
        c.user_event(148);
        assert_eq!(c.idle_seconds(149).unwrap(), 0);
        c.apply(2, false).unwrap();
        c.move_cursor([4, 8]);
        assert_eq!(c.cursor, [0, 0]);
        assert!(!c.hidden);
        c.apply(1, false).unwrap();
        assert!(!c.frozen);
    }
}
