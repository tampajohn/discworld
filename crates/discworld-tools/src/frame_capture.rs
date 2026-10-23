//! Unscaled startup framebuffer capture for independent reference comparison.
use discworld_core::{
    audio::SampleBank,
    engine::{Engine, Status},
    graphics::Resources,
    iso::Iso,
    resource::parse_index,
    text::Strings,
};
use serde_json::json;
use sha2::{Digest, Sha256};

pub fn startup(
    iso_path: std::ffi::OsString,
    mut args: impl Iterator<Item = std::ffi::OsString>,
) -> Result<(), Box<dyn std::error::Error>> {
    let ticks: u32 = args
        .next()
        .ok_or("missing tick count")?
        .into_string()
        .map_err(|_| "invalid tick count")?
        .parse()?;
    let directory = std::path::PathBuf::from(args.next().ok_or("missing new capture directory")?);
    if args.next().is_some() || !(1..=10_000).contains(&ticks) {
        return Err("usage: capture-startup-frames ISO TICKS(1..10000) NEW_DIRECTORY".into());
    }
    std::fs::create_dir(&directory)?;
    let data = std::fs::read(iso_path)?;
    let iso = Iso::parse(&data)?;
    let index = parse_index(iso.file("/DISCWLD/INDEX").ok_or("missing resource index")?)?;
    let resources = Resources(
        index
            .iter()
            .map(|entry| {
                iso.file(&format!("/DISCWLD/{}", entry.name))
                    .ok_or("missing resource")
            })
            .collect::<Result<Vec<_>, _>>()?,
    );
    let bank = SampleBank::new(
        iso.file("/DISCWLD/ENGLISH.IDX")
            .ok_or("missing sample index")?,
        iso.file("/DISCWLD/ENGLISH.SMP")
            .ok_or("missing sample bank")?,
    )?;
    let tracks =
        discworld_core::music::archive(iso.file("/DISCWLD/MIDI.DAT").ok_or("missing music")?)?;
    let strings = Strings::new(iso.file("/DISCWLD/ENGLISH.TXT").ok_or("missing strings")?)?;
    let mut engine =
        Engine::bootstrap_with_title(&resources, discworld_core::intro::title_scene(&index)?)?;
    engine.scene_calls = Some(std::collections::VecDeque::new());
    let mut frames = Vec::new();
    for _ in 0..ticks {
        let status = engine.tick_content(&resources, Some(&bank), Some(&tracks), Some(&strings))?;
        if matches!(status, Status::Unsupported(_)) {
            return Err(format!("startup stopped at {}: {status:?}", engine.ticks).into());
        }
        let surface = engine.render(&resources)?;
        if surface.rgba.len() != 320 * 200 * 4 || surface.rgba.chunks_exact(4).any(|p| p[3] != 255)
        {
            return Err("unexpected framebuffer size or transparency".into());
        }
        let rgb: Vec<u8> = surface
            .rgba
            .chunks_exact(4)
            .flat_map(|p| p[..3].iter().copied())
            .collect();
        std::fs::write(
            directory.join(format!("tick-{:06}.rgb", engine.ticks)),
            &rgb,
        )?;
        frames.push(json!({"tick":engine.ticks,"scene":engine.scene.0,
            "scene_name":index[engine.scene.resource()].name,"rgb_sha256":format!("{:x}",Sha256::digest(&rgb)), "films": engine.films,
            "world_origin": engine.world_origin}));
    }
    let report = json!({"iso_sha256":format!("{:x}",Sha256::digest(&data)),"ticks":ticks,"frames":frames,"scene_calls":engine.scene_calls,
        "scope":"Unscaled 320x200 RGB after each startup engine tick, no user input. Original PCM/MIDI state advances; no SoundFont synthesis or device audio."});
    std::fs::write(
        directory.join("capture.json"),
        serde_json::to_string_pretty(&report)? + "\n",
    )?;
    println!("{}", json!({"directory":directory,"ticks":ticks}));
    Ok(())
}
