// SPDX-License-Identifier: GPL-3.0-or-later
//! Single music channel with an absolute clock shared by successive sequences.
use crate::{
    iso::Error,
    music::Track,
    sequencer::{Message, Sequencer},
};

/// V1 CurrentMidiFacts, separate from the active sequencer position.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MidiFacts {
    pub offset: u32,
    pub repeat: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MusicPlayer {
    sequence: Option<Sequencer>,
    base: u64,
    pub clock: u64,
    pub offset: Option<u32>,
    repeat: bool,
    pending: Vec<Message>,
}
impl MusicPlayer {
    pub fn facts(&self) -> MidiFacts {
        MidiFacts {
            offset: self.offset.unwrap_or(0),
            repeat: self.repeat,
        }
    }

    /// Pinned V1 RestoreMidiFacts stops existing playback and restarts the saved
    /// offset with looping forced on. It does not rewind the host audio clock.
    /// The saved repeat flag is overwritten by PlayMidiSequence(..., true).
    pub fn restore_facts(&mut self, tracks: &[Track<'_>], facts: MidiFacts) -> Result<(), Error> {
        let mut next = self.clone();
        next.stop();
        next.play(tracks, facts.offset, true)?;
        *self = next;
        Ok(())
    }

    pub fn playing(&self) -> bool {
        self.sequence.as_ref().is_some_and(|s| s.playing)
    }
    pub fn play(&mut self, tracks: &[Track<'_>], offset: u32, repeat: bool) -> Result<(), Error> {
        // Offset zero updates saved facts but leaves current playback unchanged.
        if offset == 0 {
            self.offset = None;
            self.repeat = repeat;
            return Ok(());
        }
        let track = tracks
            .iter()
            .find(|t| t.offset == offset)
            .ok_or(Error("unknown MIDI offset"))?;
        let sequence = Sequencer::new(track.events, repeat)?;
        self.stop();
        self.sequence = Some(sequence);
        self.base = self.clock;
        self.offset = Some(offset);
        self.repeat = repeat;
        Ok(())
    }
    pub fn stop(&mut self) {
        if let Some(mut sequence) = self.sequence.take() {
            for mut message in sequence.stop() {
                message.tick = self.clock;
                self.pending.push(message);
            }
            for channel in 0..16 {
                self.pending.push(Message {
                    tick: self.clock,
                    status: 0xb0 | channel,
                    data: vec![64, 0],
                });
                self.pending.push(Message {
                    tick: self.clock,
                    status: 0xb0 | channel,
                    data: vec![123, 0],
                });
            }
        }
        self.offset = None;
        self.repeat = false;
    }
    pub fn tick(&mut self) -> Result<Vec<Message>, Error> {
        let mut next = self.clone();
        let end = next
            .clock
            .checked_add(5)
            .ok_or(Error("music clock overflow"))?;
        let mut messages = std::mem::take(&mut next.pending);
        if let Some(sequence) = &mut next.sequence {
            for mut message in sequence.advance(5)? {
                message.tick = message
                    .tick
                    .checked_add(next.base)
                    .ok_or(Error("music timestamp overflow"))?;
                messages.push(message);
            }
        }
        next.clock = end;
        *self = next;
        Ok(messages)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn restoring_facts_restarts_track_loops_and_preserves_audio_clock() {
        let tracks = [Track {
            offset: 4,
            events: &[0x90, 60, 100, 30, 40, 0xff, 0x2f, 0],
        }];
        let mut player = MusicPlayer::default();
        player.play(&tracks, 4, false).unwrap();
        let facts = player.facts();
        assert_eq!(
            facts,
            MidiFacts {
                offset: 4,
                repeat: false
            }
        );
        for _ in 0..3 {
            player.tick().unwrap();
        }
        let clock = player.clock;
        player.restore_facts(&tracks, facts).unwrap();
        assert_eq!(player.clock, clock);
        assert_eq!(
            player.facts(),
            MidiFacts {
                offset: 4,
                repeat: true
            }
        );
        let messages = player.tick().unwrap();
        assert_eq!(messages.first().unwrap().status, 0x80);
        assert_eq!(messages.last().unwrap().status, 0x90);
        assert!(messages.iter().all(|m| m.tick == clock));
        let mut clone = player.clone();
        for _ in 0..20 {
            assert_eq!(player.tick().unwrap(), clone.tick().unwrap());
        }
        assert!(player.playing());
        assert_eq!(player, clone);
        let before = player.clone();
        assert!(
            player
                .restore_facts(
                    &tracks,
                    MidiFacts {
                        offset: 8,
                        repeat: false
                    }
                )
                .is_err()
        );
        assert_eq!(player, before);
    }

    #[test]
    fn zero_offset_updates_facts_without_stopping_until_restoration() {
        let tracks = [Track {
            offset: 4,
            events: &[0x90, 60, 100, 30, 40, 0xff, 0x2f, 0],
        }];
        let mut player = MusicPlayer::default();
        player.play(&tracks, 4, false).unwrap();
        player.tick().unwrap();
        let sequence = player.sequence.clone();
        player.play(&tracks, 0, false).unwrap();
        assert_eq!(player.sequence, sequence);
        assert!(player.playing());
        assert_eq!(player.facts(), MidiFacts::default());
        let clock = player.clock;
        player.restore_facts(&tracks, player.facts()).unwrap();
        assert!(!player.playing());
        assert_eq!(player.clock, clock);
        assert_eq!(
            player.facts(),
            MidiFacts {
                offset: 0,
                repeat: true
            }
        );
        let messages = player.tick().unwrap();
        assert!(messages.iter().any(|m| m.status == 0x80));
        assert!(!messages.iter().any(|m| m.status == 0x90));
        player.stop();
        assert_eq!(player.facts(), MidiFacts::default());
    }

    #[test]
    fn replacement_uses_absolute_time_and_failed_load_is_atomic() {
        let tracks = [Track {
            offset: 4,
            events: &[0x90, 60, 100, 30, 40, 0xff, 0x2f, 0],
        }];
        let mut player = MusicPlayer::default();
        player.tick().unwrap();
        player.play(&tracks, 4, false).unwrap();
        assert_eq!(player.tick().unwrap()[0].tick, 5);
        let before = player.clone();
        assert!(player.play(&tracks, 8, false).is_err());
        assert_eq!(before, player);
        player.play(&tracks, 4, false).unwrap();
        let messages = player.tick().unwrap();
        assert_eq!(messages.first().unwrap().status, 0x80);
        assert!(messages.iter().all(|m| m.tick == 10));
        assert_eq!(messages.last().unwrap().status, 0x90);
    }
}
