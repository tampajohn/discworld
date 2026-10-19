// SPDX-License-Identifier: GPL-3.0-or-later
//! Checked MIDI.DAT archive and XMIDI event decoding. No synthesizer yet.
use crate::{film::word, iso::Error};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track<'a> {
    pub offset: u32,
    pub events: &'a [u8],
}

fn event_chunks<'a>(
    bytes: &'a [u8],
    depth: usize,
    output: &mut Vec<&'a [u8]>,
) -> Result<(), Error> {
    if depth > 8 {
        return Err(Error("IFF nesting limit"));
    }
    let mut at = 0usize;
    while at < bytes.len() {
        let header = bytes.get(at..at + 8).ok_or(Error("truncated IFF header"))?;
        let length = u32::from_be_bytes(header[4..8].try_into().unwrap()) as usize;
        let end = at
            .checked_add(8)
            .and_then(|n| n.checked_add(length))
            .ok_or(Error("IFF size overflow"))?;
        let body = bytes.get(at + 8..end).ok_or(Error("truncated IFF body"))?;
        match &header[..4] {
            b"FORM" | b"CAT " => {
                let contents = body.get(4..).ok_or(Error("missing IFF type"))?;
                if !matches!(&body[..4], b"XMID" | b"XDIR") {
                    return Err(Error("unsupported music IFF type"));
                }
                event_chunks(contents, depth + 1, output)?;
            }
            b"EVNT" => output.push(body),
            b"INFO" | b"TIMB" | b"RBRN" => {}
            _ => return Err(Error("unknown XMIDI chunk")),
        }
        at = end
            .checked_add(length & 1)
            .ok_or(Error("IFF padding overflow"))?;
        if at > bytes.len() {
            return Err(Error("missing IFF padding"));
        }
    }
    Ok(())
}

pub fn archive(bytes: &[u8]) -> Result<Vec<Track<'_>>, Error> {
    let maximum = word(bytes, 0)? as usize;
    let mut at = 4usize;
    let mut tracks = Vec::new();
    while at < bytes.len() {
        if tracks.len() >= 4096 {
            return Err(Error("music track limit"));
        }
        let length = word(bytes, at)? as usize;
        if length == 0 || length > maximum {
            return Err(Error("invalid music track length"));
        }
        let end = at
            .checked_add(4)
            .and_then(|n| n.checked_add(length))
            .ok_or(Error("music track overflow"))?;
        let payload = bytes
            .get(at + 4..end)
            .ok_or(Error("truncated music track"))?;
        let mut events = Vec::new();
        event_chunks(payload, 0, &mut events)?;
        if events.len() != 1 {
            return Err(Error("expected one XMIDI sequence per track"));
        }
        tracks.push(Track {
            offset: u32::try_from(at).map_err(|_| Error("music offset overflow"))?,
            events: events[0],
        });
        at = end;
    }
    Ok(tracks)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event<'a> {
    pub delta: u32,
    pub status: u8,
    pub meta: Option<u8>,
    pub data: &'a [u8],
    pub duration: Option<u32>,
}

fn byte(bytes: &[u8], at: &mut usize) -> Result<u8, Error> {
    let b = *bytes.get(*at).ok_or(Error("truncated XMIDI event"))?;
    *at += 1;
    Ok(b)
}
fn vlq(bytes: &[u8], at: &mut usize) -> Result<u32, Error> {
    let mut value = 0;
    for _ in 0..4 {
        let b = byte(bytes, at)?;
        value = (value << 7) | u32::from(b & 127);
        if b < 128 {
            return Ok(value);
        }
    }
    Err(Error("XMIDI VLQ too long"))
}

/// Decode encoded events in file order. Loop/branch controllers remain events;
/// this is not an expanded playback timeline or synthesizer.
pub fn events(bytes: &[u8]) -> Result<Vec<Event<'_>>, Error> {
    let mut at = 0;
    let mut result = Vec::new();
    while at < bytes.len() {
        if result.len() >= 1_000_000 {
            return Err(Error("music event limit"));
        }
        let mut delta = 0u32;
        let status = loop {
            let b = byte(bytes, &mut at)?;
            if b >= 128 {
                break b;
            }
            delta = delta
                .checked_add(u32::from(b))
                .ok_or(Error("XMIDI delay overflow"))?;
        };
        let mut meta = None;
        let length = match status {
            0x80..=0xbf | 0xe0..=0xef => 2,
            0xc0..=0xdf => 1,
            0xf0 | 0xf7 => vlq(bytes, &mut at)? as usize,
            0xff => {
                meta = Some(byte(bytes, &mut at)?);
                vlq(bytes, &mut at)? as usize
            }
            _ => return Err(Error("unsupported XMIDI status")),
        };
        let end = at
            .checked_add(length)
            .ok_or(Error("XMIDI payload overflow"))?;
        let data = bytes.get(at..end).ok_or(Error("truncated XMIDI payload"))?;
        if status < 0xf0 && data.iter().any(|b| *b >= 128) {
            return Err(Error("invalid MIDI data byte"));
        }
        at = end;
        let duration = if status & 0xf0 == 0x90 {
            Some(vlq(bytes, &mut at)?.max(1))
        } else {
            None
        };
        result.push(Event {
            delta,
            status,
            meta,
            data,
            duration,
        });
        if meta == Some(0x2f) {
            // This release includes an even-alignment zero in some EVNT sizes.
            let padded_end = at % 2 == 1 && bytes.get(at..) == Some(&[0]);
            if !data.is_empty() || (at != bytes.len() && !padded_end) {
                return Err(Error("invalid XMIDI end marker"));
            }
            return Ok(result);
        }
    }
    Err(Error("missing XMIDI end marker"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_preserves_offsets_and_bounds_nested_chunks() {
        let mut payload = b"FORM".to_vec();
        payload.extend_from_slice(&16u32.to_be_bytes());
        payload.extend_from_slice(b"XMIDEVNT");
        payload.extend_from_slice(&3u32.to_be_bytes());
        payload.extend_from_slice(&[0xff, 0x2f, 0, 0]);
        let mut data = (payload.len() as u32).to_le_bytes().to_vec();
        for _ in 0..2 {
            data.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            data.extend_from_slice(&payload);
        }
        let tracks = archive(&data).unwrap();
        assert_eq!(tracks.iter().map(|t| t.offset).collect::<Vec<_>>(), [4, 32]);
        assert_eq!(events(tracks[0].events).unwrap().len(), 1);
        assert!(archive(&data[..data.len() - 1]).is_err());
    }
    #[test]
    fn additive_delays_and_note_duration_are_distinct() {
        let data = [127, 3, 0x90, 60, 100, 0x81, 0, 0xff, 0x2f, 0];
        let parsed = events(&data).unwrap();
        assert_eq!(parsed[0].delta, 130);
        assert_eq!(parsed[0].duration, Some(128));
        for n in 0..data.len() {
            assert!(events(&data[..n]).is_err());
        }
    }
    #[test]
    fn rejects_malformed_container_and_event_data() {
        assert!(archive(&[0; 8]).is_err());
        assert!(events(&[0x90, 128, 1, 1, 0xff, 0x2f, 0]).is_err());
        assert!(events(&[0xff, 0x2f, 0, 0]).is_ok());
        assert!(events(&[0xff, 0x2f, 0, 1]).is_err());
    }
}
