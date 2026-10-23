// SPDX-License-Identifier: GPL-3.0-or-later
//! V1 polygon pointing and tag registration from pdisplay.cpp and tinlib.cpp.
//!
//! This is the state machine, not a host mouse-event handler. PointProcess must
//! poll it at its scheduler position, then spawn returned scripts in order.
//! Rendering and actor-tag priority belong to the separate TagProcess.
use crate::{
    graphics::Handle,
    iso::Error,
    polygon::{Kind, Polygons},
};

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Tags {
    /// None is TAG_OFF; Some(0) is TAG_ON with the polygon's default text.
    registered: Vec<Option<Handle>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Candidate {
    pub polygon: usize,
    /// None still selects the polygon, but must not create a text object.
    pub text: Option<Handle>,
}

impl Tags {
    pub fn new(polygons: &Polygons) -> Self {
        Self {
            registered: vec![None; polygons.active.len()],
        }
    }

    pub fn validate(&self, polygons: &Polygons) -> Result<(), Error> {
        if self.registered.len() != polygons.active.len() {
            return Err(Error("world tag polygon count mismatch"));
        }
        Ok(())
    }

    /// Cursor availability means the cursor object exists, even when hidden.
    /// Control, inventory and tag-display flags do not gate V1 PointProcess.
    /// Returns all entrants in polygon-table order, not just the first hit.
    pub fn point(
        &mut self,
        polygons: &mut Polygons,
        cursor: Option<[i32; 2]>,
    ) -> Result<Vec<usize>, Error> {
        self.validate(polygons)?;
        let Some(cursor) = cursor else {
            return Ok(Vec::new());
        };
        let mut entered = Vec::new();
        for (index, polygon) in polygons.active.iter_mut().enumerate() {
            if !polygon.enabled || !matches!(polygon.definition.kind, Kind::Tag | Kind::Exit) {
                continue;
            }
            let inside = polygon.contains(cursor);
            if inside && !polygon.pointing {
                polygon.pointing = true;
                entered.push(index);
            } else if !inside && polygon.pointing {
                polygon.pointing = false;
                self.registered[index] = None;
                // V1 never dispatches UNPOINT here.
            }
        }
        Ok(entered)
    }

    /// PRINTTAG registers once until TAG_OFF, including when no longer pointed.
    /// Handles are resolved only when TagProcess creates a new text object.
    pub fn print_tag(&mut self, polygon: usize, text: Handle) -> Result<(), Error> {
        let entry = self
            .registered
            .get_mut(polygon)
            .ok_or(Error("PRINTTAG polygon outside scene"))?;
        if entry.is_none() {
            *entry = Some(text);
        }
        Ok(())
    }

    /// Call when DISABLETAG/DISABLEEXIT changes a polygon to its disabled kind.
    /// This is immediate, not deferred until the next cursor poll.
    pub fn disable(&mut self, polygons: &mut Polygons, index: usize) -> Result<(), Error> {
        self.validate(polygons)?;
        let entry = self
            .registered
            .get_mut(index)
            .ok_or(Error("world tag polygon outside scene"))?;
        *entry = None;
        polygons.active[index].pointing = false;
        Ok(())
    }

    /// Polygon arbitration after actor-tag arbitration. Even an unlabeled
    /// pointed polygon wins over every later polygon in the table. A caller
    /// must cache selection identity: registering text later on the same
    /// selected polygon does not recreate its text object in V1.
    pub fn candidate(&self, polygons: &Polygons) -> Result<Option<Candidate>, Error> {
        self.validate(polygons)?;
        for (index, (polygon, registered)) in
            polygons.active.iter().zip(&self.registered).enumerate()
        {
            if polygon.pointing || registered.is_some() {
                return Ok(Some(Candidate {
                    polygon: index,
                    text: registered.map(|handle| {
                        if handle.0 == 0 {
                            polygon.definition.tag_text
                        } else {
                            handle
                        }
                    }),
                }));
            }
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::polygon::Polygon;

    fn polygons() -> Polygons {
        let definitions = [Kind::Path, Kind::Tag, Kind::Exit]
            .into_iter()
            .map(|kind| Polygon {
                kind,
                corners: [[0, 0], [20, 0], [20, 20], [0, 20]],
                tag_position: [10, 0],
                tag_text: Handle(100),
                node_position: [0, 0],
                film: Handle(0),
                refer_type: 0,
                id: 1,
                scales: [1, 1],
                reel: 0,
                z_factor: 0,
                nodes: Vec::new(),
                lines: Vec::new(),
                script: Handle(200),
            })
            .collect();
        Polygons::new(definitions, 0)
    }

    #[test]
    fn overlaps_enter_once_leave_without_script_and_reenter_in_table_order() {
        let mut polygons = polygons();
        let mut tags = Tags::new(&polygons);
        assert_eq!(tags.point(&mut polygons, Some([10, 10])).unwrap(), [1, 2]);
        tags.print_tag(1, Handle(42)).unwrap();
        assert!(
            tags.point(&mut polygons, Some([10, 10]))
                .unwrap()
                .is_empty()
        );
        // No cursor object suspends polling without erasing previous state.
        assert!(tags.point(&mut polygons, None).unwrap().is_empty());
        assert_eq!(
            tags.candidate(&polygons).unwrap().unwrap().text,
            Some(Handle(42))
        );
        assert!(
            tags.point(&mut polygons, Some([30, 30]))
                .unwrap()
                .is_empty()
        );
        assert_eq!(tags.candidate(&polygons).unwrap(), None);
        assert_eq!(tags.point(&mut polygons, Some([10, 10])).unwrap(), [1, 2]);
        assert_eq!(tags.candidate(&polygons).unwrap().unwrap().text, None);
    }

    #[test]
    fn unlabeled_priority_default_text_and_first_registration_wins() {
        let mut polygons = polygons();
        let mut tags = Tags::new(&polygons);
        tags.point(&mut polygons, Some([10, 10])).unwrap();
        tags.print_tag(2, Handle(99)).unwrap();
        assert_eq!(
            tags.candidate(&polygons).unwrap(),
            Some(Candidate {
                polygon: 1,
                text: None
            })
        );
        tags.print_tag(1, Handle(0)).unwrap();
        tags.print_tag(1, Handle(88)).unwrap();
        assert_eq!(
            tags.candidate(&polygons).unwrap().unwrap().text,
            Some(Handle(100))
        );
        polygons.active[1].enabled = false;
        tags.disable(&mut polygons, 1).unwrap();
        assert_eq!(
            tags.candidate(&polygons).unwrap(),
            Some(Candidate {
                polygon: 2,
                text: Some(Handle(99))
            })
        );
        polygons.active[1].enabled = true;
        assert_eq!(tags.point(&mut polygons, Some([10, 10])).unwrap(), [1]);
    }

    #[test]
    fn delayed_print_after_leaving_remains_registered_until_an_actual_leave() {
        let mut polygons = polygons();
        let mut tags = Tags::new(&polygons);
        tags.point(&mut polygons, Some([10, 10])).unwrap();
        tags.point(&mut polygons, Some([30, 30])).unwrap();
        tags.print_tag(1, Handle(77)).unwrap();
        tags.point(&mut polygons, Some([30, 30])).unwrap();
        assert_eq!(
            tags.candidate(&polygons).unwrap().unwrap().text,
            Some(Handle(77))
        );
        tags.point(&mut polygons, Some([10, 10])).unwrap();
        tags.point(&mut polygons, Some([30, 30])).unwrap();
        assert_eq!(tags.candidate(&polygons).unwrap(), None);
    }

    #[test]
    fn bad_indices_and_scene_mismatches_do_not_mutate_state() {
        let mut polygons = polygons();
        let mut tags = Tags::new(&polygons);
        let before = tags.clone();
        assert!(tags.print_tag(3, Handle(1)).is_err());
        assert!(tags.disable(&mut polygons, 3).is_err());
        assert_eq!(tags, before);
        polygons.active.pop();
        let scene_before = polygons.clone();
        assert!(tags.point(&mut polygons, Some([10, 10])).is_err());
        assert!(tags.candidate(&polygons).is_err());
        assert_eq!(tags, before);
        assert_eq!(polygons, scene_before);
    }
}
