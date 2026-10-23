// SPDX-License-Identifier: GPL-3.0-or-later
//! Scripted V1 scrolling from pinned scroll.cpp and tinlib.cpp.
use crate::iso::Error;
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraDirection {
    Left,
    Right,
    Up,
    Down,
}

/// Scene-local V1 NOSCROLL lines. Coordinates retain their supplied order.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct Boundaries {
    vertical: Vec<[i32; 3]>,
    horizontal: Vec<[i32; 3]>,
}
impl Boundaries {
    pub fn add(&mut self, from: [i32; 2], to: [i32; 2]) -> Result<(), Error> {
        let (lines, line) = if from[0] == to[0] {
            (&mut self.vertical, [from[0], from[1], to[1]])
        } else if from[1] == to[1] {
            (&mut self.horizontal, [from[1], from[0], to[0]])
        } else {
            return Ok(());
        }; // V1 ignores diagonal lines.
        if lines.len() == 10 {
            return Err(Error("no-scroll boundary limit"));
        }
        lines.push(line);
        Ok(())
    }
    /// Direction names describe camera motion (opposite the image motion names
    /// in reference NeedScroll). Screen dimensions are the DOS V1 320x200.
    pub fn blocks(&self, origin: [i32; 2], direction: CameraDirection) -> bool {
        let [x, y] = origin.map(i64::from);
        let (lines, edge, start, end) = match direction {
            CameraDirection::Right => (&self.vertical, x + 319, y, y + 199),
            CameraDirection::Left => (&self.vertical, x, y, y + 199),
            CameraDirection::Down => (&self.horizontal, y + 199, x, x + 319),
            CameraDirection::Up => (&self.horizontal, y, x, x + 319),
        };
        lines.iter().any(|line| {
            let [at, a, b] = line.map(i64::from);
            (edge - at).abs() <= 1
                && ((start..=end).contains(&a)
                    || (start..=end).contains(&b)
                    || (a < start && b > end))
        })
    }
}
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct Scroll {
    pub serial: u64,
    pub boundaries: Boundaries,
    pub focus: i32,
    pub follow_cursor: bool,
    previous: [i32; 2],
    remaining: [i64; 2],
    speed: [i64; 2],
}
impl Scroll {
    pub fn is_scrolling(&self) -> bool {
        self.remaining != [0, 0]
    }
    pub fn reset_scene(&mut self) {
        self.boundaries = Boundaries::default();
        self.initialize();
    }
    /// ScrollProcess initializes motion after its background becomes available.
    pub fn initialize(&mut self) {
        self.stop();
        self.previous = [0; 2];
        self.speed = [8; 2];
    }
    pub fn set_focus(&mut self, actor: i32) {
        if self.focus != actor {
            self.previous = [0; 2];
            self.focus = actor;
        }
    }
    /// Monitor a visible mover's world foot position and sprite height.
    pub fn monitor(
        &mut self,
        position: [i32; 2],
        height: i64,
        origin: [i32; 2],
        maximum: [i32; 2],
    ) {
        if position == self.previous {
            return;
        }
        let [x, y] = position.map(i64::from);
        let [left, top] = origin.map(i64::from);
        let mut directions = [None; 2];
        if x > left + 270 && origin[0] < maximum[0] {
            if position[0] > self.previous[0] {
                directions[0] = Some(CameraDirection::Right);
            }
        } else if x < left + 50 && origin[0] != 0 && position[0] < self.previous[0] {
            directions[0] = Some(CameraDirection::Left);
        }
        if y > top + 180 && origin[1] < maximum[1] {
            if position[1] > self.previous[1] {
                directions[1] = Some(CameraDirection::Down);
            }
        } else if origin[1] != 0
            && i128::from(y) < i128::from(top) + 20 + i128::from(height)
            && position[1] < self.previous[1]
        {
            directions[1] = Some(CameraDirection::Up);
        }
        for (axis, direction) in directions.into_iter().enumerate() {
            let Some(direction) = direction else {
                continue;
            };
            if self.boundaries.blocks(origin, direction) {
                continue;
            }
            let positive = matches!(direction, CameraDirection::Right | CameraDirection::Down);
            if (positive && self.remaining[axis] <= 0) || (!positive && self.remaining[axis] >= 0) {
                self.remaining[axis] = [160, 100][axis] * if positive { 1 } else { -1 };
                self.speed[axis] = 8;
            }
        }
        self.previous = position;
    }
    pub fn start(&mut self, origin: [i32; 2], target: [i32; 2], speed: i32) -> Result<u64, Error> {
        if speed < 0 {
            return Err(Error("negative scroll speed"));
        }
        let serial = self.supersede()?;
        self.speed = [if speed == 0 { 8 } else { i64::from(speed) }; 2];
        self.remaining = [
            i64::from(target[0]) - i64::from(origin[0]),
            i64::from(target[1]) - i64::from(origin[1]),
        ];
        Ok(serial)
    }
    /// Every SCROLL request advances its identity, including instant escape.
    pub fn supersede(&mut self) -> Result<u64, Error> {
        self.serial = self
            .serial
            .checked_add(1)
            .ok_or(Error("scroll serial overflow"))?;
        Ok(self.serial)
    }
    /// OFFSET stops motion but does not supersede the waiting SCROLL number in V1.
    pub fn stop(&mut self) {
        self.remaining = [0, 0];
    }
    pub fn tick(&mut self, origin: &mut [i32; 2], maximum: [i32; 2]) {
        for axis in 0..2 {
            let delta = self.remaining[axis].clamp(-self.speed[axis], self.speed[axis]);
            self.remaining[axis] -= delta;
            let next = i64::from(origin[axis]) + delta;
            origin[axis] = if delta > 0 {
                next.min(i64::from(maximum[axis])) as i32
            } else if delta < 0 {
                next.max(0) as i32
            } else {
                origin[axis]
            };
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn monitor_starts_bursts_only_toward_edges_and_respects_boundaries() {
        let mut scroll = Scroll::default();
        scroll.set_focus(7);
        scroll.monitor([270, 180], 40, [0, 0], [640, 200]);
        assert_eq!(scroll.remaining, [0, 0]);
        scroll.monitor([271, 181], 40, [0, 0], [640, 200]);
        assert_eq!(scroll.remaining, [160, 100]);
        let mut origin = [0, 0];
        scroll.tick(&mut origin, [640, 200]);
        assert_eq!(origin, [8, 8]);
        scroll.monitor([272, 182], 40, [0, 0], [640, 200]);
        assert_eq!(
            scroll.remaining,
            [152, 92],
            "same-direction movement does not restart an active burst"
        );
        scroll.monitor([40, 40], 40, [8, 8], [640, 200]);
        assert_eq!(scroll.remaining, [-160, -100]);
        let serial = scroll.serial;
        scroll.reset_scene();
        assert_eq!(scroll.focus, 7);
        assert_eq!(scroll.serial, serial);
        scroll.boundaries.add([320, -1], [320, 201]).unwrap();
        scroll.boundaries.add([0, 200], [639, 200]).unwrap();
        scroll.monitor([280, 190], 40, [0, 0], [640, 200]);
        assert_eq!(scroll.remaining, [0, 0]);
        // Explicit scripted scroll ignores the automatic camera boundaries.
        scroll.start([0, 0], [24, 24], 3).unwrap();
        scroll.tick(&mut origin, [640, 200]);
        assert_eq!(origin, [11, 11]);
    }
    #[test]
    fn automatic_scroll_changes_only_the_triggered_axis_speed() {
        let mut scroll = Scroll::default();
        scroll.start([0, 0], [0, 30], 3).unwrap();
        scroll.monitor([280, 0], 40, [0, 0], [640, 200]);
        assert_eq!(scroll.speed, [8, 3]);
        let mut origin = [0, 0];
        let mut restored = scroll.clone();
        let mut restored_origin = origin;
        scroll.tick(&mut origin, [640, 200]);
        restored.tick(&mut restored_origin, [640, 200]);
        assert_eq!(origin, [8, 3]);
        assert_eq!(scroll, restored);
        assert_eq!(origin, restored_origin);
    }
    #[test]
    fn boundaries_use_viewport_edges_tolerance_and_overlap() {
        use CameraDirection::*;
        let mut b = Boundaries::default();
        b.add([0, 200], [639, 200]).unwrap();
        assert!(b.blocks([0, 0], Down));
        assert!(b.blocks([320, 0], Down));
        assert!(!b.blocks([640, 0], Down));
        assert!(!b.blocks([0, -1], Down));
        assert!(b.blocks([0, 200], Up));
        assert!(!b.blocks([0, 0], Up));
        b.add([320, -50], [320, 250]).unwrap();
        assert!(b.blocks([0, 0], Right));
        assert!(b.blocks([320, 0], Left));
        assert!(!b.blocks([318, 0], Left));
        assert!(!b.blocks([0, 251], Right));
        let mut reversed = Boundaries::default();
        reversed.add([320, 250], [320, -50]).unwrap();
        assert!(
            !reversed.blocks([0, 0], Right),
            "do not normalize reversed endpoints"
        );
        assert!(!b.blocks([i32::MAX, i32::MIN], Right));
    }
    #[test]
    fn boundary_capacity_and_diagonal_noop_are_atomic() {
        let mut b = Boundaries::default();
        for i in 0..10 {
            b.add([i, 0], [i, 100]).unwrap();
        }
        let before = b.clone();
        assert!(b.add([10, 0], [10, 100]).is_err());
        assert_eq!(b, before);
        b.add([0, 0], [1, 1]).unwrap();
        assert_eq!(b, before);
        b.add([0, 10], [100, 10]).unwrap(); // Independent horizontal quota.
        assert!(b.blocks([0, 10], CameraDirection::Up));
    }
    #[test]
    fn default_speed_remainder_clipping_and_stop() {
        let mut scroll = Scroll::default();
        let mut origin = [640, 0];
        scroll.start(origin, [621, 12], 0).unwrap();
        scroll.tick(&mut origin, [640, 5]);
        assert_eq!(origin, [632, 5]);
        scroll.tick(&mut origin, [640, 5]);
        assert_eq!(origin, [624, 5]);
        scroll.tick(&mut origin, [640, 5]);
        assert_eq!(origin, [621, 5]);
        let serial = scroll.start(origin, [0, 0], 3).unwrap();
        scroll.stop();
        scroll.tick(&mut origin, [640, 5]);
        assert_eq!(origin, [621, 5]);
        assert_eq!(scroll.serial, serial);
        let before = scroll.clone();
        assert!(scroll.start(origin, [0, 0], -1).is_err());
        assert_eq!(scroll, before);
    }
}
