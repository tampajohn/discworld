//! Portable combined engine/DSP checkpoint. Filesystem and device I/O are host-owned.
use crate::MusicOutput;
use discworld_core::{
    audio::SampleBank, engine::Engine, graphics::Resources, music::Track, save::Snapshot,
};
use sha2::{Digest, Sha256};

const HEADER: usize = 52;
pub const MAX_SESSION_BYTES: usize =
    HEADER + discworld_core::save::MAX_SAVE_BYTES + crate::MAX_CHECKPOINT_BYTES;
type Error = Box<dyn std::error::Error>;

pub fn asset_identity(iso: &[u8]) -> [u8; 32] {
    Sha256::digest(iso).into()
}

/// Fully prepared replacement; hosts commit both fields together after success.
pub struct RestoredSession {
    pub engine: Engine,
    pub audio: MusicOutput,
}

pub fn encode(engine: &Engine, audio: &MusicOutput, asset_id: [u8; 32]) -> Result<Vec<u8>, Error> {
    if !audio.matches_engine_clock(engine.music.clock) {
        return Err("cannot save engine and audio at different tick boundaries".into());
    }
    let engine = Snapshot::capture(engine, asset_id).to_bytes()?;
    let audio = audio.checkpoint_bytes()?;
    let mut bytes = Vec::with_capacity(HEADER + engine.len() + audio.len());
    bytes.extend_from_slice(b"DWRPLAY\0");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&(engine.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(audio.len() as u32).to_le_bytes());
    let mut digest = Sha256::new();
    digest.update(&bytes);
    digest.update(&engine);
    digest.update(&audio);
    bytes.extend_from_slice(&digest.finalize());
    bytes.extend_from_slice(&engine);
    bytes.extend_from_slice(&audio);
    Ok(bytes)
}

fn sections(bytes: &[u8]) -> Result<(&[u8], &[u8]), Error> {
    if !(HEADER..=MAX_SESSION_BYTES).contains(&bytes.len()) {
        return Err("invalid session save size".into());
    }
    let word = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    if &bytes[..8] != b"DWRPLAY\0" || word(8) != 1 {
        return Err("unsupported session save format".into());
    }
    let engine_len = word(12) as usize;
    let audio_len = word(16) as usize;
    if engine_len > discworld_core::save::MAX_SAVE_BYTES
        || audio_len > crate::MAX_CHECKPOINT_BYTES
        || engine_len.checked_add(audio_len) != Some(bytes.len() - HEADER)
    {
        return Err("session save length mismatch".into());
    }
    let mut digest = Sha256::new();
    digest.update(&bytes[..20]);
    digest.update(&bytes[HEADER..]);
    if digest.finalize()[..] != bytes[20..HEADER] {
        return Err("session save checksum mismatch".into());
    }
    Ok(bytes[HEADER..].split_at(engine_len))
}

pub fn decode(
    bytes: &[u8],
    asset_id: [u8; 32],
    resources: &Resources<'_>,
    bank: &SampleBank<'_>,
    tracks: &[Track<'_>],
    soundfont: &[u8],
) -> Result<RestoredSession, Error> {
    let (engine_bytes, audio_bytes) = sections(bytes)?;
    let engine = Snapshot::from_bytes(engine_bytes, asset_id)?.restore(resources, bank, tracks)?;
    let audio = MusicOutput::from_checkpoint(soundfont, audio_bytes)?;
    if !audio.matches_engine_clock(engine.music.clock) {
        return Err("saved engine and audio clocks disagree".into());
    }
    Ok(RestoredSession { engine, audio })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_lengths_and_corruption_are_rejected_before_decoding() {
        let mut bytes = b"DWRPLAY\0".to_vec();
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&3u32.to_le_bytes());
        let payload = [1, 2, 3, 4, 5];
        let mut digest = Sha256::new();
        digest.update(&bytes);
        digest.update(payload);
        bytes.extend_from_slice(&digest.finalize());
        bytes.extend_from_slice(&payload);
        assert_eq!(sections(&bytes).unwrap(), (&payload[..2], &payload[2..]));
        for end in 0..bytes.len() {
            assert!(sections(&bytes[..end]).is_err());
        }
        for i in 0..bytes.len() {
            let mut invalid = bytes.clone();
            invalid[i] ^= 1;
            assert!(sections(&invalid).is_err(), "byte {i}");
        }
        bytes.extend_from_slice(&[0]);
        assert!(sections(&bytes).is_err());
    }
}
