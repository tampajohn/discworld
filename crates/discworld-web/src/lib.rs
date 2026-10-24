//! Browser boundary. Original assets, engine, renderer, DSP and saves are shared
//! with native. JavaScript owns presentation, input timing and persistence.
use discworld_audio::{MusicOutput, session};
use discworld_core::{
    audio::SampleBank,
    engine::{Engine, PolygonInteraction, Status},
    graphics::Resources,
    inventory::Scroll,
    iso::Iso,
    music,
    resource::parse_index,
    text::Strings,
};
use std::ops::Range;
use wasm_bindgen::prelude::*;

type Result<T> = std::result::Result<T, String>;
fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Portable slot metadata boundary. Full session restoration still uses Game.load.
#[wasm_bindgen]
pub struct SlotInfo {
    pub id: u32,
    pub timestamp_ms: u64,
    description: Vec<u8>,
    session: Vec<u8>,
}
#[wasm_bindgen]
impl SlotInfo {
    #[wasm_bindgen(getter)]
    pub fn description(&self) -> Vec<u8> {
        self.description.clone()
    }
    #[wasm_bindgen(getter)]
    pub fn session(&self) -> Vec<u8> {
        self.session.clone()
    }
}
#[wasm_bindgen]
pub fn encode_slot(
    id: u32,
    timestamp_ms: u64,
    description: &[u8],
    session: &[u8],
) -> Result<Vec<u8>> {
    discworld_audio::slot::encode(id, timestamp_ms, description, session).map_err(error)
}
#[wasm_bindgen]
pub fn decode_slot(bytes: &[u8]) -> Result<SlotInfo> {
    let slot = discworld_audio::slot::decode(bytes).map_err(error)?;
    Ok(SlotInfo {
        id: slot.id,
        timestamp_ms: slot.timestamp_ms,
        description: slot.description.to_vec(),
        session: slot.session.to_vec(),
    })
}

struct Assets {
    iso: Vec<u8>,
    identity: [u8; 32],
    names: Vec<String>,
    intro_title: Option<discworld_core::graphics::Handle>,
    resources: Vec<Range<usize>>,
    sample_index: Range<usize>,
    samples: Range<usize>,
    music: Range<usize>,
    text: Range<usize>,
}
impl Assets {
    fn new(data: Vec<u8>) -> Result<Self> {
        let iso = Iso::parse(&data).map_err(error)?;
        let range = |name: &str| {
            iso.entries
                .iter()
                .find(|e| e.path.eq_ignore_ascii_case(name))
                .map(|e| e.range.clone())
                .ok_or_else(|| format!("missing {name}"))
        };
        let index = parse_index(&data[range("/DISCWLD/INDEX")?]).map_err(error)?;
        let resources = index
            .iter()
            .map(|e| range(&format!("/DISCWLD/{}", e.name)))
            .collect::<Result<Vec<_>>>()?;
        let sample_index = range("/DISCWLD/ENGLISH.IDX")?;
        let samples = range("/DISCWLD/ENGLISH.SMP")?;
        let music = range("/DISCWLD/MIDI.DAT")?;
        let text = range("/DISCWLD/ENGLISH.TXT")?;
        Ok(Self {
            identity: session::asset_identity(&data),
            iso: data,
            intro_title: discworld_core::intro::title_scene(&index).map_err(error)?,
            names: index.into_iter().map(|e| e.name).collect(),
            resources,
            sample_index,
            samples,
            music,
            text,
        })
    }
    fn resources(&self) -> Resources<'_> {
        Resources(
            self.resources
                .iter()
                .map(|r| &self.iso[r.clone()])
                .collect(),
        )
    }
    fn bank(&self) -> Result<SampleBank<'_>> {
        SampleBank::new(
            &self.iso[self.sample_index.clone()],
            &self.iso[self.samples.clone()],
        )
        .map_err(error)
    }
}

/// Bounded upload avoids retaining a second complete ISO in JavaScript memory.
/// The original ISO remains byte-identical; it is parsed only after completion.
#[wasm_bindgen]
pub struct GameUpload {
    bytes: Vec<u8>,
    expected: usize,
}

#[wasm_bindgen]
impl GameUpload {
    #[wasm_bindgen(constructor)]
    pub fn new(size: u32) -> Result<GameUpload> {
        if size == 0 || size > 1024 * 1024 * 1024 {
            return Err("ISO upload must be between 1 byte and 1 GiB".into());
        }
        let expected = size as usize;
        let mut bytes = Vec::new();
        bytes.try_reserve_exact(expected).map_err(error)?;
        Ok(Self { bytes, expected })
    }
    pub fn append(&mut self, bytes: &[u8]) -> Result<()> {
        if bytes.len() > 1024 * 1024 {
            return Err("ISO upload chunks are limited to 1 MiB".into());
        }
        if bytes.len() > self.expected - self.bytes.len() {
            return Err("ISO upload exceeds declared size".into());
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
    pub fn received(&self) -> u32 {
        self.bytes.len() as u32
    }
    pub fn finish(self, soundfont: Vec<u8>) -> Result<BrowserGame> {
        if self.bytes.len() != self.expected {
            return Err("ISO upload is incomplete".into());
        }
        BrowserGame::new(self.bytes, soundfont)
    }
}

#[wasm_bindgen]
pub struct BrowserGame {
    assets: Assets,
    font: Vec<u8>,
    engine: Engine,
    audio: MusicOutput,
    audio_headroom: bool,
    fault: Option<String>,
    // Host-only request retained behind a same-cycle menu operation.
    quit_pending: bool,
    menu_entries: Vec<discworld_core::save_menu::Entry>,
    menu_requests: std::collections::VecDeque<discworld_core::menu_controller::Dispatch>,
}

#[wasm_bindgen]
impl BrowserGame {
    #[wasm_bindgen(constructor)]
    pub fn new(iso: Vec<u8>, soundfont: Vec<u8>) -> Result<BrowserGame> {
        let assets = Assets::new(iso)?;
        let resources = assets.resources();
        let engine = Engine::bootstrap_with_title(&resources, assets.intro_title).map_err(error)?;
        assets.bank()?;
        music::archive(&assets.iso[assets.music.clone()]).map_err(error)?;
        Strings::new(&assets.iso[assets.text.clone()]).map_err(error)?;
        let audio = MusicOutput::new(&soundfont, 22050).map_err(error)?;
        Ok(Self {
            assets,
            font: soundfont,
            engine,
            audio,
            audio_headroom: false,
            fault: None,
            quit_pending: false,
            menu_entries: Vec::new(),
            menu_requests: std::collections::VecDeque::new(),
        })
    }

    /// Presentation preference: lower the sum before saturation, not after it.
    /// Deliberately excluded from game saves and DSP checkpoints.
    pub fn set_audio_headroom(&mut self, enabled: bool) {
        self.audio_headroom = enabled;
    }

    /// Advance at most one second (24 ticks). Return interleaved stereo f32 at
    /// 22050 Hz, including speech. Muting must not skip engine/DSP advancement.
    pub fn advance(&mut self, ticks: u32) -> Result<Vec<f32>> {
        if ticks > 24 {
            return Err("advance is limited to 24 ticks".into());
        }
        if let Some(fault) = &self.fault {
            return Err(fault.clone());
        }
        let resources = self.assets.resources();
        let bank = self.assets.bank()?;
        let tracks = music::archive(&self.assets.iso[self.assets.music.clone()]).map_err(error)?;
        let strings = Strings::new(&self.assets.iso[self.assets.text.clone()]).map_err(error)?;
        let result = (|| {
            let mut output = Vec::new();
            for _ in 0..ticks {
                if !self.menu_requests.is_empty() || self.quit_pending {
                    break;
                }
                let (status, dispatch) = self
                    .engine
                    .tick_with_configuration(
                        &resources,
                        Some(&bank),
                        Some(&tracks),
                        &strings,
                        &self.menu_entries,
                    )
                    .map_err(error)?;
                self.quit_pending |= status == Status::Quit
                    && dispatch.outcome.request
                        != Some(discworld_core::menu_navigation::Request::Quit);
                if dispatch.outcome.request.is_some() || dispatch.outcome.write_settings {
                    self.menu_requests.push_back(dispatch);
                }
                let mut audio = self
                    .audio
                    .render_engine_music(&self.engine)
                    .map_err(error)?;
                if audio.len() != self.engine.audio_frame.len() * 2 {
                    return Err("audio clock mismatch".into());
                }
                for (stereo, speech) in audio.chunks_exact_mut(2).zip(&self.engine.audio_frame) {
                    for sample in stereo {
                        *sample =
                            discworld_audio::mix::sample(*sample, *speech, self.audio_headroom);
                    }
                }
                output.extend(audio);
                if status == Status::Quit {
                    // The queued quit request uses the same host lifecycle as
                    // the original menu, without latching a runtime fault.
                    break;
                }
                if status != Status::Running {
                    return Err(format!("runtime stopped: {status:?}"));
                }
            }
            Ok(output)
        })();
        if let Err(reason) = &result {
            self.fault = Some(reason.clone());
        }
        result
    }

    /// Original 320x200 RGBA pixels, including game fonts, cursor and overlays.
    pub fn frame(&self) -> Result<Vec<u8>> {
        self.engine
            .render(&self.assets.resources())
            .map(|s| s.rgba)
            .map_err(error)
    }
    pub fn tick(&self) -> u64 {
        self.engine.ticks
    }
    pub fn scene(&self) -> String {
        self.assets.names[self.engine.scene.resource()].clone()
    }
    pub fn move_pointer(&mut self, x: i32, y: i32) {
        self.engine.move_cursor([x.clamp(0, 319), y.clamp(0, 199)]);
    }
    pub fn move_pointer_checked(&mut self, x: i32, y: i32) -> Result<()> {
        self.engine
            .move_cursor_with_configuration(
                &self.assets.resources(),
                [x.clamp(0, 319), y.clamp(0, 199)],
            )
            .map_err(error)
    }
    pub fn inventory_dragging(&self) -> bool {
        self.engine.inventory_dragging()
            || self
                .engine
                .configuration
                .as_ref()
                .is_some_and(|menu| menu.dragging())
    }
    /// Physical button that releases a drag restored without host pointer state.
    pub fn drag_button(&self) -> Option<String> {
        self.inventory_dragging().then(|| {
            if self
                .engine
                .configuration
                .as_ref()
                .is_some_and(|menu| menu.settings().swap_buttons)
            {
                "right".to_string()
            } else {
                "left".to_string()
            }
        })
    }
    pub fn menu_active(&self) -> bool {
        self.engine
            .configuration
            .as_ref()
            .is_some_and(|menu| menu.page().is_some())
    }
    pub fn menu_page(&self) -> Option<String> {
        self.engine
            .configuration
            .as_ref()
            .and_then(|menu| menu.page())
            .map(|page| format!("{page:?}"))
    }
    pub fn menu_editing(&self) -> bool {
        self.engine
            .configuration
            .as_ref()
            .is_some_and(|menu| menu.page() == Some(discworld_core::menu_navigation::Page::Save))
    }
    pub fn clear_menu_catalog(&mut self) {
        self.menu_entries.clear();
    }
    pub fn add_menu_slot(&mut self, bytes: &[u8]) -> Result<()> {
        let slot = discworld_audio::slot::decode(bytes).map_err(error)?;
        if self.menu_entries.iter().any(|entry| entry.id == slot.id) {
            return Err("duplicate menu slot".into());
        }
        if self.menu_entries.len() >= 100 {
            return Err("menu slot capacity".into());
        }
        // Validate the entire session against this game's assets before offering it.
        let resources = self.assets.resources();
        let bank = self.assets.bank()?;
        let tracks = music::archive(&self.assets.iso[self.assets.music.clone()]).map_err(error)?;
        let strings = Strings::new(&self.assets.iso[self.assets.text.clone()]).map_err(error)?;
        session::decode(
            slot.session,
            self.assets.identity,
            &resources,
            &bank,
            &tracks,
            &self.font,
            &strings,
        )
        .map_err(error)?;
        self.menu_entries.push(discworld_core::save_menu::Entry {
            id: slot.id,
            description: slot.description.to_vec(),
        });
        Ok(())
    }
    pub fn menu_preferences(&mut self, json: &str) -> Result<()> {
        if json.len() > 4096 {
            return Err("preferences size limit".into());
        }
        let settings: discworld_core::menu_controller::Settings =
            serde_json::from_str(json).map_err(error)?;
        let menu =
            discworld_core::menu_controller::Controller::new(settings.clone()).map_err(error)?;
        if self.engine.configuration.is_none() {
            self.engine.subtitles = settings.subtitles;
            self.engine.configuration = Some(menu);
        }
        Ok(())
    }
    pub fn open_menu(&mut self, page: &str) -> Result<()> {
        use discworld_core::{
            menu_controller::{Controller, Settings},
            menu_navigation::Key,
        };
        let key = match page {
            "main" => Key::Main,
            "save" => Key::Save,
            "load" => Key::Load,
            "quit" => Key::Quit,
            _ => return Err("unknown menu".into()),
        };
        if self.engine.configuration.is_none() {
            self.engine.configuration = Some(
                Controller::new(Settings {
                    volumes: [255; 3],
                    double_click: 10,
                    swap_buttons: false,
                    text_speed: 0,
                    subtitles: self.engine.subtitles,
                    flipped: false,
                })
                .map_err(error)?,
            );
        }
        let strings = Strings::new(&self.assets.iso[self.assets.text.clone()]).map_err(error)?;
        self.engine
            .open_configuration(&self.assets.resources(), &strings, &self.menu_entries, key)
            .map_err(error)?;
        Ok(())
    }
    pub fn edit_menu(&mut self, text: &str, backspace: bool) -> Result<()> {
        if text.len() > 80 || !text.is_ascii() || text.bytes().any(|byte| byte.is_ascii_control()) {
            return Err("invalid menu text".into());
        }
        let resources = self.assets.resources();
        if backspace {
            self.engine
                .edit_configuration(&resources, 8, true)
                .map_err(error)?;
        }
        for byte in text.bytes() {
            self.engine
                .edit_configuration(&resources, byte, false)
                .map_err(error)?;
        }
        Ok(())
    }
    /// Drain after each input/advance, before accepting the next command.
    pub fn take_menu_request(&mut self) -> Result<Option<String>> {
        use discworld_core::{menu_navigation::Request, save_menu::Target};
        let dispatch = match self.menu_requests.pop_front() {
            Some(dispatch) => dispatch,
            None if self.quit_pending => {
                self.quit_pending = false;
                discworld_core::menu_controller::Dispatch {
                    outcome: discworld_core::menu_navigation::Outcome {
                        request: Some(Request::Quit),
                        ..Default::default()
                    },
                    selection: None,
                }
            }
            None => return Ok(None),
        };
        let request = dispatch.outcome.request.map(|request| match request {
            Request::Save => "save",
            Request::Load => "load",
            Request::Quit => "quit",
            Request::Restart => "restart",
        });
        let selection = dispatch.selection.map(|(target, description)| serde_json::json!({ "id": match target { Target::New => None, Target::Existing(id) => Some(id) }, "description": description }));
        let settings = if dispatch.outcome.write_settings {
            self.engine
                .configuration
                .as_ref()
                .map(|menu| menu.settings())
        } else {
            None
        };
        serde_json::to_string(&serde_json::json!({ "request": request, "selection": selection, "settings": settings })).map(Some).map_err(error)
    }
    /// Physical edges use the shared click timing and original menu dispatch.
    /// Touch controls also expose explicit walk/action/look gestures.
    pub fn input(&mut self, command: &str) -> Result<()> {
        if let Some(fault) = &self.fault {
            return Err(fault.clone());
        }
        if !self.menu_requests.is_empty() {
            return Err("menu request must be handled before input".into());
        }
        let active = self.menu_active();
        let engine = &mut self.engine;
        let resources = self.assets.resources();
        let point = engine.control.cursor;
        match command {
            "left-down" | "left-up" | "right-down" | "right-up" => {
                let button = if command.starts_with("left") {
                    discworld_core::mouse::Button::Left
                } else {
                    discworld_core::mouse::Button::Right
                };
                let dispatch = engine
                    .mouse_button_with_configuration(&resources, button, command.ends_with("down"))
                    .map_err(error)?;
                if dispatch.outcome.request.is_some() || dispatch.outcome.write_settings {
                    self.menu_requests.push_back(dispatch);
                }
            }
            "escape" => {
                let strings =
                    Strings::new(&self.assets.iso[self.assets.text.clone()]).map_err(error)?;
                let outcome = engine
                    .escape_input(&resources, &strings, &self.menu_entries)
                    .map_err(error)?;
                if outcome.request.is_some() || outcome.write_settings {
                    self.menu_requests
                        .push_back(discworld_core::menu_controller::Dispatch {
                            outcome,
                            selection: None,
                        });
                }
            }
            "up" | "down" | "page-up" | "page-down" | "home" | "end" if active => {
                use discworld_core::save_menu::Scroll;
                engine
                    .scroll_configuration(match command {
                        "up" => Scroll::RowUp,
                        "down" => Scroll::RowDown,
                        "page-up" => Scroll::PageUp,
                        "page-down" => Scroll::PageDown,
                        "home" => Scroll::Home,
                        _ => Scroll::End,
                    })
                    .map_err(error)?;
            }
            "walk" | "action" | "look" | "click" if active => {
                let interaction = match command {
                    "action" => PolygonInteraction::Action,
                    "look" => PolygonInteraction::Look,
                    _ => PolygonInteraction::WalkTo,
                };
                let dispatch = engine
                    .configuration_click(point, interaction)
                    .map_err(error)?;
                if dispatch.outcome.request.is_some() || dispatch.outcome.write_settings {
                    self.menu_requests.push_back(dispatch);
                }
            }
            "close" if active => {
                engine
                    .edit_configuration(&resources, 8, true)
                    .map_err(error)?;
            }
            "pockets" | "continue" if active => {}
            "continue" => engine.left_event().map_err(error)?,
            "start" => engine.user_event().map_err(error)?,
            "pockets" => {
                if matches!(engine.active_inventory(), Some(1 | 2)) {
                    engine.close_inventory_input(&resources).map_err(error)?;
                } else if let Some(lead) = &engine.declarations.lead {
                    engine
                        .interact_actor(&resources, lead.id as u32, PolygonInteraction::WalkTo)
                        .map_err(error)?;
                }
            }
            "close" => {
                if matches!(engine.active_inventory(), Some(1 | 2)) {
                    engine.close_inventory_input(&resources).map_err(error)?;
                } else {
                    engine
                        .interact_conversation(&resources, None)
                        .map_err(error)?;
                }
            }
            "up" | "down" => {
                engine.scroll_inventory(if command == "up" {
                    Scroll::Up
                } else {
                    Scroll::Down
                });
            }
            "click" => engine.primary_click(&resources).map_err(error)?,
            "walk" | "action" | "look" => {
                let event = match command {
                    "action" => PolygonInteraction::Action,
                    "look" => PolygonInteraction::Look,
                    _ => PolygonInteraction::WalkTo,
                };
                if let Some(slot) = engine.inventory_slot_at(point) {
                    engine
                        .interact_inventory(&resources, slot, event)
                        .map_err(error)?;
                } else {
                    engine
                        .interact_world(&resources, point, event)
                        .map_err(error)?;
                }
            }
            _ => return Err(format!("unknown input: {command}")),
        }
        Ok(())
    }
    pub fn save(&self) -> Result<Vec<u8>> {
        if let Some(fault) = &self.fault {
            return Err(fault.clone());
        }
        session::encode(&self.engine, &self.audio, self.assets.identity).map_err(error)
    }
    /// Prepare both components before replacing the live session. A rejected
    /// file leaves engine, DSP and existing fault state unchanged.
    pub fn load(&mut self, bytes: &[u8]) -> Result<()> {
        self.restore_session(bytes, false)
    }

    /// Player loads end intro mode and upgrade legacy runtime monitoring;
    /// exact runtime replay deliberately preserves the historical engine state.
    pub fn load_game(&mut self, bytes: &[u8]) -> Result<()> {
        self.restore_session(bytes, true)
    }

    fn restore_session(&mut self, bytes: &[u8], user_load: bool) -> Result<()> {
        let resources = self.assets.resources();
        let bank = self.assets.bank()?;
        let tracks = music::archive(&self.assets.iso[self.assets.music.clone()]).map_err(error)?;
        let mut restored = session::decode(
            bytes,
            self.assets.identity,
            &resources,
            &bank,
            &tracks,
            &self.font,
            &Strings::new(&self.assets.iso[self.assets.text.clone()]).map_err(error)?,
        )
        .map_err(error)?;
        if user_load {
            restored.engine.prepare_user_load().map_err(error)?;
        }
        self.engine = restored.engine;
        self.audio = restored.audio;
        self.fault = None;
        self.menu_requests.clear();
        Ok(())
    }
}

// Read-only native hit testing and UI-test observations, not exported through wasm-bindgen.
#[cfg(feature = "native-inspection")]
impl BrowserGame {
    pub fn native_inventory_contains(&self, x: i32, y: i32) -> bool {
        self.engine.inventory_contains_point([x, y])
    }

    pub fn native_inspection(&self) -> serde_json::Value {
        let lead = self
            .engine
            .declarations
            .lead
            .as_ref()
            .and_then(|lead| self.engine.movers.get(&lead.id));
        serde_json::json!({
            "control": self.engine.control.enabled,
            "world_origin": self.engine.world_origin,
            "cursor": self.engine.control.cursor,
            "lead_position": lead.map(|mover| mover.placement.position),
            "walk_target": lead.and_then(|mover| mover.walk_target),
            "inventory": self.engine.active_inventory(),
            "held_object": self.engine.held_object,
            "inventory_dragging": self.inventory_dragging(),
            "inventory_first_cell": self.engine.inventory_cell_position(0),
            "pockets": self.engine.declarations.windows[2].as_ref().map(|window| &window.contents),
            "enabled_tags": self.engine.polygons.active.iter()
                .filter(|polygon| polygon.enabled && polygon.definition.kind == discworld_core::polygon::Kind::Tag)
                .map(|polygon| polygon.definition.id).collect::<Vec<_>>(),
            "inventory_position": self.engine.active_inventory()
                .and_then(|slot| self.engine.declarations.windows.get(slot as usize))
                .and_then(|window| window.as_ref()).map(|window| window.position),
        })
    }
}

// Validation builds use precisely the same event decoder/application as the
// native recorded route. These exports are absent from the normal player.
#[cfg(feature = "replay-validation")]
#[wasm_bindgen]
impl BrowserGame {
    pub fn replay_input(&mut self, bytes: &[u8]) -> Result<()> {
        if let Some(fault) = &self.fault {
            return Err(fault.clone());
        }
        let inputs = discworld_replay::interactions(bytes, u32::MAX).map_err(error)?;
        if inputs.len() != 1 || u64::from(inputs[0].tick) != self.engine.ticks {
            return Err("replay requires one input at the current tick".into());
        }
        let scene = self.scene();
        let result = discworld_replay::apply(
            &inputs[0],
            &mut self.engine,
            &self.assets.resources(),
            &scene,
        )
        .map_err(error)
        .and_then(|accepted| {
            if accepted {
                // Match headless diagnostics: record dispatches since this input.
                self.engine.scene_calls = Some(std::collections::VecDeque::new());
                Ok(())
            } else {
                Err("recorded input was rejected".into())
            }
        });
        if let Err(reason) = &result {
            self.fault = Some(reason.clone());
        }
        result
    }
    pub fn has_restarted(&self) -> bool {
        self.engine.has_restarted()
    }
}

#[cfg(test)]
mod real_assets;

#[cfg(test)]
mod upload_tests {
    use super::GameUpload;
    #[test]
    fn upload_rejects_incomplete_overflow_and_large_chunks_without_partial_append() {
        assert!(GameUpload::new(0).is_err());
        assert!(GameUpload::new(1024 * 1024 * 1024 + 1).is_err());
        let mut upload = GameUpload::new(3).unwrap();
        upload.append(&[1, 2]).unwrap();
        assert!(upload.append(&[3, 4]).is_err());
        assert_eq!(upload.received(), 2);
        assert!(upload.append(&vec![0; 1024 * 1024 + 1]).is_err());
        assert_eq!(upload.received(), 2);
        assert!(matches!(upload.finish(Vec::new()), Err(error) if error.contains("incomplete")));
    }
}
