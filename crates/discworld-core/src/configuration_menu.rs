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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Accept,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ButtonImage {
    Normal,
    Down,
    Highlight,
}

/// ButtonPress coroutine, advanced by the inventory process. The first step
/// installs normal, then sleeps 1/2/1 ticks before dispatching the choice.
/// Removing HL1 ends the coroutine early after its first or second sleep;
/// InventoryProcess still calls the queued function. Only a full reset cancels it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ButtonPress {
    pending: Option<(Choice, u8)>,
}
impl ButtonPress {
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
    frame: Frame,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    frame: Frame,
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
