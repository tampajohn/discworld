// SPDX-License-Identifier: GPL-3.0-or-later
//! Original English CD V1 Controls and Subtitles widgets. Host setting changes,
//! input timing, and window lifecycle are supplied by the menu controller.
use crate::{
    animation::Animation,
    compositor::Surface,
    conversation_window::Frame,
    film::Film,
    graphics::{Handle, Resources},
    iso::Error,
    menu_slider,
    menu_toggle::{Effect, Toggle},
    startup::{Declarations, Window},
    text::{Font, Layout, Strings},
};

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Controls,
    Subtitles,
}

/// Logical checkpoint only; fonts, reels and drawable objects are rebuilt from
/// the caller's original resources instead of trusting serialized graphics.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct State {
    kind: Kind,
    value: u16,
    enabled: bool,
    flipped: bool,
    drag: Option<menu_slider::Drag>,
    toggle: Toggle,
    highlight: crate::menu_highlight::State,
    overlay: Option<usize>,
    thumb_offset: i32,
}
#[cfg(test)]
mod checkpoint_tests {
    use super::*;
    #[test]
    fn settings_recipe_roundtrips_and_rejects_inconsistent_fields() {
        let state = State {
            kind: Kind::Controls,
            value: 10,
            enabled: false,
            flipped: true,
            drag: Some(menu_slider::Drag::new(10, 30).unwrap()),
            toggle: Toggle::default(),
            highlight: crate::menu_highlight::State::default(),
            overlay: None,
            thumb_offset: 27,
        };
        state.validate().unwrap();
        let bytes = postcard::to_allocvec(&state).unwrap();
        let decoded: State = postcard::from_bytes(&bytes).unwrap();
        decoded.validate().unwrap();
        assert_eq!(decoded, state);
        for case in 0..6 {
            let mut invalid = state.clone();
            match case {
                0 => invalid.value = 31,
                1 => invalid.drag = Some(menu_slider::Drag::new(10, 100).unwrap()),
                2 => invalid.drag = Some(menu_slider::Drag::new(9, 30).unwrap()),
                3 => invalid.thumb_offset = i32::MAX,
                4 => invalid.overlay = Some(255),
                _ => {
                    invalid.highlight.point_at(Some((1, true)));
                    invalid.overlay = Some(2);
                }
            }
            let bytes = postcard::to_allocvec(&invalid).unwrap();
            let decoded: State = postcard::from_bytes(&bytes).unwrap();
            assert!(decoded.validate().is_err(), "case {case}");
        }
    }
}
impl State {
    pub fn validate(&self) -> Result<(), Error> {
        let maximum = if self.kind == Kind::Controls { 30 } else { 100 };
        let canonical = menu_slider::offset(self.value, maximum)?;
        self.toggle.validate_snapshot()?;
        self.highlight.validate_snapshot()?;
        if let Some(drag) = &self.drag {
            drag.validate_snapshot()?;
            if drag.maximum() != maximum || drag.value() != self.value {
                return Err(Error("settings drag differs from setting"));
            }
        }
        if self.thumb_offset != canonical
            && self
                .drag
                .as_ref()
                .is_none_or(|drag| drag.position() != self.thumb_offset)
        {
            return Err(Error("invalid settings thumb position"));
        }
        if self.overlay.is_some() != self.highlight.exists()
            || self.overlay.is_some_and(|image| !matches!(image, 1 | 2))
        {
            return Err(Error("invalid settings overlay"));
        }
        if let Some(crate::menu_highlight::Owner::Hover(id)) = self.highlight.owner() {
            if id != if self.kind == Kind::Controls { 2 } else { 1 } || self.overlay != Some(2) {
                return Err(Error("invalid settings hover"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsMenu {
    kind: Kind,
    pub(crate) frame: Frame,
    labels: Vec<Layout>,
    groove: Animation,
    thumb: Animation,
    buttons: Vec<Animation>, // cross normal/down/highlight, then tick states
    circles: Vec<Animation>,
    value: u16,
    enabled: bool,
    flipped: bool,
    drag: Option<menu_slider::Drag>,
    toggle: Toggle,
    highlight: crate::menu_highlight::State,
    overlay: Option<usize>, // state offset, uses current enabled value
}
impl SettingsMenu {
    pub fn state(&self) -> State {
        State {
            kind: self.kind,
            value: self.value,
            enabled: self.enabled,
            flipped: self.flipped,
            drag: self.drag.clone(),
            toggle: self.toggle.clone(),
            highlight: self.highlight.clone(),
            overlay: self.overlay,
            thumb_offset: self.thumb.object.x - 170,
        }
    }
    pub fn from_state(
        resources: &Resources<'_>,
        strings: &Strings<'_>,
        declarations: &Declarations,
        state: State,
    ) -> Result<Self, Error> {
        state.validate()?;
        let mut menu = Self::new(
            resources,
            strings,
            declarations,
            state.kind,
            state.value,
            state.enabled,
            state.flipped,
        )?;
        menu.drag = state.drag;
        menu.toggle = state.toggle;
        menu.highlight = state.highlight;
        menu.overlay = state.overlay;
        menu.thumb.object.x = 170 + state.thumb_offset;
        Ok(menu)
    }
    pub fn new(
        resources: &Resources<'_>,
        strings: &Strings<'_>,
        declarations: &Declarations,
        kind: Kind,
        value: u16,
        enabled: bool,
        flipped: bool,
    ) -> Result<Self, Error> {
        let controls = kind == Kind::Controls;
        let maximum = if controls { 30 } else { 100 };
        let offset = menu_slider::offset(value, maximum)?;
        let size = [10, if controls { 5 } else { 3 }];
        let window = Window {
            title: Handle(0),
            capacity: 0,
            min: size,
            size,
            max: size,
            position: [20, 16],
            first_display: 0,
            contents: vec![],
        };
        let handle = declarations
            .window_parts
            .ok_or(Error("missing window parts"))?;
        let frame = Frame::configuration(resources, handle, &window)?;
        let film = Film::read(resources, handle)?;
        let image = |reel: usize, position: [i32; 2]| -> Result<Animation, Error> {
            let mut a = Animation::new(
                resources,
                film.reels.get(reel).ok_or(Error("missing settings reel"))?,
                1,
            )?;
            a.object.x = position[0];
            a.object.y = position[1];
            Ok(a)
        };
        let font = Font::read(
            resources,
            declarations.tag_font.ok_or(Error("missing tag font"))?,
        )?;
        let handles = declarations
            .config_strings
            .ok_or(Error("missing configuration strings"))?;
        let rows = if controls {
            vec![(14, [162, 41]), (15, [162, 71]), (16, [175, 111])]
        } else {
            vec![(17, [162, 36]), (18, [162, 76])]
        };
        let mut labels = Vec::new();
        for (id, [x, y]) in rows {
            labels.push(font.right_aligned(
                resources,
                strings.get(handles[id].0)?,
                [x - 4, y + 6],
            )?);
        }
        let toggle_at = if controls { [175, 111] } else { [162, 76] };
        let buttons = [40, 41, 42, 46, 47, 48]
            .into_iter()
            .map(|r| image(r, toggle_at))
            .collect::<Result<Vec<_>, _>>()?;
        let circles = if controls {
            [38, 39]
                .into_iter()
                .map(|r| image(r, [162, 71]))
                .collect::<Result<Vec<_>, _>>()?
        } else {
            vec![]
        };
        let y = if controls { 41 } else { 36 };
        Ok(Self {
            kind,
            frame,
            labels,
            groove: image(30, [162, y])?,
            thumb: image(34, [170 + offset, y])?,
            buttons,
            circles,
            value,
            enabled,
            flipped,
            drag: None,
            toggle: Toggle::default(),
            highlight: crate::menu_highlight::State::default(),
            overlay: None,
        })
    }
    fn maximum(&self) -> u16 {
        if self.kind == Kind::Controls { 30 } else { 100 }
    }
    pub fn value(&self) -> u16 {
        self.value
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn flipped(&self) -> bool {
        self.flipped
    }
    fn contains(point: [i32; 2], origin: [i32; 2]) -> bool {
        point[0] > origin[0]
            && point[0] < origin[0] + 23
            && point[1] > origin[1]
            && point[1] < origin[1] + 19
    }
    fn toggle_origin(&self) -> [i32; 2] {
        if self.kind == Kind::Controls {
            [175, 111]
        } else {
            [162, 76]
        }
    }
    pub fn click(&mut self, point: [i32; 2], double: bool) -> bool {
        self.click_with_toggle_guard(point, double, true)
    }
    /// FLIP is immediate even while another menu button effect is running;
    /// TOGGLE must acquire the inventory process's shared button-effect slot.
    pub fn click_with_toggle_guard(
        &mut self,
        point: [i32; 2],
        double: bool,
        allow_toggle: bool,
    ) -> bool {
        if self.kind == Kind::Controls && Self::contains(point, [162, 71]) {
            if double {
                self.flipped = !self.flipped;
            }
            return double;
        }
        allow_toggle && Self::contains(point, self.toggle_origin()) && self.toggle.start()
    }
    pub fn toggle_busy(&self) -> bool {
        self.toggle.busy()
    }
    pub(crate) fn kind(&self) -> Kind {
        self.kind
    }
    pub(crate) fn clear_overlay(&mut self) {
        self.overlay = None;
        self.highlight.remove();
    }
    pub(crate) fn has_overlay(&self) -> bool {
        self.overlay.is_some()
    }
    fn effect_overlay(&mut self, image: Option<usize>) {
        self.overlay = image;
        if image.is_some() {
            self.highlight.install_effect();
        } else {
            self.highlight.remove();
        }
    }
    pub(crate) fn set_shared_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        self.thumb.object.x =
            170 + menu_slider::offset(self.value, self.maximum()).expect("validated setting");
    }
    /// A retained CONFBOX uses the current window origin after page changes.
    pub(crate) fn draw_retained_overlay(
        &self,
        resources: &Resources<'_>,
        surface: &mut Surface,
        origin: [i32; 2],
    ) -> Result<(), Error> {
        if let Some(offset) = self.overlay {
            let base = if self.enabled { 3 } else { 0 };
            let mut image = self.buttons[base + offset].clone();
            image.object.x += origin[0] - 20;
            image.object.y += origin[1] - 16;
            surface.draw_animation(resources, &image)?;
        }
        Ok(())
    }
    /// Supply actual inventory/HL1 existence, as ButtonToggle checks different
    /// objects after each sleep. Returns a setting change only at the flip step.
    pub fn step_toggle(&mut self, inventory_active: bool, overlay_exists: bool) -> Option<bool> {
        self.step_toggle_effect(inventory_active, overlay_exists)
            .filter(|effect| *effect == Effect::FlipDown)
            .map(|_| self.enabled)
    }
    pub(crate) fn step_toggle_effect(
        &mut self,
        inventory_active: bool,
        overlay_exists: bool,
    ) -> Option<Effect> {
        if !self.toggle.busy() {
            return None;
        }
        let effect = self.toggle.step(inventory_active, overlay_exists);
        match effect {
            Effect::Remove => self.effect_overlay(None),
            Effect::Down => self.effect_overlay(Some(1)),
            Effect::FlipDown => {
                self.enabled = !self.enabled;
                self.effect_overlay(Some(1));
                // addBoxes rebuilds the slider from its setting, too.
                self.thumb.object.x = 170
                    + menu_slider::offset(self.value, self.maximum())
                        .expect("private validated setting");
            }
            Effect::Highlight => self.effect_overlay(Some(2)),
            Effect::Finished => self.effect_overlay(None),
        }
        Some(effect)
    }
    pub fn begin_drag(
        &mut self,
        resources: &Resources<'_>,
        point: [i32; 2],
    ) -> Result<bool, Error> {
        if self.drag.is_some() {
            return Ok(false);
        }
        let [x, y] = point.map(i64::from);
        if let Some([l, t, r, b]) = self.thumb.bounds(resources)? {
            if x > l && x < r && y > t && y < b {
                self.drag = Some(menu_slider::Drag::new(self.value, self.maximum())?);
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub fn drag(&mut self, dx: i32) -> Result<Option<u16>, Error> {
        let Some(drag) = &mut self.drag else {
            return Ok(None);
        };
        let value = drag.move_by(dx)?;
        self.thumb.object.x = 170 + drag.position();
        if let Some(value) = value {
            self.value = value;
        }
        Ok(value)
    }
    pub fn end_drag(&mut self) -> Result<(), Error> {
        self.thumb.object.x = 170 + menu_slider::offset(self.value, self.maximum())?;
        self.drag = None;
        Ok(())
    }
    /// invBoxes owns the single HL1 overlay even while an effect is running.
    pub fn point_at(&mut self, point: Option<[i32; 2]>) {
        let target = point.and_then(|p| {
            if Self::contains(p, self.toggle_origin()) {
                Some((if self.kind == Kind::Controls { 2 } else { 1 }, true))
            } else if self.kind == Kind::Controls && Self::contains(p, [162, 71]) {
                Some((1, false))
            } else {
                None
            }
        });
        if self.highlight.point_at(target) {
            self.overlay = target.filter(|(_, highlighted)| *highlighted).map(|_| 2);
        }
    }
    pub fn draw(&self, resources: &Resources<'_>, surface: &mut Surface) -> Result<(), Error> {
        self.frame.draw(resources, surface)?;
        surface.draw_animation(resources, &self.groove)?;
        surface.draw_animation(resources, &self.thumb)?;
        let base = if self.enabled { 3 } else { 0 };
        surface.draw_animation(resources, &self.buttons[base])?;
        if self.kind == Kind::Controls {
            surface.draw_animation(resources, &self.circles[usize::from(!self.flipped)])?;
        }
        if let Some(offset) = self.overlay {
            surface.draw_animation(resources, &self.buttons[base + offset])?;
        }
        for label in &self.labels {
            label.draw(resources, surface)?;
        }
        Ok(())
    }
}
