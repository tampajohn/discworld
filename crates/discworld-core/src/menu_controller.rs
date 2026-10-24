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

#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
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
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
enum EffectGraphic {
    Button,
    CurrentToggle,
    RetainedToggle,
}
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq, Eq)]
struct ButtonRecipe {
    choice: Choice,
    image: ButtonImage,
    position: [i32; 2],
}
impl ButtonRecipe {
    fn rebuild(&self, assets: &Assets<'_, '_>) -> Result<crate::animation::Animation, Error> {
        let offsets = if self.choice == Choice::Accept {
            [[230, 44], [70, 28]]
        } else {
            [[230, 91], [30, 28]]
        };
        if ![[20, 16], [72, 23], [98, 53]].iter().any(|origin| {
            offsets
                .iter()
                .any(|offset| self.position == [origin[0] + offset[0], origin[1] + offset[1]])
        }) {
            return Err(Error("invalid menu button position"));
        }
        let film = crate::film::Film::read(
            assets.resources,
            assets
                .declarations
                .window_parts
                .ok_or(Error("missing window parts"))?,
        )?;
        let reel = if self.choice == Choice::Accept {
            46
        } else {
            40
        } + match self.image {
            ButtonImage::Normal => 0,
            ButtonImage::Down => 1,
            ButtonImage::Highlight => 2,
        };
        let mut image = crate::animation::Animation::new(
            assets.resources,
            film.reels
                .get(reel)
                .ok_or(Error("missing menu button reel"))?,
            1,
        )?;
        image.object.x = self.position[0];
        image.object.y = self.position[1];
        Ok(image)
    }
}
#[derive(serde::Serialize, serde::Deserialize)]
struct Checkpoint {
    navigation: Navigation,
    settings: Settings,
    list: Option<List>,
    cursor_warp: Option<[i32; 2]>,
    sound: Option<crate::configuration_menu::SoundState>,
    widget: Option<crate::settings_menu::State>,
    retained: Option<crate::settings_menu::State>,
    highlight: crate::menu_highlight::State,
    pointer: Option<[i32; 2]>,
    effect: Option<EffectGraphic>,
    button: Option<ButtonRecipe>,
    origin: [i32; 2],
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
    button_recipe: Option<ButtonRecipe>,
    window_origin: [i32; 2],
}

impl Controller {
    /// Logical menu checkpoint, versioned independently until embedded in the
    /// engine snapshot. The enclosing session must bind it to original assets.
    pub fn checkpoint(&self) -> Result<Vec<u8>, Error> {
        let state = Checkpoint {
            navigation: self.navigation.clone(),
            settings: self.settings.clone(),
            list: self.list.clone(),
            cursor_warp: self.cursor_warp,
            sound: match &self.display {
                Some(Display::Sound(view)) => Some(view.state()),
                _ => None,
            },
            widget: match &self.display {
                Some(Display::Settings(view)) => Some(view.state()),
                _ => None,
            },
            retained: self.retained_toggle.as_ref().map(|view| view.state()),
            highlight: self.highlight.clone(),
            pointer: self.pointer,
            effect: self.effect_graphic,
            button: self.button_recipe.clone(),
            origin: self.window_origin,
        };
        let mut bytes = 1u32.to_le_bytes().to_vec();
        bytes.extend(
            postcard::to_allocvec(&state).map_err(|_| Error("menu checkpoint encoding failed"))?,
        );
        if bytes.len() > 65536 {
            return Err(Error("menu checkpoint size limit"));
        }
        Ok(bytes)
    }
    pub fn from_checkpoint(bytes: &[u8], assets: &Assets<'_, '_>) -> Result<Self, Error> {
        if !(4..=65536).contains(&bytes.len()) || bytes[..4] != 1u32.to_le_bytes() {
            return Err(Error("unsupported menu checkpoint"));
        }
        let (state, trailing): (Checkpoint, _) =
            postcard::take_from_bytes(&bytes[4..]).map_err(|_| Error("invalid menu checkpoint"))?;
        if !trailing.is_empty() {
            return Err(Error("trailing menu checkpoint data"));
        }
        state.navigation.validate_snapshot()?;
        state.highlight.validate_snapshot()?;
        if let Some(list) = &state.list {
            list.validate_snapshot()?;
        }
        let page = state.navigation.page();
        if state.sound.is_some() != (page == Some(Page::Sound))
            || state.widget.is_some() != matches!(page, Some(Page::Controls | Page::Subtitles))
            || (state.highlight.owner() == Some(crate::menu_highlight::Owner::Effect))
                != state.effect.is_some()
            || ![[20, 16], [72, 23], [98, 53]].contains(&state.origin)
        {
            return Err(Error("inconsistent menu checkpoint"));
        }
        if let Some(page) = page {
            let origin = match page {
                Page::Main => [72, 23],
                Page::Restart | Page::Quit => [98, 53],
                _ => [20, 16],
            };
            if state.origin != origin {
                return Err(Error("menu checkpoint origin mismatch"));
            }
            match page {
                Page::Save | Page::Load => {
                    let mode = if page == Page::Save {
                        Mode::Save
                    } else {
                        Mode::Load
                    };
                    if state.list.as_ref().is_none_or(|list| list.mode() != mode) {
                        return Err(Error("menu checkpoint list mismatch"));
                    }
                }
                _ if state.list.is_some() => return Err(Error("unexpected menu checkpoint list")),
                _ => {}
            }
        }
        if let Some(warp) = state.cursor_warp {
            let expected = match page {
                Some(Page::Save) => Some([262, 91]),
                Some(Page::Restart | Page::Quit) => Some([180, 90]),
                _ => None,
            };
            if Some(warp) != expected {
                return Err(Error("menu checkpoint cursor warp mismatch"));
            }
        }
        let mut menu = Self::new(state.settings)?;
        menu.navigation = state.navigation;
        // Restored selection uses its captured catalog, not the current device catalog.
        let reconstruct = Assets {
            saves: &[],
            resources: assets.resources,
            strings: assets.strings,
            declarations: assets.declarations,
        };
        menu.rebuild(&reconstruct)?;
        if let Some(sound) = state.sound {
            menu.display = Some(Display::Sound(
                SoundMenu::from_state(
                    assets.resources,
                    assets.strings,
                    assets.declarations,
                    sound,
                )?
                .into(),
            ));
        }
        if let Some(widget) = state.widget {
            let widget = SettingsMenu::from_state(
                assets.resources,
                assets.strings,
                assets.declarations,
                widget,
            )?;
            if (widget.kind() == Kind::Controls) != (page == Some(Page::Controls)) {
                return Err(Error("menu checkpoint widget kind mismatch"));
            }
            menu.display = Some(Display::Settings(widget.into()));
        }
        menu.retained_toggle = state
            .retained
            .map(|state| {
                SettingsMenu::from_state(
                    assets.resources,
                    assets.strings,
                    assets.declarations,
                    state,
                )
                .map(Box::new)
            })
            .transpose()?;
        menu.button_overlay = state
            .button
            .as_ref()
            .map(|recipe| recipe.rebuild(assets))
            .transpose()?;
        menu.button_recipe = state.button;
        if matches!(state.effect, Some(EffectGraphic::Button)) && menu.button_overlay.is_none()
            || matches!(state.effect, Some(EffectGraphic::CurrentToggle))
                && !matches!(menu.display, Some(Display::Settings(_)))
            || matches!(state.effect, Some(EffectGraphic::RetainedToggle))
                && menu.retained_toggle.is_none()
        {
            return Err(Error("missing menu checkpoint effect owner"));
        }
        menu.list = state.list;
        menu.cursor_warp = state.cursor_warp;
        menu.highlight = state.highlight;
        menu.pointer = state.pointer;
        menu.effect_graphic = state.effect;
        menu.window_origin = state.origin;
        if matches!(menu.effect_graphic, Some(EffectGraphic::CurrentToggle))
            && menu.settings_mut().is_none_or(|view| !view.has_overlay())
            || matches!(menu.effect_graphic, Some(EffectGraphic::RetainedToggle))
                && menu
                    .retained_toggle
                    .as_ref()
                    .is_none_or(|view| !view.has_overlay())
        {
            return Err(Error("menu checkpoint effect has no graphic"));
        }
        if let Some(crate::menu_highlight::Owner::Hover(_)) = menu.highlight.owner() {
            let mut probe = menu.clone();
            probe.highlight.reset();
            probe.point_boxes(menu.pointer);
            if probe.highlight.owner() != menu.highlight.owner() {
                return Err(Error("menu checkpoint hover differs from pointer"));
            }
        }
        let active_toggle = menu.settings_mut().is_some_and(|view| view.toggle_busy());
        let retained_toggle = menu
            .retained_toggle
            .as_ref()
            .is_some_and(|view| view.toggle_busy());
        if usize::from(active_toggle)
            + usize::from(retained_toggle)
            + usize::from(menu.navigation.busy())
            > 1
        {
            return Err(Error("multiple menu checkpoint effects"));
        }
        Ok(menu)
    }
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
            button_recipe: None,
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
        // invCursor returns before invBoxes while a slider owns the drag.
        if self.dragging() {
            return;
        }
        self.point_boxes(pointer);
    }
    fn point_boxes(&mut self, pointer: Option<[i32; 2]>) {
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
        self.button_recipe = None;
    }
    /// Script SUBTITLES writes the same setting observed by live and retained
    /// subtitle widgets. Preserve their pending toggle animation and timing.
    pub(crate) fn set_subtitles(&mut self, enabled: bool) {
        self.settings.subtitles = enabled;
        if let Some(view) = self
            .settings_mut()
            .filter(|view| view.kind() == Kind::Subtitles)
        {
            view.set_script_enabled(enabled);
        }
        if let Some(view) = self
            .retained_toggle
            .as_mut()
            .filter(|view| view.kind() == Kind::Subtitles)
        {
            view.set_script_enabled(enabled);
        }
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
    /// InvKeyIn consumes character/backspace input in both Save and Load,
    /// but only Save is editable. Bytes refer to the original game font.
    pub fn edit(
        &mut self,
        resources: &Resources<'_>,
        byte: u8,
        backspace: bool,
    ) -> Result<bool, Error> {
        let (Some(Display::Saves(view)), Some(list)) = (&self.display, &mut self.list) else {
            return Ok(false);
        };
        list.edit(resources, &view.font, byte, backspace)
    }

    pub fn scroll(&mut self, action: crate::save_menu::Scroll) -> bool {
        self.list_mut().is_some_and(|list| list.scroll(action))
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
    pub fn area_at(&self, point: [i32; 2]) -> Option<crate::conversation_window::Area> {
        let frame = match self.display.as_ref() {
            Some(Display::Main(view)) => &view.frame,
            Some(Display::Confirmation(view)) => &view.frame,
            Some(Display::Sound(view)) => &view.frame,
            Some(Display::Settings(view)) => &view.frame,
            Some(Display::Saves(view)) => &view.frame,
            None => return None,
        };
        Some(frame.layout.inventory_area_at(point))
    }

    pub fn dragging(&self) -> bool {
        match &self.display {
            Some(Display::Sound(view)) => view.dragging(),
            Some(Display::Settings(view)) => view.dragging(),
            Some(Display::Saves(_)) => self.list.as_ref().is_some_and(List::dragging),
            _ => false,
        }
    }

    pub fn begin_drag(
        &mut self,
        resources: &Resources<'_>,
        point: [i32; 2],
    ) -> Result<bool, Error> {
        // INV_CONF goes directly to whichMenuBox, before generic invArea checks.
        if self.dragging() {
            return Ok(false);
        }
        let mut next = self.clone();
        let started = match &mut next.display {
            Some(Display::Sound(view)) => view.begin_drag(resources, point)?,
            Some(Display::Settings(view)) => view.begin_drag(resources, point)?,
            Some(Display::Saves(_)) => next.list.as_mut().is_some_and(|list| {
                list.slider_at(point) == Some(crate::save_menu::SliderHit::Thumb)
                    && list.begin_slider()
            }),
            _ => false,
        };
        *self = next;
        Ok(started)
    }

    /// Preserve each widget's overshoot and quantization; callers apply the
    /// resulting settings immediately and redraw even if the value is unchanged.
    pub fn drag_by(&mut self, delta: [i32; 2]) -> Result<(), Error> {
        let mut next = self.clone();
        match &mut next.display {
            Some(Display::Sound(view)) => {
                view.drag(delta[0])?;
            }
            Some(Display::Settings(view)) => {
                view.drag(delta[0])?;
            }
            Some(Display::Saves(_)) => {
                if let Some(list) = &mut next.list {
                    list.drag_slider(delta[1])?;
                }
            }
            _ => {}
        }
        next.settings = next.settings();
        *self = next;
        Ok(())
    }

    pub fn end_drag(&mut self) -> Result<(), Error> {
        if !self.dragging() {
            return Ok(());
        }
        let mut next = self.clone();
        match &mut next.display {
            Some(Display::Sound(view)) => view.end_drag()?,
            Some(Display::Settings(view)) => view.end_drag()?,
            Some(Display::Saves(_)) => {
                if let Some(list) = &mut next.list {
                    list.end_slider();
                }
            }
            _ => {}
        }
        *self = next;
        Ok(())
    }

    /// V1 invWalkTo/invAction after physical single/double-click arbitration.
    /// Right-click uses the single-click path for configuration windows.
    pub fn click(&mut self, point: [i32; 2], double: bool) -> Result<Dispatch, Error> {
        use crate::conversation_window::Area;
        // invAction ends a configuration scrollbar drag, but mixing-desk
        // controls keep their drag across a double action (ID_MDCONT).
        if double && matches!(self.display, Some(Display::Saves(_))) {
            self.end_drag()?;
        }
        let Some(area) = self.area_at(point) else {
            return Ok(self.dispatch(Outcome::default()));
        };
        match area {
            Area::Outside if !double => {
                let outcome = self.close();
                return Ok(self.dispatch(outcome));
            }
            Area::Body => {}
            _ => return Ok(self.dispatch(Outcome::default())),
        }
        if self.list_mut().is_some() {
            if List::row_at(point).is_some() {
                return self.click_row(point, double);
            }
            let list = self.list_mut().unwrap();
            use crate::save_menu::{Scroll, SliderHit};
            let action = match list.slider_at(point) {
                Some(SliderHit::Up) => Some(Scroll::RowUp),
                Some(SliderHit::Down) => Some(Scroll::RowDown),
                Some(SliderHit::PageUp) => Some(Scroll::PageUp),
                Some(SliderHit::PageDown) => Some(Scroll::PageDown),
                _ => None,
            };
            if let Some(action) = action {
                list.scroll(action);
                return Ok(self.dispatch(Outcome::default()));
            }
        }
        self.click_settings(point, double);
        self.click_button(point);
        Ok(self.dispatch(Outcome::default()))
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
            let recipe = ButtonRecipe {
                choice,
                image,
                position: [
                    next.window_origin[0] + relative[0],
                    next.window_origin[1] + relative[1],
                ],
            };
            next.button_overlay = Some(recipe.rebuild(assets)?);
            next.button_recipe = Some(recipe);
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
