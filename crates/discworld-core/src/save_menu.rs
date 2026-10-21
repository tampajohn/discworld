// SPDX-License-Identifier: GPL-3.0-or-later
//! V1 save/load list selection and description editing. Storage supplies a
//! newest-first catalog of validated snapshots with stable IDs; no I/O here.
use crate::{
    graphics::{Image, Resources},
    iso::Error,
    text::{Font, Layout},
};

pub const ROWS: usize = 9;
pub const CAPACITY: usize = 100;
pub const DESCRIPTION_LIMIT: usize = 40;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Save,
    Load,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: u32,
    pub description: Vec<u8>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    New,
    Existing(u32),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scroll {
    RowDown,
    RowUp,
    PageDown,
    PageUp,
    Home,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliderHit {
    Up,
    Down,
    PageUp,
    PageDown,
    Thumb,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct List {
    mode: Mode,
    entries: Vec<Entry>,
    first: usize,
    selected: usize,
    draft: Vec<u8>,
    slider_raw: Option<i64>,
    slider_y: i32,
    slider_drawn_y: i32,
}

/// Original save/load window presentation. Rebuilt row labels reflect the
/// catalog/draft, while the host retains snapshot bytes outside this component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct View {
    mode: Mode,
    frame: crate::conversation_window::Frame,
    extra: Vec<crate::animation::Animation>,
    thumb: crate::animation::Animation,
    buttons: Vec<crate::animation::Animation>,
    font: Font,
    heading: Layout,
}
impl View {
    pub fn new(
        resources: &Resources<'_>,
        strings: &crate::text::Strings<'_>,
        declarations: &crate::startup::Declarations,
        mode: Mode,
    ) -> Result<Self, Error> {
        use crate::{animation::Animation, film::Film, graphics::Handle, startup::Window};
        let handle = declarations
            .window_parts
            .ok_or(Error("missing window parts"))?;
        let window = Window {
            title: Handle(0),
            capacity: 0,
            min: [10, 6],
            size: [10, 6],
            max: [10, 6],
            position: [20, 16],
            first_display: 0,
            contents: vec![],
        };
        let frame = crate::conversation_window::Frame::configuration(resources, handle, &window)?;
        let film = Film::read(resources, handle)?;
        let image = |reel: usize, [x, y]: [i32; 2]| -> Result<Animation, Error> {
            let mut a = Animation::new(
                resources,
                film.reels
                    .get(reel)
                    .ok_or(Error("missing save menu reel"))?,
                1,
            )?;
            a.object.x = x;
            a.object.y = y;
            Ok(a)
        };
        let extra = [
            (21, [40, 40]),
            (49, [192, 40]),
            (17, [40, 164]),
            (18, [192, 164]),
            (11, [46, 40]),
            (11, [46, 183]),
            (4, [40, 60]),
            (4, [219, 60]),
            (4, [228, 60]),
        ]
        .into_iter()
        .map(|(reel, point)| image(reel, point))
        .collect::<Result<Vec<_>, _>>()?;
        let thumb = image(0, [223, 49])?;
        let buttons = [46, 47, 48, 40, 41, 42]
            .into_iter()
            .enumerate()
            .map(|(i, reel)| image(reel, [250, if i < 3 { 60 } else { 107 }]))
            .collect::<Result<Vec<_>, _>>()?;
        let font = Font::read(
            resources,
            declarations.tag_font.ok_or(Error("missing tag font"))?,
        )?;
        let handles = declarations
            .config_strings
            .ok_or(Error("missing configuration strings"))?;
        let heading = font.centered(
            resources,
            strings.get(handles[if mode == Mode::Load { 8 } else { 9 }].0)?,
            frame.layout.title,
        )?;
        Ok(Self {
            mode,
            frame,
            extra,
            thumb,
            buttons,
            font,
            heading,
        })
    }
    pub fn button_at([x, y]: [i32; 2]) -> Option<crate::configuration_menu::Choice> {
        if x <= 250 || x >= 273 {
            return None;
        }
        if y > 60 && y < 79 {
            Some(crate::configuration_menu::Choice::Accept)
        } else if y > 107 && y < 126 {
            Some(crate::configuration_menu::Choice::Cancel)
        } else {
            None
        }
    }
    pub fn draw(
        &self,
        resources: &Resources<'_>,
        surface: &mut crate::compositor::Surface,
        list: &List,
        pointer: Option<[i32; 2]>,
        effect: Option<(
            crate::configuration_menu::Choice,
            crate::configuration_menu::ButtonImage,
        )>,
    ) -> Result<(), Error> {
        if self.mode != list.mode {
            return Err(Error("save menu view mode mismatch"));
        }
        self.frame.draw(resources, surface)?;
        for a in &self.extra {
            surface.draw_animation(resources, a)?;
        }
        let mut thumb = self.thumb.clone();
        thumb.object.y = list.slider_drawn_y;
        surface.draw_animation(resources, &thumb)?;
        self.heading.draw(resources, surface)?;
        for row in 0..ROWS {
            let y = 47 + row as i32 * 15;
            surface.draw_palette_rect(
                [48, y, 166, 13],
                if row == list.selected { 231 } else { 228 },
            )?;
            if row == list.selected && list.mode == Mode::Save {
                list.edit_layout(resources, &self.font)?
                    .draw(resources, surface)?;
            } else if let Some(entry) = list.row(row) {
                self.font
                    .left_aligned(resources, &entry.description, [50, y])?
                    .draw(resources, surface)?;
            }
        }
        surface.draw_animation(resources, &self.buttons[0])?;
        surface.draw_animation(resources, &self.buttons[3])?;
        use crate::configuration_menu::{ButtonImage, Choice};
        if let Some((choice, state)) = effect.or_else(|| {
            pointer
                .and_then(Self::button_at)
                .map(|c| (c, ButtonImage::Highlight))
        }) {
            let base = if choice == Choice::Accept { 0 } else { 3 };
            let offset = match state {
                ButtonImage::Normal => 0,
                ButtonImage::Down => 1,
                ButtonImage::Highlight => 2,
            };
            surface.draw_animation(resources, &self.buttons[base + offset])?;
        }
        Ok(())
    }
}
impl List {
    pub fn new(mode: Mode, entries: Vec<Entry>) -> Result<Self, Error> {
        let mut ids = std::collections::BTreeSet::new();
        if entries.len() > CAPACITY
            || entries.iter().any(|entry| {
                !ids.insert(entry.id)
                    || entry.description.len() > DESCRIPTION_LIMIT
                    || entry.description.contains(&0)
            })
        {
            return Err(Error("invalid save menu catalog"));
        }
        let mut result = Self {
            mode,
            entries,
            first: 0,
            selected: 0,
            draft: vec![],
            slider_raw: None,
            slider_y: 49,
            slider_drawn_y: 49,
        };
        result.reset_draft();
        Ok(result)
    }
    pub fn first(&self) -> usize {
        self.first
    }
    pub fn selected(&self) -> usize {
        self.selected
    }
    pub fn draft(&self) -> &[u8] {
        &self.draft
    }
    fn modifier(&self) -> usize {
        usize::from(self.mode == Mode::Save && self.first == 0 && self.entries.len() < CAPACITY)
    }
    pub fn row(&self, row: usize) -> Option<&Entry> {
        if row >= ROWS {
            return None;
        }
        let index = (self.first + row).checked_sub(self.modifier())?;
        self.entries.get(index)
    }
    fn reset_draft(&mut self) {
        self.draft = self
            .row(self.selected)
            .map_or_else(Vec::new, |entry| entry.description.clone());
    }
    pub fn select(&mut self, row: usize) -> Result<bool, Error> {
        if row >= ROWS {
            return Err(Error("save menu selection outside rows"));
        }
        if self.selected == row {
            return Ok(false);
        }
        self.selected = row;
        self.reset_draft();
        Ok(true)
    }
    pub fn target(&self) -> Option<Target> {
        self.row(self.selected)
            .map(|entry| Target::Existing(entry.id))
            .or(if self.mode == Mode::Save {
                Some(Target::New)
            } else {
                None
            })
    }
    /// Scroll limits use capacity, not the number of existing saves. Wheel
    /// movement adjusts the selected row; page movement selects an edge row.
    pub fn scroll(&mut self, action: Scroll) -> bool {
        let last = CAPACITY - ROWS;
        let old = self.first;
        let row = match action {
            Scroll::RowDown if old < last => {
                self.first = old + 1;
                self.selected.saturating_sub(1)
            }
            Scroll::RowUp if old > 0 => {
                self.first = old - 1;
                (self.selected + 1).min(ROWS - 1)
            }
            Scroll::PageDown if old < last => {
                self.first = (old + ROWS - 1).min(last);
                ROWS - 1
            }
            Scroll::PageUp if old > 0 => {
                self.first = old.saturating_sub(ROWS - 1);
                0
            }
            Scroll::Home => {
                self.first = 0;
                0
            }
            Scroll::End => {
                self.first = last;
                0
            }
            _ => return false,
        };
        self.selected = row;
        self.slider_y = 49 + (self.first * 125 / (CAPACITY - ROWS)) as i32;
        self.slider_drawn_y = self.slider_y;
        self.reset_draft();
        true
    }
    pub fn slider_at(&self, [x, y]: [i32; 2]) -> Option<SliderHit> {
        if !(221..229).contains(&x) || !(42..184).contains(&y) {
            return None;
        }
        Some(if y < 47 {
            SliderHit::Up
        } else if y > 179 {
            SliderHit::Down
        } else if y < self.slider_y {
            SliderHit::PageUp
        } else if y >= self.slider_y + 5 {
            SliderHit::PageDown
        } else {
            SliderHit::Thumb
        })
    }
    pub fn begin_slider(&mut self) -> bool {
        if self.slider_raw.is_some() {
            return false;
        }
        self.slider_raw = Some(i64::from(self.slider_y));
        true
    }
    /// V1 moves the actual thumb only when extraBase changes (addBoxes(false)),
    /// retaining logical/raw positions between redraws. Release does not snap.
    pub fn drag_slider(&mut self, dy: i32) -> Result<bool, Error> {
        let Some(raw) = self.slider_raw else {
            return Ok(false);
        };
        let raw = raw
            .checked_add(i64::from(dy))
            .ok_or(Error("save slider overflow"))?;
        let y = raw.clamp(49, 174) as i32;
        let first = ((y - 49) as usize * (CAPACITY - ROWS)) / 125;
        self.slider_raw = Some(raw);
        self.slider_y = y;
        if first == self.first {
            return Ok(false);
        }
        self.selected = (self.selected as i64 + self.first as i64 - first as i64)
            .clamp(0, (ROWS - 1) as i64) as usize;
        self.first = first;
        self.slider_drawn_y = y;
        self.reset_draft();
        Ok(true)
    }
    pub fn end_slider(&mut self) {
        self.slider_raw = None;
    }
    pub fn row_at([x, y]: [i32; 2]) -> Option<usize> {
        if !(48..214).contains(&x) || !(47..180).contains(&y) {
            return None;
        }
        let dy = y - 47;
        (dy % 15 < 13).then_some((dy / 15) as usize)
    }
    pub fn edit_layout(&self, resources: &Resources<'_>, font: &Font) -> Result<Layout, Error> {
        let mut text = self.draft.clone();
        text.push(b'_');
        font.left_aligned(resources, &text, [50, 47 + self.selected as i32 * 15])
    }
    /// Input is an original-font byte. Keyboard mapping/IME remain host work.
    /// Backspace is explicit; unsupported glyphs and leading space are ignored.
    pub fn edit(
        &mut self,
        resources: &Resources<'_>,
        font: &Font,
        byte: u8,
        backspace: bool,
    ) -> Result<bool, Error> {
        if self.mode != Mode::Save || byte == 0 {
            return Ok(false);
        }
        let mut next = self.clone();
        if backspace {
            if next.draft.pop().is_none() {
                return Ok(false);
            }
        } else {
            if next.draft.len() == DESCRIPTION_LIMIT
                || !(font.glyphs[usize::from(byte)].0 != 0
                    || (byte == b' ' && !next.draft.is_empty()))
            {
                return Ok(false);
            }
            next.draft.push(byte);
        }
        // InvKeyIn redraws then backspaces once when MultiRightmost exceeds213,
        // including after a backspace on an already oversized imported name.
        let layout = next.edit_layout(resources, font)?;
        let mut right = i64::MIN;
        for glyph in layout.glyphs {
            right = right.max(
                i64::from(glyph.x) + i64::from(Image::read(resources, glyph.image)?.width) - 1,
            );
        }
        if right > 213 {
            next.draft.pop();
        }
        let changed = self.draft != next.draft;
        *self = next;
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entries(count: usize) -> Vec<Entry> {
        (0..count)
            .map(|i| Entry {
                id: (1000 + i) as u32,
                description: format!("Save {i}").into_bytes(),
            })
            .collect()
    }
    #[test]
    fn scrollbar_retains_logical_position_between_redraws_and_release() {
        let mut list = List::new(Mode::Load, entries(100)).unwrap();
        assert_eq!(list.slider_at([224, 42]), Some(SliderHit::Up));
        assert_eq!(list.slider_at([224, 48]), Some(SliderHit::PageUp));
        assert_eq!(list.slider_at([224, 49]), Some(SliderHit::Thumb));
        assert_eq!(list.slider_at([224, 54]), Some(SliderHit::PageDown));
        assert_eq!(list.slider_at([224, 180]), Some(SliderHit::Down));
        assert_eq!(list.slider_at([229, 50]), None);
        assert!(list.begin_slider());
        assert!(!list.drag_slider(1).unwrap());
        assert_eq!((list.slider_y, list.slider_drawn_y), (50, 49));
        assert!(list.drag_slider(1).unwrap());
        assert_eq!((list.first, list.slider_drawn_y), (1, 51));
        list.end_slider();
        assert_eq!(list.slider_drawn_y, 51); // rebuilding from first would yield50
        assert!(list.begin_slider());
        assert!(list.drag_slider(500).unwrap());
        assert_eq!(list.first, 91);
        assert!(!list.drag_slider(-375).unwrap());
        assert!(list.drag_slider(-3).unwrap());
        assert_eq!((list.first, list.slider_drawn_y), (90, 173));
        list.end_slider();
        list.scroll(Scroll::Home);
        assert_eq!((list.slider_y, list.slider_drawn_y), (49, 49));
        assert!(!list.drag_slider(10).unwrap());
    }
    #[test]
    fn new_row_modifier_only_applies_to_save_first_page_with_capacity() {
        let mut list = List::new(Mode::Save, entries(12)).unwrap();
        assert_eq!(list.target(), Some(Target::New));
        list.select(1).unwrap();
        assert_eq!(list.target(), Some(Target::Existing(1000)));
        list.scroll(Scroll::RowDown);
        assert_eq!((list.first(), list.selected()), (1, 0));
        assert_eq!(list.target(), Some(Target::Existing(1001)));
        list.scroll(Scroll::End);
        assert_eq!((list.first(), list.selected()), (91, 0));
        assert_eq!(list.target(), Some(Target::New));
        let mut load = List::new(Mode::Load, entries(12)).unwrap();
        assert_eq!(load.target(), Some(Target::Existing(1000)));
        load.scroll(Scroll::End);
        assert_eq!(load.target(), None);
        let full = List::new(Mode::Save, entries(100)).unwrap();
        assert_eq!(full.target(), Some(Target::Existing(1000)));
    }
    #[test]
    fn page_edges_and_click_gaps_are_not_catalog_indices() {
        let mut list = List::new(Mode::Load, entries(100)).unwrap();
        assert!(!list.scroll(Scroll::PageUp));
        list.scroll(Scroll::PageDown);
        assert_eq!((list.first(), list.selected()), (8, 8));
        assert_eq!(list.target(), Some(Target::Existing(1016)));
        list.scroll(Scroll::RowUp);
        assert_eq!((list.first(), list.selected()), (7, 8));
        assert_eq!(List::row_at([48, 47]), Some(0));
        assert_eq!(List::row_at([213, 179]), Some(8));
        for point in [
            [214, 47],
            [48, 60],
            [48, 61],
            [48, 180],
            [i32::MIN, i32::MAX],
        ] {
            assert_eq!(List::row_at(point), None);
        }
        assert!(List::new(Mode::Load, vec![entries(1).remove(0); 2]).is_err());
    }
    #[test]
    fn editing_uses_glyphs_cursor_width_and_discards_drafts_on_selection() {
        use crate::graphics::Handle;
        let mut data = vec![0; 1120];
        data[0..4].copy_from_slice(&1u32.to_le_bytes());
        data[16..20].copy_from_slice(&2u32.to_le_bytes());
        for c in [b'W', b'A', b'_'] {
            let at = 44 + usize::from(c) * 4;
            data[at..at + 4].copy_from_slice(&1100u32.to_le_bytes());
        }
        data[1100..1108].copy_from_slice(&[10, 0, 6, 0, 0, 0, 0, 0]);
        let resources = Resources(vec![&data]);
        let font = Font::read(&resources, Handle(0)).unwrap();
        let mut list = List::new(
            Mode::Save,
            vec![Entry {
                id: 8,
                description: b"AA".to_vec(),
            }],
        )
        .unwrap();
        assert!(!list.edit(&resources, &font, b' ', false).unwrap());
        assert!(!list.edit(&resources, &font, b'B', false).unwrap());
        for _ in 0..14 {
            assert!(list.edit(&resources, &font, b'A', false).unwrap());
        }
        assert!(!list.edit(&resources, &font, b'A', false).unwrap());
        assert_eq!(list.draft().len(), 14);
        assert!(list.edit(&resources, &font, 8, true).unwrap());
        assert_eq!(list.draft().len(), 13);
        let layout = list.edit_layout(&resources, &font).unwrap();
        assert_eq!(layout.glyphs[0].x, 50);
        assert_eq!(layout.glyphs.last().unwrap().x, 193);
        assert!(!list.select(0).unwrap()); // no force: retained draft
        assert_eq!(list.draft().len(), 13);
        list.select(1).unwrap();
        assert_eq!(list.draft(), b"AA");
        list.select(0).unwrap();
        assert!(list.draft().is_empty());
        let mut load = List::new(Mode::Load, vec![]).unwrap();
        assert!(!load.edit(&resources, &font, b'A', false).unwrap());
        // With narrow glyphs the byte limit, rather than width, is decisive.
        data[1100] = 2;
        let resources = Resources(vec![&data]);
        let font = Font::read(&resources, Handle(0)).unwrap();
        let mut narrow = List::new(Mode::Save, vec![]).unwrap();
        for _ in 0..DESCRIPTION_LIMIT {
            assert!(narrow.edit(&resources, &font, b'A', false).unwrap());
        }
        assert!(!narrow.edit(&resources, &font, b'A', false).unwrap());
        assert_eq!(narrow.draft().len(), DESCRIPTION_LIMIT);
    }
}
