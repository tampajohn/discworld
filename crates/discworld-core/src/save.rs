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
