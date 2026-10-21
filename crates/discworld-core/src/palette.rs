// SPDX-License-Identifier: GPL-3.0-or-later
//! DOS V1 palette slot allocation and deferred hardware uploads (palette.cpp).
//! Object ownership, swaps, fading, and renderer invalidation remain callers' work.
use crate::{
    graphics::{Handle, Resources, read_palette},
    iso::Error,
};
/// Scene palette replacement for the direct-color compositor. Retain unwritten
/// DAC entries when a replacement is shorter, as V1 SwapPalette does.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct BackgroundPalette {
    pub handles: Vec<Handle>,
    pub colors: Vec<[u8; 3]>,
}
impl BackgroundPalette {
    pub fn new(resources: &Resources<'_>, handle: Handle) -> Result<Self, Error> {
        let colors = read_palette(resources, handle)?;
        if colors.len() > 255 {
            return Err(Error("background palette exceeds screen capacity"));
        }
        Ok(Self {
            handles: vec![handle],
            colors,
        })
    }
    pub fn replace(&mut self, resources: &Resources<'_>, handle: Handle) -> Result<(), Error> {
        let colors = read_palette(resources, handle)?;
        if colors.len() > 255 {
            return Err(Error("background palette exceeds screen capacity"));
        }
        self.colors
            .resize(self.colors.len().max(colors.len()), [0; 3]);
        self.colors[..colors.len()].copy_from_slice(&colors);
        if !self.handles.contains(&handle) {
            self.handles.push(handle);
        }
        Ok(())
    }
}

const MOVED: usize = 0x8000;

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Allocation {
    pub handle: Handle,
    pub references: u32,
    position: usize,
    pub colors: usize,
}
impl Allocation {
    pub fn position(&self) -> usize {
        self.position & !MOVED
    }
    pub fn moved(&self) -> bool {
        self.position & MOVED != 0
    }
}
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
struct Upload {
    position: usize,
    colors: usize,
    handle: Handle,
}
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Palettes {
    slots: [Option<Allocation>; 32],
    pending: Vec<Upload>,
    #[serde(with = "crate::save_array")]
    pub screen: [[u8; 3]; 256],
}
impl Default for Palettes {
    fn default() -> Self {
        Self {
            slots: std::array::from_fn(|_| None),
            pending: Vec::new(),
            screen: [[0; 3]; 256],
        }
    }
}
impl Palettes {
    pub fn allocation(&self, slot: usize) -> Result<&Allocation, Error> {
        self.slots
            .get(slot)
            .and_then(Option::as_ref)
            .ok_or(Error("inactive palette slot"))
    }
    /// A shared handle retains its slot and increments its object reference count.
    /// First-free-slot insertion shifts overlapping later allocations, in order.
    /// Failed allocations leave slots and queued uploads unchanged.
    pub fn acquire(&mut self, resources: &Resources<'_>, handle: Handle) -> Result<usize, Error> {
        if handle.0 == 0 {
            return Err(Error("null palette handle"));
        }
        let mut next = self.clone();
        if let Some(slot) = next
            .slots
            .iter()
            .position(|a| a.as_ref().is_some_and(|a| a.handle == handle))
        {
            let allocation = next.slots[slot].as_mut().unwrap();
            allocation.references = allocation
                .references
                .checked_add(1)
                .ok_or(Error("palette reference overflow"))?;
            *self = next;
            return Ok(slot);
        }
        let colors = read_palette(resources, handle)?.len();
        let slot = next
            .slots
            .iter()
            .position(Option::is_none)
            .ok_or(Error("palette slot capacity"))?;
        let position = if slot == 0 {
            1
        } else {
            let previous = next.slots[slot - 1].as_ref().unwrap();
            previous.position + previous.colors
        };
        next.slots[slot] = Some(Allocation {
            handle,
            references: 1,
            position,
            colors,
        });
        next.queue(slot)?;
        let mut end = position + colors;
        for index in slot + 1..32 {
            let Some(allocation) = &mut next.slots[index] else {
                continue;
            };
            // Preserve the reference's raw moved-bit comparisons until upload.
            if allocation.position >= end {
                break;
            }
            allocation.position = end | MOVED;
            end = allocation.position + allocation.colors;
            next.queue(index)?;
        }
        *self = next;
        Ok(slot)
    }
    fn queue(&mut self, slot: usize) -> Result<(), Error> {
        if self.pending.len() >= 34 {
            return Err(Error("palette upload queue capacity"));
        }
        let a = self.allocation(slot)?;
        if a.position() > 256 {
            return Err(Error("palette starts outside screen palette"));
        }
        self.pending.push(Upload {
            position: a.position(),
            colors: a.colors,
            handle: a.handle,
        });
        Ok(())
    }
    /// Freeing the last reference does not compact later slots, clear screen
    /// colors, or cancel already queued uploads.
    pub fn release(&mut self, slot: usize) -> Result<(), Error> {
        self.allocation(slot)?;
        let allocation = self.slots[slot].as_mut().unwrap();
        allocation.references -= 1;
        if allocation.references == 0 {
            self.slots[slot] = None;
        }
        Ok(())
    }
    /// Flush in queue order. V1 release builds clip colors at DAC index 256.
    /// Resource handles are resolved at flush time, as in the reference queue.
    pub fn flush(&mut self, resources: &Resources<'_>) -> Result<(), Error> {
        let mut screen = self.screen;
        for upload in &self.pending {
            let colors = read_palette(resources, upload.handle)?;
            let count = upload.colors.min(256 - upload.position);
            let colors = colors
                .get(..count)
                .ok_or(Error("palette upload exceeds resource"))?;
            screen[upload.position..upload.position + count].copy_from_slice(colors);
        }
        self.screen = screen;
        self.pending.clear();
        for a in self.slots.iter_mut().flatten() {
            a.position &= !MOVED;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixtures() -> Vec<u8> {
        let mut data = vec![0; 4096];
        for (offset, count, color) in [(4, 2, 10), (104, 3, 20), (204, 4, 30), (304, 256, 40)] {
            data[offset..offset + 4].copy_from_slice(&(count as u32).to_le_bytes());
            for index in 0..count {
                data[offset + 4 + index * 4..offset + 8 + index * 4]
                    .copy_from_slice(&[color, 0, 0, 0]);
            }
        }
        data
    }
    #[test]
    fn shared_slots_hole_reuse_movement_and_deferred_uploads() {
        let data = fixtures();
        let r = Resources(vec![&data]);
        let mut p = Palettes::default();
        let a = p.acquire(&r, Handle(4)).unwrap();
        let b = p.acquire(&r, Handle(104)).unwrap();
        assert_eq!(p.acquire(&r, Handle(4)).unwrap(), a);
        assert_eq!(p.allocation(a).unwrap().references, 2);
        p.flush(&r).unwrap();
        assert_eq!(
            &p.screen[1..6],
            &[[10, 0, 0], [10, 0, 0], [20, 0, 0], [20, 0, 0], [20, 0, 0]]
        );
        p.release(a).unwrap();
        assert!(p.allocation(a).is_ok());
        p.release(a).unwrap();
        assert_eq!(p.allocation(b).unwrap().position(), 3);
        let c = p.acquire(&r, Handle(204)).unwrap();
        assert_eq!(c, a);
        assert_eq!(p.allocation(b).unwrap().position(), 5);
        assert!(p.allocation(b).unwrap().moved());
        assert_eq!(p.screen[1], [10, 0, 0]); // Deferred, not immediately overwritten.
        let mut restored = p.clone();
        p.flush(&r).unwrap();
        restored.flush(&r).unwrap();
        assert_eq!(p, restored);
        assert_eq!(
            &p.screen[1..8],
            &[
                [30, 0, 0],
                [30, 0, 0],
                [30, 0, 0],
                [30, 0, 0],
                [20, 0, 0],
                [20, 0, 0],
                [20, 0, 0]
            ]
        );
        assert!(!p.allocation(b).unwrap().moved());
    }
    #[test]
    fn pending_uploads_survive_release_and_errors_are_atomic() {
        let data = fixtures();
        let r = Resources(vec![&data]);
        let mut p = Palettes::default();
        let slot = p.acquire(&r, Handle(304)).unwrap();
        p.release(slot).unwrap();
        let before = p.clone();
        assert!(p.release(slot).is_err());
        assert!(p.acquire(&r, Handle(0)).is_err());
        assert!(p.flush(&Resources(vec![&[]])).is_err());
        assert_eq!(p, before);
        p.flush(&r).unwrap();
        assert_eq!(p.screen[0], [0; 3]);
        assert!(p.screen[1..].iter().all(|c| *c == [40, 0, 0]));
        for _ in 0..34 {
            let slot = p.acquire(&r, Handle(4)).unwrap();
            p.release(slot).unwrap();
        }
        let before = p.clone();
        assert!(p.acquire(&r, Handle(4)).is_err());
        assert_eq!(p, before);
        p.flush(&r).unwrap();
        assert!(p.acquire(&r, Handle(4)).is_ok());
    }
}
