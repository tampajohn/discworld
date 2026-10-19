//! Discworld DOS Tinsel V1 resource metadata. See docs/resource-format.md.
use crate::iso::Error;
use std::ops::Range;

#[derive(Debug, PartialEq, Eq)]
pub struct IndexEntry {
    pub name: String,
    pub size: u32,
    pub flags: u8,
}

pub fn parse_index(data: &[u8]) -> Result<Vec<IndexEntry>, Error> {
    if data.is_empty() || data.len() % 20 != 0 {
        return Err(Error("invalid V1 index length"));
    }
    data.chunks_exact(20)
        .map(|r| {
            let end = r[..12].iter().position(|b| *b == 0).unwrap_or(12);
            let name =
                std::str::from_utf8(&r[..end]).map_err(|_| Error("invalid resource name"))?;
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|c| c.is_ascii_alphanumeric() || c == b'.' || c == b'_')
                || name == "."
                || name == ".."
            {
                return Err(Error("invalid resource name"));
            }
            let packed = u32::from_le_bytes(r[12..16].try_into().unwrap());
            Ok(IndexEntry {
                name: name.to_owned(),
                size: packed & 0x00ff_ffff,
                flags: (packed >> 24) as u8,
            })
        })
        .collect()
}

#[derive(Debug, PartialEq, Eq)]
pub struct Chunk {
    pub kind: u32,
    pub header_offset: usize,
    pub payload: Range<usize>,
}

/// Chunk links are absolute file offsets, not lengths. Last link is zero.
/// Payload ranges are structural spans; individual decoders must validate lengths.
pub fn chunks(data: &[u8]) -> Result<Vec<Chunk>, Error> {
    let mut result = Vec::new();
    let mut offset: usize = 0;
    loop {
        let end = offset
            .checked_add(8)
            .ok_or(Error("chunk offset overflow"))?;
        let header = data
            .get(offset..end)
            .ok_or(Error("truncated chunk header"))?;
        let kind = u32::from_le_bytes(header[..4].try_into().unwrap());
        let next = u32::from_le_bytes(header[4..].try_into().unwrap()) as usize;
        let payload_end = if next == 0 { data.len() } else { next };
        if payload_end < end || payload_end > data.len() || (next != 0 && next % 4 != 0) {
            return Err(Error("invalid chunk link"));
        }
        result.push(Chunk {
            kind,
            header_offset: offset,
            payload: end..payload_end,
        });
        if result.len() > 65_536 {
            return Err(Error("chunk count limit exceeded"));
        }
        if next == 0 {
            return Ok(result);
        }
        offset = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn index_separates_flags_and_size() {
        let mut b = [0; 20];
        b[..6].copy_from_slice(b"dw.scn");
        b[12..16].copy_from_slice(&0x010bd7fcu32.to_le_bytes());
        assert_eq!(
            parse_index(&b).unwrap(),
            vec![IndexEntry {
                name: "dw.scn".into(),
                size: 776188,
                flags: 1
            }]
        );
        for n in 0..20 {
            assert!(parse_index(&b[..n]).is_err());
        }
    }
    #[test]
    fn follows_absolute_links() {
        let b = [1, 0, 0, 0, 12, 0, 0, 0, 9, 9, 9, 9, 2, 0, 0, 0, 0, 0, 0, 0];
        let c = chunks(&b).unwrap();
        assert_eq!(c[0].payload, 8..12);
        assert_eq!(c[1].payload, 20..20);
    }
    #[test]
    fn rejects_bad_links() {
        for next in [4u32, 9, 12, u32::MAX] {
            let mut b = [0; 8];
            b[4..].copy_from_slice(&next.to_le_bytes());
            assert!(chunks(&b).is_err());
        }
        assert!(chunks(&[0; 7]).is_err());
    }
}
