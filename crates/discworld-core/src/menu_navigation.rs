// SPDX-License-Identifier: GPL-3.0-or-later
//! V1 configuration-window lifecycle and deferred button dispatch.
//! Input event counters/control gating and host persistence are handled by the
//! caller. This component emits requests; it never exits or resets a host.
use crate::configuration_menu::{
    Action, ButtonImage, ButtonPress, Choice, ConfirmationMenu, MainMenu,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Main,
    Load,
    Save,
    Restart,
    Sound,
    Controls,
    Subtitles,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Restart,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Outcome {
    pub changed: bool,
    pub write_settings: bool,
    pub request: Option<Request>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Navigation {
    page: Option<Page>,
    text_action: Option<Action>,
    graphic: ButtonPress,
    graphic_page: Option<Page>,
}

impl Navigation {
    pub fn page(&self) -> Option<Page> {
        self.page
    }

    /// openMenu requires IDLE_INV, including exclusion of BOGUS_INV/TOPWINDOW.
    /// The caller must apply ProcessKeyEvent's control gate before this call.
    pub fn open(&mut self, page: Page, inventory_idle: bool) -> bool {
        if !inventory_idle || self.page.is_some() {
            return false;
        }
        self.page = Some(page);
        true
    }

    /// Physical provisional-left, resolved left/right and action events all
    /// reach menuAction. Main text actions wait for the inventory process;
    /// confirmation graphics additionally run ButtonPress before dispatch.
    /// Settings/save-slot widgets have their own handlers, not this hit test.
    pub fn click_button(&mut self, point: [i32; 2]) -> bool {
        if self.text_action.is_some() || self.graphic.pending() {
            return false;
        }
        match self.page {
            Some(Page::Main) => {
                self.text_action = MainMenu::action_at(point);
                self.text_action.is_some()
            }
            Some(Page::Restart | Page::Quit) => {
                let accepted = ConfirmationMenu::choice_at(point)
                    .is_some_and(|choice| self.graphic.start(choice));
                if accepted {
                    self.graphic_page = self.page;
                }
                accepted
            }
            _ => false,
        }
    }

    pub fn button_image(&self) -> Option<(Choice, ButtonImage)> {
        self.graphic.image()
    }

    /// InvWalkTo outside the window and CLOSEWIN both close completely.
    /// killInventory removes display objects but does not clear buttonEffect.
    /// Pending functions therefore survive closing; step still dispatches them.
    pub fn close(&mut self) -> Outcome {
        let changed = self.page.take().is_some();
        Outcome {
            changed,
            ..Outcome::default()
        }
    }

    /// Only PLR_ESCAPE sets reOpenMenu for a submenu. killInventory then
    /// reopens Main and writes configuration; a cancel-button click does not.
    pub fn escape(&mut self) -> Outcome {
        let reopen = self.page.is_some_and(|page| page != Page::Main);
        let mut outcome = self.close();
        if reopen {
            self.page = Some(Page::Main);
            outcome.write_settings = true;
        }
        outcome
    }

    /// Call once for each InventoryProcess step, after its redraw pass.
    pub fn step(&mut self, overlay_exists: bool) -> Outcome {
        if let Some(action) = self.text_action.take() {
            self.page = match action {
                Action::Load => Some(Page::Load),
                Action::Save => Some(Page::Save),
                Action::Restart => Some(Page::Restart),
                Action::Sound => Some(Page::Sound),
                Action::Controls => Some(Page::Controls),
                Action::Subtitles => Some(Page::Subtitles),
                Action::Quit => Some(Page::Quit),
                Action::Resume => None,
            };
            return Outcome {
                changed: true,
                ..Outcome::default()
            };
        }
        match self.graphic.step(overlay_exists) {
            Some(Choice::Cancel) => {
                self.graphic_page = None;
                self.close()
            }
            Some(Choice::Accept) => match self.graphic_page.take() {
                Some(Page::Restart) => {
                    let mut outcome = self.close();
                    outcome.request = Some(Request::Restart);
                    outcome
                }
                // IQUITGAME does not call killInventory before quitting.
                Some(Page::Quit) => Outcome {
                    request: Some(Request::Quit),
                    ..Outcome::default()
                },
                _ => Outcome::default(),
            },
            None => Outcome::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pending_actions_survive_window_closure_as_in_inventory_process() {
        let mut nav = Navigation::default();
        assert!(!nav.open(Page::Main, false));
        assert!(nav.open(Page::Main, true));
        assert!(!nav.open(Page::Load, true));
        assert!(nav.click_button([100, 74]));
        assert!(!nav.click_button([100, 134]));
        nav.close();
        assert!(nav.step(false).changed); // latched OPENREST still executes
        assert_eq!(nav.page(), Some(Page::Restart));
        assert!(nav.click_button([180, 90]));
        assert_eq!(nav.step(true).request, None); // installs normal HL1
        let escape = nav.escape();
        assert!(escape.changed && escape.write_settings);
        assert_eq!(nav.page(), Some(Page::Main));
        assert!(!nav.click_button([100, 43])); // global button animation remains busy
        assert_eq!(nav.step(false).request, Some(Request::Restart));
        assert_eq!(nav.page(), None);
        for _ in 0..6 {
            assert_eq!(nav.step(false), Outcome::default());
        }
    }

    #[test]
    fn cancel_and_escape_have_different_navigation_and_persistence() {
        let mut nav = Navigation::default();
        nav.open(Page::Quit, true);
        assert!(nav.click_button([140, 90]));
        for _ in 0..4 {
            assert_eq!(nav.step(true), Outcome::default());
        }
        assert_eq!(
            nav.step(true),
            Outcome {
                changed: true,
                ..Outcome::default()
            }
        );
        assert_eq!(nav.page(), None);
        for page in [
            Page::Load,
            Page::Save,
            Page::Restart,
            Page::Sound,
            Page::Controls,
            Page::Subtitles,
            Page::Quit,
        ] {
            assert!(nav.open(page, true));
            assert!(nav.escape().write_settings);
            assert_eq!(nav.page(), Some(Page::Main));
            assert!(!nav.escape().write_settings);
            assert_eq!(nav.page(), None);
        }
    }

    #[test]
    fn confirmation_dispatch_occurs_once_after_the_graphic_sequence() {
        for (page, request, after) in [
            (Page::Restart, Request::Restart, None),
            (Page::Quit, Request::Quit, Some(Page::Quit)),
        ] {
            let mut nav = Navigation::default();
            nav.open(page, true);
            assert!(nav.click_button([180, 90]));
            for image in [
                ButtonImage::Normal,
                ButtonImage::Down,
                ButtonImage::Down,
                ButtonImage::Normal,
            ] {
                assert_eq!(nav.step(true).request, None);
                assert_eq!(nav.button_image(), Some((Choice::Accept, image)));
            }
            assert_eq!(nav.step(true).request, Some(request));
            assert_eq!(nav.page(), after);
            for _ in 0..6 {
                assert_eq!(nav.step(true).request, None);
            }
        }
    }
}
