//! Native services for the original menus. Game state stays in the shared core.
use discworld_core::{
    engine::Engine,
    menu_controller::{Controller, Dispatch, Settings},
    menu_navigation::{Key, Request},
};
use discworld_tools::menu_sessions::{self, Assets, Catalog};
use std::path::PathBuf;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

pub struct Menu<'a, 'data> {
    pub assets: Assets<'a, 'data>,
    pub catalog: Catalog,
    root: PathBuf,
}

pub enum Effect {
    None,
    Saved(u32),
    Loaded(Box<discworld_audio::session::RestoredSession>),
    Quit,
}

impl<'a, 'data> Menu<'a, 'data> {
    pub fn new(root: PathBuf, assets: Assets<'a, 'data>) -> Result<Self> {
        let catalog = menu_sessions::catalog(&root, &assets)?;
        Ok(Self {
            assets,
            catalog,
            root,
        })
    }

    pub fn initialize(engine: &mut Engine) -> Result<()> {
        if engine.configuration.is_none() {
            engine.configuration = Some(Controller::new(Settings {
                volumes: [255; 3],
                double_click: 10,
                swap_buttons: false,
                text_speed: 0,
                subtitles: engine.subtitles,
                flipped: false,
            })?);
        }
        Ok(())
    }

    pub fn active(engine: &Engine) -> bool {
        engine
            .configuration
            .as_ref()
            .is_some_and(|menu| menu.page().is_some())
    }

    pub fn open(&mut self, engine: &mut Engine, key: Key) -> Result<()> {
        // Refresh before opening: another instance may have changed the slots.
        self.catalog = menu_sessions::catalog(&self.root, &self.assets)?;
        Self::initialize(engine)?;
        engine.open_configuration(
            self.assets.resources,
            self.assets.strings,
            &self.catalog.entries,
            key,
        )?;
        Ok(())
    }

    pub fn dispatch(
        &mut self,
        dispatch: &Dispatch,
        engine: &Engine,
        audio: &discworld_audio::MusicOutput,
    ) -> Result<Effect> {
        match dispatch.outcome.request {
            Some(Request::Quit) => Ok(Effect::Quit),
            Some(Request::Load) => Ok(
                match menu_sessions::prepare_load(&self.root, dispatch, &self.assets)? {
                    Some(restored) => Effect::Loaded(Box::new(restored)),
                    None => Effect::None,
                },
            ),
            Some(Request::Save) => {
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_millis()
                    .try_into()?;
                Ok(
                    match menu_sessions::save(
                        &self.root,
                        dispatch,
                        engine,
                        audio,
                        self.assets.identity,
                        timestamp,
                    )? {
                        Some(id) => {
                            self.catalog = menu_sessions::catalog(&self.root, &self.assets)?;
                            Effect::Saved(id)
                        }
                        None => Effect::None,
                    },
                )
            }
            // Restart is already applied by tick_with_configuration.
            _ => Ok(Effect::None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use discworld_core::{
        audio::SampleBank,
        graphics::Resources,
        iso::Iso,
        menu_navigation::{Outcome, Page},
        resource::parse_index,
        save_menu::Target,
        text::Strings,
    };

    #[test]
    #[ignore = "requires DISCWORLD_TEST_ISO, DISCWORLD_TEST_SOUNDFONT and DISCWORLD_TEST_SAVE"]
    fn original_menu_host_opens_saves_and_prepares_replacement() {
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
        let identity = discworld_audio::session::asset_identity(&data);
        let restored = discworld_audio::session::decode(
            &checkpoint,
            identity,
            &resources,
            &bank,
            &tracks,
            &font,
            &strings,
        )
        .unwrap();
        let mut engine = restored.engine;
        let audio = restored.audio;
        engine.close_inventory();
        engine.control.enabled = true;
        let root = std::env::temp_dir().join(format!(
            "discworld-native-menu-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut menu = Menu::new(
            root.clone(),
            Assets {
                identity,
                resources: &resources,
                bank: &bank,
                tracks: &tracks,
                strings: &strings,
                soundfont: &font,
            },
        )
        .unwrap();
        Menu::initialize(&mut engine).unwrap();
        let settings = engine.configuration.as_ref().unwrap().settings();
        assert_eq!(settings.subtitles, engine.subtitles);
        menu.open(&mut engine, Key::Main).unwrap();
        assert_eq!(
            engine.configuration.as_ref().unwrap().page(),
            Some(Page::Main)
        );
        assert!(engine.render(&resources).is_ok());
        engine
            .escape_configuration(&resources, &strings, &menu.catalog.entries)
            .unwrap();
        assert!(!Menu::active(&engine));
        let dispatch = Dispatch {
            outcome: Outcome {
                request: Some(Request::Save),
                ..Outcome::default()
            },
            selection: Some((Target::New, b"native menu".to_vec())),
        };
        let Effect::Saved(id) = menu.dispatch(&dispatch, &engine, &audio).unwrap() else {
            panic!("save effect missing")
        };
        assert_eq!(menu.catalog.entries.len(), 1);
        let dispatch = Dispatch {
            outcome: Outcome {
                request: Some(Request::Load),
                ..Outcome::default()
            },
            selection: Some((Target::Existing(id), b"native menu".to_vec())),
        };
        let before = discworld_audio::session::encode(&engine, &audio, identity).unwrap();
        let Effect::Loaded(restored) = menu.dispatch(&dispatch, &engine, &audio).unwrap() else {
            panic!("load effect missing")
        };
        assert_eq!(
            discworld_audio::session::encode(&restored.engine, &restored.audio, identity).unwrap(),
            before
        );
        assert_eq!(
            discworld_audio::session::encode(&engine, &audio, identity).unwrap(),
            before
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
