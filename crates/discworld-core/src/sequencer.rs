// SPDX-License-Identifier: GPL-3.0-or-later
//! Deterministic XMIDI event clock (120 Hz), separate from synthesis.
use crate::{iso::Error, music};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    pub tick: u64,
    pub status: u8,
    pub data: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct Event {
    delta: u32,
    status: u8,
    data: Vec<u8>,
    meta: Option<u8>,
    duration: Option<u32>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct Loop {
    start: usize,
    remaining: u8,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sequencer {
    events: Vec<Event>,
    index: usize,
    due: u64,
    pub tick: u64,
    pub playing: bool,
    repeat: bool,
    loops: Vec<Loop>,
    notes: BTreeMap<(u8, u8), u64>,
}
impl Sequencer {
    pub fn new(data: &[u8], repeat: bool) -> Result<Self, Error> {
        let events = music::events(data)?
            .into_iter()
            .map(|e| Event {
                delta: e.delta,
                status: e.status,
                data: e.data.to_vec(),
                meta: e.meta,
                duration: e.duration,
            })
            .collect::<Vec<_>>();
        let due = u64::from(events.first().ok_or(Error("empty sequence"))?.delta);
        Ok(Self {
            events,
            index: 0,
            due,
            tick: 0,
            playing: true,
            repeat,
            loops: Vec::new(),
            notes: BTreeMap::new(),
        })
    }
    /// Emit events in [tick, tick + count), preserving exact 120-Hz timestamps.
    /// Invalid loops and overflow leave state unchanged.
    pub fn advance(&mut self, count: u32) -> Result<Vec<Message>, Error> {
        let mut next = self.clone();
        let result = next.advance_inner(count)?;
        *self = next;
        Ok(result)
    }
    fn release_all(&mut self, tick: u64, output: &mut Vec<Message>) {
        for ((channel, note), _) in std::mem::take(&mut self.notes) {
            output.push(Message {
                tick,
                status: 0x80 | channel,
                data: vec![note, 0],
            });
        }
    }
    pub fn stop(&mut self) -> Vec<Message> {
        let mut output = Vec::new();
        self.release_all(self.tick, &mut output);
        self.playing = false;
        self.loops.clear();
        output
    }
    fn advance_inner(&mut self, count: u32) -> Result<Vec<Message>, Error> {
        let end = self
            .tick
            .checked_add(u64::from(count))
            .ok_or(Error("music clock overflow"))?;
        let mut output = Vec::new();
        let mut budget = 100_000usize;
        loop {
            let note_due = self.notes.values().min().copied();
            let event_due = self.playing.then_some(self.due);
            let Some(at) = note_due.into_iter().chain(event_due).min() else {
                break;
            };
            if at >= end {
                break;
            }
            if budget == 0 {
                return Err(Error("music dispatch budget"));
            }
            budget -= 1;
            // Releases precede new notes at the same musical time.
            let expired = self
                .notes
                .iter()
                .filter(|(_, due)| **due <= at)
                .map(|(key, _)| *key)
                .collect::<Vec<_>>();
            for (channel, note) in expired {
                self.notes.remove(&(channel, note));
                output.push(Message {
                    tick: at,
                    status: 0x80 | channel,
                    data: vec![note, 0],
                });
            }
            if !self.playing || self.due != at {
                continue;
            }
            let event = self
                .events
                .get(self.index)
                .ok_or(Error("sequence index outside events"))?
                .clone();
            self.index += 1;
            if event.meta == Some(0x2f) {
                self.release_all(at, &mut output);
                if self.repeat {
                    self.index = 0;
                    self.loops.clear();
                } else {
                    self.playing = false;
                    continue;
                }
            } else if event.meta.is_none() {
                let channel = event.status & 15;
                let mut status = event.status;
                if status & 0xf0 == 0x90 {
                    if event.data[1] == 0 {
                        status = 0x80 | channel;
                        self.notes.remove(&(channel, event.data[0]));
                    } else {
                        let until = at
                            .checked_add(u64::from(event.duration.unwrap_or(1)))
                            .ok_or(Error("note duration overflow"))?;
                        self.notes.insert((channel, event.data[0]), until);
                    }
                } else if status & 0xf0 == 0x80 {
                    self.notes.remove(&(channel, event.data[0]));
                } else if status & 0xf0 == 0xb0 {
                    match event.data[0] {
                        116 => {
                            if self.loops.len() >= 16 {
                                return Err(Error("XMIDI loop nesting limit"));
                            }
                            self.loops.push(Loop {
                                start: self.index,
                                remaining: event.data[1],
                            });
                        }
                        117 => {
                            if let Some(top) = self.loops.last_mut() {
                                if event.data[1] < 64 || top.remaining == 1 {
                                    self.loops.pop();
                                } else {
                                    if top.remaining > 1 {
                                        top.remaining -= 1;
                                    }
                                    self.index = top.start;
                                }
                            }
                        }
                        _ => {}
                    }
                }
                output.push(Message {
                    tick: at,
                    status,
                    data: event.data,
                });
            }
            // XMIDI uses fixed 500000us/quarter and 60 PPQN; tempo metadata
            // intentionally does not alter this clock (as in the reference).
            let delta = self
                .events
                .get(self.index)
                .ok_or(Error("missing sequence terminator"))?
                .delta;
            self.due = at
                .checked_add(u64::from(delta))
                .ok_or(Error("event time overflow"))?;
        }
        self.tick = end;
        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn notes_release_and_finite_loops_repeat_exactly() {
        let bytes = [
            0xb0, 116, 2, 0x90, 60, 100, 2, 3, 0xb0, 117, 127, 0xff, 0x2f, 0,
        ];
        let mut player = Sequencer::new(&bytes, false).unwrap();
        let out = player.advance(10).unwrap();
        assert_eq!(
            out.iter()
                .filter(|e| e.status == 0x90)
                .map(|e| e.tick)
                .collect::<Vec<_>>(),
            [0, 3]
        );
        assert_eq!(
            out.iter()
                .filter(|e| e.status == 0x80)
                .map(|e| e.tick)
                .collect::<Vec<_>>(),
            [2, 5]
        );
        assert!(!player.playing);
    }
    #[test]
    fn chunked_clock_matches_continuous_and_restored_clock() {
        let bytes = [0x90, 60, 100, 9, 12, 0xff, 0x2f, 0];
        let mut whole = Sequencer::new(&bytes, true).unwrap();
        let mut chunks = whole.clone();
        let expected = whole.advance(50).unwrap();
        let mut actual = Vec::new();
        for _ in 0..10 {
            actual.extend(chunks.advance(5).unwrap());
        }
        assert_eq!(actual, expected);
        assert_eq!(chunks, whole);
        let mut restored = chunks.clone();
        assert_eq!(chunks.advance(5).unwrap(), restored.advance(5).unwrap());
        assert_eq!(chunks.stop(), restored.stop());
    }
    #[test]
    fn zero_duration_infinite_loop_is_bounded_and_atomic() {
        let bytes = [0xb0, 116, 0, 0xb0, 117, 127, 0xff, 0x2f, 0];
        let mut player = Sequencer::new(&bytes, false).unwrap();
        let before = player.clone();
        assert!(player.advance(1).is_err());
        assert_eq!(player, before);
    }
}
