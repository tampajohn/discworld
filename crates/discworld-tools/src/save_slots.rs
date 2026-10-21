//! Native slot storage. One atomic record per stable ID; no separate manifest.
//! Session resource validation still occurs before a host activates a load.
use discworld_audio::slot;
use discworld_core::save_menu::Target;
use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    pub id: u32,
    pub timestamp_ms: u64,
    pub description: Vec<u8>,
}
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Catalog {
    pub entries: Vec<Metadata>,
    pub rejected: Vec<u32>,
}
fn path(root: &Path, id: u32) -> io::Result<PathBuf> {
    if id >= 100 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "slot ID outside capacity",
        ));
    }
    Ok(root.join(format!("slot-{id:03}.dwslot")))
}
fn invalid(error: impl ToString) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, error.to_string())
}
pub fn read_record(path: &Path) -> io::Result<Vec<u8>> {
    let bytes = crate::save_file::read_bounded(path, slot::MAX_BYTES)?;
    slot::decode(&bytes).map_err(invalid)?;
    Ok(bytes)
}
pub fn read(root: &Path, id: u32) -> io::Result<Vec<u8>> {
    let bytes = crate::save_file::read_bounded(&path(root, id)?, slot::MAX_BYTES)?;
    if slot::decode(&bytes).map_err(invalid)?.id != id {
        return Err(invalid("slot ID does not match filename"));
    }
    Ok(bytes)
}
pub fn catalog(root: &Path) -> io::Result<Catalog> {
    let mut catalog = Catalog::default();
    for id in 0..100 {
        match read(root, id) {
            Ok(bytes) => {
                let record = slot::decode(&bytes).map_err(invalid)?;
                catalog.entries.push(Metadata {
                    id,
                    timestamp_ms: record.timestamp_ms,
                    description: record.description.to_vec(),
                });
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) if e.kind() == io::ErrorKind::InvalidData => catalog.rejected.push(id),
            Err(e) => return Err(e),
        }
    }
    catalog
        .entries
        .sort_by(|a, b| b.timestamp_ms.cmp(&a.timestamp_ms).then(a.id.cmp(&b.id)));
    Ok(catalog)
}
pub fn write(
    root: &Path,
    target: Target,
    timestamp_ms: u64,
    description: &[u8],
    session: &[u8],
) -> io::Result<u32> {
    std::fs::create_dir_all(root)?;
    let ids: Box<dyn Iterator<Item = u32>> = match target {
        Target::New => Box::new(0..100),
        Target::Existing(id) => Box::new(std::iter::once(id)),
    };
    for id in ids {
        let destination = path(root, id)?;
        let replace = matches!(target, Target::Existing(_));
        if !replace && destination.try_exists()? {
            continue;
        }
        let bytes = slot::encode(id, timestamp_ms, description, session).map_err(invalid)?;
        match crate::save_file::write_bounded(&destination, &bytes, slot::MAX_BYTES, replace) {
            Ok(()) => return Ok(id),
            Err(e) if !replace && e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::StorageFull,
        "all 100 save slots are occupied",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    #[test]
    fn concurrent_new_slots_and_corrupt_occupied_names_are_preserved() {
        let root = std::env::temp_dir().join(format!(
            "discworld-slots-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(path(&root, 0).unwrap(), b"corrupt, must not replace").unwrap();
        let mut session = b"DWRPLAY\0".to_vec();
        session.extend_from_slice(&1u32.to_le_bytes());
        session.extend_from_slice(&[0; 8]);
        let digest = Sha256::digest(&session);
        session.extend_from_slice(&digest);
        std::thread::scope(|scope| {
            let handles = (0..8)
                .map(|i| {
                    let root = &root;
                    let session = &session;
                    scope.spawn(move || write(root, Target::New, i, b"Library", session).unwrap())
                })
                .collect::<Vec<_>>();
            let ids = handles
                .into_iter()
                .map(|h| h.join().unwrap())
                .collect::<std::collections::BTreeSet<_>>();
            assert_eq!(ids, (1..=8).collect());
        });
        let before = read(&root, 1).unwrap();
        assert!(write(&root, Target::Existing(1), 99, b"bad\0name", &session).is_err());
        assert_eq!(read(&root, 1).unwrap(), before);
        write(&root, Target::Existing(1), 99, b"New description", &session).unwrap();
        let catalog = catalog(&root).unwrap();
        assert_eq!(catalog.rejected, vec![0]);
        assert_eq!(catalog.entries.len(), 8);
        assert_eq!(catalog.entries[0].id, 1);
        assert_eq!(catalog.entries[0].description, b"New description");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 9);
        assert_eq!(
            std::fs::read(path(&root, 0).unwrap()).unwrap(),
            b"corrupt, must not replace"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
