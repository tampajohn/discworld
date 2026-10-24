//! Extract private host-control artwork using the game's original font and menu palette.
use discworld_core::{
    engine::Engine,
    graphics::Resources,
    iso::Iso,
    resource::parse_index,
    text::{Font, Strings},
};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let bytes = std::fs::read(args.next().ok_or("ISO path required")?)?;
    let output = std::path::PathBuf::from(args.next().ok_or("new output directory required")?);
    if args.next().is_some() || output.exists() {
        return Err("choose a new output directory".into());
    }
    let iso = Iso::parse(&bytes)?;
    let index = parse_index(iso.file("/DISCWLD/INDEX").ok_or("missing index")?)?;
    let resources = Resources(
        index
            .iter()
            .map(|entry| {
                iso.file(&format!("/DISCWLD/{}", entry.name))
                    .ok_or("missing resource")
            })
            .collect::<Result<Vec<_>, _>>()?,
    );
    let mut engine =
        Engine::bootstrap_with_title(&resources, discworld_core::intro::title_scene(&index)?)?;
    let strings = Strings::new(iso.file("/DISCWLD/ENGLISH.TXT").ok_or("missing strings")?)?;
    let bank = discworld_core::audio::SampleBank::new(
        iso.file("/DISCWLD/ENGLISH.IDX").ok_or("missing index")?,
        iso.file("/DISCWLD/ENGLISH.SMP").ok_or("missing samples")?,
    )?;
    let tracks =
        discworld_core::music::archive(iso.file("/DISCWLD/MIDI.DAT").ok_or("missing music")?)?;
    for _ in 0..500 {
        if let discworld_core::engine::Status::Unsupported(call) =
            engine.tick_content(&resources, Some(&bank), Some(&tracks), Some(&strings))?
        {
            return Err(format!("startup stopped: {call:?}").into());
        }
    }
    let font = Font::read(
        &resources,
        engine
            .declarations
            .tag_font
            .ok_or("missing original font")?,
    )?;
    let base = engine.render(&resources)?;
    std::fs::create_dir_all(&output)?;
    let labels = [
        "Cancel",
        "Import",
        "Export",
        "Files",
        "Start",
        "Continue",
        "Pockets",
        "Close",
        "Escape",
        "Menu",
        "Save",
        "Load",
        "Sound off",
        "Sound on",
        "Pause",
        "Resume",
        "Walk",
        "Action",
        "Look",
    ];
    for label in labels {
        for (suffix, color) in [("normal", 228), ("selected", 231)] {
            let mut surface = base.clone();
            // Original MainMenu COL_BOX / COL_HILIGHT, original tag-font glyphs.
            surface.draw_palette_rect([0, 0, 80, 18], color)?;
            font.centered(&resources, label.as_bytes(), [40, 4])?
                .draw(&resources, &mut surface)?;
            let pixels: Vec<u8> = surface
                .rgba
                .chunks_exact(320 * 4)
                .take(18)
                .flat_map(|row| row[..80 * 4].iter().copied())
                .collect();
            let file = std::fs::File::create(output.join(format!("{label}-{suffix}.png")))?;
            let mut encoder = png::Encoder::new(file, 80, 18);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.write_header()?.write_image_data(&pixels)?;
        }
    }
    for (operation, lines) in [
        ("save", ["Save current game?", "This replaces your quick save."]),
        ("load", ["Load saved game?", "Unsaved progress will be replaced."]),
    ] {
        let mut surface = base.clone();
        surface.draw_palette_rect([0, 0, 260, 44], 228)?;
        for (row, text) in lines.iter().enumerate() {
            font.centered(&resources, text.as_bytes(), [130, 8 + row as i32 * 16])?.draw(&resources, &mut surface)?;
        }
        let pixels: Vec<_> = surface.rgba.chunks_exact(320 * 4).take(44).flat_map(|row| row[..260 * 4].iter().copied()).collect();
        let file = std::fs::File::create(output.join(format!("confirm-{operation}.png")))?;
        let mut encoder = png::Encoder::new(file, 260, 44);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(&pixels)?;
    }
    // Inventory films are original, compact artwork suitable for touch controls.
    engine.user_event()?;
    for _ in 500..8500 {
        engine.tick_content(&resources, Some(&bank), Some(&tracks), Some(&strings))?;
    }
    let mut atlas = discworld_core::compositor::Surface::new(640, 640)?;
    let mut icon_sources = Vec::new();
    for (ordinal, object) in engine.inventory_objects.values().enumerate() {
        if ordinal >= 256 { break; }
        let film = discworld_core::film::Film::read(&resources, object.film)?;
        let Some(reel) = film.reels.first() else { continue; };
        let mut animation = discworld_core::animation::Animation::new(&resources, reel, 1)?;
        animation.tick(&resources)?;
        animation.object.x = 16;
        animation.object.y = 16;
        let mut icon = discworld_core::compositor::Surface::new(40, 40)?;
        icon.rgba.fill(0);
        icon.draw_animation(&resources, &animation)?;
        let covered: Vec<_> = icon.rgba.chunks_exact(4).enumerate().filter(|(_, p)| p[3] != 0).map(|(i, _)| (i % 40, i / 40)).collect();
        let left = covered.iter().map(|p| p.0).min().unwrap_or(0);
        let top = covered.iter().map(|p| p.1).min().unwrap_or(0);
        let width = covered.iter().map(|p| p.0).max().unwrap_or(0) + 1 - left;
        let height = covered.iter().map(|p| p.1).max().unwrap_or(0) + 1 - top;
        let cropped: Vec<_> = (top..top + height).flat_map(|y| icon.rgba[(y * 40 + left) * 4..(y * 40 + left + width) * 4].iter().copied()).collect();
        let file = std::fs::File::create(output.join(format!("object-{}.png", object.id)))?;
        let mut encoder = png::Encoder::new(file, width as u32, height as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(&cropped)?;
        let x = (ordinal % 16 * 40) as i64;
        let y = (ordinal / 16 * 40) as i64;
        atlas.blit(40, 40, &icon.rgba, [x, y], [false, false])?;
        font.centered(&resources, object.id.to_string().as_bytes(), [x as i32 + 20, y as i32 + 29])?.draw(&resources, &mut atlas)?;
        icon_sources.push(serde_json::json!({"object":object.id,"film":object.film.0,"frame":animation.frame.0}));
    }
    let file = std::fs::File::create(output.join("inventory-atlas.png"))?;
    let mut encoder = png::Encoder::new(file, 640, 640);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&atlas.rgba)?;
    std::fs::write(output.join("icon-sources.json"), serde_json::to_vec_pretty(&icon_sources)?)?;
    std::fs::write(
        output.join("provenance.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"source":"supplied ISO", "font":"startup tag_font", "palette_indices":[228,231], "labels":labels, "scope":"New host labels rendered with original game glyphs and original menu colors; not original button objects."}),
        )?,
    )?;
    Ok(())
}
