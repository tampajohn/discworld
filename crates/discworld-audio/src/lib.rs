//! Portable SoundFont presentation for timestamped engine MIDI messages.
use discworld_core::sequencer::Message;
use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};
use std::{io::Cursor, sync::Arc};

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
