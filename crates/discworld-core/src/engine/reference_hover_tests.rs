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
    let direct_entry = std::env::var_os("DISCWORLD_HOVER_DIRECT_ENTRY").is_some();
    let inventory_mode = std::env::var_os("DISCWORLD_HOVER_INVENTORY").is_some();
    let mut start = None;
    let mut rows = Vec::new();
    let mut clocks = Vec::new();
    let mut mover_states = Vec::new();
    engine.scene_calls = Some(Default::default());
    for _ in 0..4000 {
        // The reference requests its debugger scene change before cycle 450.
        // Queue the same scene/entrance; optional direct entry isolates the
        // earlier harness dispatch difference.
        if engine.ticks == 449 {
            engine.end_intro();
            if !direct_entry {
                engine.pending_scene = Some((room, 1, 1, None));
            } else {
                engine.enter_scene(&resources, room, Some(1), 1).unwrap();
            }
        }
        if engine.ticks > 449
            && engine.scene == room
            && engine.control.tags_enabled
            && start.is_none()
        {
            start = Some(engine.ticks);
        }
        if let Some(start) = start {
            let elapsed = engine.ticks - start;
            if elapsed % 12 == 0 && elapsed / 12 < points.len() as u64 {
                let position = points[(elapsed / 12) as usize];
                // Match the probe's immediate original DoCursorMove call.
                engine
                    .cursor_trails
                    .as_mut()
                    .unwrap()
                    .move_to(&resources, position, false)
                    .unwrap();
                engine.move_cursor(position);
                if inventory_mode && elapsed == 0 {
                    engine
                        .interact_actor(&resources, 7, PolygonInteraction::WalkTo)
                        .unwrap();
                }
                if inventory_mode && elapsed == 36 {
                    assert_eq!(engine.active_inventory(), Some(2));
                    let before = engine.clone();
                    assert!(
                        engine
                            .close_inventory_input(&Resources(Vec::new()))
                            .is_err()
                    );
                    assert_eq!(engine, before, "failed cursor restoration is atomic");
                    let mut scripted = engine.clone();
                    scripted.close_inventory();
                    assert_eq!(
                        scripted.cursor_animation, before.cursor_animation,
                        "script CLOSEINV kills the window without restoring the cursor"
                    );
                    engine.close_inventory_input(&resources).unwrap();
                    assert_eq!(
                        engine.cursor_animation.as_ref().unwrap().diagnostic_clock(),
                        (374236, 1, 2, 2),
                        "player close restores and steps the normal cursor"
                    );
                }
            }
        }
        if engine.ticks >= 449 {
            let states: Vec<_> = engine.snapshot_animations().map(|a| {
                let (script, index, period, remaining) = a.diagnostic_clock();
                format!("{{\"script\":{script},\"index\":{index},\"period\":{period},\"remaining\":{remaining},\"frame\":{},\"position\":[{},{}]}}", a.frame.0, a.object.x, a.object.y)
            }).collect();
            clocks.push(format!(
                "{{\"before_tick\":{},\"animations\":[{}]}}",
                engine.ticks + 1,
                states.join(",")
            ));
        }
        let tick = engine.ticks + 1;
        if !direct_entry {
            // Original read-only cycle snapshots and StepAnimScript trace:
            // SWALK advances in its caller's cycle and completes on re-entry.
            if tick == 1114 {
                assert_eq!(engine.movers[&7].placement.position, [244, 178]);
            }
            if tick == 1125 {
                assert_eq!(engine.movers[&7].placement.position, [200, 178]);
                assert_eq!(engine.movers[&7].walk_target, None);
            }
            if tick == 1126 {
                assert_eq!(
                    engine.movers[&7].animation.diagnostic_clock(),
                    (349552, 1, 2, 1)
                );
            }
        }
        if (1108..=1130).contains(&tick) {
            let mover = &engine.movers[&7];
            let target = mover
                .walk_target
                .map_or("null".to_owned(), |p| format!("{p:?}"));
            mover_states.push(format!(
                "{{\"before_tick\":{tick},\"position\":{:?},\"target\":{target},\"hidden\":{},\"special\":{},\"no_path\":{},\"path\":{},\"phase\":{}}}",
                mover.placement.position, mover.hidden, mover.special_reel(), mover.no_path,
                mover.placement.path.map_or("null".to_owned(), |p| p.to_string()), mover.step_phase
            ));
        }
        crate::animation::trace_tick(
            ((450..=455).contains(&tick) || (1108..=1130).contains(&tick)).then_some(tick),
        );
        let hidden_standing = if (452..=455).contains(&tick) {
            let mover = engine.movers.get(&7).unwrap();
            assert!(mover.hidden && mover.walk_target.is_none());
            Some(mover.animation.clone())
        } else {
            None
        };
        let status = engine
            .tick_content(&resources, Some(&bank), Some(&tracks), Some(&strings))
            .unwrap();
        crate::animation::trace_tick(None);
        if let Some(before) = hidden_standing {
            assert_eq!(
                engine.movers[&7].animation, before,
                "original reference does not step the hidden standing mover at tick {tick}"
            );
        }
        assert!(!matches!(status, Status::Unsupported(_) | Status::Quit));
        if start.is_some() {
            let surface = engine.render(&resources).unwrap();
            // Exercise the complete versioned game-save path, not only the
            // trail component's serde representation.
            let mut legacy_engine = engine.clone();
            legacy_engine.cursor_trails = None;
            // A real pre-v26 engine had no mover process placeholders.
            for (id, _) in legacy_engine.mover_tasks() {
                legacy_engine.processes.cancel(id).unwrap();
            }
            let legacy_bytes = crate::save::Snapshot::capture(&legacy_engine, [7; 32])
                .to_bytes()
                .unwrap();
            let legacy = crate::save::Snapshot::from_bytes(&legacy_bytes, [7; 32]).unwrap();
            assert_eq!(legacy.to_bytes().unwrap(), legacy_bytes);
            if rows.is_empty() && !inventory_mode {
                let mut migrated = legacy.state().clone();
                let peers: Vec<_> = migrated
                    .processes
                    .entries()
                    .map(|(id, p)| (id, p.clone(), migrated.processes.sleep_for(id)))
                    .collect();
                let movers = migrated.movers.clone();
                migrated.prepare_user_load().unwrap();
                assert_eq!(migrated.movers, movers, "migration does not reset movement");
                let surviving: Vec<_> = migrated
                    .processes
                    .entries()
                    .filter(|(id, _)| peers.iter().any(|(old, _, _)| old == id))
                    .map(|(id, p)| (id, p.clone(), migrated.processes.sleep_for(id)))
                    .collect();
                assert_eq!(surviving, peers);
                assert_eq!(
                    migrated
                        .mover_tasks()
                        .iter()
                        .map(|(_, actor)| *actor)
                        .collect::<Vec<_>>(),
                    movers.keys().copied().collect::<Vec<_>>()
                );
                assert!(matches!(
                    migrated.processes.entries().next().unwrap().1.scene_task,
                    Some(SceneTask::Mover(7))
                ));
                let once = migrated.clone();
                migrated.prepare_user_load().unwrap();
                assert_eq!(migrated, once);
                let bytes = crate::save::Snapshot::capture(&migrated, [7; 32])
                    .to_bytes()
                    .unwrap();
                assert_eq!(u32::from_le_bytes(bytes[8..12].try_into().unwrap()), 26);
                let mut restored = crate::save::Snapshot::from_bytes(&bytes, [7; 32])
                    .unwrap()
                    .restore_with_strings(&resources, &bank, &tracks, &strings)
                    .unwrap();
                migrated
                    .tick_content(&resources, Some(&bank), Some(&tracks), Some(&strings))
                    .unwrap();
                restored
                    .tick_content(&resources, Some(&bank), Some(&tracks), Some(&strings))
                    .unwrap();
                assert_eq!(migrated, restored);
                let mut full = legacy.state().clone();
                while full.processes.states().count() < 1024 {
                    full.processes
                        .spawn(None, ScriptProcess::new(Handle(0)))
                        .unwrap();
                }
                let before = full.clone();
                assert_eq!(
                    full.prepare_user_load(),
                    Err(Error("process capacity exceeded"))
                );
                assert_eq!(full, before, "failed mover migration is atomic");
            }
            let mut saved_engine = engine.clone();
            let save_bytes = crate::save::Snapshot::capture(&saved_engine, [7; 32])
                .to_bytes()
                .unwrap();
            assert_eq!(
                u32::from_le_bytes(save_bytes[8..12].try_into().unwrap()),
                26
            );
            let decoded = crate::save::Snapshot::from_bytes(&save_bytes, [7; 32]).unwrap();
            assert_eq!(decoded.to_bytes().unwrap(), save_bytes);
            let mut loaded = decoded
                .restore_with_strings(&resources, &bank, &tracks, &strings)
                .unwrap();
            assert_eq!(loaded.cursor_trails, saved_engine.cursor_trails);
            assert_eq!(
                loaded
                    .tick_content(&resources, Some(&bank), Some(&tracks), Some(&strings))
                    .unwrap(),
                saved_engine
                    .tick_content(&resources, Some(&bank), Some(&tracks), Some(&strings))
                    .unwrap()
            );
            assert_eq!(
                crate::save::Snapshot::capture(&loaded, [7; 32])
                    .to_bytes()
                    .unwrap(),
                crate::save::Snapshot::capture(&saved_engine, [7; 32])
                    .to_bytes()
                    .unwrap()
            );
            if rows.is_empty() && !inventory_mode {
                std::fs::write(directory.join("mover-state-v26.dws"), &save_bytes).unwrap();
                let mut input = engine.clone();
                input
                    .cursor_trails
                    .as_mut()
                    .unwrap()
                    .reset_scene(input.control.cursor);
                let old = input.control.cursor;
                input.move_cursor([old[0] + 2, old[1] + 2]);
                input.update_cursor_trails(&resources).unwrap();
                let emitted = input
                    .cursor_trails
                    .as_ref()
                    .unwrap()
                    .animations()
                    .next()
                    .unwrap();
                assert_eq!([emitted.object.x, emitted.object.y], old);
                input.control.frozen = true;
                let frozen_position = input.control.cursor;
                input.move_cursor([250, 180]);
                input.update_cursor_trails(&resources).unwrap();
                assert_eq!(input.control.cursor, frozen_position);
                input.control.hidden = true;
                input.update_cursor_trails(&resources).unwrap();
                assert!(
                    input
                        .cursor_trails
                        .as_ref()
                        .unwrap()
                        .animations()
                        .all(|a| a.frame.0 == 0)
                );
                input.control.hidden = false;
                input.control.frozen = false;
                input.active_inventory = Some(1);
                input.inventory_geometry.open(1);
                input.conversation.hidden = false;
                input.rebuild_inventory(&resources, &strings).unwrap();
                let inside = (0..200)
                    .flat_map(|y| (0..320).map(move |x| [x, y]))
                    .find(|&point| input.inventory_contains_point(point))
                    .unwrap();
                input.move_cursor(inside);
                input.update_cursor_trails(&resources).unwrap();
                assert_eq!(
                    input.cursor_trails.as_ref().unwrap().animations().count(),
                    0
                );
                input.move_cursor([-1, -1]);
                input.update_cursor_trails(&resources).unwrap();
                assert_eq!(
                    input.cursor_trails.as_ref().unwrap().animations().count(),
                    0
                );
                input.move_cursor([-2, -2]);
                input.update_cursor_trails(&resources).unwrap();
                assert_eq!(
                    input.cursor_trails.as_ref().unwrap().animations().count(),
                    1
                );
                input.enter_scene(&resources, room, Some(1), 1).unwrap();
                assert_eq!(
                    input.cursor_trails.as_ref().unwrap().animations().count(),
                    0
                );

                let mut invalid = saved_engine.clone();
                invalid.declarations.cursor = None;
                assert!(
                    crate::save::Snapshot::capture(&invalid, [7; 32])
                        .to_bytes()
                        .is_err()
                );
                let mut trail_engine = saved_engine.clone();
                trail_engine.clear_mover_tasks();
                let trail_bytes = crate::save::Snapshot::capture(&trail_engine, [7; 32])
                    .to_bytes()
                    .unwrap();
                // An extension may not recursively contain itself, and trailing
                // bytes must be rejected even with a valid envelope checksum.
                for nested in [true, false] {
                    let (mut inner, state): (Vec<u8>, crate::cursor_trails::Trails) =
                        postcard::from_bytes(&trail_bytes[80..]).unwrap();
                    if nested {
                        inner[8..12].copy_from_slice(&25u32.to_le_bytes());
                    }
                    let mut payload = postcard::to_allocvec(&(inner, state)).unwrap();
                    if !nested {
                        payload.push(0);
                    }
                    let mut malformed = trail_bytes[..80].to_vec();
                    malformed[44..48].copy_from_slice(&(payload.len() as u32).to_le_bytes());
                    malformed[48..80].copy_from_slice(&Sha256::digest(&payload));
                    malformed.extend(payload);
                    assert!(crate::save::Snapshot::from_bytes(&malformed, [7; 32]).is_err());
                }
                // The outer mover wrapper must reject empty/duplicate actors,
                // absent processes/actors, recursive nesting and trailing bytes.
                for case in 0..6 {
                    let (mut inner, mut tasks): (Vec<u8>, Vec<(crate::scheduler::ProcessId, i32)>) =
                        postcard::from_bytes(&save_bytes[80..]).unwrap();
                    match case {
                        0 => tasks.clear(),
                        1 => tasks.push(tasks[0]),
                        2 => tasks[0].1 = i32::MAX,
                        3 => inner[8..12].copy_from_slice(&26u32.to_le_bytes()),
                        4 => tasks[0].0 = postcard::from_bytes(&[0]).unwrap(),
                        _ => (),
                    }
                    let mut payload = postcard::to_allocvec(&(inner, tasks)).unwrap();
                    if case == 5 {
                        payload.push(0);
                    }
                    let mut malformed = save_bytes[..80].to_vec();
                    malformed[44..48].copy_from_slice(&(payload.len() as u32).to_le_bytes());
                    malformed[48..80].copy_from_slice(&Sha256::digest(&payload));
                    malformed.extend(payload);
                    assert!(
                        crate::save::Snapshot::from_bytes(&malformed, [7; 32]).is_err(),
                        "mover extension case {case}"
                    );
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
    std::fs::write(
        directory.join("mover-states.json"),
        format!("[{}]\n", mover_states.join(",")),
    )
    .unwrap();
    std::fs::write(
        directory.join("animation-events.json"),
        format!("[{}]\n", crate::animation::take_trace().join(",")),
    )
    .unwrap();
    std::fs::write(
        directory.join("entry-mode.json"),
        format!("{{\"direct_entry\":{direct_entry}}}\n"),
    )
    .unwrap();
    std::fs::write(
        directory.join("animation-clocks.json"),
        format!("[{}]\n", clocks.join(",")),
    )
    .unwrap();
    std::fs::write(directory.join("capture.json"), format!("{{\"scope\":\"Integrated runtime output; controlled original scene entry and immediate cursor movement, not ordinary host-input replay\",\"hover_start_tick\":{},\"frames\":[{}]}}\n", start.unwrap(), rows.join(","))).unwrap();
    println!(
        "Captured 72 hover frames beginning at tick {}",
        start.unwrap()
    );
}
