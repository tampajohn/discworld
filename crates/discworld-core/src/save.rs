// SPDX-License-Identifier: GPL-3.0-or-later
//! Versioned complete-runtime snapshot codec. Host persistence and validated
//! activation are separate: reading a file does not replace a running engine.
use crate::{engine::Engine, iso::Error};
use sha2::{Digest, Sha256};

const MAGIC: &[u8; 8] = b"DWRSAVE\0";
const VERSION: u32 = 1;
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

    pub fn to_bytes(&self) -> Result<Vec<u8>, Error> {
        self.state.validate_snapshot_structure()?;
        let payload =
            postcard::to_allocvec(&self.state).map_err(|_| Error("snapshot encode failed"))?;
        if payload.len() > MAX_SAVE_BYTES - HEADER {
            return Err(Error("save size limit"));
        }
        let mut bytes = Vec::with_capacity(HEADER + payload.len());
        bytes.extend_from_slice(MAGIC);
        bytes.extend_from_slice(&VERSION.to_le_bytes());
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
        if word(8) != VERSION {
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
        let (state, remaining) = postcard::take_from_bytes::<Engine>(payload)
            .map_err(|_| Error("invalid snapshot payload"))?;
        if !remaining.is_empty() {
            return Err(Error("trailing snapshot data"));
        }
        state.validate_snapshot_structure()?;
        Ok(Self { asset_id, state })
    }

    #[cfg(test)]
    pub(crate) fn state(&self) -> &Engine {
        &self.state
    }
}
