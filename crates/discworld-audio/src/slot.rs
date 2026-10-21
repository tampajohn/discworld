//! Portable save-slot envelope: metadata and complete engine/DSP checkpoint in
//! one checksummed record. Envelope validation is not full session restoration.
use sha2::{Digest, Sha256};
const HEADER: usize = 64;
pub const MAX_BYTES: usize = HEADER + 40 + crate::session::MAX_SESSION_BYTES;
type Error = Box<dyn std::error::Error>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Slot<'a> {
    pub id: u32,
    pub timestamp_ms: u64,
    pub description: &'a [u8],
    pub session: &'a [u8],
}
pub fn encode(
    id: u32,
    timestamp_ms: u64,
    description: &[u8],
    session: &[u8],
) -> Result<Vec<u8>, Error> {
    if id >= 100 || description.len() > 40 || description.contains(&0) {
        return Err("invalid slot metadata".into());
    }
    crate::session::sections(session)?;
    let mut bytes = Vec::with_capacity(HEADER + description.len() + session.len());
    bytes.extend_from_slice(b"DWRSLOT\0");
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&id.to_le_bytes());
    bytes.extend_from_slice(&timestamp_ms.to_le_bytes());
    bytes.extend_from_slice(&(description.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(session.len() as u32).to_le_bytes());
    let mut digest = Sha256::new();
    digest.update(&bytes);
    digest.update(description);
    digest.update(session);
    bytes.extend_from_slice(&digest.finalize());
    bytes.extend_from_slice(description);
    bytes.extend_from_slice(session);
    Ok(bytes)
}
pub fn decode(bytes: &[u8]) -> Result<Slot<'_>, Error> {
    if !(HEADER..=MAX_BYTES).contains(&bytes.len()) {
        return Err("invalid slot size".into());
    }
    let word = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    if &bytes[..8] != b"DWRSLOT\0" || word(8) != 1 {
        return Err("unsupported slot format".into());
    }
    let id = word(12);
    let description_len = word(24) as usize;
    let session_len = word(28) as usize;
    if id >= 100
        || description_len > 40
        || session_len > crate::session::MAX_SESSION_BYTES
        || description_len.checked_add(session_len) != Some(bytes.len() - HEADER)
    {
        return Err("invalid slot lengths or ID".into());
    }
    let mut digest = Sha256::new();
    digest.update(&bytes[..32]);
    digest.update(&bytes[HEADER..]);
    if digest.finalize()[..] != bytes[32..HEADER] {
        return Err("slot checksum mismatch".into());
    }
    let (description, session) = bytes[HEADER..].split_at(description_len);
    if description.contains(&0) {
        return Err("invalid slot description".into());
    }
    crate::session::sections(session)?;
    Ok(Slot {
        id,
        timestamp_ms: u64::from_le_bytes(bytes[16..24].try_into().unwrap()),
        description,
        session,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metadata_and_session_are_one_checked_record() {
        // Structurally valid session envelope; full engine/DSP decode is a
        // separate resource-dependent test, not claimed by this fixture.
        let mut session = b"DWRPLAY\0".to_vec();
        session.extend_from_slice(&1u32.to_le_bytes());
        session.extend_from_slice(&0u32.to_le_bytes());
        session.extend_from_slice(&0u32.to_le_bytes());
        let digest = Sha256::digest(&session);
        session.extend_from_slice(&digest);
        let bytes = encode(99, 1234, b"Library", &session).unwrap();
        assert_eq!(
            decode(&bytes).unwrap(),
            Slot {
                id: 99,
                timestamp_ms: 1234,
                description: b"Library",
                session: &session
            }
        );
        for end in 0..bytes.len() {
            assert!(decode(&bytes[..end]).is_err());
        }
        for i in 0..bytes.len() {
            let mut bad = bytes.clone();
            bad[i] ^= 1;
            assert!(decode(&bad).is_err(), "byte {i}");
        }
        assert!(encode(100, 0, b"", &session).is_err());
        assert!(encode(0, 0, &[b'a'; 41], &session).is_err());
        assert!(encode(0, 0, b"bad\0name", &session).is_err());
        let mut trailing = bytes;
        trailing.push(0);
        assert!(decode(&trailing).is_err());
    }
}
