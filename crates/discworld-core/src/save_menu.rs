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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct List {
    mode: Mode,
    entries: Vec<Entry>,
    first: usize,
    selected: usize,
    draft: Vec<u8>,
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
        self.reset_draft();
        true
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
