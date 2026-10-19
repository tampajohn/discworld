//! Read-only ISO 9660 primary-volume reader. No filesystem writes or host APIs.
//! Deliberately rejects interleaved and multi-extent records rather than guessing.
use std::collections::BTreeSet;
use std::fmt;
use std::ops::Range;

const SECTOR: usize = 2048;
const MAX_RECORDS: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub &'static str);
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for Error {}
type Result<T> = std::result::Result<T, Error>;

fn bytes(data: &[u8], offset: usize, len: usize) -> Result<&[u8]> {
    let end = offset.checked_add(len).ok_or(Error("offset overflow"))?;
    data.get(offset..end).ok_or(Error("truncated data"))
}
fn both32(data: &[u8], offset: usize) -> Result<u32> {
    let b = bytes(data, offset, 8)?;
    let le = u32::from_le_bytes(b[..4].try_into().unwrap());
    let be = u32::from_be_bytes(b[4..].try_into().unwrap());
    if le != be {
        return Err(Error("inconsistent endian copies"));
    }
    Ok(le)
}
fn both16(data: &[u8], offset: usize) -> Result<u16> {
    let b = bytes(data, offset, 4)?;
    let le = u16::from_le_bytes(b[..2].try_into().unwrap());
    let be = u16::from_be_bytes(b[2..].try_into().unwrap());
    if le != be {
        return Err(Error("inconsistent endian copies"));
    }
    Ok(le)
}
fn extent(record: &[u8], volume_len: usize) -> Result<Range<usize>> {
    bytes(record, 0, 34)?;
    if record[1] != 0 || record[26] != 0 || record[27] != 0 || record[25] & 0x80 != 0 {
        return Err(Error(
            "unsupported extended, interleaved, or multi-extent record",
        ));
    }
    if both16(record, 28)? != 1 {
        return Err(Error("unsupported volume sequence"));
    }
    let start = usize::try_from(both32(record, 2)?)
        .map_err(|_| Error("extent overflow"))?
        .checked_mul(SECTOR)
        .ok_or(Error("extent overflow"))?;
    let len = usize::try_from(both32(record, 10)?).map_err(|_| Error("length overflow"))?;
    let end = start.checked_add(len).ok_or(Error("extent overflow"))?;
    if end > volume_len {
        return Err(Error("extent outside volume"));
    }
    Ok(start..end)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub range: Range<usize>,
}

#[derive(Debug)]
pub struct Iso<'a> {
    data: &'a [u8],
    pub volume_id: String,
    pub entries: Vec<Entry>,
}

impl<'a> Iso<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        let mut primary = None;
        let mut terminated = false;
        for sector in 16..80 {
            let desc = bytes(data, sector * SECTOR, SECTOR)?;
            if &desc[1..6] != b"CD001" || desc[6] != 1 {
                return Err(Error("invalid volume descriptor"));
            }
            match desc[0] {
                1 if primary.is_some() => return Err(Error("duplicate primary volume")),
                1 => primary = Some(desc),
                255 => {
                    terminated = true;
                    break;
                }
                _ => {}
            }
        }
        if !terminated {
            return Err(Error("missing descriptor terminator"));
        }
        let p = primary.ok_or(Error("missing primary volume"))?;
        if both16(p, 128)? != SECTOR as u16 {
            return Err(Error("unsupported block size"));
        }
        let volume_len = usize::try_from(both32(p, 80)?)
            .map_err(|_| Error("volume overflow"))?
            .checked_mul(SECTOR)
            .ok_or(Error("volume overflow"))?;
        bytes(data, 0, volume_len)?;
        let root = bytes(p, 156, usize::from(p[156]))?;
        if root.get(25).is_none_or(|f| f & 2 == 0) {
            return Err(Error("root is not a directory"));
        }
        let root_range = extent(root, volume_len)?;
        let mut pending = vec![(String::new(), root_range)];
        let mut visited = BTreeSet::new();
        let mut paths = BTreeSet::new();
        let mut entries = Vec::new();
        let mut count = 0;
        while let Some((parent, range)) = pending.pop() {
            if !visited.insert(range.start) {
                return Err(Error("repeated directory extent"));
            }
            let dir = &data[range];
            let mut pos = 0;
            while pos < dir.len() {
                let len = usize::from(dir[pos]);
                if len == 0 {
                    pos = (pos / SECTOR + 1) * SECTOR;
                    continue;
                }
                count += 1;
                if count > MAX_RECORDS {
                    return Err(Error("directory record limit exceeded"));
                }
                if pos % SECTOR + len > SECTOR {
                    return Err(Error("record crosses sector"));
                }
                let record = bytes(dir, pos, len)?;
                pos += len;
                bytes(record, 0, 34)?;
                let name = bytes(record, 33, usize::from(record[32]))?;
                let file_range = extent(record, volume_len)?;
                if name == [0] || name == [1] {
                    continue;
                }
                let raw =
                    std::str::from_utf8(name).map_err(|_| Error("non-UTF8 ISO identifier"))?;
                let base = raw.split(';').next().unwrap_or("").trim_end_matches('.');
                if base.is_empty()
                    || base == "."
                    || base == ".."
                    || !base
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'.')
                {
                    return Err(Error("unsupported ISO identifier"));
                }
                let path = format!("{parent}/{base}");
                if path.len() > 4096 {
                    return Err(Error("path limit exceeded"));
                }
                if !paths.insert(path.clone()) {
                    return Err(Error("duplicate normalized path"));
                }
                if record[25] & 2 != 0 {
                    pending.push((path, file_range));
                } else {
                    entries.push(Entry {
                        path,
                        range: file_range,
                    });
                }
            }
        }
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(Self {
            data,
            volume_id: String::from_utf8_lossy(&p[40..72]).trim().to_owned(),
            entries,
        })
    }

    pub fn file(&self, path: &str) -> Option<&'a [u8]> {
        let e = self
            .entries
            .iter()
            .find(|e| e.path.eq_ignore_ascii_case(path))?;
        self.data.get(e.range.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn b32(b: &mut [u8], at: usize, n: u32) {
        b[at..at + 4].copy_from_slice(&n.to_le_bytes());
        b[at + 4..at + 8].copy_from_slice(&n.to_be_bytes());
    }
    fn b16(b: &mut [u8], at: usize, n: u16) {
        b[at..at + 2].copy_from_slice(&n.to_le_bytes());
        b[at + 2..at + 4].copy_from_slice(&n.to_be_bytes());
    }
    fn record(block: u32, size: u32, flags: u8, name: &[u8]) -> Vec<u8> {
        let mut b = vec![0; (33 + name.len() + 1) & !1];
        b[0] = b.len() as u8;
        b32(&mut b, 2, block);
        b32(&mut b, 10, size);
        b[25] = flags;
        b16(&mut b, 28, 1);
        b[32] = name.len() as u8;
        b[33..33 + name.len()].copy_from_slice(name);
        b
    }
    fn fixture() -> Vec<u8> {
        let mut b = vec![0; 21 * SECTOR];
        let p = &mut b[16 * SECTOR..17 * SECTOR];
        p[0] = 1;
        p[1..6].copy_from_slice(b"CD001");
        p[6] = 1;
        b32(p, 80, 21);
        b16(p, 128, 2048);
        p[156..190].copy_from_slice(&record(19, 2048, 2, &[0]));
        let t = &mut b[17 * SECTOR..18 * SECTOR];
        t[0] = 255;
        t[1..6].copy_from_slice(b"CD001");
        t[6] = 1;
        let r = record(20, 3, 0, b"TEST.TXT;1");
        b[19 * SECTOR..19 * SECTOR + r.len()].copy_from_slice(&r);
        b[20 * SECTOR..20 * SECTOR + 3].copy_from_slice(b"abc");
        b
    }
    #[test]
    fn reads_file() {
        let b = fixture();
        let iso = Iso::parse(&b).unwrap();
        assert_eq!(iso.file("/test.txt"), Some(b"abc".as_slice()));
    }
    #[test]
    fn truncated_inputs_do_not_panic() {
        let b = fixture();
        for n in (0..b.len()).step_by(127) {
            assert!(Iso::parse(&b[..n]).is_err());
        }
    }
    #[test]
    fn rejects_outside_extent() {
        let mut b = fixture();
        b32(&mut b, 19 * SECTOR + 2, 99);
        assert!(Iso::parse(&b).is_err());
    }
    #[test]
    fn rejects_endian_mismatch() {
        let mut b = fixture();
        b[16 * SECTOR + 84] = 1;
        assert!(Iso::parse(&b).is_err());
    }
    #[test]
    fn rejects_directory_cycle() {
        let mut b = fixture();
        b[19 * SECTOR..20 * SECTOR].fill(0);
        let r = record(19, 2048, 2, b"LOOP");
        b[19 * SECTOR..19 * SECTOR + r.len()].copy_from_slice(&r);
        assert_eq!(Iso::parse(&b).unwrap_err().0, "repeated directory extent");
    }
    #[test]
    fn rejects_path_traversal() {
        let mut b = fixture();
        let r = record(20, 3, 0, b"../BAD;1");
        b[19 * SECTOR..19 * SECTOR + r.len()].copy_from_slice(&r);
        assert!(Iso::parse(&b).is_err());
    }
    #[test]
    fn rejects_multi_extent() {
        let mut b = fixture();
        b[19 * SECTOR + 25] = 128;
        assert!(Iso::parse(&b).is_err());
    }
}
