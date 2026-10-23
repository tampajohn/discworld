// SPDX-License-Identifier: GPL-3.0-or-later
//! V1 polygon pointing and tag registration from pdisplay.cpp and tinlib.cpp.
//!
//! This is the state machine, not a host mouse-event handler. PointProcess must
//! poll it at its scheduler position, then spawn returned scripts in order.
//! Rendering and actor-tag priority belong to the separate TagProcess.
use crate::{
    graphics::{Handle, Resources},
    iso::Error,
    polygon::{Kind, Polygons},
    text::{Font, Layout, Strings},
};

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Selection {
    Actor(u32),
    Polygon(usize),
}

/// A resolved hotspot. Actor arbitration must run before polygon arbitration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub selection: Selection,
    pub text: Option<Handle>,
    pub anchor: [i32; 2],
}

pub struct TextResources<'a, 'b> {
    pub resources: &'a Resources<'b>,
    pub strings: &'a Strings<'b>,
    pub font: Handle,
    pub background_size: [i32; 2],
}

/// V1 retains the text object for the selected hotspot. Neither a changed text
/// handle nor a moving actor causes a rebuild; only camera motion moves it.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct Display {
    selection: Option<Selection>,
    layout: Option<Layout>,
    origin: [i32; 2],
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct State {
    pub tags: Tags,
    pub display: Display,
}

impl State {
    pub fn new(polygons: &Polygons) -> Self {
        Self {
            tags: Tags::new(polygons),
            display: Display::default(),
        }
    }

    pub fn validate(
        &self,
        polygons: &Polygons,
        actors: &crate::actor::Actors,
    ) -> Result<(), Error> {
        self.tags.validate(polygons)?;
        match self.display.selection {
            Some(Selection::Actor(id)) => {
                actors.get(id)?;
            }
            Some(Selection::Polygon(index)) if index >= polygons.active.len() => {
                return Err(Error("saved world tag outside scene"));
            }
            _ => {}
        }
        if let Some(layout) = &self.display.layout {
            if self.display.selection.is_none() || layout.glyphs.len() > 4096 {
                return Err(Error("invalid saved world tag layout"));
            }
        }
        Ok(())
    }
}

impl Display {
    pub(crate) fn selection(&self) -> Option<Selection> {
        self.selection
    }

    pub fn layout(&self) -> Option<&Layout> {
        self.layout.as_ref()
    }

    /// Tags disabled or no eligible hotspot removes the text and selected ID.
    /// When the cursor object is unavailable, the caller must suspend instead.
    pub fn clear(&mut self) {
        self.selection = None;
        self.layout = None;
    }

    pub fn update(
        &mut self,
        target: Option<Target>,
        origin: [i32; 2],
        assets: &TextResources<'_, '_>,
    ) -> Result<(), Error> {
        let Some(target) = target else {
            self.clear();
            return Ok(());
        };
        if self.selection == Some(target.selection) {
            // PolyTag's unregistered-but-pointed branch returns immediately.
            // In particular, a later PRINTTAG does not create missing text.
            if matches!(target.selection, Selection::Polygon(_)) && target.text.is_none() {
                return Ok(());
            }
            let mut layout = self.layout.clone();
            if let Some(layout) = &mut layout {
                layout.shift(std::array::from_fn(|i| {
                    i64::from(self.origin[i]) - i64::from(origin[i])
                }))?;
            }
            self.layout = layout;
            self.origin = origin;
            return Ok(());
        }

        // Resolve assets only for a new selection, keeping errors atomic.
        let mut layout = None;
        if let Some(text) = target.text {
            let bytes = assets.strings.get(text.0)?;
            if bytes.is_empty() && matches!(target.selection, Selection::Actor(_)) {
                return Err(Error("empty V1 actor tag text"));
            }
            if !bytes.is_empty() {
                let font = Font::read(assets.resources, assets.font)?;
                let mut new_layout = font.centered(assets.resources, bytes, target.anchor)?;
                if matches!(target.selection, Selection::Polygon(_)) {
                    new_layout.keep_visible(
                        assets.resources,
                        assets.background_size[0],
                        assets.background_size[1],
                    )?;
                }
                new_layout.shift(origin.map(|v| -i64::from(v)))?;
                layout = Some(new_layout);
            }
        }
        self.selection = Some(target.selection);
        self.layout = layout;
        self.origin = origin;
        Ok(())
    }
}

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

    fn text_data() -> (Vec<u8>, Vec<u8>) {
        let mut font = vec![0; 1120];
        let glyph = 44 + usize::from(b'W') * 4;
        font[glyph..glyph + 4].copy_from_slice(&1100u32.to_le_bytes());
        font[1100..1108].copy_from_slice(&[4, 0, 6, 0, 0, 0, 0, 0]);
        // String 0 is empty, 1 is W, 2 is WW.
        let strings = vec![1, 0, 52, 51, 0, 0, 0, 0, 0, 1, b'W', 2, b'W', b'W'];
        (font, strings)
    }

    fn target(selection: Selection, text: Option<u32>, anchor: [i32; 2]) -> Target {
        Target {
            selection,
            text: text.map(Handle),
            anchor,
        }
    }

    #[test]
    fn label_cache_tracks_camera_but_not_actor_motion_or_text_changes() {
        let (font, strings) = text_data();
        let resources = Resources(vec![&font]);
        let strings = Strings::new(&strings).unwrap();
        let assets = TextResources {
            resources: &resources,
            strings: &strings,
            font: Handle(0),
            background_size: [640, 400],
        };
        let mut display = Display::default();
        display
            .update(
                Some(target(Selection::Actor(1), Some(1), [100, 50])),
                [20, 10],
                &assets,
            )
            .unwrap();
        let glyph = &display.layout().unwrap().glyphs[0];
        assert_eq!((glyph.x, glyph.y), (78, 40));
        // Invalid replacement text proves same-selection updates do not resolve
        // new resources. The actor's new position is deliberately ignored too.
        display
            .update(
                Some(target(Selection::Actor(1), Some(u32::MAX), [200, 90])),
                [23, 8],
                &assets,
            )
            .unwrap();
        let glyph = &display.layout().unwrap().glyphs[0];
        assert_eq!((glyph.x, glyph.y), (75, 42));
        let encoded = postcard::to_allocvec(&display).unwrap();
        let mut restored: Display = postcard::from_bytes(&encoded).unwrap();
        let next = Some(target(Selection::Actor(1), Some(2), [300, 90]));
        display.update(next, [30, 0], &assets).unwrap();
        restored.update(next, [30, 0], &assets).unwrap();
        assert_eq!(display, restored);
        display
            .update(
                Some(target(Selection::Actor(2), Some(2), [200, 90])),
                [30, 0],
                &assets,
            )
            .unwrap();
        assert_eq!(display.layout().unwrap().glyphs.len(), 2);
        assert_eq!(display.layout().unwrap().glyphs[0].x, 166);
    }

    #[test]
    fn delayed_label_is_not_created_until_selection_changes() {
        let (font, strings) = text_data();
        let resources = Resources(vec![&font]);
        let strings = Strings::new(&strings).unwrap();
        let assets = TextResources {
            resources: &resources,
            strings: &strings,
            font: Handle(0),
            background_size: [640, 400],
        };
        let mut display = Display::default();
        display
            .update(
                Some(target(Selection::Polygon(1), None, [100, 50])),
                [0, 0],
                &assets,
            )
            .unwrap();
        display
            .update(
                Some(target(Selection::Polygon(1), Some(1), [100, 50])),
                [5, 0],
                &assets,
            )
            .unwrap();
        assert!(display.layout().is_none());
        display.clear();
        display
            .update(
                Some(target(Selection::Polygon(1), Some(1), [100, 50])),
                [5, 0],
                &assets,
            )
            .unwrap();
        assert_eq!(display.layout().unwrap().glyphs[0].x, 93);
        display.update(None, [5, 0], &assets).unwrap();
        assert!(display.layout().is_none());
    }

    #[test]
    fn polygon_clamps_to_background_not_viewport_and_actor_does_not_clamp() {
        let (font, strings) = text_data();
        let resources = Resources(vec![&font]);
        let strings = Strings::new(&strings).unwrap();
        let assets = TextResources {
            resources: &resources,
            strings: &strings,
            font: Handle(0),
            background_size: [640, 400],
        };
        let mut display = Display::default();
        display
            .update(
                Some(target(Selection::Polygon(1), Some(1), [640, 400])),
                [100, 50],
                &assets,
            )
            .unwrap();
        let glyph = &display.layout().unwrap().glyphs[0];
        assert_eq!((glyph.x, glyph.y), (535, 345));
        display
            .update(
                Some(target(Selection::Actor(1), Some(1), [640, 400])),
                [100, 50],
                &assets,
            )
            .unwrap();
        let glyph = &display.layout().unwrap().glyphs[0];
        assert_eq!((glyph.x, glyph.y), (538, 350));
        display
            .update(
                Some(target(Selection::Polygon(1), Some(1), [0, 10])),
                [100, 50],
                &assets,
            )
            .unwrap();
        let glyph = &display.layout().unwrap().glyphs[0];
        assert_eq!((glyph.x, glyph.y), (-99, -40));
    }

    #[test]
    fn empty_polygon_text_and_invalid_assets_preserve_selection_semantics() {
        let (font, strings) = text_data();
        let resources = Resources(vec![&font]);
        let strings = Strings::new(&strings).unwrap();
        let assets = TextResources {
            resources: &resources,
            strings: &strings,
            font: Handle(0),
            background_size: [640, 400],
        };
        let mut display = Display::default();
        display
            .update(
                Some(target(Selection::Polygon(1), Some(0), [20, 20])),
                [0, 0],
                &assets,
            )
            .unwrap();
        assert!(display.layout().is_none());
        let before = display.clone();
        assert!(
            display
                .update(
                    Some(target(Selection::Actor(1), Some(0), [20, 20])),
                    [0, 0],
                    &assets
                )
                .is_err()
        );
        assert_eq!(display, before);
        assert!(
            display
                .update(
                    Some(target(Selection::Polygon(2), Some(100), [20, 20])),
                    [0, 0],
                    &assets
                )
                .is_err()
        );
        assert_eq!(display, before);
        display
            .update(
                Some(target(Selection::Polygon(1), Some(1), [20, 20])),
                [0, 0],
                &assets,
            )
            .unwrap();
        assert!(display.layout().is_none());
    }

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
