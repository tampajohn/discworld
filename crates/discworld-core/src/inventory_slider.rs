// SPDX-License-Identifier: GPL-3.0-or-later
//! DOS V1 adjustTop/nearestSlideY/slideSlider. The unclamped accumulator is
//! intentional: dragging beyond a limit must be undone before the thumb moves.
use crate::{conversation_window::Scrollbar, iso::Error, startup::Window};

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub(crate) struct Drag {
    range: [i32; 2],
    stops: Vec<(usize, i32)>,
    accumulated: i64,
    selected: usize,
}
impl Drag {
    pub fn new(window: &Window, bar: &Scrollbar) -> Result<Self, Error> {
        let cols = usize::try_from(window.size[0]).map_err(|_| Error("invalid slider columns"))?;
        let rows = usize::try_from(window.size[1]).map_err(|_| Error("invalid slider rows"))?;
        if cols == 0 || rows == 0 || window.contents.len() > 150 || window.first_display > 150 {
            return Err(Error("invalid slider window"));
        }
        let top = window.first_display.div_ceil(cols);
        let bottom = window
            .contents
            .len()
            .saturating_sub(window.first_display)
            .div_ceil(cols)
            .saturating_sub(rows);
        let missing = top + bottom;
        let span = i64::from(bar.range[1]) - i64::from(bar.range[0]);
        if span < 0 || missing > 150 {
            return Err(Error("invalid slider range"));
        }
        let base = window.first_display as i64 - (top * cols) as i64;
        let stops = (0..=missing)
            .map(|i| {
                let index = (base + (i * cols) as i64).max(0) as usize;
                let y = i64::from(bar.range[0])
                    + if missing == 0 {
                        0
                    } else {
                        i as i64 * span / missing as i64
                    };
                (index, y as i32)
            })
            .collect();
        let mut drag = Self {
            range: bar.range,
            stops,
            accumulated: i64::from(bar.thumb[1]),
            selected: 0,
        };
        drag.selected = drag.nearest(drag.y());
        drag.validate(bar)?;
        Ok(drag)
    }
    fn nearest(&self, y: i32) -> usize {
        // Strict-less-than in nearestSlideY preserves the first stop on ties.
        self.stops
            .iter()
            .enumerate()
            .min_by_key(|(_, (_, stop))| (i64::from(*stop) - i64::from(y)).abs())
            .map_or(0, |(i, _)| i)
    }
    pub fn y(&self) -> i32 {
        self.accumulated
            .clamp(i64::from(self.range[0]), i64::from(self.range[1])) as i32
    }
    pub fn first_display(&self) -> usize {
        self.stops[self.nearest(self.y())].0
    }
    pub fn move_by(&mut self, dy: i32) -> Result<Option<usize>, Error> {
        self.accumulated = self
            .accumulated
            .checked_add(i64::from(dy))
            .ok_or(Error("slider movement overflow"))?;
        let selected = self.nearest(self.y());
        let changed = selected != self.selected;
        self.selected = selected;
        Ok(changed.then(|| self.stops[selected].0))
    }
    pub fn validate(&self, bar: &Scrollbar) -> Result<(), Error> {
        if self.range != bar.range
            || self.range[0] > self.range[1]
            || self.stops.is_empty()
            || self.stops.len() > 151
            || self.selected >= self.stops.len()
            || self
                .stops
                .iter()
                .any(|(n, y)| *n > 150 || !(self.range[0]..=self.range[1]).contains(y))
            || self
                .stops
                .windows(2)
                .any(|s| s[0].0 > s[1].0 || s[0].1 > s[1].1)
            || self.stops.first().unwrap().1 != self.range[0]
            || self.stops.last().unwrap().1 != self.range[1]
            || self.y() != bar.thumb[1]
            || self.selected != self.nearest(self.y())
        {
            return Err(Error("invalid inventory slider drag"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(first: usize, count: usize, range: [i32; 2]) -> Drag {
        let window = Window {
            title: crate::graphics::Handle(0),
            capacity: 150,
            min: [1, 1],
            size: [3, 2],
            max: [8, 6],
            position: [100, 100],
            first_display: first,
            contents: vec![1; count],
        };
        let top = first.div_ceil(3);
        let bottom = count.saturating_sub(first).div_ceil(3).saturating_sub(2);
        let y = range[0] + top as i32 * (range[1] - range[0]) / (top + bottom) as i32;
        Drag::new(
            &window,
            &Scrollbar {
                top: 100,
                right: 200,
                bottom: 180,
                thumb: [190, y],
                range,
            },
        )
        .unwrap()
    }
    #[test]
    fn nearest_rows_keep_ties_and_off_grid_first_display() {
        let mut drag = fixture(0, 15, [120, 150]);
        assert_eq!(drag.stops, vec![(0, 120), (3, 130), (6, 140), (9, 150)]);
        assert_eq!(drag.move_by(5).unwrap(), None);
        assert_eq!(drag.move_by(1).unwrap(), Some(3));
        assert_eq!(drag.move_by(14).unwrap(), Some(6));
        let off_grid = fixture(1, 15, [120, 150]);
        assert_eq!(
            off_grid.stops,
            vec![(0, 120), (1, 127), (4, 135), (7, 142), (10, 150)]
        );
    }
    #[test]
    fn malformed_drag_tables_and_thumb_mismatches_are_rejected() {
        let valid = fixture(0, 15, [120, 150]);
        let bar = Scrollbar {
            top: 100,
            right: 200,
            bottom: 180,
            thumb: [190, 120],
            range: [120, 150],
        };
        for kind in 0..7 {
            let mut bad = valid.clone();
            match kind {
                0 => bad.stops.clear(),
                1 => bad.selected = usize::MAX,
                2 => bad.range = [151, 120],
                3 => bad.stops[1].1 = 119,
                4 => bad.stops[1].0 = 151,
                5 => bad.accumulated = i64::MAX,
                _ => bad.stops.reverse(),
            }
            assert!(bad.validate(&bar).is_err(), "case {kind}");
        }
        let mut extremes = valid;
        extremes.move_by(i32::MAX).unwrap();
        assert_eq!(extremes.y(), 150);
        extremes.move_by(i32::MIN).unwrap();
        assert_eq!(extremes.y(), 120);
    }

    #[test]
    fn overshoot_is_retained_and_duplicate_stops_choose_first() {
        let mut drag = fixture(0, 15, [120, 150]);
        drag.move_by(50).unwrap();
        assert_eq!(drag.y(), 150);
        assert_eq!(drag.move_by(-10).unwrap(), None);
        assert_eq!(drag.y(), 150);
        drag.move_by(-11).unwrap();
        assert_eq!(drag.y(), 149);
        let crowded = fixture(0, 150, [120, 122]);
        assert_eq!(crowded.first_display(), 0);
        assert_eq!(crowded.nearest(121), 24);
    }
}
