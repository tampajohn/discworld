// SPDX-License-Identifier: GPL-3.0-or-later
//! DOS V1 gettingWider/Narrower/Taller/Shorter and constructInventory(EMPTY).
use crate::{
    animation::Animation,
    conversation_window::Area,
    film::{self, Film},
    graphics::{Handle, Image, Resources},
    iso::Error,
    startup::Window,
};
use std::collections::BTreeMap;
const REELS: [usize; 18] = [
    1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 17, 21, 22, 23,
];

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub(crate) struct Drag {
    direction: [i32; 2],
    changes: [i64; 2],
    supplement: [i32; 2],
    initial_size: [i32; 2],
    initial_position: [i32; 2],
    dimensions: [i32; 4], // resize TL width/height, TR width, BL height
    templates: BTreeMap<usize, Animation>,
    pub overlay: Vec<Animation>,
}
impl Drag {
    pub fn direction(area: Area) -> Option<[i32; 2]> {
        Some(match area {
            Area::Left => [-1, 0],
            Area::Right => [1, 0],
            Area::Top => [0, -1],
            Area::Bottom => [0, 1],
            Area::TopLeft => [-1, -1],
            Area::TopRight => [1, -1],
            Area::BottomLeft => [-1, 1],
            Area::BottomRight => [1, 1],
            _ => return None,
        })
    }
    pub fn new(
        resources: &Resources<'_>,
        handle: Handle,
        window: &Window,
        direction: [i32; 2],
    ) -> Result<Self, Error> {
        let film = Film::read(resources, handle)?;
        let size = |index: usize| -> Result<[i32; 2], Error> {
            let reel = film.reels.get(index).ok_or(Error("missing resize reel"))?;
            let object = film::Object::read(resources, reel.object)?;
            let frames = film::frame(resources, object.frame)?;
            let image = Image::read(
                resources,
                *frames.first().ok_or(Error("empty resize frame"))?,
            )?;
            Ok([i32::from(image.width), i32::from(image.height)])
        };
        let tl = size(21)?;
        let dimensions = [tl[0], tl[1], size(22)?[0], size(17)?[1]];
        let templates = REELS
            .into_iter()
            .map(|i| {
                Ok((
                    i,
                    Animation::new(
                        resources,
                        film.reels.get(i).ok_or(Error("missing resize reel"))?,
                        1,
                    )?,
                ))
            })
            .collect::<Result<_, Error>>()?;
        let drag = Self {
            direction,
            changes: [0, 0],
            supplement: [0, 0],
            initial_size: window.size,
            initial_position: window.position,
            dimensions,
            templates,
            overlay: vec![],
        };
        drag.validate(window)?;
        Ok(drag)
    }
    fn change_axis(&mut self, window: &mut Window, axis: usize) -> Result<(), Error> {
        let start = window.size[axis];
        let old = self.supplement[axis];
        let change = &mut self.changes[axis];
        let supp = &mut self.supplement[axis];
        if *change > 0 {
            *change = change
                .checked_add(i64::from(*supp))
                .ok_or(Error("resize accumulation overflow"))?;
            *supp = 0;
            while *change > 26 && window.size[axis] < window.max[axis] {
                *change -= 26;
                window.size[axis] += 1;
            }
            if window.size[axis] < window.max[axis] {
                *supp = *change as i32;
                *change = 0;
            }
        } else if *change < 0 {
            if *supp != 0 {
                *change = change
                    .checked_add(i64::from(*supp) - 26)
                    .ok_or(Error("resize accumulation overflow"))?;
                window.size[axis] += 1;
                *supp = 0;
            }
            while *change < -26 && window.size[axis] > window.min[axis] {
                *change += 26;
                window.size[axis] -= 1;
            }
            if window.size[axis] > window.min[axis] && *change != 0 {
                *supp = (26 + *change) as i32;
                window.size[axis] -= 1;
                *change = 0;
            }
        }
        if self.direction[axis] < 0 {
            let shift = i64::from(start - window.size[axis]) * 26 - i64::from(*supp - old);
            window.position[axis] = i32::try_from(i64::from(window.position[axis]) + shift)
                .map_err(|_| Error("resize position overflow"))?;
        }
        Ok(())
    }
    pub fn move_by(&mut self, window: &mut Window, delta: [i32; 2]) -> Result<(), Error> {
        // cursor.cpp dispatches X first, then Y. Each dispatch runs both axes.
        for (axis, movement) in delta.into_iter().enumerate() {
            if movement == 0 || self.direction[axis] == 0 {
                continue;
            }
            self.changes[axis] = self.changes[axis]
                .checked_add(i64::from(movement) * i64::from(self.direction[axis]))
                .ok_or(Error("resize accumulation overflow"))?;
            self.change_axis(window, 1)?;
            self.change_axis(window, 0)?;
            self.overlay = self.parts(window)?;
        }
        Ok(())
    }
    pub fn finish(&self, window: &mut Window) {
        for axis in 0..2 {
            if self.supplement[axis] > 12 {
                window.size[axis] += 1;
            }
        }
    }
    pub fn cancel(&self, window: &mut Window) {
        window.size = self.initial_size;
        window.position = self.initial_position;
    }
    fn parts(&self, window: &Window) -> Result<Vec<Animation>, Error> {
        let [x, y] = window.position.map(i64::from);
        let [tw, th, rw, bh] = self.dimensions.map(i64::from);
        let [cols, rows] = window.size;
        let eh = i64::from(cols - 1) * 26 + i64::from(self.supplement[0]);
        let ev = i64::from(rows - 1) * 26 + i64::from(self.supplement[1]);
        let mut specs = vec![
            (21, x, y),
            (22, x + tw + eh, y),
            (17, x, y + th + ev),
            (23, x + tw + eh, y + th + ev),
        ];
        if cols > 1 {
            specs.extend([
                (cols as usize + 4, x + tw, y),
                (cols as usize + 4, x + tw, y + th + ev + bh - 4),
            ]);
        }
        if self.supplement[0] != 0 {
            let off = (tw + eh - 26).max(tw);
            specs.extend([(6, x + off, y), (6, x + off, y + th + ev + bh - 4)]);
        }
        if rows > 1 {
            specs.extend([
                (rows as usize - 1, x, y + th),
                (rows as usize - 1, x + tw + eh + rw - 4, y + th),
            ]);
        }
        if self.supplement[1] != 0 {
            let off = (th + ev - 26).max(5);
            specs.extend([(1, x, y + off), (1, x + tw + eh + rw - 4, y + off)]);
        }
        specs
            .into_iter()
            .map(|(reel, x, y)| {
                let mut animation = self
                    .templates
                    .get(&reel)
                    .ok_or(Error("missing resize template"))?
                    .clone();
                animation.object.x =
                    i32::try_from(x).map_err(|_| Error("resize frame overflow"))?;
                animation.object.y =
                    i32::try_from(y).map_err(|_| Error("resize frame overflow"))?;
                animation.object.z = 22;
                Ok(animation)
            })
            .collect()
    }
    pub fn animations(&self) -> impl Iterator<Item = &Animation> {
        self.templates.values().chain(&self.overlay)
    }
    pub fn validate(&self, window: &Window) -> Result<(), Error> {
        if self.direction == [0, 0]
            || self.direction.iter().any(|v| !(-1..=1).contains(v))
            || self.supplement.iter().any(|s| !(0..=26).contains(s))
            || self.dimensions.iter().any(|d| !(1..=320).contains(d))
            || self.templates.keys().copied().collect::<Vec<_>>() != REELS
            || !(1..=10).contains(&self.initial_size[0])
            || !(1..=6).contains(&self.initial_size[1])
            || (0..2).any(|a| {
                !(1..=[10, 6][a]).contains(&window.min[a])
                    || !(1..=[10, 6][a]).contains(&window.max[a])
                    || window.size[a] < window.min[a]
                    || window.size[a] > window.max[a]
                    || (window.size[a] == window.max[a] && self.supplement[a] != 0)
            })
            || self.overlay.len() > 12
        {
            return Err(Error("invalid inventory resize drag"));
        }
        if !self.overlay.is_empty() && self.overlay != self.parts(window)? {
            return Err(Error("inconsistent inventory resize outline"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn window() -> Window {
        Window {
            title: Handle(0),
            capacity: 100,
            min: [2, 1],
            size: [3, 2],
            max: [10, 6],
            position: [100, 100],
            first_display: 0,
            contents: vec![],
        }
    }
    fn drag(direction: [i32; 2]) -> Drag {
        Drag {
            direction,
            changes: [0, 0],
            supplement: [0, 0],
            initial_size: [3, 2],
            initial_position: [100, 100],
            dimensions: [8, 26, 35, 21],
            templates: BTreeMap::new(),
            overlay: vec![],
        }
    }
    #[test]
    fn partial_cells_round_at_thirteen_and_leading_edge_keeps_opposite_side() {
        for axis in 0..2 {
            for sign in [-1, 1] {
                for distance in [12, 13, 26, 27, 39] {
                    let mut w = window();
                    let mut dir = [0, 0];
                    dir[axis] = sign;
                    let mut d = drag(dir);
                    d.changes[axis] = distance;
                    d.change_axis(&mut w, axis).unwrap();
                    assert_eq!(
                        (w.size[axis] - [3, 2][axis]) * 26 + d.supplement[axis],
                        distance as i32
                    );
                    assert_eq!(
                        w.position[axis],
                        100 - if sign < 0 { distance as i32 } else { 0 }
                    );
                    d.finish(&mut w);
                    assert_eq!(
                        w.size[axis],
                        [3, 2][axis]
                            + if distance > 38 {
                                2
                            } else if distance > 12 {
                                1
                            } else {
                                0
                            }
                    );
                }
            }
        }
    }
    #[test]
    fn shrinking_and_limit_overshoot_follow_reference() {
        let mut w = window();
        let mut d = drag([-1, 0]);
        d.changes[0] = -13;
        d.change_axis(&mut w, 0).unwrap();
        assert_eq!((w.size[0], d.supplement[0], w.position[0]), (2, 13, 113));
        d.changes[0] = -100;
        d.change_axis(&mut w, 0).unwrap();
        assert_eq!((w.size[0], d.supplement[0], w.position[0]), (2, 0, 126));
        d.changes[0] += 50;
        d.change_axis(&mut w, 0).unwrap();
        assert_eq!(w.size[0], 2);
        d.changes[0] += 51;
        d.change_axis(&mut w, 0).unwrap();
        assert_eq!(d.supplement[0], 14);
        assert_eq!(w.position[0], 112);
    }
}
