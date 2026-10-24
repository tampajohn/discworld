//! Native application bridge. All calls must be serialized on the main thread.
//! Returned buffers remain borrowed until the next dw_ipad_call. The caller
//! copies them immediately and never frees or writes through these pointers.
use discworld_web::BrowserGame;
use std::{cell::RefCell, path::PathBuf};

#[derive(Default)]
struct Host {
    request: Vec<u8>,
    response: Vec<u8>,
    frame: Vec<u8>,
    audio: Vec<f32>,
    game: Option<BrowserGame>,
    save_directory: PathBuf,
}

fn refresh_catalog(game: &mut BrowserGame, root: &std::path::Path) -> Result<(), String> {
    game.clear_menu_catalog();
    let catalog = discworld_tools::save_slots::catalog(root).map_err(|e| e.to_string())?;
    for entry in catalog.entries {
        let bytes = discworld_tools::save_slots::read(root, entry.id).map_err(|e| e.to_string())?;
        // Invalid/incompatible records remain reserved on disk, as in the browser host.
        let _ = game.add_menu_slot(&bytes);
    }
    Ok(())
}

impl Host {
    fn dispatch(&mut self) -> Result<(), String> {
        let request: serde_json::Value =
            serde_json::from_slice(&self.request).map_err(|e| e.to_string())?;
        let text = |key| {
            request[key]
                .as_str()
                .ok_or_else(|| format!("missing {key}"))
        };
        self.audio.clear();
        let mut notice = None;
        if text("op")? == "open" {
            if self.game.is_some() {
                return Err("Game already open".into());
            }
            let bundle = PathBuf::from(text("bundle")?);
            let save_directory = PathBuf::from(text("saves")?);
            let iso = std::fs::read(bundle.join("discworld.iso")).map_err(|e| e.to_string())?;
            let font = std::fs::read(bundle.join("music.sf2")).map_err(|e| e.to_string())?;
            let mut game = BrowserGame::new(iso, font)?;
            std::fs::create_dir_all(&save_directory).map_err(|e| e.to_string())?;
            match discworld_tools::settings_file::read(&save_directory.join("settings.json")) {
                Ok(Some(settings)) => game.menu_preferences(
                    &serde_json::to_string(&settings).map_err(|e| e.to_string())?,
                )?,
                Ok(None) => {}
                Err(error) => {
                    eprintln!("Ignoring unreadable iPad preferences: {error}");
                    notice = Some("Settings could not be read; using defaults.");
                }
            }
            refresh_catalog(&mut game, &save_directory.join("slots"))?;
            self.save_directory = save_directory;
            self.game = Some(game);
        } else {
            let game = self.game.as_mut().ok_or("Game is not open")?;
            match text("op")? {
                "advance" => self.audio = game.advance(1)?,
                "menu" => {
                    refresh_catalog(game, &self.save_directory.join("slots"))?;
                    game.open_menu(text("page")?)?;
                }
                "edit-menu" => game.edit_menu(
                    request["text"].as_str().unwrap_or(""),
                    request["backspace"].as_bool().unwrap_or(false),
                )?,
                "pointer" | "overlay-press" => {
                    let x = request["x"].as_i64().ok_or("missing x")?;
                    let y = request["y"].as_i64().ok_or("missing y")?;
                    let x = i32::try_from(x).map_err(|e| e.to_string())?;
                    let y = i32::try_from(y).map_err(|e| e.to_string())?;
                    if text("op")? == "pointer" {
                        game.move_pointer_checked(x, y)?;
                    } else if game.menu_active() || game.native_inventory_contains(x, y) {
                        game.move_pointer_checked(x, y)?;
                        game.input("left-down")?;
                    }
                }
                "input" => {
                    if let (Some(x), Some(y)) = (request["x"].as_i64(), request["y"].as_i64()) {
                        game.move_pointer_checked(
                            i32::try_from(x).map_err(|e| e.to_string())?,
                            i32::try_from(y).map_err(|e| e.to_string())?,
                        )?;
                    }
                    game.input(text("command")?)?;
                }
                "save" => {
                    let bytes = game.save()?;
                    discworld_tools::save_file::write_atomic(
                        &self.save_directory.join("quick.dws"),
                        &bytes,
                    )
                    .map_err(|e| e.to_string())?;
                }
                "export" => {
                    discworld_tools::save_file::write_atomic(
                        std::path::Path::new(text("path")?),
                        &game.save()?,
                    )
                    .map_err(|e| e.to_string())?;
                }
                "import" => {
                    let bytes =
                        discworld_tools::save_file::read(std::path::Path::new(text("path")?))
                            .map_err(|e| e.to_string())?;
                    // Decoder validates the complete engine/DSP session before replacing it.
                    // Import does not overwrite quick saves or named slots.
                    game.load_game(&bytes)?;
                }
                "load" => {
                    let bytes =
                        discworld_tools::save_file::read(&self.save_directory.join("quick.dws"))
                            .map_err(|e| e.to_string())?;
                    game.load_game(&bytes)?;
                }
                _ => return Err("Unknown native operation".into()),
            }
        }
        let game = self.game.as_mut().ok_or("Game is not open")?;
        let mut reset_audio = false;
        let mut quit = false;
        while let Some(raw) = game.take_menu_request()? {
            let request: serde_json::Value =
                serde_json::from_str(&raw).map_err(|e| e.to_string())?;
            if !request["settings"].is_null() {
                let settings = serde_json::from_value(request["settings"].clone())
                    .map_err(|e| e.to_string())?;
                discworld_tools::settings_file::write(
                    &self.save_directory.join("settings.json"),
                    &settings,
                )
                .map_err(|e| e.to_string())?;
            }
            match request["request"].as_str() {
                Some("save") => {
                    let selection = &request["selection"];
                    let target = if selection["id"].is_null() {
                        discworld_core::save_menu::Target::New
                    } else {
                        discworld_core::save_menu::Target::Existing(
                            serde_json::from_value(selection["id"].clone())
                                .map_err(|e| e.to_string())?,
                        )
                    };
                    let description: Vec<u8> =
                        serde_json::from_value(selection["description"].clone())
                            .map_err(|e| e.to_string())?;
                    let timestamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_err(|e| e.to_string())?
                        .as_millis();
                    discworld_tools::save_slots::write(
                        &self.save_directory.join("slots"),
                        target,
                        u64::try_from(timestamp).map_err(|e| e.to_string())?,
                        &description,
                        &game.save()?,
                    )
                    .map_err(|e| e.to_string())?;
                    refresh_catalog(game, &self.save_directory.join("slots"))?;
                    notice = Some("Game saved on this device.");
                }
                Some("load") => {
                    let id = serde_json::from_value(request["selection"]["id"].clone())
                        .map_err(|e| e.to_string())?;
                    let bytes =
                        discworld_tools::save_slots::read(&self.save_directory.join("slots"), id)
                            .map_err(|e| e.to_string())?;
                    game.load_game(
                        discworld_audio::slot::decode(&bytes)
                            .map_err(|e| e.to_string())?
                            .session,
                    )?;
                    reset_audio = true;
                    notice = Some("Saved game loaded.");
                }
                Some("quit") => quit = true,
                Some("restart") => reset_audio = true,
                None => {}
                Some(_) => return Err("Unknown native menu request".into()),
            }
        }
        if reset_audio {
            self.audio.clear();
        }
        self.frame = game.frame()?;
        self.response = serde_json::to_vec(&serde_json::json!({
            "tick": game.tick(), "scene": game.scene(), "notice": notice, "reset_audio": reset_audio, "quit": quit,
            "menu": game.menu_active(), "menu_page": game.menu_page(), "editing": game.menu_editing(),
            "inspection": game.native_inspection()
        }))
        .map_err(|e| e.to_string())?;
        Ok(())
    }
}

thread_local! { static HOST: RefCell<Host> = RefCell::new(Host::default()); }

// Rust 2024 treats symbol export attributes as unsafe. These uniquely prefixed
// exports need that attribute only: no raw pointer is dereferenced in Rust.
#[allow(unsafe_code)]
mod exports {
    use super::*;
    #[unsafe(no_mangle)]
    pub extern "C" fn dw_ipad_request_clear() {
        HOST.with(|h| h.borrow_mut().request.clear());
    }
    #[unsafe(no_mangle)]
    pub extern "C" fn dw_ipad_request_byte(byte: u8) -> i32 {
        HOST.with(|h| {
            let mut h = h.borrow_mut();
            if h.request.len() >= 65536 {
                return -1;
            }
            h.request.push(byte);
            0
        })
    }
    #[unsafe(no_mangle)]
    pub extern "C" fn dw_ipad_call() -> i32 {
        let result = std::panic::catch_unwind(|| HOST.with(|h| h.borrow_mut().dispatch()));
        match result {
            Ok(Ok(())) => 0,
            error => {
                let message = match error {
                    Ok(Err(e)) => e,
                    _ => "Native runtime panic".into(),
                };
                HOST.with(|h| h.borrow_mut().response = message.into_bytes());
                -1
            }
        }
    }
    #[unsafe(no_mangle)]
    pub extern "C" fn dw_ipad_response() -> *const u8 {
        HOST.with(|h| h.borrow().response.as_ptr())
    }
    #[unsafe(no_mangle)]
    pub extern "C" fn dw_ipad_response_len() -> usize {
        HOST.with(|h| h.borrow().response.len())
    }
    #[unsafe(no_mangle)]
    pub extern "C" fn dw_ipad_frame() -> *const u8 {
        HOST.with(|h| h.borrow().frame.as_ptr())
    }
    #[unsafe(no_mangle)]
    pub extern "C" fn dw_ipad_frame_len() -> usize {
        HOST.with(|h| h.borrow().frame.len())
    }
    #[unsafe(no_mangle)]
    pub extern "C" fn dw_ipad_audio() -> *const f32 {
        HOST.with(|h| h.borrow().audio.as_ptr())
    }
    #[unsafe(no_mangle)]
    pub extern "C" fn dw_ipad_audio_len() -> usize {
        HOST.with(|h| h.borrow().audio.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Uses a private temporary directory, never an installed application's saves.
    #[test]
    #[ignore = "requires DISCWORLD_IPAD_BUNDLE with private ISO and SoundFont"]
    fn quick_save_reload_and_invalid_files_preserve_session() {
        let bundle = std::env::var("DISCWORLD_IPAD_BUNDLE").expect("private bundle path");
        let root = std::env::temp_dir().join(format!(
            "discworld-ipad-quick-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let mut host = Host::default();
        fn call(host: &mut Host, value: serde_json::Value) -> Result<(), String> {
            host.request = serde_json::to_vec(&value).unwrap();
            host.dispatch()
        }
        call(
            &mut host,
            serde_json::json!({"op":"open", "bundle":bundle, "saves":root}),
        )
        .unwrap();
        for tick in 0..8500 {
            if tick == 500 {
                host.game.as_mut().unwrap().input("start").unwrap();
            }
            host.game.as_mut().unwrap().advance(1).unwrap();
        }
        call(&mut host, serde_json::json!({"op":"save"})).unwrap();
        let saved = host.game.as_ref().unwrap().save().unwrap();
        let frame = host.frame.clone();
        host.game.as_mut().unwrap().advance(10).unwrap();
        call(&mut host, serde_json::json!({"op":"load"})).unwrap();
        assert!(
            host.game.as_ref().unwrap().save().unwrap() == saved,
            "session bytes changed"
        );
        assert_eq!(host.frame, frame);

        // A newly constructed host must restore both engine and synthesizer state.
        // Keep it as an untouched baseline through the failed I/O attempts below.
        let mut fresh = Host::default();
        call(
            &mut fresh,
            serde_json::json!({"op":"open", "bundle":bundle, "saves":root}),
        )
        .unwrap();
        call(&mut fresh, serde_json::json!({"op":"load"})).unwrap();
        assert!(fresh.game.as_ref().unwrap().save().unwrap() == saved);
        assert_eq!(fresh.frame, frame);
        let transfer = root.with_extension("dws");
        call(
            &mut host,
            serde_json::json!({"op":"export", "path":transfer}),
        )
        .unwrap();
        assert!(
            std::fs::read(&transfer).unwrap() == saved,
            "export changed session bytes"
        );
        assert!(host.game.as_ref().unwrap().save().unwrap() == saved);
        call(&mut fresh, serde_json::json!({"op":"advance"})).unwrap();
        call(&mut fresh, serde_json::json!({"op":"save"})).unwrap();
        let quick_before = std::fs::read(root.join("quick.dws")).unwrap();
        assert!(
            quick_before != saved,
            "quick save must differ from imported snapshot"
        );
        call(
            &mut fresh,
            serde_json::json!({"op":"import", "path":transfer}),
        )
        .unwrap();
        assert!(fresh.game.as_ref().unwrap().save().unwrap() == saved);
        assert_eq!(fresh.frame, frame);
        assert!(
            std::fs::read(root.join("quick.dws")).unwrap() == quick_before,
            "import must not overwrite the device quick save"
        );
        std::fs::write(&transfer, b"invalid external save").unwrap();
        assert!(
            call(
                &mut fresh,
                serde_json::json!({"op":"import", "path":transfer})
            )
            .is_err()
        );
        assert!(fresh.game.as_ref().unwrap().save().unwrap() == saved);
        std::fs::remove_file(&transfer).unwrap();

        let path = root.join("quick.dws");
        std::fs::write(&path, b"truncated").unwrap();
        assert!(call(&mut host, serde_json::json!({"op":"load"})).is_err());
        assert!(
            host.game.as_ref().unwrap().save().unwrap() == saved,
            "session bytes changed"
        );
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(discworld_audio::session::MAX_SESSION_BYTES as u64 + 1)
            .unwrap();
        drop(file);
        assert!(
            call(&mut host, serde_json::json!({"op":"load"}))
                .unwrap_err()
                .contains("size limit")
        );
        assert!(
            host.game.as_ref().unwrap().save().unwrap() == saved,
            "session bytes changed"
        );

        // A failed publication leaves no temporary file and keeps gameplay intact.
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        assert!(call(&mut host, serde_json::json!({"op":"save"})).is_err());
        assert!(
            host.game.as_ref().unwrap().save().unwrap() == saved,
            "session bytes changed"
        );
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        let mut samples = 0;
        let mut nonzero = 0;
        for tick in 0..240 {
            call(&mut host, serde_json::json!({"op":"advance"})).unwrap();
            call(&mut fresh, serde_json::json!({"op":"advance"})).unwrap();
            assert!(
                host.frame == fresh.frame,
                "frame diverged at continuation tick {tick}"
            );
            assert_eq!(host.audio.len(), fresh.audio.len());
            assert!(
                host.audio
                    .iter()
                    .zip(&fresh.audio)
                    .all(|(a, b)| a.to_bits() == b.to_bits()),
                "PCM diverged at continuation tick {tick}"
            );
            assert!(
                host.game.as_ref().unwrap().save().unwrap()
                    == fresh.game.as_ref().unwrap().save().unwrap(),
                "session diverged at continuation tick {tick}"
            );
            samples += host.audio.len();
            nonzero += host.audio.iter().filter(|value| **value != 0.0).count();
        }
        assert!(
            samples > 0 && nonzero > 0,
            "audio comparison must include non-silent output"
        );
        println!(
            "fresh-host continuation: 240 ticks, {samples} PCM samples, {nonzero} nonzero samples; exact frames, PCM bits and session bytes"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "requires DISCWORLD_IPAD_BUNDLE with private ISO and SoundFont"]
    fn original_menu_overwrite_cancel_and_fresh_host_load() {
        fn call(host: &mut Host, request: serde_json::Value) -> serde_json::Value {
            host.request = serde_json::to_vec(&request).unwrap();
            host.dispatch().unwrap();
            serde_json::from_slice(&host.response).unwrap()
        }
        fn open(host: &mut Host, page: &str) {
            for _ in 0..500 {
                let result = call(host, serde_json::json!({"op":"menu", "page":page}));
                if result["menu"] == true {
                    return;
                }
                call(host, serde_json::json!({"op":"advance"}));
            }
            panic!("menu never opened: {page}");
        }
        fn click(host: &mut Host, x: i32, y: i32) {
            call(
                host,
                serde_json::json!({"op":"input", "command":"walk", "x":x,"y":y}),
            );
        }
        fn accept(host: &mut Host, notice: &str) {
            click(host, 262, 69);
            for _ in 0..20 {
                let result = call(host, serde_json::json!({"op":"advance"}));
                if result["notice"] == notice {
                    assert_eq!(result["menu"], false);
                    return;
                }
            }
            panic!("missing menu result: {notice}");
        }
        let bundle = std::env::var("DISCWORLD_IPAD_BUNDLE").expect("private bundle path");
        let root = std::env::temp_dir().join(format!(
            "discworld-ipad-menu-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let mut host = Host::default();
        call(
            &mut host,
            serde_json::json!({"op":"open","bundle":bundle,"saves":root}),
        );
        for tick in 0..8500 {
            if tick == 500 {
                host.game.as_mut().unwrap().input("start").unwrap();
            }
            host.game.as_mut().unwrap().advance(1).unwrap();
        }
        let slots = root.join("slots");
        for name in ["first", "second"] {
            open(&mut host, "save");
            call(&mut host, serde_json::json!({"op":"edit-menu","text":name}));
            accept(&mut host, "Game saved on this device.");
        }
        let catalog = discworld_tools::save_slots::catalog(&slots).unwrap();
        assert_eq!(catalog.entries.len(), 2);
        assert_eq!(catalog.entries[0].description, b"second");
        let second = discworld_tools::save_slots::read(&slots, 1).unwrap();
        let first = discworld_tools::save_slots::read(&slots, 0).unwrap();
        // Save row zero is New; row two selects the older stable ID zero.
        open(&mut host, "save");
        click(&mut host, 90, 78);
        for _ in 0..5 {
            call(
                &mut host,
                serde_json::json!({"op":"edit-menu","backspace":true}),
            );
        }
        call(
            &mut host,
            serde_json::json!({"op":"edit-menu","text":"replacement"}),
        );
        accept(&mut host, "Game saved on this device.");
        let replaced = discworld_tools::save_slots::read(&slots, 0).unwrap();
        assert_ne!(first, replaced);
        let record = discworld_audio::slot::decode(&replaced).unwrap();
        assert_eq!(record.id, 0);
        assert_eq!(record.description, b"replacement");
        assert_eq!(
            discworld_tools::save_slots::read(&slots, 1).unwrap(),
            second
        );
        assert_eq!(std::fs::read_dir(&slots).unwrap().count(), 2);
        // Editing then cancelling must not publish a third slot or mutate an old one.
        open(&mut host, "save");
        call(
            &mut host,
            serde_json::json!({"op":"edit-menu","text":"cancel me"}),
        );
        call(
            &mut host,
            serde_json::json!({"op":"input","command":"escape"}),
        );
        assert_eq!(
            discworld_tools::save_slots::read(&slots, 0).unwrap(),
            replaced
        );
        assert_eq!(
            discworld_tools::save_slots::read(&slots, 1).unwrap(),
            second
        );
        assert_eq!(std::fs::read_dir(&slots).unwrap().count(), 2);

        let mut fresh = Host::default();
        call(
            &mut fresh,
            serde_json::json!({"op":"open","bundle":bundle,"saves":root}),
        );
        // Enter gameplay normally before requesting the original Load page.
        for tick in 0..8500 {
            if tick == 500 {
                fresh.game.as_mut().unwrap().input("start").unwrap();
            }
            fresh.game.as_mut().unwrap().advance(1).unwrap();
        }
        open(&mut fresh, "load");
        accept(&mut fresh, "Saved game loaded.");
        assert!(
            fresh.game.as_ref().unwrap().save().unwrap() == record.session,
            "original-menu load did not restore the overwritten slot exactly"
        );
        println!(
            "original menu: two slots, overwrite ID 0, preserve ID 1, cancel without writes, fresh-host exact load passed"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "requires DISCWORLD_IPAD_BUNDLE with private ISO and SoundFont"]
    fn damaged_preferences_do_not_prevent_startup_or_modify_files() {
        let bundle = std::env::var("DISCWORLD_IPAD_BUNDLE").expect("private bundle path");
        let root = std::env::temp_dir().join(format!(
            "discworld-ipad-preferences-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        let open =
            serde_json::to_vec(&serde_json::json!({"op":"open","bundle":bundle,"saves":root}))
                .unwrap();
        let mut baseline = Host {
            request: open.clone(),
            ..Host::default()
        };
        baseline.dispatch().unwrap();
        let default_state = baseline.game.as_ref().unwrap().save().unwrap();
        let path = root.join("settings.json");
        for bytes in [b"{".to_vec(), vec![b' ';4097], br#"{"volumes":[999,255,255],"double_click":10,"swap_buttons":false,"text_speed":0,"subtitles":true,"flipped":false}"#.to_vec()] {
            std::fs::write(&path, &bytes).unwrap();
            let mut host = Host { request:open.clone(), ..Host::default() };
            host.dispatch().unwrap();
            let response: serde_json::Value = serde_json::from_slice(&host.response).unwrap();
            assert_eq!(response["notice"], "Settings could not be read; using defaults.");
            assert!(host.game.as_ref().unwrap().save().unwrap() == default_state);
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
            assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
        }
        let settings = discworld_core::menu_controller::Settings {
            volumes: [125, 64, 0],
            double_click: 10,
            swap_buttons: false,
            text_speed: 0,
            subtitles: true,
            flipped: false,
        };
        discworld_tools::settings_file::write(&path, &settings).unwrap();
        let valid_bytes = std::fs::read(&path).unwrap();
        let mut host = Host {
            request: open,
            ..Host::default()
        };
        host.dispatch().unwrap();
        let response: serde_json::Value = serde_json::from_slice(&host.response).unwrap();
        assert!(response["notice"].is_null());
        assert_eq!(
            response["inspection"]["settings"],
            serde_json::to_value(settings).unwrap()
        );
        assert_eq!(std::fs::read(&path).unwrap(), valid_bytes);
        println!(
            "preference startup: missing, malformed, oversized, out-of-range, valid cases passed; existing files preserved"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
