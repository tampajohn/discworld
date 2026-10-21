// SPDX-License-Identifier: GPL-3.0-or-later
//! English DOS V1 configuration menu presentation (dialogs.cpp t1OptionBox).
//! Activation, action dispatch and save-slot persistence belong to the controller.
use crate::{
    compositor::Surface,
    conversation_window::Frame,
    graphics::Resources,
    iso::Error,
    startup::Declarations,
    text::{Font, Layout, Strings},
};

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
