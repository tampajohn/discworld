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
    std::fs::write(
        output.join("provenance.json"),
        serde_json::to_vec_pretty(
            &serde_json::json!({"source":"supplied ISO", "font":"startup tag_font", "palette_indices":[228,231], "labels":labels, "scope":"New host labels rendered with original game glyphs and original menu colors; not original button objects."}),
        )?,
    )?;
    Ok(())
}
