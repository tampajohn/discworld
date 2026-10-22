//! Keep the host resampler continuous across the engine's 24 Hz audio blocks.
use rodio::{Sink, Source, mixer::Mixer, queue::SourcesQueueOutput};
use std::time::Duration;

struct Continuous(SourcesQueueOutput);
impl Iterator for Continuous {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        self.0.next()
    }
}
impl Source for Continuous {
    fn current_span_len(&self) -> Option<usize> {
        None
    }
    fn channels(&self) -> u16 {
        2
    }
    fn sample_rate(&self) -> u32 {
        discworld_core::audio::SAMPLE_RATE
    }
    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

/// Only append interleaved stereo at SAMPLE_RATE. Queue filler is silence;
/// retain our fixed format even while the underlying queue reports its default
/// mono/44100 format. Buffer boundaries must not reset interpolation phase.
pub fn connect(mixer: &Mixer) -> Sink {
    let (sink, source) = Sink::new();
    mixer.add(Continuous(source));
    sink
}

#[cfg(test)]
mod tests {
    use super::*;
    fn render(rate: u32, blocks: bool, continuous: bool) -> Vec<f32> {
        let (mixer, output) = rodio::mixer::mixer(2, rate);
        let sink = if continuous {
            connect(&mixer)
        } else {
            Sink::connect_new(&mixer)
        };
        let wave: Vec<_> = (0..22050)
            .flat_map(|i| {
                let sample = (i as f32 * std::f32::consts::TAU * 997.0 / 22050.0).sin() * 0.5;
                [sample, -sample]
            })
            .collect();
        if blocks {
            for tick in 0..24 {
                let start = tick * 22050 / 24 * 2;
                let end = (tick + 1) * 22050 / 24 * 2;
                sink.append(rodio::buffer::SamplesBuffer::new(
                    2,
                    22050,
                    wave[start..end].to_vec(),
                ));
            }
        } else {
            sink.append(rodio::buffer::SamplesBuffer::new(2, 22050, wave));
        }
        output.take(rate as usize * 2).collect()
    }
    #[test]
    fn engine_block_boundaries_do_not_change_device_audio() {
        for rate in [44100, 48000] {
            let whole = render(rate, false, true);
            let blocks = render(rate, true, true);
            assert_eq!(whole, blocks, "device rate {rate}");
            let old_whole = render(rate, false, false);
            let old_blocks = render(rate, true, false);
            let differing = old_whole
                .iter()
                .zip(&old_blocks)
                .filter(|(a, b)| a != b)
                .count();
            println!(
                "{rate} Hz: old buffer partition changed {differing} samples; continuous output changed zero"
            );
            assert!(
                differing > 0,
                "regression must exercise the original discontinuity"
            );
        }
    }
}
