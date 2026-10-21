//! Native file persistence. Write a complete sibling file before atomic rename.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
static SERIAL: AtomicU64 = AtomicU64::new(0);

pub fn read(path: &Path) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(discworld_audio::session::MAX_SESSION_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > discworld_audio::session::MAX_SESSION_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "save file size limit",
        ));
    }
    Ok(bytes)
}

struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if bytes.len() > discworld_audio::session::MAX_SESSION_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "save file size limit",
        ));
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "save path has no filename"))?;
    let (temp, mut file) = loop {
        let mut filename = name.to_os_string();
        filename.push(format!(
            ".tmp-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        let candidate = parent.join(filename);
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&candidate)
        {
            Ok(file) => break (Temporary(candidate), file),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    };
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    fs::rename(&temp.0, path)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacement_is_complete_and_failed_rename_cleans_temporary_file() {
        let dir = std::env::temp_dir().join(format!(
            "discworld-save-test-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&dir).unwrap();
        let path = dir.join("slot.dws");
        write_atomic(&path, b"first").unwrap();
        write_atomic(&path, b"complete replacement").unwrap();
        assert_eq!(read(&path).unwrap(), b"complete replacement");
        let blocked = dir.join("directory.dws");
        fs::create_dir(&blocked).unwrap();
        assert!(write_atomic(&blocked, b"must not replace a directory").is_err());
        assert!(blocked.is_dir());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 2);
        assert_eq!(read(&path).unwrap(), b"complete replacement");
        fs::remove_dir_all(dir).unwrap();
    }
}
