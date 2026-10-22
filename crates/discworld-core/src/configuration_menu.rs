// SPDX-License-Identifier: GPL-3.0-or-later
//! English DOS V1 configuration menu presentation (dialogs.cpp t1OptionBox).
//! Activation, action dispatch and save-slot persistence belong to the controller.
use crate::{
    animation::Animation,
    compositor::Surface,
    conversation_window::Frame,
    film::Film,
    graphics::Resources,
    iso::Error,
    startup::{Declarations, Window},
    text::{Font, Layout, Strings},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confirmation {
    Restart,
    Quit,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Accept,
    Cancel,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonImage {
    Normal,
    Down,
    Highlight,
}

/// ButtonPress coroutine, advanced by the inventory process. The first step
/// installs normal, then sleeps 1/2/1 ticks before dispatching the choice.
/// Removing HL1 ends the coroutine early after its first or second sleep;
/// InventoryProcess still calls the queued function. Only a full reset cancels it.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq, Default)]
pub struct ButtonPress {
    pending: Option<(Choice, u8)>,
}
impl ButtonPress {
    pub(crate) fn validate_snapshot(&self) -> Result<(), Error> {
        if self.pending.is_some_and(|(_, age)| age > 4) {
            return Err(Error("invalid menu button coroutine age"));
        }
        Ok(())
    }
    pub fn start(&mut self, choice: Choice) -> bool {
        if self.pending.is_some() {
            return false;
        }
        self.pending = Some((choice, 0));
        true
    }
    pub fn reset(&mut self) {
        self.pending = None;
    }
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }
    pub fn step(&mut self, overlay_exists: bool) -> Option<Choice> {
        let (choice, age) = self.pending.as_mut()?;
        if *age == 4 || (matches!(*age, 1 | 3) && !overlay_exists) {
            let result = *choice;
            self.pending = None;
            return Some(result);
        }
        *age += 1;
        None
    }
    pub fn image(&self) -> Option<(Choice, ButtonImage)> {
        let (choice, age) = self.pending?;
        if age == 0 {
            return None;
        }
        Some((
            choice,
            if matches!(age, 2 | 3) {
                ButtonImage::Down
            } else {
                ButtonImage::Normal
            },
        ))
    }
}

/// Original graphic buttons have three distinct reels, not recolored text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmationMenu {
    pub(crate) frame: Frame,
    heading: Layout,
    buttons: [[Animation; 3]; 2],
}

impl ConfirmationMenu {
    pub fn new(
        resources: &Resources<'_>,
        strings: &Strings<'_>,
        declarations: &Declarations,
        kind: Confirmation,
    ) -> Result<Self, Error> {
        let handles = declarations
            .config_strings
            .ok_or(Error("missing configuration strings"))?;
        let title = handles[match kind {
            Confirmation::Restart => 10,
            Confirmation::Quit => 19,
        }];
        let window = Window {
            title,
            capacity: 0,
            min: [4, 2],
            size: [4, 2],
            max: [4, 2],
            position: [98, 53],
            first_display: 0,
            contents: vec![],
        };
        let handle = declarations
            .window_parts
            .ok_or(Error("missing window parts"))?;
        let frame = Frame::configuration(resources, handle, &window)?;
        let font = Font::read(
            resources,
            declarations.tag_font.ok_or(Error("missing tag font"))?,
        )?;
        let heading = font.centered(resources, strings.get(title.0)?, frame.layout.title)?;
        let film = Film::read(resources, handle)?;
        let button = |base: usize, x| -> Result<[Animation; 3], Error> {
            let mut images = Vec::new();
            for offset in 0..3 {
                let reel = film
                    .reels
                    .get(base + offset)
                    .ok_or(Error("missing confirmation button reel"))?;
                let mut image = Animation::new(resources, reel, 1)?;
                image.object.x = x;
                image.object.y = 81;
                images.push(image);
            }
            images
                .try_into()
                .map_err(|_| Error("invalid confirmation button states"))
        };
        Ok(Self {
            frame,
            heading,
            buttons: [button(46, 168)?, button(40, 128)?],
        })
    }

    /// AAGBUT uses strict inequalities on all four edges, unlike text buttons.
    pub fn choice_at(point: [i32; 2]) -> Option<Choice> {
        [Choice::Accept, Choice::Cancel]
            .into_iter()
            .zip([168, 128])
            .find_map(|(choice, x)| {
                (point[0] > x && point[0] < x + 23 && point[1] > 81 && point[1] < 100)
                    .then_some(choice)
            })
    }

    pub fn draw(
        &self,
        resources: &Resources<'_>,
        surface: &mut Surface,
        pointer: Option<[i32; 2]>,
        effect: Option<(Choice, ButtonImage)>,
    ) -> Result<(), Error> {
        self.frame.draw(resources, surface)?;
        self.heading.draw(resources, surface)?;
        for images in &self.buttons {
            surface.draw_animation(resources, &images[0])?;
        }
        // HL1 is a single overlay shared by hover and press animation.
        if let Some((choice, state)) = effect.or_else(|| {
            pointer
                .and_then(Self::choice_at)
                .map(|choice| (choice, ButtonImage::Highlight))
        }) {
            let button = match choice {
                Choice::Accept => 0,
                Choice::Cancel => 1,
            };
            let image = match state {
                ButtonImage::Normal => 0,
                ButtonImage::Down => 1,
                ButtonImage::Highlight => 2,
            };
            surface.draw_animation(resources, &self.buttons[button][image])?;
        }
        Ok(())
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Load,
    Save,
    Restart,
    Sound,
    Controls,
    Subtitles,
    Quit,
    Resume,
}

/// DOS sound settings: music, effects, voice, in that order. Host mixer updates
/// are emitted by drag(), not performed by the presentation component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundMenu {
    pub(crate) frame: Frame,
    labels: Vec<Layout>,
    grooves: Vec<Animation>,
    thumbs: Vec<Animation>,
    values: [u16; 3],
    drag: Option<(usize, crate::menu_slider::Drag)>,
}
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SoundState {
    values: [u16; 3],
    drag: Option<(usize, crate::menu_slider::Drag)>,
}
impl SoundState {
    pub fn validate(&self) -> Result<(), Error> {
        if self.values.iter().any(|value| *value > 255) {
            return Err(Error("invalid sound menu values"));
        }
        if let Some((channel, drag)) = &self.drag {
            drag.validate_snapshot()?;
            if self.values.get(*channel) != Some(&drag.value()) || drag.maximum() != 255 {
                return Err(Error("invalid sound menu drag"));
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod sound_checkpoint_tests {
    use super::*;
    #[test]
    fn sound_recipe_roundtrips_and_rejects_invalid_channel_or_value() {
        let state = SoundState {
            values: [255, 128, 0],
            drag: Some((1, crate::menu_slider::Drag::new(128, 255).unwrap())),
        };
        state.validate().unwrap();
        let bytes = postcard::to_allocvec(&state).unwrap();
        let decoded: SoundState = postcard::from_bytes(&bytes).unwrap();
        decoded.validate().unwrap();
        assert_eq!(decoded, state);
        for case in 0..4 {
            let mut invalid = state.clone();
            match case {
                0 => invalid.values[0] = 256,
                1 => invalid.drag.as_mut().unwrap().0 = usize::MAX,
                2 => invalid.values[1] = 127,
                _ => {
                    invalid.values[1] = 10;
                    invalid.drag = Some((1, crate::menu_slider::Drag::new(10, 30).unwrap()));
                }
            }
            assert!(invalid.validate().is_err());
        }
    }
}
impl SoundMenu {
    pub fn state(&self) -> SoundState {
        SoundState {
            values: self.values,
            drag: self.drag.clone(),
        }
    }
    pub fn from_state(
        resources: &Resources<'_>,
        strings: &Strings<'_>,
        declarations: &Declarations,
        state: SoundState,
    ) -> Result<Self, Error> {
        state.validate()?;
        let mut menu = Self::new(resources, strings, declarations, state.values)?;
        if let Some((channel, drag)) = &state.drag {
            menu.thumbs[*channel].object.x = 170 + drag.position();
        }
        menu.drag = state.drag;
        Ok(menu)
    }
    pub fn new(
        resources: &Resources<'_>,
        strings: &Strings<'_>,
        declarations: &Declarations,
        values: [u16; 3],
    ) -> Result<Self, Error> {
        for value in values {
            crate::menu_slider::offset(value, 255)?;
        }
        let window = Window {
            title: crate::graphics::Handle(0),
            capacity: 0,
            min: [10, 5],
            size: [10, 5],
            max: [10, 5],
            position: [20, 16],
            first_display: 0,
            contents: vec![],
        };
        let handle = declarations
            .window_parts
            .ok_or(Error("missing window parts"))?;
        let frame = Frame::configuration(resources, handle, &window)?;
        let font = Font::read(
            resources,
            declarations.tag_font.ok_or(Error("missing tag font"))?,
        )?;
        let handles = declarations
            .config_strings
            .ok_or(Error("missing configuration strings"))?;
        let film = Film::read(resources, handle)?;
        let mut labels = Vec::new();
        let mut grooves = Vec::new();
        let mut thumbs = Vec::new();
        for (row, value) in values.iter().enumerate() {
            let y = 16 + 25 + row as i32 * 40;
            labels.push(font.right_aligned(
                resources,
                strings.get(handles[11 + row].0)?,
                [158, y + 6],
            )?);
            for (reel, x, out) in [
                (30, 162, &mut grooves),
                (
                    34,
                    170 + crate::menu_slider::offset(*value, 255)?,
                    &mut thumbs,
                ),
            ] {
                let mut image = Animation::new(
                    resources,
                    film.reels
                        .get(reel)
                        .ok_or(Error("missing mixing desk reel"))?,
                    1,
                )?;
                image.object.x = x;
                image.object.y = y;
                out.push(image);
            }
        }
        Ok(Self {
            frame,
            labels,
            grooves,
            thumbs,
            values,
            drag: None,
        })
    }
    pub fn values(&self) -> [u16; 3] {
        self.values
    }

    /// whichMenuBox tests the actual multipart thumb bounds with strict edges.
    /// English V1's arrow Y interval is empty (9..0); do not invent arrow hits.
    pub fn begin_drag(
        &mut self,
        resources: &Resources<'_>,
        point: [i32; 2],
    ) -> Result<bool, Error> {
        if self.drag.is_some() {
            return Ok(false);
        }
        let [x, y] = point.map(i64::from);
        for (row, thumb) in self.thumbs.iter().enumerate() {
            if let Some([left, top, right, bottom]) = thumb.bounds(resources)? {
                if x > left && x < right && y > top && y < bottom {
                    self.drag = Some((row, crate::menu_slider::Drag::new(self.values[row], 255)?));
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
    /// Returns the changed channel and value. The caller applies MUSICVOL
    /// immediately; effects/voice settings are read by their respective mixers.
    pub fn drag(&mut self, dx: i32) -> Result<Option<(usize, u16)>, Error> {
        let Some((row, drag)) = &mut self.drag else {
            return Ok(None);
        };
        let value = drag.move_by(dx)?;
        self.thumbs[*row].object.x = 170 + drag.position();
        if let Some(value) = value {
            self.values[*row] = value;
        }
        Ok(value.map(|value| (*row, value)))
    }
    pub fn end_drag(&mut self) -> Result<(), Error> {
        for (thumb, value) in self.thumbs.iter_mut().zip(self.values) {
            thumb.object.x = 170 + crate::menu_slider::offset(value, 255)?;
        }
        self.drag = None;
        Ok(())
    }
    pub fn draw(&self, resources: &Resources<'_>, surface: &mut Surface) -> Result<(), Error> {
        self.frame.draw(resources, surface)?;
        for image in self.grooves.iter().chain(&self.thumbs) {
            surface.draw_animation(resources, image)?;
        }
        for label in &self.labels {
            label.draw(resources, surface)?;
        }
        Ok(())
    }
}

const ACTIONS: [Action; 8] = [
    Action::Load,
    Action::Save,
    Action::Restart,
    Action::Sound,
    Action::Controls,
    Action::Subtitles,
    Action::Quit,
    Action::Resume,
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MainMenu {
    pub(crate) frame: Frame,
    labels: Vec<Layout>,
}

fn bounds(row: usize) -> [i32; 4] {
    [72 + 15, 23 + 20 + row as i32 * 15, 145, 13]
}

impl MainMenu {
    pub fn new(
        resources: &Resources<'_>,
        strings: &Strings<'_>,
        declarations: &Declarations,
    ) -> Result<Self, Error> {
        let handles = declarations
            .config_strings
            .ok_or(Error("missing configuration strings"))?;
        let font = Font::read(
            resources,
            declarations.tag_font.ok_or(Error("missing tag font"))?,
        )?;
        let frame = Frame::top_window(
            resources,
            declarations
                .window_parts
                .ok_or(Error("missing window parts"))?,
        )?;
        let labels = handles[..8]
            .iter()
            .enumerate()
            .map(|(i, handle)| {
                let [x, y, width, _] = bounds(i);
                font.centered(resources, strings.get(handle.0)?, [x + width / 2, y])
            })
            .collect::<Result<Vec<_>, Error>>()?;
        Ok(Self { frame, labels })
    }

    /// AATBUT boxes include top/left, exclude bottom/right; the two-pixel
    /// inter-row gaps are inactive (whichMenuBox's default branch).
    pub fn action_at(point: [i32; 2]) -> Option<Action> {
        ACTIONS.iter().enumerate().find_map(|(i, &action)| {
            let [x, y, width, height] = bounds(i);
            (point[0] >= x && point[0] < x + width && point[1] >= y && point[1] < y + height)
                .then_some(action)
        })
    }

    pub fn draw(
        &self,
        resources: &Resources<'_>,
        surface: &mut Surface,
        pointer: Option<[i32; 2]>,
    ) -> Result<(), Error> {
        self.frame.draw(resources, surface)?;
        let pointed = pointer.and_then(Self::action_at);
        for (i, label) in self.labels.iter().enumerate() {
            // COL_BOX=TBLUE1, COL_HILIGHT=TBLUE4. Text stays above both.
            surface.draw_palette_rect(
                bounds(i),
                if pointed == Some(ACTIONS[i]) {
                    231
                } else {
                    228
                },
            )?;
            label.draw(resources, surface)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn confirmation_press_waits_and_full_reset_discards_effect() {
        let mut press = ButtonPress::default();
        assert!(press.start(Choice::Accept));
        assert!(!press.start(Choice::Cancel));
        assert_eq!(press.image(), None);
        for image in [
            ButtonImage::Normal,
            ButtonImage::Down,
            ButtonImage::Down,
            ButtonImage::Normal,
        ] {
            assert_eq!(press.step(true), None);
            assert_eq!(press.image(), Some((Choice::Accept, image)));
        }
        assert_eq!(press.step(true), Some(Choice::Accept));
        assert_eq!(press.step(true), None);
        assert_eq!(press.image(), None);
        assert!(press.start(Choice::Cancel));
        press.step(true);
        press.reset();
        for _ in 0..6 {
            assert_eq!(press.step(true), None);
        }
    }
    #[test]
    fn deleted_overlay_aborts_animation_but_still_returns_queued_action() {
        for elapsed in 1..=4 {
            let mut press = ButtonPress::default();
            press.start(Choice::Accept);
            for _ in 0..elapsed {
                assert_eq!(press.step(true), None);
            }
            // The second tick of the depressed-image sleep has no HL1 check.
            if elapsed == 2 {
                assert_eq!(press.step(false), None);
            }
            assert_eq!(press.step(false), Some(Choice::Accept));
            assert!(!press.pending());
            assert_eq!(press.step(false), None);
        }
    }
    #[test]
    fn graphic_confirmation_edges_are_excluded() {
        for p in [
            [168, 90],
            [191, 90],
            [180, 81],
            [180, 100],
            [128, 90],
            [151, 90],
        ] {
            assert_eq!(ConfirmationMenu::choice_at(p), None);
        }
        assert_eq!(ConfirmationMenu::choice_at([169, 82]), Some(Choice::Accept));
        assert_eq!(ConfirmationMenu::choice_at([190, 99]), Some(Choice::Accept));
        assert_eq!(ConfirmationMenu::choice_at([129, 82]), Some(Choice::Cancel));
        assert_eq!(ConfirmationMenu::choice_at([i32::MIN, i32::MAX]), None);
    }
    #[test]
    fn menu_hit_regions_keep_gaps_and_extreme_points_inactive() {
        assert_eq!(MainMenu::action_at([87, 43]), Some(Action::Load));
        assert_eq!(MainMenu::action_at([231, 55]), Some(Action::Load));
        assert_eq!(MainMenu::action_at([232, 55]), None);
        for y in [56, 57] {
            assert_eq!(MainMenu::action_at([100, y]), None);
        }
        assert_eq!(MainMenu::action_at([87, 58]), Some(Action::Save));
        assert_eq!(MainMenu::action_at([100, 160]), Some(Action::Resume));
        assert_eq!(MainMenu::action_at([100, 161]), None);
        assert_eq!(MainMenu::action_at([i32::MIN, i32::MAX]), None);
    }
}
