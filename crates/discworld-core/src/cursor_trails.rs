// SPDX-License-Identifier: GPL-3.0-or-later
//! Original V1 cursor trail reels. Scheduling and host input stay with the caller.
use crate::{
    animation::{Animation, Tick},
    film::Film,
    graphics::{Handle, Resources},
    iso::Error,
};

/// Bounded ring from cursor.cpp: reel zero is the pointer, subsequent reels are
/// reused in order. Keep this state when saving, including the next reel index.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Trails {
    film: Handle,
    slots: Vec<Option<Animation>>,
    next: usize,
    previous: [i32; 2],
    suppressed: bool,
}

impl Trails {
    pub fn new(resources: &Resources<'_>, film: Handle, position: [i32; 2]) -> Result<Self, Error> {
        let data = Film::read(resources, film)?;
        if data.reels.is_empty() || data.reels.len() > 11 || data.rate > 24 {
            return Err(Error("invalid cursor trail film"));
        }
        Ok(Self {
            film,
            slots: vec![None; data.reels.len() - 1],
            next: 0,
            previous: position,
            suppressed: false,
        })
    }

    /// DropCursor deletes trail objects; InitCurPos establishes the new position.
    /// Neither resets the rotating reel index.
    pub fn reset_scene(&mut self, position: [i32; 2]) {
        self.slots.fill(None);
        self.previous = position;
        self.suppressed = false;
    }

    /// Inventory window entry deletes trails immediately, rather than pausing
    /// them for later display. Leaving the window permits new trails.
    pub fn suppress(&mut self, suppressed: bool) {
        self.suppressed = suppressed;
        if suppressed {
            self.slots.fill(None);
        }
    }

    /// DoCursorMove emits at the previous driver position, only on movement.
    /// A hidden cursor still updates its previous position but emits no trail.
    /// A frozen cursor must not call this method (matching AnimateProcess).
    pub fn move_to(
        &mut self,
        resources: &Resources<'_>,
        position: [i32; 2],
        cursor_hidden: bool,
    ) -> Result<(), Error> {
        self.validate_structure()?;
        if position != self.previous && !cursor_hidden && !self.suppressed && !self.slots.is_empty()
        {
            let film = Film::read(resources, self.film)?;
            let column = self.next + 1;
            let reel = film
                .reels
                .get(column)
                .ok_or(Error("missing cursor trail reel"))?;
            let mut animation = Animation::new(resources, reel, 24 / film.rate as u32)?;
            animation.reel = Some(Handle(
                self.film
                    .0
                    .checked_add(8 + column as u32 * 8)
                    .ok_or(Error("cursor trail reel overflow"))?,
            ));
            animation.status_layer = true;
            [animation.object.x, animation.object.y] = self.previous;
            animation.object.z = 999;
            // InitCurTrailObj steps immediately, even if AnimateProcess already
            // stepped the existing trails during this cycle.
            animation.tick(resources)?;
            self.slots[self.next] = Some(animation);
            self.next = (self.next + 1) % self.slots.len();
        }
        self.previous = position;
        Ok(())
    }

    /// AnimateProcess advances existing trails before ordinary pointer movement.
    /// Do not call while the original cursor process is asleep/hidden.
    pub fn tick(&mut self, resources: &Resources<'_>) -> Result<(), Error> {
        self.validate_structure()?;
        let mut next = self.clone();
        for slot in &mut next.slots {
            if let Some(animation) = slot {
                if animation.tick(resources)? == Tick::Finished {
                    *slot = None;
                }
            }
        }
        *self = next;
        Ok(())
    }

    /// Slot order is animation order. Composition must also consider the main
    /// cursor/status objects' Z and Y ordering; this is not a draw-order API.
    pub fn animations(&self) -> impl Iterator<Item = &Animation> {
        self.slots.iter().flatten()
    }

    /// HideCursorProcess hides the current images without deleting clocks.
    pub fn hide_images(&mut self) {
        for animation in self.slots.iter_mut().flatten() {
            animation.frame = Handle(0);
        }
    }

    /// InsertObject sorts ascending Z/Y, inserting new equal-Y objects first.
    pub fn draw_order(&self) -> Vec<&Animation> {
        let count = self.slots.len();
        let mut objects: Vec<_> = self
            .slots
            .iter()
            .enumerate()
            .filter_map(|(slot, animation)| animation.as_ref().map(|a| (slot, a)))
            .collect();
        objects.sort_by_key(|(slot, a)| {
            (
                a.object.z,
                a.object.y,
                std::cmp::Reverse((slot + count - self.next) % count),
            )
        });
        objects.into_iter().map(|(_, a)| a).collect()
    }

    pub(crate) fn validate_structure(&self) -> Result<(), Error> {
        if self.slots.len() > 10
            || self.next >= self.slots.len().max(1)
            || (self.suppressed && self.animations().next().is_some())
        {
            return Err(Error("invalid cursor trail state"));
        }
        Ok(())
    }

    pub(crate) fn film(&self) -> Handle {
        self.film
    }

    pub fn validate(&self, resources: &Resources<'_>) -> Result<(), Error> {
        self.validate_structure()?;
        let film = Film::read(resources, self.film)?;
        if film.reels.len() != self.slots.len() + 1 || film.rate > 24 {
            return Err(Error("invalid cursor trail film"));
        }
        for (slot, animation) in self.slots.iter().enumerate() {
            if let Some(animation) = animation {
                if animation.reel
                    != self
                        .film
                        .0
                        .checked_add(8 + (slot as u32 + 1) * 8)
                        .map(Handle)
                    || !animation.status_layer
                    || animation.object.z != 999
                    || !animation.uses_script(film.reels[slot + 1].script)
                {
                    return Err(Error("invalid cursor trail animation"));
                }
                animation.validate_snapshot_resources(resources)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        let mut bytes = vec![0; 128];
        let mut put = |offset: usize, value: u32| {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes())
        };
        put(0, 24);
        put(4, 3);
        for index in 0..3 {
            put(8 + index * 8, 40);
            put(12 + index * 8, 80);
        }
        // Empty multipart frame, then end: sufficient to test lifetime safely.
        put(80, 96);
        put(84, 0);
        bytes
    }

    #[test]
    fn ring_movement_suppression_reset_and_serialized_continuation() {
        let bytes = fixture();
        let resources = Resources(vec![&bytes]);
        let mut trails = Trails::new(&resources, Handle(0), [10, 20]).unwrap();
        trails.move_to(&resources, [10, 20], false).unwrap();
        assert_eq!(trails.animations().count(), 0);
        trails.move_to(&resources, [30, 40], false).unwrap();
        assert_eq!(trails.slots[0].as_ref().unwrap().object.x, 10);
        assert_eq!(trails.next, 1);
        trails.validate(&resources).unwrap();
        let mut restored: Trails =
            postcard::from_bytes(&postcard::to_allocvec(&trails).unwrap()).unwrap();
        for point in [[50, 60], [70, 80], [90, 100]] {
            trails.move_to(&resources, point, false).unwrap();
            restored.move_to(&resources, point, false).unwrap();
            assert_eq!(trails, restored);
            assert_eq!(trails.animations().count(), 2);
        }
        trails.tick(&resources).unwrap();
        assert_eq!(trails.animations().count(), 0);
        trails.move_to(&resources, [100, 100], false).unwrap();
        let next = trails.next;
        trails.reset_scene([2, 3]);
        assert_eq!(trails.next, next);
        trails.move_to(&resources, [4, 5], false).unwrap();
        trails.suppress(true);
        trails.move_to(&resources, [6, 7], false).unwrap();
        assert_eq!(trails.animations().count(), 0);
        trails.suppress(false);
        trails.move_to(&resources, [8, 9], true).unwrap();
        assert_eq!(trails.animations().count(), 0);
        trails.move_to(&resources, [10, 11], false).unwrap();
        let emitted = trails.animations().next().unwrap();
        assert_eq!([emitted.object.x, emitted.object.y], [8, 9]);
        trails.validate(&resources).unwrap();
    }

    #[test]
    fn failed_updates_are_atomic_and_invalid_state_is_rejected() {
        let bytes = fixture();
        let resources = Resources(vec![&bytes]);
        let mut trails = Trails::new(&resources, Handle(0), [0, 0]).unwrap();
        let before = trails.clone();
        assert!(trails.move_to(&Resources(vec![]), [1, 1], false).is_err());
        assert_eq!(trails, before);
        trails.move_to(&resources, [1, 1], false).unwrap();
        let before = trails.clone();
        assert!(trails.tick(&Resources(vec![])).is_err());
        assert_eq!(trails, before);
        trails.next = 2;
        assert!(trails.validate(&resources).is_err());
        assert!(trails.move_to(&resources, [2, 2], false).is_err());
        assert!(trails.tick(&resources).is_err());
    }

    #[test]
    fn equal_y_draw_order_and_hidden_frame_clock() {
        let mut bytes = fixture();
        bytes[0..4].copy_from_slice(&12u32.to_le_bytes());
        let resources = Resources(vec![&bytes]);
        let mut trails = Trails::new(&resources, Handle(0), [1, 20]).unwrap();
        trails.move_to(&resources, [2, 20], false).unwrap();
        trails.move_to(&resources, [3, 20], false).unwrap();
        assert_eq!(
            trails
                .draw_order()
                .iter()
                .map(|a| a.object.x)
                .collect::<Vec<_>>(),
            [2, 1]
        );
        trails.move_to(&resources, [4, 20], false).unwrap();
        assert_eq!(
            trails
                .draw_order()
                .iter()
                .map(|a| a.object.x)
                .collect::<Vec<_>>(),
            [3, 2]
        );
        trails.hide_images();
        trails.tick(&resources).unwrap();
        assert_eq!(trails.animations().count(), 2);
        assert!(trails.animations().all(|a| a.frame.0 == 0));
        trails.tick(&resources).unwrap();
        assert_eq!(trails.animations().count(), 0);
    }

    #[test]
    fn no_trails_and_bounded_film_declarations() {
        let mut bytes = fixture();
        bytes[4..8].copy_from_slice(&1u32.to_le_bytes());
        let resources = Resources(vec![&bytes]);
        let mut trails = Trails::new(&resources, Handle(0), [0, 0]).unwrap();
        trails.move_to(&resources, [1, 1], false).unwrap();
        trails.tick(&resources).unwrap();
        trails.validate(&resources).unwrap();
        assert_eq!(trails.animations().count(), 0);
        for (rate, count) in [(24u32, 0u32), (24, 12), (25, 1)] {
            bytes[0..4].copy_from_slice(&rate.to_le_bytes());
            bytes[4..8].copy_from_slice(&count.to_le_bytes());
            assert!(Trails::new(&Resources(vec![&bytes]), Handle(0), [0, 0]).is_err());
        }
    }
}
