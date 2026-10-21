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
const HEADER: usize = 80;
pub const MAX_SAVE_BYTES: usize = 16 * 1024 * 1024;

/// Captured at a tick boundary. External game archives are supplied separately.
/// The schema is tied to this engine format version, not ScummVM save files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    asset_id: [u8; 32],
    state: Engine,
}

impl Snapshot {
    /// The host supplies a SHA-256 identity covering its original game assets.
    pub fn capture(engine: &Engine, asset_id: [u8; 32]) -> Self {
        Self {
            asset_id,
            state: engine.clone(),
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
        self.verify_scripts(resources)?;
        self.verify_audio(bank, tracks)?;
        self.state.validate_snapshot_polygons(resources)?;
        self.preview(resources)?;
        Ok(self.state)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        self.state.validate_snapshot_structure()?;
        let extended = self.state.clicks != crate::click::State::default();
        let payload = if let Some(mouse) = &self.state.mouse {
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
            &(if self.state.mouse.is_some() {
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
        if ![VERSION, INPUT_VERSION, DROP_VERSION, MOUSE_VERSION].contains(&word(8)) {
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
        let (state, remaining) = if word(8) == MOUSE_VERSION {
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
        Ok(Self { asset_id, state })
    }

    #[cfg(test)]
    pub(crate) fn state(&self) -> &Engine {
        &self.state
    }
}
