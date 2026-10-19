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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SamplePlayer {
    pub serial: u64,
    pub id: Option<i32>,
    pub position: usize,
    remainder: u32,
}
impl SamplePlayer {
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
        Ok(true)
    }
    pub fn stop(&mut self) {
        self.id = None;
        self.position = 0;
    }
    /// Exactly one 1/24 second block, preserving the fractional sample rate.
    pub fn tick(&mut self, bank: &SampleBank<'_>) -> Result<Vec<i16>, Error> {
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
                *out = (i16::from(input) - 128) * 256;
            }
            next.position += n;
            if next.position == data.len() {
                next.id = None;
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
