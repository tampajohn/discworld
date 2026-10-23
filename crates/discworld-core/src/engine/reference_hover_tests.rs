//! Opt-in original-asset diagnostics for the controlled reference hover probe.
use super::*;
use sha2::{Digest, Sha256};

#[test]
#[ignore = "requires DISCWORLD_TEST_ISO and new DISCWORLD_HOVER_CAPTURE directory"]
fn capture_original_debug_room_hover() {
    let directory = std::path::PathBuf::from(std::env::var_os("DISCWORLD_HOVER_CAPTURE").unwrap());
    std::fs::create_dir(&directory).unwrap();
    let data = std::fs::read(std::env::var_os("DISCWORLD_TEST_ISO").unwrap()).unwrap();
    let iso = crate::iso::Iso::parse(&data).unwrap();
    let index = crate::resource::parse_index(iso.file("/DISCWLD/INDEX").unwrap()).unwrap();
    assert_eq!(index[18].name, "rinceroo.scn");
    let resources = Resources(
        index
            .iter()
            .map(|entry| iso.file(&format!("/DISCWLD/{}", entry.name)).unwrap())
            .collect(),
    );
    let bank = SampleBank::new(
        iso.file("/DISCWLD/ENGLISH.IDX").unwrap(),
        iso.file("/DISCWLD/ENGLISH.SMP").unwrap(),
    )
    .unwrap();
    let tracks = crate::music::archive(iso.file("/DISCWLD/MIDI.DAT").unwrap()).unwrap();
    let strings = crate::text::Strings::new(iso.file("/DISCWLD/ENGLISH.TXT").unwrap()).unwrap();
    let mut engine =
        Engine::bootstrap_with_title(&resources, crate::intro::title_scene(&index).unwrap())
            .unwrap();
    let room = Handle(18 << 23);
    let points = [
        [50, 120],
        [147, 110],
        [294, 56],
        [250, 160],
        [149, 60],
        [10, 10],
    ];
    let mut start = None;
    let mut rows = Vec::new();
    let mut previous_cursor = None;
    let mut trails = None;
    let trail_directory = directory.join("with-trails");
    std::fs::create_dir(&trail_directory).unwrap();
    let mut trail_rows = Vec::new();
    engine.scene_calls = Some(Default::default());
    for _ in 0..4000 {
        // The reference requests its debugger scene change before cycle 450.
        // Here the same original scene/entrance is dispatched before tick 450.
        if engine.ticks == 449 {
            engine.end_intro();
            engine.enter_scene(&resources, room, Some(1), 1).unwrap();
        }
        if engine.ticks > 449
            && engine.scene == room
            && engine.control.tags_enabled
            && start.is_none()
        {
            start = Some(engine.ticks);
            previous_cursor = Some(engine.control.cursor);
            trails = Some(
                crate::cursor_trails::Trails::new(
                    &resources,
                    engine.declarations.cursor.as_ref().unwrap().0,
                    engine.control.cursor,
                )
                .unwrap(),
            );
        }
        if let Some(start) = start {
            let elapsed = engine.ticks - start;
            if elapsed % 12 == 0 && elapsed / 12 < points.len() as u64 {
                let position = points[(elapsed / 12) as usize];
                trails
                    .as_mut()
                    .unwrap()
                    .move_to(&resources, position, false)
                    .unwrap();
                engine.move_cursor(position);
            }
        }
        let status = engine
            .tick_content(&resources, Some(&bank), Some(&tracks), Some(&strings))
            .unwrap();
        assert!(!matches!(status, Status::Unsupported(_) | Status::Quit));
        if start.is_some() {
            let surface = engine.render(&resources).unwrap();
            let trails = trails.as_mut().unwrap();
            let bytes = postcard::to_allocvec(trails).unwrap();
            let mut restored: crate::cursor_trails::Trails = postcard::from_bytes(&bytes).unwrap();
            restored.validate(&resources).unwrap();
            trails.tick(&resources).unwrap();
            restored.tick(&resources).unwrap();
            assert_eq!(*trails, restored);
            let mut candidate = surface.clone();
            for animation in trails.animations() {
                candidate.draw_animation(&resources, animation).unwrap();
            }
            let trail_rgb: Vec<_> = candidate
                .rgba
                .chunks_exact(4)
                .flat_map(|p| p[..3].iter().copied())
                .collect();
            std::fs::write(
                trail_directory.join(format!("tick-{:06}.rgb", engine.ticks)),
                &trail_rgb,
            )
            .unwrap();
            trail_rows.push(format!(
                "{{\"tick\":{},\"scene\":{},\"rgb_sha256\":\"{:x}\"}}",
                engine.ticks,
                engine.scene.0,
                Sha256::digest(&trail_rgb)
            ));
            if rows.is_empty() {
                let cursor_film = engine.declarations.cursor.as_ref().unwrap().0;
                let film = Film::read(&resources, cursor_film).unwrap();
                println!(
                    "Cursor film {}: {} reels, previous position {:?}",
                    cursor_film.0,
                    film.reels.len(),
                    previous_cursor.unwrap()
                );
                // Diagnostic only: test whether original trail reel pixels account
                // for the first-frame difference. This does not change runtime state.
                for (column, reel) in film.reels.iter().enumerate().skip(1) {
                    let mut trail =
                        Animation::new(&resources, reel, 24 / film.rate as u32).unwrap();
                    [trail.object.x, trail.object.y] = previous_cursor.unwrap();
                    for step in 1..=2 {
                        trail.tick(&resources).unwrap();
                        let mut candidate = surface.clone();
                        candidate.draw_animation(&resources, &trail).unwrap();
                        let rgb: Vec<_> = candidate
                            .rgba
                            .chunks_exact(4)
                            .flat_map(|p| p[..3].iter().copied())
                            .collect();
                        std::fs::write(
                            directory.join(format!("trail-reel-{column}-step-{step}.rgb")),
                            &rgb,
                        )
                        .unwrap();
                        println!(
                            "Trail reel {column} step {step}: {:x}",
                            Sha256::digest(&rgb)
                        );
                    }
                }
            }
            let rgb: Vec<_> = surface
                .rgba
                .chunks_exact(4)
                .flat_map(|p| p[..3].iter().copied())
                .collect();
            std::fs::write(
                directory.join(format!("tick-{:06}.rgb", engine.ticks)),
                &rgb,
            )
            .unwrap();
            let polygon = engine
                .world_hover
                .as_ref()
                .unwrap()
                .tags
                .candidate(&engine.polygons)
                .unwrap()
                .map_or("null".to_owned(), |c| c.polygon.to_string());
            rows.push(format!("{{\"tick\":{},\"scene\":{},\"rgb_sha256\":\"{:x}\",\"polygon_candidate\":{},\"cursor\":{:?}}}", engine.ticks, engine.scene.0, Sha256::digest(&rgb), polygon, engine.control.cursor));
            if rows.len() == 72 {
                break;
            }
        }
    }
    assert_eq!(rows.len(), 72, "room did not reach active tags");
    std::fs::write(trail_directory.join("capture.json"), format!("{{\"scope\":\"Diagnostic trail composition over runtime frames; not integrated engine output\",\"frames\":[{}]}}\n", trail_rows.join(","))).unwrap();
    std::fs::write(directory.join("capture.json"), format!("{{\"scope\":\"Controlled original scene entry and cursor movement; not ordinary input replay\",\"hover_start_tick\":{},\"frames\":[{}]}}\n", start.unwrap(), rows.join(","))).unwrap();
    println!(
        "Captured 72 hover frames beginning at tick {}",
        start.unwrap()
    );
}
