//! Bounded, validated native preferences, independent of game save slots.
use discworld_core::menu_controller::{Controller, Settings};
use std::{io, path::Path};

const LIMIT: usize = 4096;

pub fn read(path: &Path) -> io::Result<Option<Settings>> {
    let bytes = match crate::save_file::read_bounded(path, LIMIT) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let settings: Settings = serde_json::from_slice(&bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    Controller::new(settings.clone())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
    Ok(Some(settings))
}

pub fn write(path: &Path, settings: &Settings) -> io::Result<()> {
    Controller::new(settings.clone())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
    let bytes = serde_json::to_vec_pretty(settings)?;
    crate::save_file::write_bounded(path, &bytes, LIMIT, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preferences_validate_before_replacing_and_bound_reads() {
        let root = std::env::temp_dir().join(format!(
            "discworld-settings-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let path = root.join("settings.json");
        assert_eq!(read(&path).unwrap(), None);
        let settings = Settings {
            volumes: [0, 123, 255],
            double_click: 30,
            swap_buttons: true,
            text_speed: 100,
            subtitles: false,
            flipped: true,
        };
        write(&path, &settings).unwrap();
        assert_eq!(read(&path).unwrap(), Some(settings.clone()));
        for invalid in [
            Settings {
                text_speed: 101,
                ..settings.clone()
            },
            Settings {
                double_click: 31,
                ..settings.clone()
            },
            Settings {
                volumes: [256, 0, 0],
                ..settings.clone()
            },
        ] {
            assert_eq!(
                write(&path, &invalid).unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
            assert_eq!(read(&path).unwrap(), Some(settings.clone()));
        }
        for bytes in [
            b"{".to_vec(),
            vec![b' '; LIMIT + 1],
            serde_json::to_vec(&Settings {
                text_speed: 101,
                ..settings.clone()
            })
            .unwrap(),
        ] {
            std::fs::write(&path, bytes).unwrap();
            assert_eq!(read(&path).unwrap_err().kind(), io::ErrorKind::InvalidData);
        }
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }
}
