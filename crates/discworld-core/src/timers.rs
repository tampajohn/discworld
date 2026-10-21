// SPDX-License-Identifier: GPL-3.0-or-later
//! V1 script timers, following pinned ScummVM timers.cpp (see NOTICE).
//! Advance once after script scheduling, independent of host wall-clock time.
use crate::iso::Error;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Timer {
    id: i32,
    ticks: i32,
    seconds: i32,
    delta: i32,
    frames: bool,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct Timers([Timer; 16]);
impl Timers {
    pub fn start(&mut self, id: i32, start: i32, up: bool, frames: bool) -> Result<(), Error> {
        if id == 0 {
            return Err(Error("zero timer ID"));
        }
        let index = self
            .0
            .iter()
            .position(|t| t.id == id)
            .or_else(|| self.0.iter().position(|t| t.id == 0))
            .ok_or(Error("too many timers"))?;
        self.0[index] = Timer {
            id,
            ticks: if frames { start } else { 0 },
            seconds: if frames { 0 } else { start },
            delta: if up { 1 } else { -1 },
            frames,
        };
        Ok(())
    }
    pub fn get(&self, id: i32) -> i32 {
        self.0
            .iter()
            .find(|t| t.id == id)
            .map_or(-1, |t| if t.frames { t.ticks } else { t.seconds })
    }
    pub fn tick(&mut self) -> Result<(), Error> {
        let mut next = self.clone();
        for t in &mut next.0 {
            if t.id == 0 {
                continue;
            }
            t.ticks = t
                .ticks
                .checked_add(t.delta)
                .ok_or(Error("timer overflow"))?;
            if t.frames {
                t.ticks = t.ticks.max(0);
            } else if t.ticks < 0 {
                // Preserve V1's immediate first decrement and 24 reset value.
                t.ticks = 24;
                t.seconds = t
                    .seconds
                    .checked_sub(1)
                    .ok_or(Error("timer overflow"))?
                    .max(0);
            } else if t.ticks == 24 {
                t.ticks = 0;
                t.seconds = t.seconds.checked_add(1).ok_or(Error("timer overflow"))?;
            }
        }
        *self = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn boundaries_restart_capacity_and_cloned_continuation() {
        let mut timers = Timers::default();
        assert_eq!(timers.get(1), -1);
        assert_eq!(timers.get(0), 0);
        assert!(timers.start(0, 1, true, true).is_err());
        timers.start(1, 2, false, true).unwrap();
        timers.start(2, 2, false, false).unwrap();
        timers.start(3, 0, true, false).unwrap();
        let mut copy = timers.clone();
        for tick in 1..=52 {
            timers.tick().unwrap();
            copy.tick().unwrap();
            assert_eq!(timers, copy);
            assert_eq!(timers.get(1), (2 - tick).max(0));
            assert_eq!(timers.get(2), if tick < 26 { 1 } else { 0 });
            assert_eq!(timers.get(3), tick / 24);
        }
        for id in 4..=16 {
            timers.start(id, 0, true, true).unwrap();
        }
        let before = timers.clone();
        assert!(timers.start(17, 0, true, true).is_err());
        assert_eq!(timers, before);
        assert_eq!(timers.get(0), -1);
        timers.start(1, 10, true, true).unwrap();
        timers.tick().unwrap();
        assert_eq!(timers.get(1), 11);
        timers.start(1, i32::MAX, true, true).unwrap();
        let before = timers.clone();
        assert!(timers.tick().is_err());
        assert_eq!(timers, before);
    }
}
