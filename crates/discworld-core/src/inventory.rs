// SPDX-License-Identifier: GPL-3.0-or-later
//! Checked DOS V1 inventory definitions from inv_objects.cpp and scn.h.
use crate::{
    film::word,
    graphics::{Handle, Resources},
    iso::Error,
    resource::chunks,
};
use std::collections::BTreeMap;

/// Resolved original inventory navigation, shared by all hosts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scroll {
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
}

/// dialogs.cpp adjustTop: move up by rows when deletion leaves spare rows.
pub fn adjust_top(window: &mut crate::startup::Window) {
    let cols = window.size[0].max(1) as usize;
    let rows = window.size[1].max(1) as usize;
    while window.first_display > 0
        && window
            .contents
            .len()
            .saturating_sub(window.first_display)
            .div_ceil(cols)
            < rows
    {
        window.first_display = window.first_display.saturating_sub(cols);
    }
}
pub fn scroll(window: &mut crate::startup::Window, action: Scroll) {
    let cols = window.size[0].max(1) as usize;
    let rows = window.size[1].max(1) as usize;
    let page = cols * rows;
    match action {
        Scroll::Home => window.first_display = 0,
        Scroll::End => window.first_display = window.contents.len().saturating_sub(page),
        Scroll::Up | Scroll::PageUp => {
            let count = if action == Scroll::PageUp {
                rows.saturating_sub(1).max(1)
            } else {
                1
            };
            window.first_display = window.first_display.saturating_sub(cols * count);
        }
        Scroll::Down | Scroll::PageDown => {
            let count = if action == Scroll::PageDown {
                rows.saturating_sub(1).max(1)
            } else {
                1
            };
            for _ in 0..count {
                if window.first_display + page < window.contents.len() {
                    window.first_display += cols;
                }
            }
        }
    }
}

/// ADDINV1/2 move an item between inventories, deduplicate, and append. The
/// declared display/capacity is not the backing array limit used by this call.
pub fn add(
    windows: &mut [Option<crate::startup::Window>; 3],
    slot: usize,
    item: i32,
) -> Result<(), Error> {
    if !matches!(slot, 1 | 2) {
        return Err(Error("invalid inventory slot"));
    }
    let mut next = windows.clone();
    let destination = next[slot]
        .as_mut()
        .ok_or(Error("missing inventory declaration"))?;
    if !destination.contents.contains(&item) {
        if destination.contents.len() >= 150 {
            return Err(Error("inventory contents limit"));
        }
        destination.contents.push(item);
    }
    if let Some(other) = &mut next[3 - slot] {
        if let Some(index) = other.contents.iter().position(|id| *id == item) {
            other.contents.remove(index);
        }
    }
    *windows = next;
    Ok(())
}
/// ADDOPENINV inserts at the last item-event index, clamped after deletions.
pub fn add_open(
    windows: &mut [Option<crate::startup::Window>; 3],
    slot: usize,
    item: i32,
    index: usize,
) -> Result<(), Error> {
    let existed = windows
        .get(slot)
        .and_then(Option::as_ref)
        .is_some_and(|w| w.contents.contains(&item));
    add(windows, slot, item)?;
    if !existed {
        let contents = &mut windows[slot].as_mut().unwrap().contents;
        contents.pop();
        contents.insert(index.min(contents.len()), item);
    }
    Ok(())
}

/// Resolved ordinary put-down. Capacity applies to new entries, but an item
/// already present can be reordered even when the inventory is full.
pub fn put_down(
    windows: &mut [Option<crate::startup::Window>; 3],
    slot: usize,
    item: i32,
    index: usize,
) -> Result<bool, Error> {
    if !matches!(slot, 1 | 2) {
        return Err(Error("invalid inventory slot"));
    }
    let window = windows[slot]
        .as_ref()
        .ok_or(Error("missing inventory declaration"))?;
    let old = window.contents.iter().position(|id| *id == item);
    if old.is_none() && window.contents.len() >= window.capacity as usize {
        return Ok(false);
    }
    let mut next = windows.clone();
    add(&mut next, slot, item)?;
    let contents = &mut next[slot].as_mut().unwrap().contents;
    let old = contents.iter().position(|id| *id == item).unwrap();
    contents.remove(old);
    contents.insert(index.min(contents.len()), item);
    *windows = next;
    Ok(true)
}

/// V1 invSetSize changes the stored geometry, without rebuilding an open
/// display or altering item capacity. Start sizes are capped only at max.
pub fn set_size(
    window: &mut crate::startup::Window,
    min: [i32; 2],
    start: [i32; 2],
    max: [i32; 2],
) {
    window.min = min;
    window.max = max;
    window.size = [start[0].min(max[0]), start[1].min(max[1])];
}

pub fn set_limit(window: &mut crate::startup::Window, capacity: i32) -> Result<(), Error> {
    if capacity < 0 || (capacity as usize) < window.contents.len() {
        return Err(Error("inventory limit below current contents"));
    }
    window.capacity = capacity.min(150);
    Ok(())
}

/// V1 membership includes an item held outside both inventories. This is
/// separate from WHICHINVENTORY's slot/held precedence.
pub fn contains(windows: &[Option<crate::startup::Window>; 3], held: i32, item: i32) -> bool {
    held == item
        || windows[1..]
            .iter()
            .flatten()
            .any(|w| w.contents.contains(&item))
}

/// DELINV removes from inventory 1 first, otherwise inventory 2. Dropping the
/// held cursor is the engine's responsibility and must not store the old item.
pub fn delete(windows: &mut [Option<crate::startup::Window>; 3], item: i32) {
    for window in windows[1..].iter_mut().flatten() {
        if let Some(index) = window.contents.iter().position(|id| *id == item) {
            window.contents.remove(index);
            return;
        }
    }
}
pub fn which(windows: &[Option<crate::startup::Window>; 3], held: i32, item: i32) -> i32 {
    if held == item {
        return 0;
    }
    for (slot, window) in windows.iter().enumerate().skip(1) {
        if window.as_ref().is_some_and(|w| w.contents.contains(&item)) {
            return slot as i32;
        }
    }
    -1
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object {
    pub id: i32,
    pub film: Handle,
    pub script: Handle,
    pub attributes: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldCursor {
    pub animation: crate::animation::Animation,
    pub offset: [i32; 2],
}
impl HeldCursor {
    pub fn new(resources: &Resources<'_>, film: Handle) -> Result<Self, Error> {
        let film = crate::film::Film::read(resources, film)?;
        let reel = film.reels.first().ok_or(Error("empty held cursor film"))?;
        let animation = crate::animation::Animation::new(resources, reel, 24 / film.rate as u32)?;
        let shape = crate::film::frame(resources, animation.object.frame)?;
        let image = crate::graphics::Image::read(
            resources,
            *shape.first().ok_or(Error("empty held cursor shape"))?,
        )?;
        Ok(Self {
            animation,
            offset: [
                (i32::from(image.width) / 2 - i32::from(image.animation_x)) as i16 as i32,
                (i32::from(image.height) / 2 - i32::from(image.animation_y)) as i16 as i32,
            ],
        })
    }
    pub fn draw(
        &self,
        resources: &Resources<'_>,
        surface: &mut crate::compositor::Surface,
        cursor: [i32; 2],
    ) -> Result<(), Error> {
        let mut animation = self.animation.clone();
        animation.object.x = cursor[0]
            .checked_sub(self.offset[0])
            .ok_or(Error("held cursor x overflow"))?;
        animation.object.y = cursor[1]
            .checked_sub(self.offset[1])
            .ok_or(Error("held cursor y overflow"))?;
        surface.draw_animation(resources, &animation)
    }
}

pub fn read(resources: &Resources<'_>) -> Result<BTreeMap<i32, Object>, Error> {
    let Some(file) = resources.0.get(1) else {
        return Ok(BTreeMap::new());
    };
    let headers = chunks(file)?;
    let count = headers
        .iter()
        .find(|c| c.kind == 0x33340011)
        .map(|c| word(&file[c.payload.clone()], 0))
        .transpose()?
        .unwrap_or(0) as usize;
    if count > 65536 {
        return Err(Error("inventory object allocation limit"));
    }
    if count == 0 {
        return Ok(BTreeMap::new());
    }
    let table = headers
        .iter()
        .find(|c| c.kind == 0x33340012)
        .ok_or(Error("missing inventory object table"))?;
    let bytes = file[table.payload.clone()]
        .get(..count * 16)
        .ok_or(Error("truncated inventory object table"))?;
    let mut objects = BTreeMap::new();
    for record in bytes.chunks_exact(16) {
        let id = word(record, 0)? as i32;
        let object = Object {
            id,
            film: Handle(word(record, 4)?),
            script: Handle(word(record, 8)?),
            attributes: word(record, 12)?,
        };
        if id == -1 {
            return Err(Error("inventory object uses empty-hand sentinel"));
        }
        if objects.insert(id, object).is_some() {
            return Err(Error("duplicate inventory object ID"));
        }
    }
    Ok(objects)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scroll_navigation_preserves_order_and_non_aligned_end_then_adjusts_after_deletion() {
        let mut w = crate::startup::Window {
            title: Handle(0),
            capacity: 150,
            min: [1, 1],
            size: [3, 2],
            max: [10, 6],
            position: [0, 0],
            first_display: 0,
            contents: (0..13).collect(),
        };
        scroll(&mut w, Scroll::Down);
        assert_eq!(w.first_display, 3);
        scroll(&mut w, Scroll::PageDown);
        assert_eq!(w.first_display, 6);
        scroll(&mut w, Scroll::Down);
        assert_eq!(w.first_display, 9);
        scroll(&mut w, Scroll::Down);
        assert_eq!(w.first_display, 9);
        scroll(&mut w, Scroll::End);
        assert_eq!(w.first_display, 7);
        scroll(&mut w, Scroll::Up);
        assert_eq!(w.first_display, 4);
        scroll(&mut w, Scroll::Home);
        assert_eq!(w.first_display, 0);
        assert_eq!(w.contents, (0..13).collect::<Vec<_>>());
        scroll(&mut w, Scroll::End);
        w.contents.truncate(8);
        adjust_top(&mut w);
        assert_eq!(w.first_display, 4);
        w.contents.clear();
        adjust_top(&mut w);
        assert_eq!(w.first_display, 0);
        w.size = [3, 1];
        w.contents = (0..7).collect();
        scroll(&mut w, Scroll::PageDown);
        assert_eq!(w.first_display, 3);
        scroll(&mut w, Scroll::PageUp);
        assert_eq!(w.first_display, 0);
    }
    #[test]
    fn sizing_caps_at_max_without_raising_to_min_or_changing_contents() {
        let mut window = crate::startup::Window {
            title: Handle(7),
            capacity: 150,
            min: [1, 1],
            size: [3, 2],
            max: [10, 6],
            position: [40, 50],
            first_display: 0,
            contents: vec![198, 314],
        };
        set_size(&mut window, [2, 2], [9, 1], [4, 3]);
        assert_eq!(window.min, [2, 2]);
        assert_eq!(window.size, [4, 1]);
        assert_eq!(window.max, [4, 3]);
        assert_eq!(window.capacity, 150);
        assert_eq!(window.contents, [198, 314]);
        assert_eq!(window.position, [40, 50]);
        assert_eq!(window.title, Handle(7));
        set_size(&mut window, [2, 2], [2, 2], [2, 2]);
        assert_eq!(window.size, [2, 2]);
    }

    #[test]
    fn ordered_insertion_and_capacity_checked_put_down() {
        let window = crate::startup::Window {
            title: Handle(0),
            capacity: 3,
            min: [1, 1],
            size: [3, 1],
            max: [10, 6],
            position: [0; 2],
            first_display: 0,
            contents: vec![10, 11],
        };
        let mut windows = [None, Some(window.clone()), Some(window)];
        add_open(&mut windows, 1, 12, 1).unwrap();
        assert_eq!(windows[1].as_ref().unwrap().contents, [10, 12, 11]);
        let before = windows.clone();
        assert!(!put_down(&mut windows, 1, 13, 0).unwrap());
        assert_eq!(windows, before);
        assert!(put_down(&mut windows, 1, 10, 99).unwrap());
        assert_eq!(windows[1].as_ref().unwrap().contents, [12, 11, 10]);
        assert!(!windows[2].as_ref().unwrap().contents.contains(&10));
        add_open(&mut windows, 1, 12, 0).unwrap();
        assert_eq!(windows[1].as_ref().unwrap().contents, [12, 11, 10]);
        // Scripted ADDOPENINV uses the backing limit, not ordinary drop capacity.
        add_open(&mut windows, 1, 13, 99).unwrap();
        assert_eq!(windows[1].as_ref().unwrap().contents, [12, 11, 10, 13]);
    }

    #[test]
    fn transfer_deduplication_held_priority_deletion_and_capacity() {
        let window = crate::startup::Window {
            title: Handle(0),
            capacity: 1,
            min: [1, 1],
            size: [1, 1],
            max: [10, 6],
            position: [0; 2],
            first_display: 0,
            contents: vec![],
        };
        let mut windows = [None, Some(window.clone()), Some(window)];
        add(&mut windows, 1, 10).unwrap();
        add(&mut windows, 1, 11).unwrap();
        add(&mut windows, 1, 10).unwrap();
        assert_eq!(windows[1].as_ref().unwrap().contents, [10, 11]);
        add(&mut windows, 2, 10).unwrap();
        assert_eq!(windows[1].as_ref().unwrap().contents, [11]);
        assert_eq!(which(&windows, -1, 10), 2);
        assert_eq!(which(&windows, 10, 10), 0);
        assert_eq!(which(&windows, -1, 99), -1);
        delete(&mut windows, 10);
        assert_eq!(which(&windows, -1, 10), -1);
        windows[2].as_mut().unwrap().contents = (100..250).collect();
        let before = windows.clone();
        assert!(add(&mut windows, 2, 11).is_err());
        assert_eq!(windows, before);
        add(&mut windows, 2, 100).unwrap(); // Already present even when full.
        assert_eq!(windows, before);
        assert!(add(&mut windows, 0, 11).is_err());
    }
    fn fixture(count: u32, ids: &[i32]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for word in [0x33340011u32, 12, count, 0x33340012, 0] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        for &id in ids {
            for word in [id as u32, 123, 456, 0x80000001] {
                bytes.extend_from_slice(&word.to_le_bytes());
            }
        }
        bytes
    }
    #[test]
    fn object_table_preserves_ids_handles_and_attribute_bits() {
        let bytes = fixture(2, &[284, 42]);
        let objects = read(&Resources(vec![&[], &bytes])).unwrap();
        assert_eq!(
            objects[&284],
            Object {
                id: 284,
                film: Handle(123),
                script: Handle(456),
                attributes: 0x80000001
            }
        );
        assert_eq!(objects.len(), 2);
        assert!(read(&Resources(vec![])).unwrap().is_empty());
    }
    #[test]
    fn malformed_object_tables_are_rejected() {
        for bytes in [
            fixture(2, &[1]),
            fixture(2, &[1, 1]),
            fixture(1, &[-1]),
            fixture(65537, &[]),
        ] {
            assert!(read(&Resources(vec![&[], &bytes])).is_err());
        }
    }
}
