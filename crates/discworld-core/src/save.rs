// SPDX-License-Identifier: GPL-3.0-or-later
//! Versioned complete-runtime snapshots with explicit validation and restoration.
//! Host persistence is separate: decoding does not replace a running engine.
use crate::{engine::Engine, iso::Error};
use sha2::{Digest, Sha256};

const MAGIC: &[u8; 8] = b"DWRSAVE\0";
const VERSION: u32 = 1;
const INPUT_VERSION: u32 = 2;
const DROP_VERSION: u32 = 3;
const MOUSE_VERSION: u32 = 4;
const DRAG_VERSION: u32 = 5;
const SLIDER_VERSION: u32 = 6;
const GEOMETRY_VERSION: u32 = 7;
const RESIZE_VERSION: u32 = 8;
const HOVER_VERSION: u32 = 9;
const MENU_VERSION: u32 = 10;
const TALK_PHASE_VERSION: u32 = 11;
const SPEECH_CHANNEL_VERSION: u32 = 12;
const MOVER_REEL_VERSION: u32 = 13;
const ESCAPE_VERSION: u32 = 14;
const REEL_ESCAPE_VERSION: u32 = 15;
const SAMPLE_WAIT_VERSION: u32 = 16;
const INTRO_VERSION: u32 = 17;
const MOVER_VISIBILITY_VERSION: u32 = 18;
const SPLAY_VERSION: u32 = 19;
const CREATED_MOVER_VERSION: u32 = 20;
const REEL_TASK_VERSION: u32 = 21;
const RESTORED_REEL_VERSION: u32 = 22;
const SCROLL_TASK_VERSION: u32 = 23;
const WORLD_HOVER_VERSION: u32 = 24;
const CURSOR_TRAILS_VERSION: u32 = 25;
const MOVER_TASK_VERSION: u32 = 26;
const HEADER: usize = 80;
pub const MAX_SAVE_BYTES: usize = 16 * 1024 * 1024;

/// Captured at a tick boundary. External game archives are supplied separately.
/// The schema is tied to this engine format version, not ScummVM save files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    asset_id: [u8; 32],
    state: Engine,
    menu: Option<Vec<u8>>,
}

impl Snapshot {
    /// The host supplies a SHA-256 identity covering its original game assets.
    pub fn capture(engine: &Engine, asset_id: [u8; 32]) -> Self {
        Self {
            asset_id,
            state: engine.clone(),
            menu: None,
        }
    }

    pub fn tick(&self) -> u64 {
        self.state.ticks
    }

    /// Check active and saved script references against the supplied bytecode.
    /// This is one validation stage, not permission to activate the snapshot:
    /// animation, graphics, audio and other resource/state checks remain separate.
    pub fn verify_scripts(&self, resources: &crate::graphics::Resources<'_>) -> Result<(), Error> {
        self.state.validate_snapshot_scripts(resources)
    }

    /// Validate sample position and sequencer content against original archives.
    /// Host synthesizer/device restoration is not covered by this check.
    pub fn verify_audio(
        &self,
        bank: &crate::audio::SampleBank<'_>,
        tracks: &[crate::music::Track<'_>],
    ) -> Result<(), Error> {
        self.state.sample_player.validate_snapshot_bank(bank)?;
        self.state.music.validate_snapshot_tracks(tracks)
    }

    /// Validate current animation/image references and render without activation.
    /// Future scene transitions and host state require additional validation.
    pub fn preview(
        &self,
        resources: &crate::graphics::Resources<'_>,
    ) -> Result<crate::compositor::Surface, Error> {
        if self.menu.is_some() {
            return Err(Error("menu snapshot requires original strings"));
        }
        self.state.snapshot_preview(resources)
    }

    /// Construct a restored engine after validating its current scripts, audio,
    /// navigation geometry and drawable resources. The caller replaces its live
    /// engine only after this succeeds and its host audio is prepared.
    pub fn restore(
        self,
        resources: &crate::graphics::Resources<'_>,
        bank: &crate::audio::SampleBank<'_>,
        tracks: &[crate::music::Track<'_>],
    ) -> Result<Engine, Error> {
        if self.menu.is_some() {
            return Err(Error("menu snapshot requires original strings"));
        }
        self.state.validate_snapshot_structure()?;
        self.verify_scripts(resources)?;
        self.verify_audio(bank, tracks)?;
        self.state.validate_snapshot_polygons(resources)?;
        self.preview(resources)?;
        Ok(self.state)
    }

    pub fn restore_with_strings(
        mut self,
        resources: &crate::graphics::Resources<'_>,
        bank: &crate::audio::SampleBank<'_>,
        tracks: &[crate::music::Track<'_>],
        strings: &crate::text::Strings<'_>,
    ) -> Result<Engine, Error> {
        self.rebuild_menu(resources, strings)?;
        self.restore(resources, bank, tracks)
    }

    pub fn preview_with_strings(
        &self,
        resources: &crate::graphics::Resources<'_>,
        strings: &crate::text::Strings<'_>,
    ) -> Result<crate::compositor::Surface, Error> {
        let mut snapshot = self.clone();
        snapshot.rebuild_menu(resources, strings)?;
        snapshot.preview(resources)
    }

    fn rebuild_menu(
        &mut self,
        resources: &crate::graphics::Resources<'_>,
        strings: &crate::text::Strings<'_>,
    ) -> Result<(), Error> {
        if let Some(bytes) = self.menu.take() {
            let assets = crate::menu_controller::Assets {
                resources,
                strings,
                declarations: &self.state.declarations,
                saves: &[],
            };
            self.state.configuration = Some(crate::menu_controller::Controller::from_checkpoint(
                &bytes, &assets,
            )?);
        }
        Ok(())
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        self.state.validate_snapshot_structure()?;
        let mover_tasks = self.state.mover_tasks();
        if !mover_tasks.is_empty() {
            let mut legacy = self.clone();
            legacy.state.clear_mover_tasks();
            let payload = postcard::to_allocvec(&(legacy.to_bytes()?, mover_tasks))
                .map_err(|_| Error("snapshot mover tasks encode failed"))?;
            if payload.len() > MAX_SAVE_BYTES - HEADER {
                return Err(Error("save size limit"));
            }
            let mut bytes = Vec::with_capacity(HEADER + payload.len());
            bytes.extend_from_slice(MAGIC);
            bytes.extend_from_slice(&MOVER_TASK_VERSION.to_le_bytes());
            bytes.extend_from_slice(&self.asset_id);
            bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&Sha256::digest(&payload));
            bytes.extend_from_slice(&payload);
            return Ok(bytes);
        }
        if let Some(trails) = &self.state.cursor_trails {
            let mut legacy = self.clone();
            legacy.state.cursor_trails = None;
            let payload = postcard::to_allocvec(&(legacy.to_bytes()?, trails))
                .map_err(|_| Error("snapshot cursor trails encode failed"))?;
            if payload.len() > MAX_SAVE_BYTES - HEADER {
                return Err(Error("save size limit"));
            }
            let mut bytes = Vec::with_capacity(HEADER + payload.len());
            bytes.extend_from_slice(MAGIC);
            bytes.extend_from_slice(&CURSOR_TRAILS_VERSION.to_le_bytes());
            bytes.extend_from_slice(&self.asset_id);
            bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&Sha256::digest(&payload));
            bytes.extend_from_slice(&payload);
            return Ok(bytes);
        }
        let tasks = self.state.scene_tasks();
        if self.state.world_hover.is_some()
            || self.state.scheduled_scroll
            || !tasks.is_empty()
            || self.state.reel_dispatch_depth != [0; 2]
        {
            let mut legacy = self.clone();
            legacy.state.clear_scene_tasks();
            legacy.state.scheduled_scroll = false;
            legacy.state.reel_dispatch_depth = [0; 2];
            legacy.state.world_hover = None;
            let version = if self.state.world_hover.is_some() {
                WORLD_HOVER_VERSION
            } else if self.state.scheduled_scroll {
                SCROLL_TASK_VERSION
            } else if tasks
                .iter()
                .any(|(_, t)| matches!(t, crate::engine::SceneTask::Restored { .. }))
            {
                RESTORED_REEL_VERSION
            } else {
                REEL_TASK_VERSION
            };
            let payload = if let Some(hover) = &self.state.world_hover {
                postcard::to_allocvec(&(
                    legacy.to_bytes()?,
                    tasks,
                    self.state.reel_dispatch_depth,
                    hover,
                ))
            } else {
                postcard::to_allocvec(&(legacy.to_bytes()?, tasks, self.state.reel_dispatch_depth))
            }
            .map_err(|_| Error("snapshot scene task encode failed"))?;
            if payload.len() > MAX_SAVE_BYTES - HEADER {
                return Err(Error("save size limit"));
            }
            let mut bytes = Vec::with_capacity(HEADER + payload.len());
            bytes.extend_from_slice(MAGIC);
            bytes.extend_from_slice(&version.to_le_bytes());
            bytes.extend_from_slice(&self.asset_id);
            bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&Sha256::digest(&payload));
            bytes.extend_from_slice(&payload);
            return Ok(bytes);
        }
        if !self.state.created_reel_movers.is_empty() {
            let created: Vec<(i32, u64)> = self
                .state
                .created_reel_movers
                .iter()
                .map(|(&actor, &playback)| (actor, playback))
                .collect();
            let mut legacy = self.clone();
            legacy.state.created_reel_movers.clear();
            let payload = postcard::to_allocvec(&(legacy.to_bytes()?, created))
                .map_err(|_| Error("snapshot created mover encode failed"))?;
            if payload.len() > MAX_SAVE_BYTES - HEADER {
                return Err(Error("save size limit"));
            }
            let mut bytes = Vec::with_capacity(HEADER + payload.len());
            bytes.extend_from_slice(MAGIC);
            bytes.extend_from_slice(&CREATED_MOVER_VERSION.to_le_bytes());
            bytes.extend_from_slice(&self.asset_id);
            bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&Sha256::digest(&payload));
            bytes.extend_from_slice(&payload);
            return Ok(bytes);
        }
        if !self.state.splay_reels.is_empty() {
            let ids: Vec<u64> = self.state.splay_reels.iter().copied().collect();
            let mut legacy = self.clone();
            legacy.state.splay_reels.clear();
            let payload = postcard::to_allocvec(&(legacy.to_bytes()?, ids))
                .map_err(|_| Error("snapshot SPLAY encode failed"))?;
            if payload.len() > MAX_SAVE_BYTES - HEADER {
                return Err(Error("save size limit"));
            }
            let mut bytes = Vec::with_capacity(HEADER + payload.len());
            bytes.extend_from_slice(MAGIC);
            bytes.extend_from_slice(&SPLAY_VERSION.to_le_bytes());
            bytes.extend_from_slice(&self.asset_id);
            bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&Sha256::digest(&payload));
            bytes.extend_from_slice(&payload);
            return Ok(bytes);
        }
        let hidden: Vec<i32> = self
            .state
            .movers
            .iter()
            .filter_map(|(&id, mover)| mover.hidden.then_some(id))
            .collect();
        if !hidden.is_empty() {
            let mut legacy = self.clone();
            for mover in legacy.state.movers.values_mut() {
                mover.hidden = false;
            }
            let payload = postcard::to_allocvec(&(legacy.to_bytes()?, hidden))
                .map_err(|_| Error("snapshot mover visibility encode failed"))?;
            if payload.len() > MAX_SAVE_BYTES - HEADER {
                return Err(Error("save size limit"));
            }
            let mut bytes = Vec::with_capacity(HEADER + payload.len());
            bytes.extend_from_slice(MAGIC);
            bytes.extend_from_slice(&MOVER_VISIBILITY_VERSION.to_le_bytes());
            bytes.extend_from_slice(&self.asset_id);
            bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&Sha256::digest(&payload));
            bytes.extend_from_slice(&payload);
            return Ok(bytes);
        }
        if let Some(intro) = &self.state.intro {
            let mut legacy = self.clone();
            legacy.state.intro = None;
            let payload = postcard::to_allocvec(&(legacy.to_bytes()?, intro))
                .map_err(|_| Error("snapshot intro encode failed"))?;
            if payload.len() > MAX_SAVE_BYTES - HEADER {
                return Err(Error("save size limit"));
            }
            let mut bytes = Vec::with_capacity(HEADER + payload.len());
            bytes.extend_from_slice(MAGIC);
            bytes.extend_from_slice(&INTRO_VERSION.to_le_bytes());
            bytes.extend_from_slice(&self.asset_id);
            bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&Sha256::digest(&payload));
            bytes.extend_from_slice(&payload);
            return Ok(bytes);
        }
        let serials = self.state.sample_wait_checkpoint();
        if serials.iter().any(Option::is_some) {
            let mut legacy = self.clone();
            legacy.state.clear_sample_wait_checkpoint();
            let payload = postcard::to_allocvec(&(legacy.to_bytes()?, serials))
                .map_err(|_| Error("snapshot sample wait encode failed"))?;
            if payload.len() > MAX_SAVE_BYTES - HEADER {
                return Err(Error("save size limit"));
            }
            let mut bytes = Vec::with_capacity(HEADER + payload.len());
            bytes.extend_from_slice(MAGIC);
            bytes.extend_from_slice(&SAMPLE_WAIT_VERSION.to_le_bytes());
            bytes.extend_from_slice(&self.asset_id);
            bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&Sha256::digest(&payload));
            bytes.extend_from_slice(&payload);
            return Ok(bytes);
        }
        if !self.state.reel_escape.is_empty() {
            let mut legacy = self.clone();
            let captures: Vec<_> = std::mem::take(&mut legacy.state.reel_escape)
                .into_iter()
                .collect();
            let payload = postcard::to_allocvec(&(legacy.to_bytes()?, captures))
                .map_err(|_| Error("snapshot reel escape encode failed"))?;
            if payload.len() > MAX_SAVE_BYTES - HEADER {
                return Err(Error("save size limit"));
            }
            let mut bytes = Vec::with_capacity(HEADER + payload.len());
            bytes.extend_from_slice(MAGIC);
            bytes.extend_from_slice(&REEL_ESCAPE_VERSION.to_le_bytes());
            bytes.extend_from_slice(&self.asset_id);
            bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&Sha256::digest(&payload));
            bytes.extend_from_slice(&payload);
            return Ok(bytes);
        }
        if let Some((event, captures)) = self.state.escape_checkpoint() {
            let mut legacy = self.clone();
            legacy.state.clear_escape_checkpoint();
            let payload = postcard::to_allocvec(&(legacy.to_bytes()?, event, captures))
                .map_err(|_| Error("snapshot escape encode failed"))?;
            if payload.len() > MAX_SAVE_BYTES - HEADER {
                return Err(Error("save size limit"));
            }
            let mut bytes = Vec::with_capacity(HEADER + payload.len());
            bytes.extend_from_slice(MAGIC);
            bytes.extend_from_slice(&ESCAPE_VERSION.to_le_bytes());
            bytes.extend_from_slice(&self.asset_id);
            bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&Sha256::digest(&payload));
            bytes.extend_from_slice(&payload);
            return Ok(bytes);
        }
        let reels: Vec<_> = self
            .state
            .movers
            .iter()
            .filter_map(|(&actor, mover)| mover.reel_state.map(|state| (actor, state)))
            .collect();
        if !reels.is_empty() {
            // A bounded, non-recursive legacy envelope preserves all existing
            // optional extensions and their exact encodings.
            let mut legacy = self.clone();
            for mover in legacy.state.movers.values_mut() {
                mover.reel_state = None;
            }
            let payload = postcard::to_allocvec(&(legacy.to_bytes()?, reels))
                .map_err(|_| Error("snapshot mover reel encode failed"))?;
            if payload.len() > MAX_SAVE_BYTES - HEADER {
                return Err(Error("save size limit"));
            }
            let mut bytes = Vec::with_capacity(HEADER + payload.len());
            bytes.extend_from_slice(MAGIC);
            bytes.extend_from_slice(&MOVER_REEL_VERSION.to_le_bytes());
            bytes.extend_from_slice(&self.asset_id);
            bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            bytes.extend_from_slice(&Sha256::digest(&payload));
            bytes.extend_from_slice(&payload);
            return Ok(bytes);
        }
        let extended = self.state.clicks != crate::click::State::default();
        let geometry = self.state.inventory_geometry != crate::inventory_geometry::State::default();
        let hover = self.state.inventory_hover != crate::inventory_hover::State::default();
        let menu = match &self.state.configuration {
            Some(menu) => Some(menu.checkpoint()?),
            None => self.menu.clone(),
        };
        let finished_talks = self.state.finished_talks();
        let speech = self.state.sample_player.is_speech();
        let payload = if speech || !finished_talks.is_empty() {
            postcard::to_allocvec(&(
                &self.state,
                &self.state.clicks,
                &self.state.mouse,
                self.state.inventory_drag,
                &self.state.inventory_slider,
                &self.state.inventory_geometry,
                &self.state.inventory_resize,
                &self.state.inventory_hover,
                &menu,
                &finished_talks,
            ))
        } else if let Some(menu) = &menu {
            postcard::to_allocvec(&(
                &self.state,
                &self.state.clicks,
                &self.state.mouse,
                self.state.inventory_drag,
                &self.state.inventory_slider,
                &self.state.inventory_geometry,
                &self.state.inventory_resize,
                &self.state.inventory_hover,
                menu,
            ))
        } else if hover {
            postcard::to_allocvec(&(
                &self.state,
                &self.state.clicks,
                &self.state.mouse,
                self.state.inventory_drag,
                &self.state.inventory_slider,
                &self.state.inventory_geometry,
                &self.state.inventory_resize,
                &self.state.inventory_hover,
            ))
        } else if let Some(resize) = &self.state.inventory_resize {
            postcard::to_allocvec(&(
                &self.state,
                &self.state.clicks,
                &self.state.mouse,
                self.state.inventory_drag,
                &self.state.inventory_geometry,
                resize,
            ))
        } else if geometry {
            postcard::to_allocvec(&(
                &self.state,
                &self.state.clicks,
                &self.state.mouse,
                self.state.inventory_drag,
                &self.state.inventory_slider,
                &self.state.inventory_geometry,
            ))
        } else if let Some(slider) = &self.state.inventory_slider {
            postcard::to_allocvec(&(
                &self.state,
                &self.state.clicks,
                &self.state.mouse,
                self.state.inventory_drag,
                slider,
            ))
        } else if let Some(slot) = self.state.inventory_drag {
            postcard::to_allocvec(&(&self.state, &self.state.clicks, &self.state.mouse, slot))
        } else if let Some(mouse) = &self.state.mouse {
            postcard::to_allocvec(&(&self.state, &self.state.clicks, mouse))
        } else if extended {
            postcard::to_allocvec(&(&self.state, &self.state.clicks))
        } else {
            postcard::to_allocvec(&self.state)
        }
        .map_err(|_| Error("snapshot encode failed"))?;
        if payload.len() > MAX_SAVE_BYTES - HEADER {
            return Err(Error("save size limit"));
        }
        let mut bytes = Vec::with_capacity(HEADER + payload.len());
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(
            &(if speech {
                SPEECH_CHANNEL_VERSION
            } else if !finished_talks.is_empty() {
                TALK_PHASE_VERSION
            } else if menu.is_some() {
                MENU_VERSION
            } else if hover {
                HOVER_VERSION
            } else if self.state.inventory_resize.is_some() {
                RESIZE_VERSION
            } else if geometry {
                GEOMETRY_VERSION
            } else if self.state.inventory_slider.is_some() {
                SLIDER_VERSION
            } else if self.state.inventory_drag.is_some() {
                DRAG_VERSION
            } else if self.state.mouse.is_some() {
                MOUSE_VERSION
            } else if self.state.clicks.has_drop() {
                DROP_VERSION
            } else if extended {
                INPUT_VERSION
            } else {
                VERSION
            })
            .to_le_bytes(),
        );
        bytes.extend_from_slice(&self.asset_id);
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&Sha256::digest(&payload));
        bytes.extend_from_slice(&payload);
        Ok(bytes)
    }

    /// Check format, asset identity, size and corruption before decoding.
    /// This does not yet activate the snapshot or validate resource references.
    pub fn from_bytes(bytes: &[u8], asset_id: [u8; 32]) -> Result<Self, Error> {
        if bytes.len() < HEADER || bytes.len() > MAX_SAVE_BYTES {
            return Err(Error("invalid save size"));
        }
        if &bytes[..8] != MAGIC {
            return Err(Error("invalid save signature"));
        }
        let word = |start| u32::from_le_bytes(bytes[start..start + 4].try_into().unwrap());
        if ![
            VERSION,
            INPUT_VERSION,
            DROP_VERSION,
            MOUSE_VERSION,
            DRAG_VERSION,
            SLIDER_VERSION,
            GEOMETRY_VERSION,
            RESIZE_VERSION,
            HOVER_VERSION,
            MENU_VERSION,
            TALK_PHASE_VERSION,
            SPEECH_CHANNEL_VERSION,
            MOVER_REEL_VERSION,
            ESCAPE_VERSION,
            REEL_ESCAPE_VERSION,
            SAMPLE_WAIT_VERSION,
            INTRO_VERSION,
            MOVER_VISIBILITY_VERSION,
            SPLAY_VERSION,
            CREATED_MOVER_VERSION,
            REEL_TASK_VERSION,
            RESTORED_REEL_VERSION,
            SCROLL_TASK_VERSION,
            WORLD_HOVER_VERSION,
            CURSOR_TRAILS_VERSION,
            MOVER_TASK_VERSION,
        ]
        .contains(&word(8))
        {
            return Err(Error("unsupported save version"));
        }
        if bytes[12..44] != asset_id {
            return Err(Error("save belongs to different game assets"));
        }
        let payload = &bytes[HEADER..];
        if word(44) as usize != payload.len() {
            return Err(Error("save length mismatch"));
        }
        if Sha256::digest(payload)[..] != bytes[48..80] {
            return Err(Error("save checksum mismatch"));
        }
        if word(8) == MOVER_TASK_VERSION {
            type Tasks = Vec<(crate::scheduler::ProcessId, i32)>;
            let ((legacy, tasks), remaining) =
                postcard::take_from_bytes::<(Vec<u8>, Tasks)>(payload)
                    .map_err(|_| Error("invalid snapshot mover tasks payload"))?;
            if !remaining.is_empty()
                || legacy.len() < HEADER
                || u32::from_le_bytes(legacy[8..12].try_into().unwrap()) > CURSOR_TRAILS_VERSION
            {
                return Err(Error("invalid snapshot mover tasks extension"));
            }
            let mut snapshot = Self::from_bytes(&legacy, asset_id)?;
            snapshot.state.restore_mover_tasks(tasks)?;
            return Ok(snapshot);
        }
        if word(8) == CURSOR_TRAILS_VERSION {
            let ((legacy, trails), remaining) =
                postcard::take_from_bytes::<(Vec<u8>, crate::cursor_trails::Trails)>(payload)
                    .map_err(|_| Error("invalid snapshot cursor trails payload"))?;
            if !remaining.is_empty()
                || legacy.len() < HEADER
                || u32::from_le_bytes(legacy[8..12].try_into().unwrap()) > WORLD_HOVER_VERSION
            {
                return Err(Error("invalid snapshot cursor trails extension"));
            }
            let mut snapshot = Self::from_bytes(&legacy, asset_id)?;
            snapshot.state.cursor_trails = Some(trails);
            snapshot.state.validate_snapshot_structure()?;
            return Ok(snapshot);
        }
        if word(8) == WORLD_HOVER_VERSION {
            type Tasks = Vec<(crate::scheduler::ProcessId, crate::engine::SceneTask)>;
            let ((legacy, tasks, depth, hover), remaining) =
                postcard::take_from_bytes::<(Vec<u8>, Tasks, [i64; 2], crate::world_hover::State)>(
                    payload,
                )
                .map_err(|_| Error("invalid snapshot world hover payload"))?;
            if !remaining.is_empty()
                || legacy.len() < HEADER
                || u32::from_le_bytes(legacy[8..12].try_into().unwrap()) > CREATED_MOVER_VERSION
            {
                return Err(Error("invalid snapshot world hover extension"));
            }
            let mut snapshot = Self::from_bytes(&legacy, asset_id)?;
            snapshot.state.scheduled_scroll = true;
            snapshot.state.world_hover = Some(hover);
            snapshot.state.restore_scene_tasks(tasks)?;
            snapshot.state.reel_dispatch_depth = depth;
            snapshot.state.validate_snapshot_structure()?;
            return Ok(snapshot);
        }
        if matches!(
            word(8),
            REEL_TASK_VERSION | RESTORED_REEL_VERSION | SCROLL_TASK_VERSION
        ) {
            type Tasks = Vec<(crate::scheduler::ProcessId, crate::engine::SceneTask)>;
            let ((legacy, tasks, depth), remaining) =
                postcard::take_from_bytes::<(Vec<u8>, Tasks, [i64; 2])>(payload)
                    .map_err(|_| Error("invalid snapshot reel task payload"))?;
            if !remaining.is_empty()
                || tasks.iter().any(|(_, task)| {
                    matches!(
                        task,
                        crate::engine::SceneTask::Point | crate::engine::SceneTask::Tags
                    )
                })
                || legacy.len() < HEADER
                || u32::from_le_bytes(legacy[8..12].try_into().unwrap()) > CREATED_MOVER_VERSION
                || (word(8) != SCROLL_TASK_VERSION && tasks.is_empty() && depth == [0; 2])
                || (word(8) != SCROLL_TASK_VERSION
                    && tasks
                        .iter()
                        .any(|(_, t)| matches!(t, crate::engine::SceneTask::Scroll { .. })))
                || (word(8) != SCROLL_TASK_VERSION
                    && (tasks
                        .iter()
                        .any(|(_, t)| matches!(t, crate::engine::SceneTask::Restored { .. }))
                        != (word(8) == RESTORED_REEL_VERSION)))
            {
                return Err(Error("invalid snapshot reel task extension"));
            }
            let mut snapshot = Self::from_bytes(&legacy, asset_id)?;
            snapshot.state.scheduled_scroll = word(8) == SCROLL_TASK_VERSION;
            snapshot.state.restore_scene_tasks(tasks)?;
            snapshot.state.reel_dispatch_depth = depth;
            snapshot.state.validate_snapshot_structure()?;
            return Ok(snapshot);
        }
        if word(8) == CREATED_MOVER_VERSION {
            let ((legacy, created), remaining) =
                postcard::take_from_bytes::<(Vec<u8>, Vec<(i32, u64)>)>(payload)
                    .map_err(|_| Error("invalid snapshot created mover payload"))?;
            if !remaining.is_empty()
                || legacy.len() < HEADER
                || u32::from_le_bytes(legacy[8..12].try_into().unwrap()) > SPLAY_VERSION
                || created.is_empty()
                || created.windows(2).any(|pair| pair[0].0 >= pair[1].0)
            {
                return Err(Error("invalid snapshot created mover extension"));
            }
            let mut snapshot = Self::from_bytes(&legacy, asset_id)?;
            snapshot.state.created_reel_movers = created.into_iter().collect();
            snapshot.state.validate_snapshot_structure()?;
            return Ok(snapshot);
        }
        if word(8) == SPLAY_VERSION {
            let ((legacy, ids), remaining) =
                postcard::take_from_bytes::<(Vec<u8>, Vec<u64>)>(payload)
                    .map_err(|_| Error("invalid snapshot SPLAY payload"))?;
            if !remaining.is_empty()
                || legacy.len() < HEADER
                || u32::from_le_bytes(legacy[8..12].try_into().unwrap()) > MOVER_VISIBILITY_VERSION
                || ids.is_empty()
                || ids.windows(2).any(|pair| pair[0] >= pair[1])
            {
                return Err(Error("invalid snapshot SPLAY extension"));
            }
            let mut snapshot = Self::from_bytes(&legacy, asset_id)?;
            snapshot.state.splay_reels = ids.into_iter().collect();
            snapshot.state.validate_snapshot_structure()?;
            return Ok(snapshot);
        }
        if word(8) == MOVER_VISIBILITY_VERSION {
            let ((legacy, hidden), remaining) =
                postcard::take_from_bytes::<(Vec<u8>, Vec<i32>)>(payload)
                    .map_err(|_| Error("invalid snapshot mover visibility payload"))?;
            if !remaining.is_empty()
                || legacy.len() < HEADER
                || u32::from_le_bytes(legacy[8..12].try_into().unwrap()) > INTRO_VERSION
                || hidden.is_empty()
                || hidden.windows(2).any(|pair| pair[0] >= pair[1])
            {
                return Err(Error("invalid snapshot mover visibility extension"));
            }
            let mut snapshot = Self::from_bytes(&legacy, asset_id)?;
            for id in hidden {
                snapshot
                    .state
                    .movers
                    .get_mut(&id)
                    .ok_or(Error("hidden snapshot mover is absent"))?
                    .hidden = true;
            }
            snapshot.state.validate_snapshot_structure()?;
            return Ok(snapshot);
        }
        if word(8) == INTRO_VERSION {
            let ((legacy, intro), remaining) =
                postcard::take_from_bytes::<(Vec<u8>, crate::intro::Intro)>(payload)
                    .map_err(|_| Error("invalid snapshot intro payload"))?;
            if !remaining.is_empty()
                || legacy.len() < HEADER
                || u32::from_le_bytes(legacy[8..12].try_into().unwrap()) > SAMPLE_WAIT_VERSION
            {
                return Err(Error("invalid snapshot intro extension"));
            }
            let mut snapshot = Self::from_bytes(&legacy, asset_id)?;
            snapshot.state.intro = Some(intro);
            snapshot.state.validate_snapshot_structure()?;
            return Ok(snapshot);
        }
        if word(8) == SAMPLE_WAIT_VERSION {
            let ((legacy, serials), remaining) =
                postcard::take_from_bytes::<(Vec<u8>, Vec<Option<u64>>)>(payload)
                    .map_err(|_| Error("invalid snapshot sample wait payload"))?;
            if !remaining.is_empty()
                || !serials.iter().any(Option::is_some)
                || legacy.len() < HEADER
                || u32::from_le_bytes(legacy[8..12].try_into().unwrap()) > REEL_ESCAPE_VERSION
            {
                return Err(Error("invalid snapshot sample wait extension"));
            }
            let mut snapshot = Self::from_bytes(&legacy, asset_id)?;
            snapshot.state.restore_sample_wait_checkpoint(&serials)?;
            snapshot.state.validate_snapshot_structure()?;
            return Ok(snapshot);
        }
        if word(8) == REEL_ESCAPE_VERSION {
            let ((legacy, captures), remaining) =
                postcard::take_from_bytes::<(Vec<u8>, Vec<(i32, (u64, u64))>)>(payload)
                    .map_err(|_| Error("invalid snapshot reel escape payload"))?;
            if !remaining.is_empty()
                || captures.is_empty()
                || legacy.len() < HEADER
                || u32::from_le_bytes(legacy[8..12].try_into().unwrap()) > ESCAPE_VERSION
            {
                return Err(Error("invalid snapshot reel escape extension"));
            }
            let mut snapshot = Self::from_bytes(&legacy, asset_id)?;
            let mut previous = None;
            for (actor, capture) in captures {
                if previous.is_some_and(|id| actor <= id) {
                    return Err(Error("unordered reel escape actors"));
                }
                snapshot.state.reel_escape.insert(actor, capture);
                previous = Some(actor);
            }
            snapshot.state.validate_snapshot_structure()?;
            return Ok(snapshot);
        }
        if word(8) == ESCAPE_VERSION {
            let ((legacy, event, captures), remaining) =
                postcard::take_from_bytes::<(Vec<u8>, u64, Vec<Option<u64>>)>(payload)
                    .map_err(|_| Error("invalid snapshot escape payload"))?;
            if !remaining.is_empty()
                || legacy.len() < HEADER
                || u32::from_le_bytes(legacy[8..12].try_into().unwrap()) > MOVER_REEL_VERSION
            {
                return Err(Error("invalid snapshot escape extension"));
            }
            let mut snapshot = Self::from_bytes(&legacy, asset_id)?;
            snapshot.state.restore_escape_checkpoint(event, &captures)?;
            snapshot.state.validate_snapshot_structure()?;
            return Ok(snapshot);
        }
        if word(8) == MOVER_REEL_VERSION {
            let ((legacy, reels), remaining) = postcard::take_from_bytes::<(
                Vec<u8>,
                Vec<(i32, crate::mover_reel::State)>,
            )>(payload)
            .map_err(|_| Error("invalid snapshot mover reel payload"))?;
            if !remaining.is_empty()
                || reels.is_empty()
                || legacy.len() < HEADER
                || u32::from_le_bytes(legacy[8..12].try_into().unwrap()) > SPEECH_CHANNEL_VERSION
            {
                return Err(Error("invalid snapshot mover reel extension"));
            }
            // Check the inner version before invoking the legacy decoder:
            // malicious nested v13 envelopes cannot recurse.
            let mut snapshot = Self::from_bytes(&legacy, asset_id)?;
            let mut previous = None;
            for (actor, state) in reels {
                if previous.is_some_and(|id| actor <= id) {
                    return Err(Error("duplicate or unordered mover reel actor"));
                }
                let mover = snapshot
                    .state
                    .movers
                    .get_mut(&actor)
                    .ok_or(Error("unknown mover reel actor"))?;
                mover.reel_state = Some(state);
                previous = Some(actor);
            }
            snapshot.state.validate_snapshot_structure()?;
            return Ok(snapshot);
        }
        let mut menu = None;
        let (state, remaining) = if matches!(word(8), TALK_PHASE_VERSION | SPEECH_CHANNEL_VERSION) {
            let (
                (mut state, clicks, mouse, owner, slider, geometry, resize, hover, bytes, finished),
                remaining,
            ) = postcard::take_from_bytes::<(
                Engine,
                crate::click::State,
                Option<crate::mouse::MouseInput>,
                Option<i32>,
                Option<crate::inventory_slider::Drag>,
                crate::inventory_geometry::State,
                Option<crate::inventory_resize::Drag>,
                crate::inventory_hover::State,
                Option<Vec<u8>>,
                Vec<crate::scheduler::ProcessId>,
            )>(payload)
            .map_err(|_| Error("invalid snapshot talk phase payload"))?;
            if bytes
                .as_ref()
                .is_some_and(|bytes| !(4..=65536).contains(&bytes.len()))
                || (finished.is_empty() && word(8) == TALK_PHASE_VERSION)
                || finished.len() > 1024
            {
                return Err(Error("invalid snapshot talk phase extension"));
            }
            state.clicks = clicks;
            state.mouse = mouse;
            state.inventory_drag = owner;
            state.inventory_slider = slider;
            state.inventory_geometry = geometry;
            state.inventory_resize = resize;
            state.inventory_hover = hover;
            state.restore_finished_talks(&finished)?;
            if word(8) == SPEECH_CHANNEL_VERSION {
                state.sample_player.restore_speech()?;
            }
            menu = bytes;
            (state, remaining)
        } else if word(8) == MENU_VERSION {
            let (
                (mut state, clicks, mouse, owner, slider, geometry, resize, hover, bytes),
                remaining,
            ) = postcard::take_from_bytes::<(
                Engine,
                crate::click::State,
                Option<crate::mouse::MouseInput>,
                Option<i32>,
                Option<crate::inventory_slider::Drag>,
                crate::inventory_geometry::State,
                Option<crate::inventory_resize::Drag>,
                crate::inventory_hover::State,
                Vec<u8>,
            )>(payload)
            .map_err(|_| Error("invalid snapshot menu payload"))?;
            if !(4..=65536).contains(&bytes.len()) {
                return Err(Error("invalid snapshot menu size"));
            }
            state.clicks = clicks;
            state.mouse = mouse;
            state.inventory_drag = owner;
            state.inventory_slider = slider;
            state.inventory_geometry = geometry;
            state.inventory_resize = resize;
            state.inventory_hover = hover;
            menu = Some(bytes);
            (state, remaining)
        } else if word(8) == HOVER_VERSION {
            let ((mut state, clicks, mouse, owner, slider, geometry, resize, hover), remaining) =
                postcard::take_from_bytes::<(
                    Engine,
                    crate::click::State,
                    Option<crate::mouse::MouseInput>,
                    Option<i32>,
                    Option<crate::inventory_slider::Drag>,
                    crate::inventory_geometry::State,
                    Option<crate::inventory_resize::Drag>,
                    crate::inventory_hover::State,
                )>(payload)
                .map_err(|_| Error("invalid snapshot hover payload"))?;
            state.clicks = clicks;
            state.mouse = mouse;
            state.inventory_drag = owner;
            state.inventory_slider = slider;
            state.inventory_geometry = geometry;
            state.inventory_resize = resize;
            state.inventory_hover = hover;
            (state, remaining)
        } else if word(8) == RESIZE_VERSION {
            let ((mut state, clicks, mouse, owner, geometry, resize), remaining) =
                postcard::take_from_bytes::<(
                    Engine,
                    crate::click::State,
                    Option<crate::mouse::MouseInput>,
                    Option<i32>,
                    crate::inventory_geometry::State,
                    crate::inventory_resize::Drag,
                )>(payload)
                .map_err(|_| Error("invalid snapshot resize payload"))?;
            state.clicks = clicks;
            state.mouse = mouse;
            state.inventory_drag = owner;
            state.inventory_geometry = geometry;
            state.inventory_resize = Some(resize);
            (state, remaining)
        } else if word(8) == GEOMETRY_VERSION {
            let ((mut state, clicks, mouse, owner, slider, geometry), remaining) =
                postcard::take_from_bytes::<(
                    Engine,
                    crate::click::State,
                    Option<crate::mouse::MouseInput>,
                    Option<i32>,
                    Option<crate::inventory_slider::Drag>,
                    crate::inventory_geometry::State,
                )>(payload)
                .map_err(|_| Error("invalid snapshot inventory geometry payload"))?;
            state.clicks = clicks;
            state.mouse = mouse;
            state.inventory_drag = owner;
            state.inventory_slider = slider;
            state.inventory_geometry = geometry;
            (state, remaining)
        } else if word(8) == SLIDER_VERSION {
            let ((mut state, clicks, mouse, owner, slider), remaining) =
                postcard::take_from_bytes::<(
                    Engine,
                    crate::click::State,
                    Option<crate::mouse::MouseInput>,
                    Option<i32>,
                    crate::inventory_slider::Drag,
                )>(payload)
                .map_err(|_| Error("invalid snapshot slider payload"))?;
            state.clicks = clicks;
            state.mouse = mouse;
            state.inventory_drag = owner;
            state.inventory_slider = Some(slider);
            (state, remaining)
        } else if word(8) == DRAG_VERSION {
            let ((mut state, clicks, mouse, slot), remaining) = postcard::take_from_bytes::<(
                Engine,
                crate::click::State,
                Option<crate::mouse::MouseInput>,
                i32,
            )>(payload)
            .map_err(|_| Error("invalid snapshot drag payload"))?;
            state.clicks = clicks;
            state.mouse = mouse;
            state.inventory_drag = Some(slot);
            (state, remaining)
        } else if word(8) == MOUSE_VERSION {
            let ((mut state, clicks, mouse), remaining) = postcard::take_from_bytes::<(
                Engine,
                crate::click::State,
                crate::mouse::MouseInput,
            )>(payload)
            .map_err(|_| Error("invalid snapshot mouse payload"))?;
            state.clicks = clicks;
            state.mouse = Some(mouse);
            (state, remaining)
        } else if word(8) >= INPUT_VERSION {
            let ((mut state, clicks), remaining) =
                postcard::take_from_bytes::<(Engine, crate::click::State)>(payload)
                    .map_err(|_| Error("invalid snapshot input payload"))?;
            state.clicks = clicks;
            (state, remaining)
        } else {
            postcard::take_from_bytes::<Engine>(payload)
                .map_err(|_| Error("invalid snapshot payload"))?
        };
        if !remaining.is_empty() {
            return Err(Error("trailing snapshot data"));
        }
        if word(8) < DROP_VERSION && state.clicks.has_drop() {
            return Err(Error("inventory drop requires save version 3"));
        }
        state.validate_snapshot_structure()?;
        Ok(Self {
            asset_id,
            state,
            menu,
        })
    }

    #[cfg(test)]
    pub(crate) fn state(&self) -> &Engine {
        &self.state
    }
}
