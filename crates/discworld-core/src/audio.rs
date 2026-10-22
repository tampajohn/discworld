// SPDX-License-Identifier: GPL-3.0-or-later
//! Original DOS CD sample bank: indexed unsigned 8-bit mono PCM at 22050 Hz.
use crate::{film::word, iso::Error};
pub const SAMPLE_RATE: u32 = 22050;
pub struct SampleBank<'a> {
    index: &'a [u8],
    data: &'a [u8],
}
impl<'a> SampleBank<'a> {
    pub fn new(index: &'a [u8], data: &'a [u8]) -> Result<Self, Error> {
        if index.len() < 4 || index.len() % 4 != 0 {
            return Err(Error("invalid sample index length"));
        }
        if matches!(&index[..4], b"MP3 " | b"OGG " | b"FLAC") {
            return Err(Error("compressed sample bank unsupported"));
        }
        Ok(Self { index, data })
    }
    pub fn count(&self) -> usize {
        self.index.len() / 4
    }
    pub fn sample(&self, id: i32) -> Result<Option<&'a [u8]>, Error> {
        if id <= 0 || id as usize >= self.count() {
            return Ok(None);
        }
        let start = word(self.index, id as usize * 4)? as usize;
        if start == 0 {
            return Ok(None);
        }
        let len = word(self.data, start)? as usize;
        let payload = start
            .checked_add(4)
            .ok_or(Error("sample offset overflow"))?;
        let end = payload
            .checked_add(len)
            .ok_or(Error("sample length overflow"))?;
        Ok(Some(
            self.data
                .get(payload..end)
                .ok_or(Error("sample outside archive"))?,
        ))
    }
}
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Default, PartialEq, Eq)]
pub struct SamplePlayer {
    #[serde(skip)]
    speech: bool,
    pub serial: u64,
    pub id: Option<i32>,
    pub position: usize,
    remainder: u32,
}
impl SamplePlayer {
    pub(crate) fn validate_snapshot_structure(&self) -> Result<(), Error> {
        if self.remainder >= 24
            || self.id.is_some_and(|id| id <= 0)
            || (self.speech && self.id.is_none())
        {
            return Err(Error("invalid snapshot sample clock or identity"));
        }
        Ok(())
    }

    pub(crate) fn validate_snapshot_bank(&self, bank: &SampleBank<'_>) -> Result<(), Error> {
        self.validate_snapshot_structure()?;
        if let Some(id) = self.id {
            let data = bank
                .sample(id)?
                .ok_or(Error("snapshot sample is missing"))?;
            if self.position >= data.len() {
                return Err(Error("snapshot active sample cursor outside data"));
            }
        }
        Ok(())
    }

    /// Replaces the V1 single sample channel. Missing IDs do not interrupt it.
    pub fn play(&mut self, bank: &SampleBank<'_>, id: i32) -> Result<bool, Error> {
        let Some(data) = bank.sample(id)? else {
            return Ok(false);
        };
        self.serial = self
            .serial
            .checked_add(1)
            .ok_or(Error("sample identity overflow"))?;
        self.id = if data.is_empty() { None } else { Some(id) };
        self.position = 0;
        self.speech = false;
        Ok(true)
    }
    pub fn is_speech(&self) -> bool {
        self.speech
    }
    pub(crate) fn restore_speech(&mut self) -> Result<(), Error> {
        if self.id.is_none() {
            return Err(Error("speech channel has no active sample"));
        }
        self.speech = true;
        Ok(())
    }
    pub fn play_speech(&mut self, bank: &SampleBank<'_>, id: i32) -> Result<bool, Error> {
        let played = self.play(bank, id)?;
        if played && self.id.is_some() {
            self.speech = true;
        }
        Ok(played)
    }
    pub fn stop(&mut self) {
        self.speech = false;
        self.id = None;
        self.position = 0;
    }
    /// Exactly one 1/24 second block, preserving the fractional sample rate.
    pub fn tick(&mut self, bank: &SampleBank<'_>) -> Result<Vec<i16>, Error> {
        self.tick_with_volume(bank, 255)
    }
    /// Gain changes output only; sample clocks and completion remain identical.
    pub fn tick_with_volume(
        &mut self,
        bank: &SampleBank<'_>,
        volume: u16,
    ) -> Result<Vec<i16>, Error> {
        if volume > 255 {
            return Err(Error("invalid sample volume"));
        }
        let mut next = self.clone();
        next.remainder += SAMPLE_RATE;
        let count = next.remainder / 24;
        next.remainder %= 24;
        let mut pcm = vec![0; count as usize];
        if let Some(id) = next.id {
            let data = bank.sample(id)?.ok_or(Error("active sample disappeared"))?;
            if next.position > data.len() {
                return Err(Error("sample cursor outside data"));
            }
            let n = pcm.len().min(data.len() - next.position);
            for (out, &input) in pcm[..n]
                .iter_mut()
                .zip(&data[next.position..next.position + n])
            {
                *out = (((i32::from(input) - 128) * 256 * i32::from(volume)) / 255) as i16;
            }
            next.position += n;
            if next.position == data.len() {
                next.id = None;
                next.speech = false;
            }
        }
        *self = next;
        Ok(pcm)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn channel_identity_gain_and_completion_are_independent() {
        let index = [0, 0, 0, 0, 4, 0, 0, 0];
        let mut data = vec![0; 8];
        data[4..8].copy_from_slice(&1000u32.to_le_bytes());
        data.extend((0..1000).map(|i| [0, 128, 255][i % 3]));
        let bank = SampleBank::new(&index, &data).unwrap();
        let mut player = SamplePlayer::default();
        player.play_speech(&bank, 1).unwrap();
        assert!(player.is_speech());
        assert!(!player.play(&bank, 99).unwrap());
        assert!(
            player.is_speech(),
            "missing sample preserves the playing channel"
        );
        let before = player.clone();
        assert!(player.tick_with_volume(&bank, 256).is_err());
        assert_eq!(player, before);
        let mut full = player.clone();
        let expected = full.tick(&bank).unwrap();
        for volume in [0, 128, 255] {
            let mut scaled = player.clone();
            let actual = scaled.tick_with_volume(&bank, volume).unwrap();
            assert_eq!(
                scaled, full,
                "gain does not alter clocks or channel ownership"
            );
            assert_eq!(
                actual,
                expected
                    .iter()
                    .map(|sample| (i32::from(*sample) * i32::from(volume) / 255) as i16)
                    .collect::<Vec<_>>()
            );
            scaled.tick_with_volume(&bank, volume).unwrap();
            assert!(scaled.id.is_none());
            assert!(!scaled.is_speech());
        }
        player.play(&bank, 1).unwrap();
        assert!(!player.is_speech());
        player.play_speech(&bank, 1).unwrap();
        player.stop();
        assert!(!player.is_speech());
        assert!(player.restore_speech().is_err());
    }

    #[test]
    fn snapshot_checks_fractional_clock_and_active_sample_bounds() {
        let index = [0, 0, 0, 0, 4, 0, 0, 0];
        let data = [0, 0, 0, 0, 3, 0, 0, 0, 0, 128, 255];
        let bank = SampleBank::new(&index, &data).unwrap();
        let mut player = SamplePlayer::default();
        player.play(&bank, 1).unwrap();
        player.validate_snapshot_bank(&bank).unwrap();
        player.position = 3;
        assert!(player.validate_snapshot_bank(&bank).is_err());
        player.position = 0;
        player.id = Some(2);
        assert!(player.validate_snapshot_bank(&bank).is_err());
        player.id = Some(1);
        player.remainder = 24;
        assert!(player.validate_snapshot_bank(&bank).is_err());
        player.remainder = 0;
        player.tick(&bank).unwrap();
        assert_eq!(player.position, 3);
        assert!(player.id.is_none());
        player.validate_snapshot_bank(&bank).unwrap();
    }

    #[test]
    fn validates_offsets_lengths_and_missing_ids() {
        let index = [0, 0, 0, 0, 4, 0, 0, 0];
        let data = [0, 0, 0, 0, 3, 0, 0, 0, 0, 128, 255];
        let bank = SampleBank::new(&index, &data).unwrap();
        assert_eq!(bank.sample(1).unwrap(), Some([0, 128, 255].as_slice()));
        assert!(bank.sample(0).unwrap().is_none());
        assert!(bank.sample(5).unwrap().is_none());
        assert!(
            SampleBank::new(&index, &data[..10])
                .unwrap()
                .sample(1)
                .is_err()
        );
    }
    #[test]
    fn signed_conversion_and_exact_second() {
        let index = [0, 0, 0, 0, 4, 0, 0, 0];
        let data = [0, 0, 0, 0, 3, 0, 0, 0, 0, 128, 255];
        let bank = SampleBank::new(&index, &data).unwrap();
        let mut player = SamplePlayer::default();
        player.play(&bank, 1).unwrap();
        let first = player.tick(&bank).unwrap();
        assert_eq!(&first[..4], &[-32768, 0, 32512, 0]);
        assert!(player.id.is_none());
        let mut count = first.len();
        for _ in 1..24 {
            count += player.tick(&bank).unwrap().len();
        }
        assert_eq!(count, 22050);
    }
    #[test]
    fn replacement_and_snapshot() {
        let index = [0, 0, 0, 0, 4, 0, 0, 0];
        let mut data = vec![0; 8];
        data[4..8].copy_from_slice(&2000u32.to_le_bytes());
        data.extend(vec![129; 2000]);
        let bank = SampleBank::new(&index, &data).unwrap();
        let mut p = SamplePlayer::default();
        p.play(&bank, 1).unwrap();
        p.tick(&bank).unwrap();
        let mut saved = p.clone();
        assert_eq!(p.tick(&bank).unwrap(), saved.tick(&bank).unwrap());
        p.play(&bank, 1).unwrap();
        assert_eq!(p.position, 0);
        p.stop();
        assert!(p.id.is_none());
    }
}
