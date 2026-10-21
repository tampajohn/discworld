//! Generate native reference outputs for the WASM boundary integration check.
use discworld_web::BrowserGame;
use std::{fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let iso = args.next().ok_or("missing ISO")?;
    let font = args.next().ok_or("missing SoundFont")?;
    let save = args.next().ok_or("missing initial save")?;
    let out = PathBuf::from(args.next().ok_or("missing output directory")?);
    if args.next().is_some() {
        return Err("unexpected argument".into());
    }
    fs::create_dir(&out)?;
    let mut game = BrowserGame::new(fs::read(iso)?, fs::read(font)?)?;
    game.load(&fs::read(save)?)?;
    fs::write(out.join("before.dws"), game.save()?)?;
    let audio = game.advance(24)?;
    fs::write(out.join("after.dws"), game.save()?)?;
    fs::write(out.join("frame.rgba"), game.frame()?)?;
    fs::write(
        out.join("audio.f32le"),
        audio
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect::<Vec<_>>(),
    )?;
    fs::write(out.join("tick.txt"), game.tick().to_string())?;
    println!("{} {}", game.tick(), game.scene());
    Ok(())
}
