//! Native menu/session boundary. Original menu selections name stable slots;
//! storage never guesses a destination and loads are prepared before activation.
use discworld_audio::{MusicOutput, session, slot};
use discworld_core::{
    audio::SampleBank,
    engine::Engine,
    graphics::Resources,
    menu_controller::Dispatch,
    menu_navigation::Request,
    music::Track,
    save_menu::{Entry, Target},
    text::Strings,
};
use std::{io, path::Path};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub struct Assets<'a, 'data> {
    pub identity: [u8; 32],
    pub resources: &'a Resources<'data>,
    pub bank: &'a SampleBank<'data>,
    pub tracks: &'a [Track<'data>],
    pub strings: &'a Strings<'data>,
    pub soundfont: &'a [u8],
}

impl Assets<'_, '_> {
    fn decode(&self, bytes: &[u8]) -> Result<session::RestoredSession> {
        session::decode(
            bytes,
            self.identity,
            self.resources,
            self.bank,
            self.tracks,
            self.soundfont,
            self.strings,
        )
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Catalog {
    pub entries: Vec<Entry>,
    /// Corrupt/incompatible slots are excluded from menus, but remain reserved
    /// by save_slots::write when allocating a new ID.
    pub rejected: Vec<u32>,
}

/// Validate complete sessions, not merely slot checksums, before offering Load.
/// A changed file is revalidated again by prepare_load at activation time.
pub fn catalog(root: &Path, assets: &Assets<'_, '_>) -> Result<Catalog> {
    let mut result = Catalog::default();
    let mut valid = Vec::new();
    for id in 0..100 {
        let bytes = match crate::save_slots::read(root, id) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) if error.kind() == io::ErrorKind::InvalidData => {
                result.rejected.push(id);
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        let record = slot::decode(&bytes)?;
        if assets.decode(record.session).is_err() {
            result.rejected.push(id);
            continue;
        }
        valid.push((
            record.timestamp_ms,
            Entry {
                id,
                description: record.description.to_vec(),
            },
        ));
    }
    valid.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.id.cmp(&b.1.id)));
    result.entries = valid.into_iter().map(|(_, entry)| entry).collect();
    Ok(result)
}

/// Encode a complete synchronized engine/DSP state, then atomically publish it.
/// NOBOX/empty selection does nothing, matching invSaveGame's guard.
pub fn save(
    root: &Path,
    dispatch: &Dispatch,
    engine: &Engine,
    audio: &MusicOutput,
    identity: [u8; 32],
    timestamp_ms: u64,
) -> Result<Option<u32>> {
    if dispatch.outcome.request != Some(Request::Save) {
        return Ok(None);
    }
    let Some((target, description)) = &dispatch.selection else {
        return Ok(None);
    };
    let bytes = session::encode(engine, audio, identity)?;
    Ok(Some(crate::save_slots::write(
        root,
        *target,
        timestamp_ms,
        description,
        &bytes,
    )?))
}

/// Prepare both engine and synthesizer without touching the running instance.
/// Hosts replace both together and reset queued device audio only after success.
pub fn prepare_load(
    root: &Path,
    dispatch: &Dispatch,
    assets: &Assets<'_, '_>,
) -> Result<Option<session::RestoredSession>> {
    if dispatch.outcome.request != Some(Request::Load) {
        return Ok(None);
    }
    let Some((target, _)) = &dispatch.selection else {
        return Ok(None);
    };
    let Target::Existing(id) = target else {
        return Err("load requires an existing slot".into());
    };
    let bytes = crate::save_slots::read(root, *id)?;
    let record = slot::decode(&bytes)?;
    Ok(Some(assets.decode(record.session)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use discworld_core::{iso::Iso, menu_navigation::Outcome, resource::parse_index};

    #[test]
    #[ignore = "requires DISCWORLD_TEST_ISO, DISCWORLD_TEST_SOUNDFONT and DISCWORLD_TEST_SAVE"]
    fn menu_slots_validate_assets_and_prepare_exact_session_replacement() {
        let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
        let data = read("DISCWORLD_TEST_ISO");
        let font = read("DISCWORLD_TEST_SOUNDFONT");
        let checkpoint = read("DISCWORLD_TEST_SAVE");
        let iso = Iso::parse(&data).unwrap();
        let file = |name: &str| iso.file(name).unwrap();
        let index = parse_index(file("/DISCWLD/INDEX")).unwrap();
        let resources = Resources(
            index
                .iter()
                .map(|entry| file(&format!("/DISCWLD/{}", entry.name)))
                .collect(),
        );
        let bank =
            SampleBank::new(file("/DISCWLD/ENGLISH.IDX"), file("/DISCWLD/ENGLISH.SMP")).unwrap();
        let strings = Strings::new(file("/DISCWLD/ENGLISH.TXT")).unwrap();
        let tracks = discworld_core::music::archive(file("/DISCWLD/MIDI.DAT")).unwrap();
        let assets = Assets {
            identity: session::asset_identity(&data),
            resources: &resources,
            bank: &bank,
            tracks: &tracks,
            strings: &strings,
            soundfont: &font,
        };
        let current = assets.decode(&checkpoint).unwrap();
        let root = std::env::temp_dir().join(format!(
            "discworld-menu-slots-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        let request = |kind, selection| Dispatch {
            outcome: Outcome {
                request: Some(kind),
                ..Outcome::default()
            },
            selection,
        };
        assert_eq!(
            save(
                &root,
                &request(Request::Save, None),
                &current.engine,
                &current.audio,
                assets.identity,
                1
            )
            .unwrap(),
            None
        );
        assert!(!root.exists());
        assert!(
            prepare_load(&root, &request(Request::Load, None), &assets)
                .unwrap()
                .is_none()
        );
        let save_request = request(Request::Save, Some((Target::New, b"Library".to_vec())));
        assert_eq!(
            save(
                &root,
                &save_request,
                &current.engine,
                &current.audio,
                assets.identity,
                1
            )
            .unwrap(),
            Some(0)
        );
        let load_request = request(
            Request::Load,
            Some((Target::Existing(0), b"ignored stale description".to_vec())),
        );
        let loaded = prepare_load(&root, &load_request, &assets)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.engine, current.engine);
        assert_eq!(
            session::encode(&loaded.engine, &loaded.audio, assets.identity).unwrap(),
            checkpoint
        );
        let foreign = session::encode(&current.engine, &current.audio, [0; 32]).unwrap();
        crate::save_slots::write(&root, Target::Existing(1), 3, b"Wrong ISO", &foreign).unwrap();
        std::fs::write(root.join("slot-002.dwslot"), b"corrupt").unwrap();
        let catalog = catalog(&root, &assets).unwrap();
        assert_eq!(
            catalog.entries,
            vec![Entry {
                id: 0,
                description: b"Library".to_vec()
            }]
        );
        assert_eq!(catalog.rejected, [1, 2]);
        for id in [1, 2] {
            assert!(
                prepare_load(
                    &root,
                    &request(Request::Load, Some((Target::Existing(id), Vec::new()))),
                    &assets
                )
                .is_err()
            );
        }
        assert_eq!(
            save(
                &root,
                &save_request,
                &current.engine,
                &current.audio,
                assets.identity,
                4
            )
            .unwrap(),
            Some(3)
        );
        assert_eq!(
            std::fs::read(root.join("slot-002.dwslot")).unwrap(),
            b"corrupt"
        );
        assert_eq!(
            session::encode(&current.engine, &current.audio, assets.identity).unwrap(),
            checkpoint
        );
    }
}
