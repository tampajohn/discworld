// SPDX-License-Identifier: GPL-3.0-or-later
//! Shared original-menu navigation, presentation and dispatch-time selection.
//! Engine input gating, widget events, scene lifecycle and host I/O are separate.
use crate::{
    compositor::Surface,
    configuration_menu::{Confirmation, ConfirmationMenu, MainMenu, SoundMenu},
    graphics::Resources,
    iso::Error,
    menu_navigation::{Navigation, Outcome, Page, Request},
    save_menu::{Entry, List, Mode, Target, View},
    settings_menu::{Kind, SettingsMenu},
    startup::Declarations,
    text::Strings,
};

pub struct Assets<'a, 'data> {
    pub resources: &'a Resources<'data>,
    pub strings: &'a Strings<'data>,
    pub declarations: &'a Declarations,
    pub saves: &'a [Entry],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub volumes: [u16; 3],
    pub double_click: u16,
    pub swap_buttons: bool,
    pub text_speed: u16,
    pub subtitles: bool,
    pub flipped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Display {
    Main(Box<MainMenu>),
    Confirmation(Box<ConfirmationMenu>),
    Sound(Box<SoundMenu>),
    Settings(Box<SettingsMenu>),
    Saves(Box<View>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dispatch {
    pub outcome: Outcome,
    /// Resolved before CLOSEWIN discards the graphics. None means NOBOX/empty
    /// load selection; the host must not invent a save/load destination.
    pub selection: Option<(Target, Vec<u8>)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Controller {
    navigation: Navigation,
    display: Option<Display>,
    settings: Settings,
    // killInventory leaves selection intact; opening any new page resets it.
    list: Option<List>,
    // SetCursorScreenXY runs once on opening, not on every redraw. Hosts consume
    // this after committing the transition and update both engine and pointer.
    cursor_warp: Option<[i32; 2]>,
}

impl Controller {
    pub fn new(settings: Settings) -> Result<Self, Error> {
        if settings.volumes.iter().any(|v| *v > 255)
            || settings.double_click > 30
            || settings.text_speed > 100
        {
            return Err(Error("invalid menu settings"));
        }
        Ok(Self {
            navigation: Navigation::default(),
            display: None,
            settings,
            list: None,
            cursor_warp: None,
        })
    }
    pub fn page(&self) -> Option<Page> {
        self.navigation.page()
    }
    pub fn take_cursor_warp(&mut self) -> Option<[i32; 2]> {
        self.cursor_warp.take()
    }
    pub fn settings(&self) -> Settings {
        let mut settings = self.settings.clone();
        match &self.display {
            Some(Display::Sound(view)) => settings.volumes = view.values(),
            Some(Display::Settings(view)) => {
                settings.flipped = view.flipped();
                if self.page() == Some(Page::Controls) {
                    settings.double_click = view.value();
                    settings.swap_buttons = view.enabled();
                } else {
                    settings.text_speed = view.value();
                    settings.subtitles = view.enabled();
                }
            }
            _ => {}
        }
        settings
    }
    pub fn list_mut(&mut self) -> Option<&mut List> {
        if matches!(self.page(), Some(Page::Save | Page::Load)) {
            self.list.as_mut()
        } else {
            None
        }
    }
    pub fn sound_mut(&mut self) -> Option<&mut SoundMenu> {
        match &mut self.display {
            Some(Display::Sound(view)) => Some(view),
            _ => None,
        }
    }
    pub fn settings_mut(&mut self) -> Option<&mut SettingsMenu> {
        match &mut self.display {
            Some(Display::Settings(view)) => Some(view),
            _ => None,
        }
    }
    /// The caller has already applied the engine's control/inventory key gate.
    pub fn open(&mut self, page: Page, assets: &Assets<'_, '_>) -> Result<bool, Error> {
        let mut next = self.clone();
        if !next.navigation.open(page, true) {
            return Ok(false);
        }
        next.rebuild(assets)?;
        *self = next;
        Ok(true)
    }
    pub fn click_button(&mut self, point: [i32; 2]) -> bool {
        if self.settings_mut().is_some_and(|view| view.toggle_busy()) {
            return false;
        }
        self.navigation.click_button(point)
    }
    pub fn click_settings(&mut self, point: [i32; 2], double: bool) -> bool {
        let allow_toggle = !self.navigation.busy();
        self.settings_mut()
            .is_some_and(|view| view.click_with_toggle_guard(point, double, allow_toggle))
    }
    pub fn click_row(&mut self, point: [i32; 2], double: bool) -> Result<Dispatch, Error> {
        let mut next = self.clone();
        let outcome = if let Some(list) = &mut next.list {
            next.navigation.click_save_row(list, point, double)?
        } else {
            Outcome::default()
        };
        let dispatch = next.dispatch(outcome);
        if next.page().is_none() {
            next.display = None;
            next.cursor_warp = None;
        }
        *self = next;
        Ok(dispatch)
    }
    pub fn close(&mut self) -> Outcome {
        self.settings = self.settings();
        self.display = None;
        self.cursor_warp = None;
        self.navigation.close()
    }
    pub fn escape(&mut self, assets: &Assets<'_, '_>) -> Result<Outcome, Error> {
        let mut next = self.clone();
        next.settings = next.settings();
        let outcome = next.navigation.escape();
        next.rebuild(assets)?;
        *self = next;
        Ok(outcome)
    }
    pub fn step(
        &mut self,
        assets: &Assets<'_, '_>,
        overlay_exists: bool,
    ) -> Result<Dispatch, Error> {
        let mut next = self.clone();
        if let Some(view) = next.settings_mut() {
            view.step_toggle(true, overlay_exists);
        }
        next.settings = next.settings();
        let outcome = next.navigation.step(overlay_exists);
        let dispatch = next.dispatch(outcome);
        if outcome.changed {
            next.rebuild(assets)?;
        }
        *self = next;
        Ok(dispatch)
    }
    fn dispatch(&self, outcome: Outcome) -> Dispatch {
        let selection = if matches!(outcome.request, Some(Request::Save | Request::Load)) {
            self.list
                .as_ref()
                .and_then(|list| list.target().map(|target| (target, list.draft().to_vec())))
        } else {
            None
        };
        Dispatch { outcome, selection }
    }
    fn rebuild(&mut self, assets: &Assets<'_, '_>) -> Result<(), Error> {
        let Assets {
            resources,
            strings,
            declarations,
            saves,
        } = assets;
        let Some(page) = self.page() else {
            self.display = None;
            self.cursor_warp = None;
            return Ok(());
        };
        self.list = None; // popUpInventory resets selBox even when opening Main.
        self.display = Some(match page {
            Page::Main => Display::Main(MainMenu::new(resources, strings, declarations)?.into()),
            Page::Restart | Page::Quit => Display::Confirmation(
                ConfirmationMenu::new(
                    resources,
                    strings,
                    declarations,
                    if page == Page::Restart {
                        Confirmation::Restart
                    } else {
                        Confirmation::Quit
                    },
                )?
                .into(),
            ),
            Page::Sound => Display::Sound(
                SoundMenu::new(resources, strings, declarations, self.settings.volumes)?.into(),
            ),
            Page::Controls | Page::Subtitles => {
                let controls = page == Page::Controls;
                Display::Settings(
                    SettingsMenu::new(
                        resources,
                        strings,
                        declarations,
                        if controls {
                            Kind::Controls
                        } else {
                            Kind::Subtitles
                        },
                        if controls {
                            self.settings.double_click
                        } else {
                            self.settings.text_speed
                        },
                        if controls {
                            self.settings.swap_buttons
                        } else {
                            self.settings.subtitles
                        },
                        self.settings.flipped,
                    )?
                    .into(),
                )
            }
            Page::Save | Page::Load => {
                let mode = if page == Page::Save {
                    Mode::Save
                } else {
                    Mode::Load
                };
                self.list = Some(List::new(mode, saves.to_vec())?);
                Display::Saves(View::new(resources, strings, declarations, mode)?.into())
            }
        });
        self.cursor_warp = match page {
            Page::Save => Some([262, 91]),
            Page::Restart | Page::Quit => Some([180, 90]),
            _ => None,
        };
        Ok(())
    }
    pub fn draw(
        &self,
        resources: &Resources<'_>,
        surface: &mut Surface,
        pointer: Option<[i32; 2]>,
    ) -> Result<(), Error> {
        match &self.display {
            Some(Display::Main(view)) => view.draw(resources, surface, pointer),
            Some(Display::Confirmation(view)) => {
                view.draw(resources, surface, pointer, self.navigation.button_image())
            }
            Some(Display::Sound(view)) => view.draw(resources, surface),
            Some(Display::Settings(view)) => view.draw(resources, surface),
            Some(Display::Saves(view)) => view.draw(
                resources,
                surface,
                self.list.as_ref().ok_or(Error("missing menu save list"))?,
                pointer,
                self.navigation.button_image(),
            ),
            None => Ok(()),
        }
    }
}
