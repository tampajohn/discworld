//! Native simulator probe of the shared session API, without a browser runtime.
//! This is a diagnostic executable, not the iPad application's platform host.
use discworld_web::BrowserGame;
use sha2::{Digest, Sha256};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bundle = std::path::PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("missing GameData.bundle")?,
    );
    let mut game = BrowserGame::new(
        std::fs::read(bundle.join("discworld.iso"))?,
        std::fs::read(bundle.join("music.sf2"))?,
    )?;
    let mut samples = 0usize;
    while game.tick() < 8500 {
        if game.tick() == 500 {
            game.input("continue")?;
        }
        let audio = game.advance(1)?;
        assert!(audio.iter().all(|s| s.is_finite()));
        samples += audio.len();
        assert!(game.take_menu_request()?.is_none());
    }
    assert_eq!(game.scene(), "rinceroo.scn");
    let frame = game.frame()?;
    assert_eq!(frame.len(), 320 * 200 * 4);
    let saved = game.save()?;
    let next_audio = game.advance(1)?;
    let next_frame = game.frame()?;
    let next_save = game.save()?;
    game.load(&saved)?;
    assert_eq!(game.frame()?, frame);
    assert_eq!(game.advance(1)?, next_audio);
    assert_eq!(game.frame()?, next_frame);
    assert_eq!(game.save()?, next_save);
    println!(
        "{}",
        serde_json::json!({
            "ticks": game.tick(), "scene": game.scene(), "audio_samples": samples,
            "frame_sha256": format!("{:x}", Sha256::digest(&frame)),
            "save_restore_next_frame_audio_and_state_equal": true,
            "scope": "Native session probe; no UI, audio device, signing or touch acceptance"
        })
    );
    Ok(())
}
