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
        if text("op")? == "open" {
            if self.game.is_some() {
                return Err("Game already open".into());
            }
            let bundle = PathBuf::from(text("bundle")?);
            let save_directory = PathBuf::from(text("saves")?);
            let iso = std::fs::read(bundle.join("discworld.iso")).map_err(|e| e.to_string())?;
            let font = std::fs::read(bundle.join("music.sf2")).map_err(|e| e.to_string())?;
            let game = BrowserGame::new(iso, font)?;
            std::fs::create_dir_all(&save_directory).map_err(|e| e.to_string())?;
            self.save_directory = save_directory;
            self.game = Some(game);
        } else {
            let game = self.game.as_mut().ok_or("Game is not open")?;
            match text("op")? {
                "advance" => self.audio = game.advance(1)?,
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
                    let temporary = self.save_directory.join("quick.dws.tmp");
                    std::fs::write(&temporary, bytes).map_err(|e| e.to_string())?;
                    std::fs::rename(temporary, self.save_directory.join("quick.dws"))
                        .map_err(|e| e.to_string())?;
                }
                "load" => {
                    let bytes = std::fs::read(self.save_directory.join("quick.dws"))
                        .map_err(|e| e.to_string())?;
                    game.load_game(&bytes)?;
                }
                _ => return Err("Unknown native operation".into()),
            }
        }
        let game = self.game.as_mut().ok_or("Game is not open")?;
        let mut requests = Vec::new();
        while let Some(request) = game.take_menu_request()? {
            requests.push(request);
        }
        self.frame = game.frame()?;
        self.response = serde_json::to_vec(&serde_json::json!({
            "tick": game.tick(), "scene": game.scene(), "menu_requests": requests,
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
