// SPDX-License-Identifier: GPL-3.0-or-later
//! Original DOS V1 window frame construction (dialogs.cpp FULL).
use crate::{
    animation::Animation,
    compositor::Surface,
    film::{self, Film},
    graphics::{Handle, Image, Resources},
    iso::Error,
    startup::Window,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    /// Original window-parts reel index and animation origin, in draw order.
    pub parts: Vec<(usize, [i32; 2])>,
    pub background: [i32; 4], // x, y, width, height
    pub title: [i32; 2],
    pub icons: Vec<[i32; 2]>,
    pub scrollbar: Option<Scrollbar>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scrollbar {
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub thumb: [i32; 2],
    pub range: [i32; 2],
}
impl Scrollbar {
    pub fn action_at(&self, [x, y]: [i32; 2]) -> Option<crate::inventory::Scroll> {
        use crate::inventory::Scroll;
        if x <= self.right - 9 || x > self.right - 2 {
            return None;
        }
        if y > self.top + 17 && y < self.top + 23 {
            return Some(Scroll::Up);
        }
        if y > self.bottom - 6 && y <= self.bottom - 2 {
            return Some(Scroll::Down);
        }
        if y >= self.range[0] && y < self.range[1] + 5 {
            if y < self.thumb[1] {
                return Some(Scroll::PageUp);
            }
            if y >= self.thumb[1] + 5 {
                return Some(Scroll::PageDown);
            }
        }
        None
    }
}
impl Layout {
    pub fn new(
        window: &Window,
        top_left: [i32; 2],
        top_right_width: i32,
        bottom_left_height: i32,
    ) -> Result<Self, Error> {
        Self::build(
            window,
            top_left,
            top_right_width,
            bottom_left_height,
            true,
            false,
        )
    }
    pub fn inventory(
        window: &Window,
        top_left: [i32; 2],
        top_right_width: i32,
        bottom_left_height: i32,
    ) -> Result<Self, Error> {
        Self::build(
            window,
            top_left,
            top_right_width,
            bottom_left_height,
            false,
            false,
        )
    }
    pub fn configuration(
        window: &Window,
        top_left: [i32; 2],
        top_right_width: i32,
        bottom_left_height: i32,
    ) -> Result<Self, Error> {
        Self::build(
            window,
            top_left,
            top_right_width,
            bottom_left_height,
            true,
            true,
        )
    }
    fn build(
        window: &Window,
        top_left: [i32; 2],
        top_right_width: i32,
        bottom_left_height: i32,
        conversation: bool,
        configuration: bool,
    ) -> Result<Self, Error> {
        let [cols, rows] = window.size;
        if !(1..=10).contains(&cols)
            || !(1..=6).contains(&rows)
            || top_left.iter().any(|v| *v <= 0)
            || top_right_width <= 0
            || bottom_left_height <= 0
        {
            return Err(Error("invalid conversation frame dimensions"));
        }
        let [x, y] = window.position.map(i64::from);
        let [tw, th] = top_left.map(i64::from);
        let rw = i64::from(top_right_width);
        let bh = i64::from(bottom_left_height);
        let eh = i64::from(cols - 1) * 26;
        let ev = i64::from(rows - 1) * 26;
        let point = |x, y| -> Result<[i32; 2], Error> {
            Ok([
                i32::try_from(x).map_err(|_| Error("conversation frame x overflow"))?,
                i32::try_from(y).map_err(|_| Error("conversation frame y overflow"))?,
            ])
        };
        let corners = if conversation {
            [21, 22, 17, 23]
        } else {
            [15, 16, 17, 18]
        };
        let mut parts = vec![
            (corners[0], point(x, y)?),
            (corners[1], point(x + tw + eh, y)?),
            (corners[2], point(x, y + th + ev)?),
            (corners[3], point(x + tw + eh, y + th + ev)?),
        ];
        if cols > 1 {
            let filler = (cols + 4) as usize; // IX_H26 + cols - 2
            parts.push((filler, point(x + tw, y)?));
            if !configuration {
                parts.push((filler, point(x + tw, y + 15)?));
            }
            if conversation && !configuration {
                parts.extend([
                    (6, point(x + tw - 2, y + 15)?),
                    (7, point(x + eh - 10, y + 15)?),
                ]);
            }
            parts.push((filler, point(x + tw, y + th + ev + bh - 4)?));
        }
        if rows > 1 {
            let filler = (rows - 1) as usize; // IX_V26 + rows - 2
            parts.push((filler, point(x, y + th)?));
            if !conversation {
                parts.push((filler, point(x + tw + eh + 27, y + th)?));
            }
            parts.push((filler, point(x + tw + eh + rw - 4, y + th)?));
        }
        let width = tw + eh + rw - 3;
        let height = th + ev + bh - 3;
        if width <= 0 || height <= 0 {
            return Err(Error("empty conversation background"));
        }
        let size = point(width, height)?;
        let origin = point(x + 1, y + 1)?;
        let mut icons = Vec::new();
        for row in 0..if configuration { 0 } else { rows } {
            for col in 0..cols {
                icons.push(point(
                    x + 6 + i64::from(col) * 26,
                    y + 20 + i64::from(row) * 26,
                )?);
            }
        }
        let scrollbar = if !conversation && window.contents.len() > (cols * rows) as usize {
            let top_missing = window.first_display.div_ceil(cols as usize);
            let wanted = window
                .contents
                .len()
                .saturating_sub(window.first_display)
                .div_ceil(cols as usize);
            let bottom_missing = wanted.saturating_sub(rows as usize);
            let min = th - 2;
            let max = th + ev + 10;
            let offset = if top_missing == 0 {
                min
            } else if bottom_missing == 0 {
                max
            } else {
                min + top_missing as i64 * (max - min) / (top_missing + bottom_missing) as i64
            };
            let thumb = point(x + width - 7, y + offset)?;
            parts.push((0, thumb));
            Some(Scrollbar {
                top: window.position[1],
                right: point(x + width + 1, y)?[0],
                bottom: point(x, y + height + 1)?[1],
                thumb,
                range: [point(x, y + min)?[1], point(x, y + max)?[1]],
            })
        } else {
            None
        };
        Ok(Self {
            parts,
            scrollbar,
            background: [origin[0], origin[1], size[0], size[1]],
            title: point(x + width / 2, y + 4)?,
            icons,
        })
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub layout: Layout,
    pub parts: Vec<Animation>,
}
impl Frame {
    pub fn new(resources: &Resources<'_>, handle: Handle, window: &Window) -> Result<Self, Error> {
        Self::build(resources, handle, window, true, false)
    }
    pub fn inventory(
        resources: &Resources<'_>,
        handle: Handle,
        window: &Window,
    ) -> Result<Self, Error> {
        Self::build(resources, handle, window, false, false)
    }
    pub fn top_window(resources: &Resources<'_>, handle: Handle) -> Result<Self, Error> {
        let window = Window {
            title: Handle(0),
            capacity: 0,
            min: [6, 5],
            size: [6, 5],
            max: [6, 5],
            position: [72, 23],
            first_display: 0,
            contents: vec![],
        };
        Self::build(resources, handle, &window, true, true)
    }
    fn build(
        resources: &Resources<'_>,
        handle: Handle,
        window: &Window,
        conversation: bool,
        configuration: bool,
    ) -> Result<Self, Error> {
        let film = Film::read(resources, handle)?;
        let size = |index: usize| -> Result<[i32; 2], Error> {
            let reel = film
                .reels
                .get(index)
                .ok_or(Error("missing conversation corner reel"))?;
            let object = film::Object::read(resources, reel.object)?;
            let frame = film::frame(resources, object.frame)?;
            let image = Image::read(
                resources,
                *frame.first().ok_or(Error("empty conversation corner"))?,
            )?;
            Ok([i32::from(image.width), i32::from(image.height)])
        };
        let layout = if configuration {
            Layout::configuration(window, size(21)?, size(22)?[0], size(17)?[1])?
        } else if conversation {
            Layout::new(window, size(21)?, size(22)?[0], size(17)?[1])?
        } else {
            Layout::inventory(window, size(15)?, size(16)?[0], size(17)?[1])?
        };
        let parts = layout
            .parts
            .iter()
            .map(|(index, position)| {
                let reel = film
                    .reels
                    .get(*index)
                    .ok_or(Error("missing conversation filler reel"))?;
                let mut animation = Animation::new(resources, reel, 1)?;
                animation.object.x = position[0];
                animation.object.y = position[1];
                Ok(animation)
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(Self { layout, parts })
    }
    /// Original translucent backing precedes the frame sprites; title and icons
    /// are composed by the caller above the frame.
    pub fn draw(&self, resources: &Resources<'_>, surface: &mut Surface) -> Result<(), Error> {
        surface.draw_backdrop(self.layout.background)?;
        for part in &self.parts {
            surface.draw_animation(resources, part)?;
        }
        Ok(())
    }
}

/// Initial visible page of conversation topics. Scrolling and window lifecycle
/// belong to the inventory controller; rebuilding starts each icon script anew.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Content {
    pub title: crate::text::Layout,
    pub icons: Vec<Topic>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Topic {
    pub slot: usize,
    pub id: i32,
    pub animation: Animation,
}
impl Content {
    pub fn new(
        resources: &Resources<'_>,
        window: &Window,
        layout: &Layout,
        font: &crate::text::Font,
        strings: &crate::text::Strings<'_>,
        objects: &std::collections::BTreeMap<i32, crate::inventory::Object>,
        held: i32,
    ) -> Result<Self, Error> {
        let title = font.centered(resources, strings.get(window.title.0)?, layout.title)?;
        let mut icons = Vec::new();
        for (slot, (&id, position)) in window
            .contents
            .iter()
            .skip(window.first_display)
            .zip(&layout.icons)
            .enumerate()
        {
            // Holding an item leaves a hole; later topics retain their slots.
            if id == held {
                continue;
            }
            let object = objects
                .get(&id)
                .ok_or(Error("unknown conversation topic"))?;
            let film = Film::read(resources, object.film)?;
            let reel = film
                .reels
                .first()
                .ok_or(Error("empty conversation icon film"))?;
            let mut animation = Animation::new(resources, reel, 24 / film.rate as u32)?;
            animation.object.x = position[0];
            animation.object.y = position[1];
            icons.push(Topic {
                slot,
                id,
                animation,
            });
        }
        Ok(Self { title, icons })
    }
    /// Advance all visible icon scripts atomically, once per inventory redraw.
    pub fn tick(&mut self, resources: &Resources<'_>) -> Result<(), Error> {
        let mut next = self.clone();
        for topic in &mut next.icons {
            topic.animation.tick(resources)?;
        }
        *self = next;
        Ok(())
    }
    pub fn draw(&self, resources: &Resources<'_>, surface: &mut Surface) -> Result<(), Error> {
        self.title.draw(resources, surface)?;
        for topic in &self.icons {
            surface.draw_animation(resources, &topic.animation)?;
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn window(size: [i32; 2]) -> Window {
        Window {
            title: Handle(0),
            capacity: 10,
            min: [1, 1],
            size,
            max: [10, 6],
            position: [20, 8],
            first_display: 0,
            contents: vec![],
        }
    }
    #[test]
    fn configuration_frame_has_no_title_separator_scrollbar_or_icon_cells() {
        let mut w = window([6, 5]);
        w.position = [72, 23];
        let layout = Layout::configuration(&w, [8, 16], 12, 8).unwrap();
        assert_eq!(
            layout.parts,
            vec![
                (21, [72, 23]),
                (22, [210, 23]),
                (17, [72, 143]),
                (23, [210, 143]),
                (10, [80, 23]),
                (10, [80, 147]),
                (4, [72, 39]),
                (4, [218, 39])
            ]
        );
        assert!(layout.icons.is_empty());
        assert_eq!(layout.background, [73, 24, 147, 125]);
    }

    #[test]
    fn corners_fillers_title_and_icon_grid_use_v1_offsets() {
        let one = Layout::new(&window([1, 1]), [8, 16], 12, 8).unwrap();
        assert_eq!(
            one.parts,
            [(21, [20, 8]), (22, [28, 8]), (17, [20, 24]), (23, [28, 24])]
        );
        assert_eq!(one.icons, [[26, 28]]);
        let many = Layout::new(&window([3, 2]), [8, 16], 12, 8).unwrap();
        assert_eq!(
            many.parts,
            [
                (21, [20, 8]),
                (22, [80, 8]),
                (17, [20, 50]),
                (23, [80, 50]),
                (7, [28, 8]),
                (7, [28, 23]),
                (6, [26, 23]),
                (7, [62, 23]),
                (7, [28, 54]),
                (1, [20, 24]),
                (1, [88, 24])
            ]
        );
        assert_eq!(many.background, [21, 9, 69, 47]);
        assert_eq!(many.title, [54, 12]);
        assert_eq!(
            many.icons,
            [[26, 28], [52, 28], [78, 28], [26, 54], [52, 54], [78, 54]]
        );
    }
    #[test]
    fn regular_inventory_uses_its_corners_and_scrollbar_divider() {
        let layout = Layout::inventory(&window([3, 2]), [8, 16], 40, 8).unwrap();
        assert_eq!(
            &layout.parts[..4],
            &[(15, [20, 8]), (16, [80, 8]), (17, [20, 50]), (18, [80, 50])]
        );
        assert_eq!(
            &layout.parts[4..],
            &[
                (7, [28, 8]),
                (7, [28, 23]),
                (7, [28, 54]),
                (1, [20, 24]),
                (1, [107, 24]),
                (1, [116, 24])
            ]
        );
        assert_eq!(layout.icons.len(), 6);
        assert_eq!(layout.background, [21, 9, 97, 47]);
        assert_eq!(layout.title, [68, 12]);
    }
    #[test]
    fn scrollbar_uses_original_sprite_and_tracks_visible_rows() {
        let mut w = window([3, 2]);
        w.contents = (0..12).collect();
        let top = Layout::inventory(&w, [8, 24], 40, 8).unwrap();
        let bar = top.scrollbar.as_ref().unwrap();
        assert_eq!(bar.thumb[1], w.position[1] + 22);
        assert_eq!(top.parts.last(), Some(&(0, bar.thumb)));
        assert_eq!(
            bar.action_at([bar.right - 5, bar.top + 20]),
            Some(crate::inventory::Scroll::Up)
        );
        assert_eq!(
            bar.action_at([bar.right - 5, bar.bottom - 4]),
            Some(crate::inventory::Scroll::Down)
        );
        w.first_display = 3;
        let middle = Layout::inventory(&w, [8, 24], 40, 8).unwrap();
        let middle = middle.scrollbar.unwrap();
        assert_eq!(middle.thumb[1], (middle.range[0] + middle.range[1]) / 2);
        w.first_display = 6;
        let bottom = Layout::inventory(&w, [8, 24], 40, 8)
            .unwrap()
            .scrollbar
            .unwrap();
        assert_eq!(bottom.thumb[1], bottom.range[1]);
        assert_eq!(
            bottom.action_at([bottom.right - 5, bottom.range[0] + 6]),
            Some(crate::inventory::Scroll::PageUp)
        );
    }
    #[test]
    fn dimensions_and_coordinates_are_bounded() {
        assert!(Layout::new(&window([0, 1]), [8, 16], 12, 8).is_err());
        assert!(Layout::new(&window([11, 1]), [8, 16], 12, 8).is_err());
        assert!(Layout::new(&window([10, 7]), [8, 16], 12, 8).is_err());
        let mut w = window([10, 6]);
        assert_eq!(Layout::new(&w, [8, 16], 12, 8).unwrap().icons.len(), 60);
        w.position = [i32::MAX, 0];
        assert!(Layout::new(&w, [8, 16], 12, 8).is_err());
    }
    #[test]
    fn topic_slots_initial_frames_and_atomic_animation() {
        let mut data = vec![0u8; 256];
        for (at, value) in [
            (0, 12u32),
            (4, 1),
            (8, 32),
            (12, 80),
            (32, 64),
            (80, 96),
            (96, 128),
            (252, 1),
        ] {
            data[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        data[128..132].copy_from_slice(&[4, 0, 4, 0]);
        let resources = Resources(vec![&data]);
        let strings = crate::text::Strings::new(&[1, 0, 52, 51, 0, 0, 0, 0, 0]).unwrap();
        let font = crate::text::Font {
            x_spacing: 0,
            y_spacing: 0,
            space: 2,
            flags: 0,
            glyphs: [Handle(128); 256],
        };
        let mut w = window([3, 1]);
        w.contents = vec![10, 11, 12, 13];
        let objects = [10, 11, 12, 13]
            .into_iter()
            .map(|id| {
                (
                    id,
                    crate::inventory::Object {
                        id,
                        film: Handle(0),
                        script: Handle(0),
                        attributes: 0,
                    },
                )
            })
            .collect();
        let layout = Layout::new(&w, [8, 16], 12, 8).unwrap();
        let mut content =
            Content::new(&resources, &w, &layout, &font, &strings, &objects, 11).unwrap();
        assert_eq!(
            content
                .icons
                .iter()
                .map(|t| (t.slot, t.id, t.animation.object.x))
                .collect::<Vec<_>>(),
            [(0, 10, 26), (2, 12, 78)]
        );
        assert!(
            content
                .icons
                .iter()
                .all(|t| t.animation.frame == Handle(64))
        );
        let mut restored = content.clone();
        content.tick(&resources).unwrap();
        restored.tick(&resources).unwrap();
        assert_eq!(content, restored);
        assert!(
            content
                .icons
                .iter()
                .all(|t| t.animation.frame == Handle(96))
        );

        w.first_display = 1;
        let scrolled =
            Content::new(&resources, &w, &layout, &font, &strings, &objects, 12).unwrap();
        assert_eq!(
            scrolled
                .icons
                .iter()
                .map(|i| (i.slot, i.id, i.animation.object.x))
                .collect::<Vec<_>>(),
            [(0, 11, 26), (2, 13, 78)]
        );
        w.first_display = 0;
        let mut malformed = content.clone();
        malformed.icons[1].animation = Animation::new(
            &resources,
            &film::Reel {
                object: Handle(32),
                script: Handle(252),
            },
            1,
        )
        .unwrap();
        let before = malformed.clone();
        assert!(malformed.tick(&resources).is_err());
        assert_eq!(malformed, before);
        w.contents[0] = 999;
        assert!(Content::new(&resources, &w, &layout, &font, &strings, &objects, -1).is_err());
    }
}
