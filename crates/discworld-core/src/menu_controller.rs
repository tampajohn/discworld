// SPDX-License-Identifier: GPL-3.0-or-later
//! Shared original-menu navigation, presentation and dispatch-time selection.
//! Engine input gating, widget events, scene lifecycle and host I/O are separate.
use crate::{
    compositor::Surface,
    configuration_menu::{
        ButtonImage, Choice, Confirmation, ConfirmationMenu, MainMenu, SoundMenu,
    },
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EffectGraphic {
    Button,
    CurrentToggle,
    RetainedToggle,
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
    retained_toggle: Option<Box<SettingsMenu>>,
    highlight: crate::menu_highlight::State,
    pointer: Option<[i32; 2]>,
    effect_graphic: Option<EffectGraphic>,
    button_overlay: Option<crate::animation::Animation>,
    window_origin: [i32; 2],
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
            retained_toggle: None,
            highlight: crate::menu_highlight::State::default(),
            pointer: None,
            effect_graphic: None,
            button_overlay: None,
            window_origin: [20, 16],
        })
    }
    pub fn page(&self) -> Option<Page> {
        self.navigation.page()
    }
    pub fn take_cursor_warp(&mut self) -> Option<[i32; 2]> {
        self.cursor_warp.take()
    }
    /// invBoxes: box identity and HL1 ownership are distinct. Call before the
    /// inventory-process step when pointer monitoring runs, even while outside.
    pub fn point_at(&mut self, pointer: Option<[i32; 2]>) {
        let target = pointer.and_then(|point| match self.page()? {
            Page::Main => MainMenu::action_at(point).map(|action| (action as u16, true)),
            Page::Restart | Page::Quit => {
                ConfirmationMenu::choice_at(point).map(|choice| (choice as u16, true))
            }
            Page::Save | Page::Load => List::row_at(point)
                .map(|row| (row as u16, false))
                .or_else(|| View::button_at(point).map(|choice| (9 + choice as u16, true))),
            Page::Controls | Page::Subtitles => {
                let controls = self.page() == Some(Page::Controls);
                let inside = |[x, y]: [i32; 2]| {
                    point[0] > x && point[0] < x + 23 && point[1] > y && point[1] < y + 19
                };
                if inside(if controls { [175, 111] } else { [162, 76] }) {
                    Some((if controls { 2 } else { 1 }, true))
                } else if controls && inside([162, 71]) {
                    Some((1, false))
                } else {
                    None
                }
            }
            Page::Sound => None,
        });
        self.pointer = pointer;
        if self.highlight.point_at(target) {
            self.effect_graphic = None;
            if let Some(view) = &mut self.retained_toggle {
                view.clear_overlay();
            }
        }
        if let Some(view) = self.settings_mut() {
            view.point_at(pointer);
        }
    }
    pub fn advance(&mut self, assets: &Assets<'_, '_>) -> Result<Dispatch, Error> {
        self.step(assets, self.highlight.exists())
    }
    fn toggle_graphic(
        &mut self,
        effect: Option<crate::menu_toggle::Effect>,
        graphic: EffectGraphic,
    ) {
        use crate::menu_toggle::Effect;
        match effect {
            Some(Effect::Remove | Effect::Finished) => {
                self.highlight.remove();
                self.effect_graphic = None;
            }
            Some(_) => {
                self.highlight.install_effect();
                self.effect_graphic = Some(graphic);
            }
            None => {}
        }
    }
    fn reset_highlight(&mut self) {
        self.highlight.reset();
        self.pointer = None;
        self.effect_graphic = None;
        self.button_overlay = None;
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
        if self
            .retained_toggle
            .as_ref()
            .is_some_and(|view| view.toggle_busy())
            || self.settings_mut().is_some_and(|view| view.toggle_busy())
        {
            return false;
        }
        self.navigation.click_button(point)
    }
    pub fn click_settings(&mut self, point: [i32; 2], double: bool) -> bool {
        let allow_toggle = !self.navigation.busy()
            && !self
                .retained_toggle
                .as_ref()
                .is_some_and(|view| view.toggle_busy());
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
            next.retain_toggle();
            next.reset_highlight();
            next.display = None;
            next.cursor_warp = None;
        }
        *self = next;
        Ok(dispatch)
    }
    pub fn close(&mut self) -> Outcome {
        self.settings = self.settings();
        self.retain_toggle();
        self.reset_highlight();
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
        if !overlay_exists {
            next.highlight.remove();
            next.effect_graphic = None;
        }
        let effect = next
            .settings_mut()
            .and_then(|view| view.step_toggle_effect(true, overlay_exists));
        next.toggle_graphic(effect, EffectGraphic::CurrentToggle);
        next.settings = next.settings();
        let active = next.page().is_some();
        let retained = next.retained_toggle.as_mut().map(|view| {
            if !overlay_exists {
                view.clear_overlay();
            }
            (
                view.step_toggle_effect(active, overlay_exists),
                view.kind(),
                view.enabled(),
            )
        });
        let changed = retained.and_then(|(effect, kind, value)| {
            next.toggle_graphic(effect, EffectGraphic::RetainedToggle);
            (effect == Some(crate::menu_toggle::Effect::FlipDown)).then_some((kind, value))
        });
        if let Some((kind, value)) = changed {
            match kind {
                Kind::Controls => next.settings.swap_buttons = value,
                Kind::Subtitles => next.settings.subtitles = value,
            }
            if let Some(view) = next.settings_mut().filter(|view| view.kind() == kind) {
                view.set_shared_enabled(value);
            }
        }
        let previous_image = next.navigation.button_image();
        let outcome = next.navigation.step(overlay_exists);
        if let Some((choice, image)) = next
            .navigation
            .button_image()
            .filter(|image| Some(*image) != previous_image)
        {
            next.highlight.install_effect();
            let original = next
                .navigation
                .graphic_page()
                .ok_or(Error("missing menu graphic origin"))?;
            let relative = match (original, choice) {
                (Page::Save | Page::Load, Choice::Accept) => [230, 44],
                (Page::Save | Page::Load, Choice::Cancel) => [230, 91],
                (_, Choice::Accept) => [70, 28],
                (_, Choice::Cancel) => [30, 28],
            };
            let film = crate::film::Film::read(
                assets.resources,
                assets
                    .declarations
                    .window_parts
                    .ok_or(Error("missing window parts"))?,
            )?;
            let reel = if choice == Choice::Accept { 46 } else { 40 }
                + match image {
                    ButtonImage::Normal => 0,
                    ButtonImage::Down => 1,
                    ButtonImage::Highlight => 2,
                };
            let mut animation = crate::animation::Animation::new(
                assets.resources,
                film.reels
                    .get(reel)
                    .ok_or(Error("missing menu button reel"))?,
                1,
            )?;
            animation.object.x = next.window_origin[0] + relative[0];
            animation.object.y = next.window_origin[1] + relative[1];
            next.button_overlay = Some(animation);
            next.effect_graphic = Some(EffectGraphic::Button);
        }
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
        self.retain_toggle();
        self.reset_highlight();
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
        self.window_origin = match page {
            Page::Main => [72, 23],
            Page::Restart | Page::Quit => [98, 53],
            _ => [20, 16],
        };
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
    fn retain_toggle(&mut self) {
        if let Some(Display::Settings(view)) = &self.display {
            if view.toggle_busy() {
                self.retained_toggle = Some(view.clone());
            }
        }
        if let Some(view) = &mut self.retained_toggle {
            view.clear_overlay();
        }
    }
    pub fn draw(&self, resources: &Resources<'_>, surface: &mut Surface) -> Result<(), Error> {
        use crate::menu_highlight::Owner;
        let pointer = if matches!(self.highlight.owner(), Some(Owner::Hover(_))) {
            self.pointer
        } else {
            None
        };
        let effect = if self.highlight.owner() == Some(Owner::Effect) {
            self.effect_graphic
        } else {
            None
        };
        match &self.display {
            Some(Display::Main(view)) => view.draw(resources, surface, pointer),
            Some(Display::Confirmation(view)) => view.draw(resources, surface, pointer, None),
            Some(Display::Sound(view)) => view.draw(resources, surface),
            Some(Display::Settings(view)) => {
                if pointer.is_some() || effect == Some(EffectGraphic::CurrentToggle) {
                    view.draw(resources, surface)
                } else {
                    let mut base = view.clone();
                    base.clear_overlay();
                    base.draw(resources, surface)
                }
            }
            Some(Display::Saves(view)) => view.draw(
                resources,
                surface,
                self.list.as_ref().ok_or(Error("missing menu save list"))?,
                pointer,
                None,
            ),
            None => Ok(()),
        }?;
        if effect == Some(EffectGraphic::Button) {
            if let Some(image) = &self.button_overlay {
                surface.draw_animation(resources, image)?;
            }
        }
        if effect == Some(EffectGraphic::RetainedToggle) {
            if let (Some(view), Some(page)) = (&self.retained_toggle, self.page()) {
                let origin = match page {
                    Page::Main => [72, 23],
                    Page::Restart | Page::Quit => [98, 53],
                    _ => [20, 16],
                };
                view.draw_retained_overlay(resources, surface, origin)?;
            }
        }
        Ok(())
    }
}
