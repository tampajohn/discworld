//! Portable SoundFont presentation for timestamped engine MIDI messages.
use discworld_core::sequencer::Message;
use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};
use std::{io::Cursor, sync::Arc};

/// Cloning captures all DSP state, including active voices, effect buffers and
/// the partly consumed synthesis block. The immutable SoundFont is shared.
/// This is an in-memory checkpoint; durable encoding is a separate host concern.
#[derive(Clone)]
pub struct MusicOutput {
    synth: Synthesizer,
    sample: u64,
    rate: u32,
}
impl MusicOutput {
    pub fn new(soundfont: &[u8], rate: u32) -> Result<Self, Box<dyn std::error::Error>> {
        let font = Arc::new(SoundFont::new(&mut Cursor::new(soundfont))?);
        let rate_i32 = i32::try_from(rate)?;
        let synth = Synthesizer::new(&font, &SynthesizerSettings::new(rate_i32))?;
        Ok(Self {
            synth,
            sample: 0,
            rate,
        })
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
        let mut restored = original.clone();
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
