// SPDX-License-Identifier: GPL-3.0-or-later
//! Bounded, deterministic V1 reel animation, based on ScummVM anim.cpp.
use crate::{
    film::{Object, Reel, frame, word},
    graphics::{Handle, Resources},
    iso::Error,
};
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tick {
    Waiting,
    Frame,
    Hidden,
    Finished,
}
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ReelSource {
    pub film: Handle,
    pub column: usize,
    /// V1 stored play arguments (which can differ from embedded object XY).
    pub position: [i32; 2],
    pub depth: i32,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Animation {
    pub source: Option<ReelSource>,
    /// FIELD_STATUS uses screen coordinates, independently of world scrolling.
    pub status_layer: bool,
    pub reel: Option<Handle>,
    pub playback: u64,
    pub steps: u32,
    pub object: Object,
    pub frame: Handle,
    pub flip_x: bool,
    pub flip_y: bool,
    pub capacity: usize,
    script: Handle,
    index: usize,
    period: u32,
    remaining: u32,
    finished: bool,
}
impl Animation {
    pub(crate) fn validate_snapshot_structure(&self) -> Result<(), Error> {
        if self.period == 0
            || self.remaining == 0
            || self.remaining > self.period
            || self.capacity == 0
            || self.capacity > 4096
        {
            return Err(Error("invalid snapshot animation clock or capacity"));
        }
        Ok(())
    }

    pub(crate) fn validate_snapshot_resources(
        &self,
        resources: &Resources<'_>,
    ) -> Result<(), Error> {
        self.validate_snapshot_structure()?;
        let words = self
            .index
            .checked_add(usize::from(!self.finished))
            .and_then(|n| n.checked_mul(4))
            .ok_or(Error("snapshot animation offset overflow"))?;
        resources.resolve(self.script, words)?;
        if let Some(source) = &self.source {
            let film = crate::film::Film::read(resources, source.film)?;
            if source.column >= film.reels.len() {
                return Err(Error("snapshot reel column outside film"));
            }
        }
        if let Some(reel) = self.reel {
            resources.resolve(reel, 8)?;
        }
        let images = self.images(resources)?;
        if let Some(handle) = images.first() {
            let palette = crate::graphics::Image::read(resources, *handle)?.palette;
            crate::graphics::read_palette(resources, palette)?;
        }
        for handle in images {
            let image = crate::graphics::Image::read(resources, handle)?;
            crate::graphics::decode_tiles(resources, &image)?;
        }
        Ok(())
    }

    /// V1 SkipFrames synchronizes walking reels without changing the visible
    /// shape or clock. Control instructions before the next frame still run.
    /// Errors leave the entire animation unchanged.
    pub fn skip_frames(&mut self, resources: &Resources<'_>, count: i32) -> Result<(), Error> {
        if count <= 0 {
            return Ok(());
        }
        let mut next = self.clone();
        let mut remaining = count;
        for _ in 0..4096 {
            match next.read(resources)? {
                0 => return Err(Error("walking phase passed animation end")),
                1 => {
                    let operand_index = next.index;
                    let delta = next.read(resources)? as i32;
                    next.index = usize::try_from(operand_index as i64 + i64::from(delta))
                        .map_err(|_| Error("negative animation jump"))?;
                }
                2 => next.flip_x = !next.flip_x,
                3 => next.flip_y = !next.flip_y,
                4 => {
                    next.flip_x = !next.flip_x;
                    next.flip_y = !next.flip_y;
                }
                5 => {
                    let dx = next.read(resources)? as i32;
                    next.object.x = next.object.x.wrapping_add(dx);
                }
                6 => {
                    let dy = next.read(resources)? as i32;
                    next.object.y = next.object.y.wrapping_add(dy);
                }
                7 => {
                    let dx = next.read(resources)? as i32;
                    let dy = next.read(resources)? as i32;
                    next.object.x = next.object.x.wrapping_add(dx);
                    next.object.y = next.object.y.wrapping_add(dy);
                }
                8 | 10 => {} // Neither NOSLEEP nor HIDE counts as a frame.
                9 => {
                    next.read(resources)?;
                } // Skip CALL's address; do not execute it.
                _ => {
                    if remaining == 0 {
                        next.index -= 1; // Next tick consumes this frame.
                        *self = next;
                        return Ok(());
                    }
                    remaining -= 1;
                }
            }
        }
        Err(Error("animation skip instruction budget exhausted"))
    }
    pub(crate) fn uses_script(&self, script: Handle) -> bool {
        self.script == script
    }
    /// InitStepAnimScript changes the script/clock, retaining the multi-object.
    pub fn set_script(&mut self, script: Handle, period: u32) -> Result<(), Error> {
        if period == 0 {
            return Err(Error("zero animation period"));
        }
        self.script = script;
        self.period = period;
        self.remaining = 1;
        self.index = 0;
        self.steps = 0;
        self.finished = false;
        Ok(())
    }
    pub fn new(resources: &Resources<'_>, reel: &Reel, period: u32) -> Result<Self, Error> {
        if period == 0 {
            return Err(Error("zero animation period"));
        }
        let object = Object::read(resources, reel.object)?;
        let capacity = if object.frame.0 == 0 {
            1
        } else {
            frame(resources, object.frame)?.len().max(1)
        };
        Ok(Self {
            reel: None,
            source: None,
            status_layer: false,
            playback: 0,
            steps: 0,
            frame: object.frame,
            object,
            flip_x: false,
            flip_y: false,
            capacity,
            script: reel.script,
            index: 0,
            period,
            remaining: 1,
            finished: false,
        })
    }
    fn read(&mut self, resources: &Resources<'_>) -> Result<u32, Error> {
        let bytes = self
            .index
            .checked_add(1)
            .and_then(|n| n.checked_mul(4))
            .ok_or(Error("animation offset overflow"))?;
        let value = word(resources.resolve(self.script, bytes)?, self.index * 4)?;
        self.index += 1;
        Ok(value)
    }
    /// One engine tick. Errors leave all animation state unchanged.
    pub fn tick(&mut self, resources: &Resources<'_>) -> Result<Tick, Error> {
        let mut next = self.clone();
        let outcome = next.advance(resources)?;
        *self = next;
        Ok(outcome)
    }
    fn advance(&mut self, resources: &Resources<'_>) -> Result<Tick, Error> {
        if self.finished {
            return Ok(Tick::Finished);
        }
        self.remaining -= 1;
        if self.remaining != 0 {
            return Ok(Tick::Waiting);
        }
        self.remaining = self.period;
        self.steps = self
            .steps
            .checked_add(1)
            .ok_or(Error("animation step overflow"))?;
        for _ in 0..4096 {
            match self.read(resources)? {
                0 => {
                    self.finished = true;
                    return Ok(Tick::Finished);
                }
                1 => {
                    let operand_index = self.index;
                    let delta = self.read(resources)? as i32;
                    self.index = usize::try_from(operand_index as i64 + i64::from(delta))
                        .map_err(|_| Error("negative animation jump"))?;
                }
                2 => self.flip_x = !self.flip_x,
                3 => self.flip_y = !self.flip_y,
                4 => {
                    self.flip_x = !self.flip_x;
                    self.flip_y = !self.flip_y;
                }
                5 => {
                    let dx = self.read(resources)? as i32;
                    self.object.x = self.object.x.wrapping_add(dx);
                }
                6 => {
                    let dy = self.read(resources)? as i32;
                    self.object.y = self.object.y.wrapping_add(dy);
                }
                7 => {
                    let dx = self.read(resources)? as i32;
                    let dy = self.read(resources)? as i32;
                    self.object.x = self.object.x.wrapping_add(dx);
                    self.object.y = self.object.y.wrapping_add(dy);
                }
                8 => {} // V1 consumes NOSLEEP within this same tick.
                9 => return Err(Error("unsupported animation CALL")),
                10 => {
                    self.frame = Handle(0);
                    return Ok(Tick::Hidden);
                }
                value => {
                    let handle = Handle(value);
                    frame(resources, handle)?;
                    self.frame = handle;
                    return Ok(Tick::Frame);
                }
            }
        }
        Err(Error("animation instruction budget exhausted"))
    }
    pub fn images(&self, resources: &Resources<'_>) -> Result<Vec<Handle>, Error> {
        if self.frame.0 == 0 {
            return Ok(Vec::new());
        }
        let mut images = frame(resources, self.frame)?;
        images.truncate(self.capacity);
        Ok(images)
    }
    /// Inclusive world bounds of the current multipart shape, respecting flips.
    pub fn bounds(&self, resources: &Resources<'_>) -> Result<Option<[i64; 4]>, Error> {
        let mut bounds: Option<[i64; 4]> = None;
        for handle in self.images(resources)? {
            let image = crate::graphics::Image::read(resources, handle)?;
            let ax = if self.flip_x {
                -i64::from(image.animation_x) + i64::from(image.width) - 1
            } else {
                i64::from(image.animation_x)
            };
            let ay = if self.flip_y {
                -i64::from(image.animation_y) + i64::from(image.height) - 1
            } else {
                i64::from(image.animation_y)
            };
            let left = i64::from(self.object.x) - ax;
            let top = i64::from(self.object.y) - ay;
            let right = left + i64::from(image.width) - 1;
            let bottom = top + i64::from(image.height) - 1;
            bounds = Some(match bounds {
                None => [left, top, right, bottom],
                Some([l, t, r, b]) => [l.min(left), t.min(top), r.max(right), b.max(bottom)],
            });
        }
        Ok(bounds)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(script: &[u32]) -> Vec<u8> {
        let mut b = vec![0; 64];
        b[0..4].copy_from_slice(&32u32.to_le_bytes());
        b[32..36].copy_from_slice(&48u32.to_le_bytes());
        for v in script {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b
    }
    fn anim(data: &[u8], period: u32) -> Animation {
        Animation::new(
            &Resources(vec![data]),
            &Reel {
                object: Handle(0),
                script: Handle(64),
            },
            period,
        )
        .unwrap()
    }
    #[test]
    fn snapshot_checks_animation_clock_capacity_and_script_bounds() {
        let mut data = fixture(&[0]);
        data[0..4].copy_from_slice(&0u32.to_le_bytes());
        let resources = Resources(vec![&data]);
        let valid = anim(&data, 2);
        valid.validate_snapshot_resources(&resources).unwrap();
        for remaining in [0, 3, u32::MAX] {
            let mut invalid = valid.clone();
            invalid.remaining = remaining;
            assert!(invalid.validate_snapshot_resources(&resources).is_err());
        }
        for capacity in [0, 4097, usize::MAX] {
            let mut invalid = valid.clone();
            invalid.capacity = capacity;
            assert!(invalid.validate_snapshot_resources(&resources).is_err());
        }
        let mut invalid = valid.clone();
        invalid.index = usize::MAX;
        assert!(invalid.validate_snapshot_resources(&resources).is_err());
        let mut invalid = valid.clone();
        invalid.frame = Handle(u32::MAX);
        assert!(invalid.validate_snapshot_resources(&resources).is_err());
        let mut finished = valid;
        assert_eq!(finished.tick(&resources).unwrap(), Tick::Finished);
        finished.validate_snapshot_resources(&resources).unwrap();
    }
    #[test]
    fn timing_and_relative_loop() {
        let b = fixture(&[32, 1, (-2i32) as u32]);
        let r = Resources(vec![&b]);
        let mut a = anim(&b, 2);
        assert_eq!(a.tick(&r).unwrap(), Tick::Frame);
        assert_eq!(a.tick(&r).unwrap(), Tick::Waiting);
        assert_eq!(a.tick(&r).unwrap(), Tick::Frame);
    }
    #[test]
    fn transforms_hide_and_resume() {
        let b = fixture(&[2, 7, (-3i32) as u32, 8, 8, 32, 10, 32, 0]);
        let r = Resources(vec![&b]);
        let mut a = anim(&b, 1);
        assert_eq!(a.tick(&r).unwrap(), Tick::Frame);
        assert_eq!((a.object.x, a.object.y, a.flip_x), (-3, 8, true));
        let mut restored = a.clone();
        assert_eq!(restored.tick(&r).unwrap(), Tick::Hidden);
        assert!(restored.images(&r).unwrap().is_empty());
        assert_eq!(restored.tick(&r).unwrap(), Tick::Frame);
        assert_eq!(restored.tick(&r).unwrap(), Tick::Finished);
    }
    #[test]
    fn cycle_is_bounded_and_atomic() {
        let b = fixture(&[1, (-1i32) as u32]);
        let r = Resources(vec![&b]);
        let mut a = anim(&b, 1);
        let before = a.clone();
        assert!(a.tick(&r).is_err());
        assert_eq!(a, before);
    }
    #[test]
    fn walking_phase_skips_shapes_but_applies_intervening_controls() {
        let b = fixture(&[32, 2, 7, 3, 4, 10, 9, 123, 8, 32, 0]);
        let r = Resources(vec![&b]);
        let mut a = anim(&b, 2);
        a.skip_frames(&r, 1).unwrap();
        assert_eq!((a.object.x, a.object.y, a.flip_x), (3, 4, true));
        assert_eq!(a.frame, Handle(32));
        assert_eq!(a.steps, 0);
        assert_eq!(a.index, 9);
        assert_eq!(a.tick(&r).unwrap(), Tick::Frame);
        assert_eq!(a.tick(&r).unwrap(), Tick::Waiting);
        assert_eq!(a.tick(&r).unwrap(), Tick::Finished);
    }
    #[test]
    fn walking_phase_follows_loops_and_rejects_invalid_scripts_atomically() {
        let b = fixture(&[32, 1, (-2i32) as u32]);
        let r = Resources(vec![&b]);
        let mut a = anim(&b, 1);
        a.skip_frames(&r, 5).unwrap();
        assert_eq!(a.index, 0);
        assert_eq!(a.tick(&r).unwrap(), Tick::Frame);
        for script in [&[2, 32, 0][..], &[2, 1, (-1i32) as u32][..], &[9][..]] {
            let b = fixture(script);
            let r = Resources(vec![&b]);
            let mut a = anim(&b, 1);
            let before = a.clone();
            a.skip_frames(&r, 0).unwrap();
            a.skip_frames(&r, -1).unwrap();
            assert_eq!(a, before);
            assert!(a.skip_frames(&r, 1).is_err());
            assert_eq!(a, before);
        }
    }
}
