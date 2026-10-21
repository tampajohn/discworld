//! Real-asset regression: save/load between a physical click and its first tick.
use discworld_web::BrowserGame;
use std::fs;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err("usage: check_pending_walk ISO SOUNDFONT INVENTORY_SAVE".into());
    }
    let mut game = BrowserGame::new(fs::read(&args[0])?, fs::read(&args[1])?)?;
    game.load(&fs::read(&args[2])?)?;
    game.input("close")?;
    game.move_pointer(300, 180);
    game.input("left-down")?;
    let pending = game.save()?;
    game.load(&pending)?;
    assert_eq!(game.save()?, pending);
    let audio = game.advance(24)?;
    let frame = game.frame()?;
    let continued = game.save()?;
    game.load(&pending)?;
    assert_eq!(game.advance(24)?, audio);
    assert_eq!(game.frame()?, frame);
    assert_eq!(game.save()?, continued);
    println!(
        "Pending ground click reload and 24-tick continuation match at {} {}",
        game.tick(),
        game.scene()
    );
    Ok(())
}
