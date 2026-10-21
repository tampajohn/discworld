//! Portable SoundFont presentation for timestamped engine MIDI messages.
use discworld_core::sequencer::Message;
use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};
use sha2::{Digest, Sha256};
use std::{io::Cursor, sync::Arc};

pub mod session;

const CHECKPOINT_HEADER: usize = 92;
pub const MAX_CHECKPOINT_BYTES: usize = 8 * 1024 * 1024;

/// Cloning captures all DSP state, including active voices, effect buffers and
/// the partly consumed synthesis block. The immutable SoundFont is shared.
/// Versioned durable checkpoints are available through `checkpoint_bytes`.
#[derive(Clone)]
pub struct MusicOutput {
    synth: Synthesizer,
    sample: u64,
    rate: u32,
    font_id: [u8; 32],
}
impl MusicOutput {
    pub fn matches_engine_clock(&self, clock: u64) -> bool {
        self.rate == discworld_core::audio::SAMPLE_RATE
            && clock.checked_mul(u64::from(self.rate)).map(|n| n / 120) == Some(self.sample)
    }

    pub fn new(soundfont: &[u8], rate: u32) -> Result<Self, Box<dyn std::error::Error>> {
        let font = Arc::new(SoundFont::new(&mut Cursor::new(soundfont))?);
        let rate_i32 = i32::try_from(rate)?;
        let synth = Synthesizer::new(&font, &SynthesizerSettings::new(rate_i32))?;
        Ok(Self {
            synth,
            sample: 0,
            rate,
            font_id: Sha256::digest(soundfont).into(),
        })
    }
    /// Versioned DSP checkpoint, bound to the exact external SoundFont bytes.
    pub fn checkpoint_bytes(&self) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let payload = postcard::to_allocvec(&self.synth.checkpoint())?;
        if payload.len() > MAX_CHECKPOINT_BYTES - CHECKPOINT_HEADER {
            return Err("audio checkpoint size limit".into());
        }
        let mut bytes = Vec::with_capacity(CHECKPOINT_HEADER + payload.len());
        bytes.extend_from_slice(b"DWRMIDI\0");
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&self.font_id);
        bytes.extend_from_slice(&self.rate.to_le_bytes());
        bytes.extend_from_slice(&self.sample.to_le_bytes());
        bytes.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        let mut digest = Sha256::new();
        digest.update(&bytes);
        digest.update(&payload);
        bytes.extend_from_slice(&digest.finalize());
        bytes.extend_from_slice(&payload);
        Ok(bytes)
    }

    /// Build a separate audio output; failure cannot modify the live output.
    pub fn from_checkpoint(
        soundfont: &[u8],
        bytes: &[u8],
    ) -> Result<Self, Box<dyn std::error::Error>> {
        if !(CHECKPOINT_HEADER..=MAX_CHECKPOINT_BYTES).contains(&bytes.len()) {
            return Err("invalid audio checkpoint size".into());
        }
        let word = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
        if &bytes[..8] != b"DWRMIDI\0" || word(8) != 1 {
            return Err("unsupported audio checkpoint format".into());
        }
        if Sha256::digest(soundfont)[..] != bytes[12..44] {
            return Err("audio checkpoint belongs to another SoundFont".into());
        }
        let payload = &bytes[CHECKPOINT_HEADER..];
        let mut digest = Sha256::new();
        digest.update(&bytes[..60]);
        digest.update(payload);
        if word(56) as usize != payload.len() || digest.finalize()[..] != bytes[60..92] {
            return Err("audio checkpoint length or checksum mismatch".into());
        }
        let (state, trailing) =
            postcard::take_from_bytes::<rustysynth::SynthesizerCheckpoint>(payload)?;
        if !trailing.is_empty() {
            return Err("trailing audio checkpoint data".into());
        }
        let mut output = Self::new(soundfont, word(44))?;
        output.synth.restore_checkpoint(state)?;
        output.sample = u64::from_le_bytes(bytes[48..56].try_into().unwrap());
        Ok(output)
    }
    /// Render up to an exclusive musical tick; messages must be ordered and
    /// within this interval. Returns interleaved stereo samples.
    pub fn render(
        &mut self,
        end_tick: u64,
        messages: &[Message],
    ) -> Result<Vec<f32>, &'static str> {
        let end = end_tick
            .checked_mul(u64::from(self.rate))
            .ok_or("music sample overflow")?
            / 120;
        let count = end
            .checked_sub(self.sample)
            .ok_or("music clock moved backwards")?;
        if count > u64::from(self.rate) * 60 {
            return Err("music render block limit");
        }
        let mut previous = self.sample;
        for m in messages {
            let sample = m
                .tick
                .checked_mul(u64::from(self.rate))
                .ok_or("music sample overflow")?
                / 120;
            if sample < previous || sample >= end {
                return Err("message outside render interval");
            }
            if !(0x80..0xf0).contains(&m.status) {
                return Err("unsupported synth system message");
            }
            let expected = if matches!(m.status & 0xf0, 0xc0 | 0xd0) {
                1
            } else {
                2
            };
            if m.data.len() != expected || m.data.iter().any(|b| *b >= 128) {
                return Err("invalid synth MIDI payload");
            }
            previous = sample;
        }
        let mut output = Vec::with_capacity(count as usize * 2);
        for m in messages {
            let at = m.tick * u64::from(self.rate) / 120;
            self.render_to(at, &mut output);
            self.synth.process_midi_message(
                i32::from(m.status & 15),
                i32::from(m.status & 0xf0),
                i32::from(m.data.first().copied().unwrap_or(0)),
                i32::from(m.data.get(1).copied().unwrap_or(0)),
            );
        }
        self.render_to(end, &mut output);
        Ok(output)
    }
    fn render_to(&mut self, end: u64, output: &mut Vec<f32>) {
        let count = (end - self.sample) as usize;
        if count == 0 {
            return;
        }
        let mut left = vec![0.0; count];
        let mut right = vec![0.0; count];
        self.synth.render(&mut left, &mut right);
        for (l, r) in left.into_iter().zip(right) {
            output.extend([l, r]);
        }
        self.sample = end;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires local TimGM6mb.sf2 or DISCWORLD_TEST_SOUNDFONT"]
    fn checkpoint_preserves_active_voices_effect_tails_and_partial_blocks() {
        let path = std::env::var_os("DISCWORLD_TEST_SOUNDFONT")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../assets/soundfonts/TimGM6mb.sf2")
            });
        let font = std::fs::read(path).unwrap();
        let mut original = MusicOutput::new(&font, 22050).unwrap();
        let message = |tick, status, data: &[u8]| Message {
            tick,
            status,
            data: data.to_vec(),
        };
        let first = original
            .render(
                7,
                &[
                    message(0, 0xc0, &[0]),
                    message(0, 0xb0, &[91, 127]),
                    message(0, 0xb0, &[93, 127]),
                    message(0, 0xb0, &[64, 127]),
                    message(0, 0x90, &[60, 100]),
                    message(2, 0x90, &[67, 95]),
                ],
            )
            .unwrap();
        assert!(first.iter().any(|sample| sample.abs() > 0.00001));
        assert_ne!(original.sample % 64, 0, "checkpoint within a synth block");
        let checkpoint = original.checkpoint_bytes().unwrap();
        eprintln!("audio checkpoint bytes: {}", checkpoint.len());
        let mut restored = MusicOutput::from_checkpoint(&font, &checkpoint).unwrap();
        assert_eq!(restored.checkpoint_bytes().unwrap(), checkpoint);
        for end in [0, 7, 12, 44, 60, 91, checkpoint.len() - 1] {
            assert!(MusicOutput::from_checkpoint(&font, &checkpoint[..end]).is_err());
        }
        for offset in [48, 59, 60, 92, checkpoint.len() - 1] {
            let mut corrupted = checkpoint.clone();
            corrupted[offset] ^= 1;
            assert!(MusicOutput::from_checkpoint(&font, &corrupted).is_err());
        }
        let mut wrong_font = font.clone();
        wrong_font[0] ^= 1;
        assert!(MusicOutput::from_checkpoint(&wrong_font, &checkpoint).is_err());
        // A repaired checksum cannot bypass fixed DSP settings validation.
        let mut invalid_state = checkpoint.clone();
        invalid_state[CHECKPOINT_HEADER] ^= 1; // serialized synthesizer sample rate
        let mut digest = Sha256::new();
        digest.update(&invalid_state[..60]);
        digest.update(&invalid_state[CHECKPOINT_HEADER..]);
        invalid_state[60..92].copy_from_slice(&digest.finalize());
        assert!(MusicOutput::from_checkpoint(&font, &invalid_state).is_err());
        for (end, messages) in [
            (11, vec![message(8, 0xe0, &[0, 80])]),
            (
                24,
                vec![message(12, 0x80, &[60, 0]), message(13, 0x80, &[67, 0])],
            ),
            (61, vec![message(25, 0xb0, &[64, 0])]),
            (125, vec![]),
            (240, vec![]),
            (480, vec![]),
        ] {
            let a = original.render(end, &messages).unwrap();
            let b = restored.render(end, &messages).unwrap();
            assert_eq!(
                a.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                b.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                "tick {end}"
            );
        }
        // Modifying one branch must not alter another branch's voices/buffers.
        let mut control = original.clone();
        restored
            .render(485, &[message(480, 0x90, &[72, 127])])
            .unwrap();
        assert_eq!(
            original.render(485, &[]).unwrap(),
            control.render(485, &[]).unwrap()
        );
    }
}
