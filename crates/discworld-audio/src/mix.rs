//! Host presentation only; never changes engine PCM or synthesizer state.
/// Optional 6 dB headroom is applied before saturation, preserving peaks up to
/// twice full scale. It is not denoising or a guarantee against arbitrary peaks.
pub fn sample(music: f32, pcm: i16, headroom: bool) -> f32 {
    let mixed = music + f32::from(pcm) / 32768.0;
    let gain = if headroom { 0.5 } else { 1.0 };
    (mixed * gain).clamp(-1.0, 1.0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn headroom_preserves_peaks_lost_by_downstream_volume() {
        let original = super::sample(0.75, 24576, false);
        let reduced = super::sample(0.75, 24576, true);
        assert_eq!(original, 1.0);
        assert_eq!(reduced, 0.75);
        assert_ne!(reduced, original * 0.5);
        assert_eq!(super::sample(-0.75, -24576, true), -0.75);
        assert_eq!(super::sample(3.0, 32767, true), 1.0);
    }
}
