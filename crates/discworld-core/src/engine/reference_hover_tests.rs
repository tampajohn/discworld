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
    let inventory_resize = std::env::var_os("DISCWORLD_HOVER_INVENTORY_RESIZE").is_some();
    let resize_corner = std::env::var("DISCWORLD_HOVER_RESIZE_CORNER")
        .unwrap_or_else(|_| "bottom-right".to_owned());
    assert!(matches!(
        resize_corner.as_str(),
        "bottom-right" | "top-left"
    ));
    let resize_delta =
        std::env::var("DISCWORLD_HOVER_RESIZE_DELTA").map_or(26, |s| s.parse::<i32>().unwrap());
    assert!(matches!(resize_delta, -26 | 26));
    assert!(resize_delta == 26 || inventory_resize);
    let resize_top_left = resize_corner == "top-left";
    assert!(!resize_top_left || inventory_resize);
    let mut resources = Resources(
        index
            .iter()
            .map(|entry| iso.file(&format!("/DISCWLD/{}", entry.name)).unwrap())
            .collect(),
    );
    // Diagnostic-only library call: PUSH 1; LIB INVENTORY; HALT.
    // Original resource bytes remain unchanged; this process is never scheduled.
    let mut open_inventory = Vec::from(0x3334000au32.to_le_bytes());
    open_inventory.extend_from_slice(&0u32.to_le_bytes());
    open_inventory.extend_from_slice(&[0x41, 1, 0x4e, 37, 0]);
    let open_inventory_code = Handle(((resources.0.len() as u32) << 23) | 8);
    if inventory_resize {
        resources.0.push(&open_inventory);
    }
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
    let mut points = [
        [50, 120],
        [147, 110],
        [294, 56],
        [250, 160],
        [149, 60],
        [10, 10],
    ];
    let direct_entry = std::env::var_os("DISCWORLD_HOVER_DIRECT_ENTRY").is_some();
    let inventory_fixed_corner =
        std::env::var_os("DISCWORLD_HOVER_INVENTORY_FIXED_CORNER").is_some();
    assert!(!(inventory_fixed_corner && inventory_resize));
    let inventory_drag = inventory_fixed_corner
        || inventory_resize
        || std::env::var_os("DISCWORLD_HOVER_INVENTORY_DRAG").is_some();
    if inventory_drag {
        points = [
            [50, 120],
            [140, 110],
            [170, 130],
            [180, 135],
            [10, 10],
            [10, 10],
        ];
    }
    if inventory_fixed_corner {
        points = [
            [50, 120],
            [169, 147],
            [189, 167],
            [189, 167],
            [10, 10],
            [10, 10],
        ];
    }
    if inventory_resize {
        let corner = if resize_top_left {
            [100, 100]
        } else {
            [194, 172]
        };
        let direction = if resize_top_left { -1 } else { 1 };
        let moved = corner.map(|p| p + direction * resize_delta);
        points = [[50, 120], corner, moved, moved, [10, 10], [10, 10]];
    }
    let inventory_mode = inventory_drag || std::env::var_os("DISCWORLD_HOVER_INVENTORY").is_some();
    let university_retarget = std::env::var_os("DISCWORLD_HOVER_UNIVERSITY_RETARGET").is_some();
    let arch_idle = std::env::var_os("DISCWORLD_HOVER_ARCH_IDLE").is_some();
    let arch_inventory = arch_idle || std::env::var_os("DISCWORLD_HOVER_ARCH_INVENTORY").is_some();
    let arch_skip = arch_inventory || std::env::var_os("DISCWORLD_HOVER_ARCH_SKIP").is_some();
    let arch_entry = arch_skip || std::env::var_os("DISCWORLD_HOVER_ARCH_ENTRY").is_some();
    let university_walk = arch_entry
        || university_retarget
        || std::env::var_os("DISCWORLD_HOVER_UNIVERSITY_WALK").is_some();
    let door_exit = university_walk || std::env::var_os("DISCWORLD_HOVER_DOOR_EXIT").is_some();
    let door_action = door_exit || std::env::var_os("DISCWORLD_HOVER_DOOR_ACTION").is_some();
    let look_mode = std::env::var_os("DISCWORLD_HOVER_LOOK").is_some();
    let walk_delay =
        std::env::var("DISCWORLD_HOVER_WALK_DELAY").map_or(1, |s| s.parse::<u64>().unwrap());
    assert!(walk_delay <= 1);
    let walk_from_actor = std::env::var_os("DISCWORLD_HOVER_WALK_FROM_ACTOR").is_some();
    let walk_mode = std::env::var_os("DISCWORLD_HOVER_WALK").is_some();
    assert!(
        usize::from(inventory_mode)
            + usize::from(look_mode)
            + usize::from(walk_mode)
            + usize::from(door_action)
            <= 1
    );
    if walk_mode {
        points[0] = if walk_from_actor {
            [200, 140]
        } else {
            [250, 180]
        };
    }
    let frame_limit = std::env::var("DISCWORLD_HOVER_FRAMES").map_or(
        if arch_entry {
            480
        } else if door_exit {
            240
        } else {
            72
        },
        |s| s.parse::<usize>().unwrap(),
    );
    assert!((72..=480).contains(&frame_limit));
    let mut audio_states = Vec::new();
    let mut look_pcm = Vec::new();
    let mut look_completion = None;
    let mut start = None;
    let mut rows = Vec::new();
    let mut door_calls = Vec::new();
    let mut door_control_off = false;
    let mut clocks = Vec::new();
    let mut mover_states = Vec::new();
    let mut university_walk_start = None;
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
            if arch_skip && elapsed == 460 {
                assert!(
                    engine.sample_player.id.is_some(),
                    "skip during active speech"
                );
                engine.escape_input(&resources, &strings, &[]).unwrap();
            }
            if arch_entry && elapsed == 240 {
                assert_eq!(index[engine.scene.resource()].name, "unseen.scn");
                assert!(
                    engine.control.enabled,
                    "door exit must restore control before next entry"
                );
                assert!(
                    engine
                        .interact_polygon(&resources, 50, PolygonInteraction::WalkTo)
                        .unwrap()
                );
            }
            if university_retarget && elapsed == 192 {
                assert!(
                    engine.movers[&7].walk_target.is_some(),
                    "retarget an active walk"
                );
                engine
                    .move_cursor_with_configuration(&resources, [110, 180])
                    .unwrap();
            }
            if university_retarget && elapsed == 193 {
                assert!(
                    engine
                        .world_click(&resources, [110, 180], PolygonInteraction::WalkTo)
                        .unwrap()
                );
            }
            if university_walk && elapsed == 168 {
                assert_eq!(index[engine.scene.resource()].name, "unseen.scn");
                university_walk_start = Some(engine.movers[&7].placement.position);
                engine
                    .move_cursor_with_configuration(&resources, [175, 180])
                    .unwrap();
            }
            if university_walk && elapsed == 169 {
                assert!(
                    engine
                        .world_click(&resources, [175, 180], PolygonInteraction::WalkTo)
                        .unwrap()
                );
            }
            if door_exit && elapsed == 72 {
                assert!(
                    engine
                        .interact_polygon(&resources, 6, PolygonInteraction::WalkTo)
                        .unwrap()
                );
            }
            if walk_mode && walk_delay == 1 && elapsed == 1 {
                let click_point = if walk_from_actor {
                    assert_eq!(
                        engine.world_hover.as_ref().unwrap().display.selection(),
                        Some(crate::world_hover::Selection::Actor(7))
                    );
                    engine
                        .move_cursor_with_configuration(&resources, [250, 180])
                        .unwrap();
                    [250, 180]
                } else {
                    points[0]
                };
                assert!(
                    engine
                        .world_click(&resources, click_point, PolygonInteraction::WalkTo)
                        .unwrap()
                );
            }
            if elapsed % 12 == 0 && elapsed / 12 < points.len() as u64 {
                let position = points[(elapsed / 12) as usize];
                // Match the probe's immediate original DoCursorMove call.
                engine
                    .move_cursor_with_configuration(&resources, position)
                    .unwrap();
                if walk_mode && walk_delay == 0 && elapsed == 0 {
                    assert_eq!(
                        engine.world_hover.as_ref().unwrap().display.selection(),
                        Some(crate::world_hover::Selection::Polygon(8))
                    );
                    let bytes = crate::save::Snapshot::capture(&engine, [7; 32])
                        .to_bytes()
                        .unwrap();
                    let mut restored = crate::save::Snapshot::from_bytes(&bytes, [7; 32])
                        .unwrap()
                        .restore_with_strings(&resources, &bank, &tracks, &strings)
                        .unwrap();
                    assert!(
                        restored
                            .world_click(&resources, position, PolygonInteraction::WalkTo)
                            .unwrap()
                    );
                    let mut resolved = engine.clone();
                    assert!(
                        resolved
                            .interact_world(&resources, position, PolygonInteraction::WalkTo)
                            .unwrap()
                    );
                    assert!(resolved.processes.states().any(|p| p.player_walk.is_some()));
                    assert!(
                        engine
                            .world_click(&resources, position, PolygonInteraction::WalkTo)
                            .unwrap()
                    );
                    assert_eq!(
                        engine, restored,
                        "cached click selection survives complete save/load"
                    );
                    assert!(engine.processes.states().any(|p| p.polygon == Some(8)));
                    assert!(!engine.processes.states().any(|p| p.player_walk.is_some()));
                }
                if inventory_mode && elapsed == 0 {
                    if inventory_resize {
                        let mut process = ScriptProcess::new(open_inventory_code);
                        assert_eq!(
                            engine
                                .run_process(
                                    &resources,
                                    Some(&bank),
                                    Some(&tracks),
                                    Some(&strings),
                                    &mut process
                                )
                                .unwrap(),
                            Status::Halted
                        );
                        assert_eq!(
                            engine.declarations.windows[1].as_ref().unwrap().size,
                            [3, 2]
                        );
                    } else {
                        engine
                            .interact_actor(&resources, 7, PolygonInteraction::WalkTo)
                            .unwrap();
                    }
                }
                if door_action && elapsed == 0 {
                    assert!(
                        engine
                            .interact_polygon(&resources, 6, PolygonInteraction::Action)
                            .unwrap()
                    );
                }
                if look_mode && elapsed == 0 {
                    assert!(
                        engine
                            .interact_actor(&resources, 7, PolygonInteraction::Look)
                            .unwrap()
                    );
                }
                if inventory_drag && elapsed == 12 {
                    if inventory_resize {
                        let parts =
                            Film::read(&resources, engine.declarations.window_parts.unwrap())
                                .unwrap();
                        assert!(
                            engine
                                .cursor_animation
                                .as_ref()
                                .unwrap()
                                .uses_script(parts.reels[27].script),
                            "movement selects resize cursor before the button edge"
                        );
                    }
                    if inventory_fixed_corner || inventory_resize {
                        assert_eq!(
                            engine
                                .inventory_display
                                .as_ref()
                                .unwrap()
                                .0
                                .layout
                                .inventory_area_at(position),
                            if resize_top_left {
                                crate::conversation_window::Area::TopLeft
                            } else {
                                crate::conversation_window::Area::BottomRight
                            }
                        );
                    }
                    engine
                        .mouse_button(&resources, crate::mouse::Button::Left, true)
                        .unwrap();
                    if inventory_fixed_corner {
                        let window = engine.declarations.windows[2].as_ref().unwrap();
                        assert_eq!(window.min, window.max, "pockets have fixed dimensions");
                        assert!(!engine.inventory_dragging());
                        assert!(engine.inventory_resize.is_none());
                    } else {
                        assert!(engine.inventory_dragging());
                        if inventory_resize {
                            assert!(engine.inventory_resize.is_some());
                        }
                    }
                }
                if inventory_drag && elapsed == 36 {
                    engine
                        .mouse_button(&resources, crate::mouse::Button::Left, false)
                        .unwrap();
                    assert!(!engine.inventory_dragging());
                    assert!(engine.inventory_resize.is_none());
                    if inventory_resize {
                        assert_eq!(
                            engine.declarations.windows[1].as_ref().unwrap().size,
                            if resize_delta > 0 { [4, 3] } else { [2, 1] }
                        );
                        assert_eq!(
                            engine.declarations.windows[1].as_ref().unwrap().position,
                            if resize_top_left {
                                [100 - resize_delta, 100 - resize_delta]
                            } else {
                                [100, 100]
                            }
                        );
                    }
                }
                if inventory_mode && elapsed == (if inventory_drag { 48 } else { 36 }) {
                    assert_eq!(
                        engine.active_inventory(),
                        Some(if inventory_resize { 1 } else { 2 })
                    );
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
        if (1108..=1130).contains(&tick) || (university_walk && (1424..=1496).contains(&tick)) {
            let mover = &engine.movers[&7];
            let target = mover
                .walk_target
                .map_or("null".to_owned(), |p| format!("{p:?}"));
            let leg = mover
                .node_walk
                .as_ref()
                .map(|w| w.leg)
                .or(mover.direct_leg)
                .map_or("null".to_owned(), |leg| format!("{:?}", leg.target));
            let mover_order = engine
                .processes
                .states()
                .position(|p| p.scene_task == Some(SceneTask::Mover(7)));
            let walk_order = engine
                .processes
                .states()
                .position(|p| p.player_walk.is_some());
            mover_states.push(format!(
                "{{\"before_tick\":{tick},\"position\":{:?},\"target\":{target},\"hidden\":{},\"special\":{},\"no_path\":{},\"path\":{},\"phase\":{},\"leg\":{leg},\"mover_order\":{},\"walk_order\":{}}}",
                mover.placement.position, mover.hidden, mover.special_reel(), mover.no_path,
                mover.placement.path.map_or("null".to_owned(), |p| p.to_string()), mover.step_phase,
                mover_order.map_or("null".to_owned(), |i| i.to_string()), walk_order.map_or("null".to_owned(), |i| i.to_string())
            ));
        }
        crate::animation::trace_tick(
            ((450..=455).contains(&tick)
                || (1108..=1130).contains(&tick)
                || (university_walk && (1424..=1496).contains(&tick)))
            .then_some(tick),
        );
        let hidden_standing = if (452..=455).contains(&tick) {
            let mover = engine.movers.get(&7).unwrap();
            assert!(mover.hidden && mover.walk_target.is_none());
            Some(mover.animation.clone())
        } else {
            None
        };
        let collecting_look = look_mode && engine.sample_player.id == Some(3636);
        let look_position = engine.sample_player.position;
        let status = engine
            .tick_content(&resources, Some(&bank), Some(&tracks), Some(&strings))
            .unwrap();
        crate::animation::trace_tick(None);
        if door_action && start.is_some() && engine.scene == room {
            door_control_off |= !engine.control.enabled;
            if engine.control.hidden {
                assert_eq!(
                    engine.cursor_trails.as_ref().unwrap().animations().count(),
                    0,
                    "scripted cursor hiding deletes trails"
                );
            }
            let script = engine.polygons.active[6].definition.script;
            door_calls.extend(
                engine
                    .scene_calls
                    .as_ref()
                    .unwrap()
                    .iter()
                    .filter(|call| call.tick == engine.ticks && call.script == script)
                    .map(|call| call.call.id),
            );
        }
        if walk_from_actor && start.is_some_and(|start| tick == start + 14) {
            // Original cycle1271 still steps all three trails after the window
            // appeared under a stationary cursor; opening is not movement.
            assert_eq!(engine.active_inventory(), Some(2));
            assert_eq!(
                engine.cursor_trails.as_ref().unwrap().animations().count(),
                3
            );
        }
        if collecting_look {
            let source = bank.sample(3636).unwrap().unwrap();
            let count = engine.audio_frame.len().min(source.len() - look_position);
            look_pcm.extend_from_slice(&engine.audio_frame[..count]);
            if look_position + count == source.len() {
                assert!(
                    engine.audio_frame[count..]
                        .iter()
                        .all(|&sample| sample == 0)
                );
                assert!(engine.sample_player.id.is_none());
                assert!(engine.control.enabled);
                assert_eq!(engine.control.cursor, points[0]);
                assert!(engine.processes.states().all(|p| p.print_wait.is_none()));
                look_completion = Some(engine.ticks);
            } else {
                assert!(
                    !engine.control.enabled,
                    "speech must retain control until PCM ends"
                );
            }
        }

        if let Some(before) = hidden_standing {
            assert_eq!(
                engine.movers[&7].animation, before,
                "original reference does not step the hidden standing mover at tick {tick}"
            );
        }
        assert!(!matches!(status, Status::Unsupported(_) | Status::Quit));
        if start.is_some() {
            let surface = engine.render(&resources).unwrap();
            let sample = &engine.sample_player;
            let id = sample.id.map_or("null".to_owned(), |id| id.to_string());
            let pcm = sample.id.and_then(|id| bank.sample(id).unwrap());
            let hash = pcm.map_or("null".to_owned(), |data| {
                format!("\"{:x}\"", Sha256::digest(data))
            });
            audio_states.push(format!("{{\"tick\":{},\"id\":{id},\"serial\":{},\"position\":{},\"bytes\":{},\"sha256\":{hash}}}",
                engine.ticks, sample.serial, sample.position, pcm.map_or(0, |data| data.len())));
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
            if rows.is_empty() && !inventory_mode && !look_mode && !walk_mode && !door_action {
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
            if rows.is_empty() && !inventory_mode && !look_mode && !walk_mode && !door_action {
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
                .and_then(|hover| hover.tags.candidate(&engine.polygons).unwrap())
                .map_or("null".to_owned(), |c| c.polygon.to_string());
            rows.push(format!("{{\"tick\":{},\"scene\":{},\"rgb_sha256\":\"{:x}\",\"polygon_candidate\":{},\"cursor\":{:?}}}", engine.ticks, engine.scene.0, Sha256::digest(&rgb), polygon, engine.control.cursor));
            if rows.len() == frame_limit {
                break;
            }
        }
    }
    assert_eq!(rows.len(), frame_limit, "room did not reach active tags");
    if arch_inventory {
        // Escape skips one line, not the entire scripted introduction. Wait
        // for the original script to return control before opening pockets.
        let mut timeline = Vec::new();
        let mut previous = None;
        let mut control_tick = None;
        while engine.ticks < if arch_idle { 12000 } else { 3999 } {
            let status = engine
                .tick_content(&resources, Some(&bank), Some(&tracks), Some(&strings))
                .unwrap();
            assert!(!matches!(status, Status::Unsupported(_) | Status::Quit));
            if engine.control.enabled && control_tick.is_none() {
                control_tick = Some(engine.ticks);
            }
            let state = (
                engine.scene,
                engine.control.enabled,
                engine.sample_player.id,
                engine.sample_player.serial,
            );
            if previous != Some(state) {
                let id = engine
                    .sample_player
                    .id
                    .map_or("null".to_owned(), |id| id.to_string());
                let hash = engine
                    .sample_player
                    .id
                    .and_then(|id| bank.sample(id).unwrap())
                    .map_or("null".to_owned(), |pcm| {
                        format!("\"{:x}\"", Sha256::digest(pcm))
                    });
                timeline.push(format!("{{\"tick\":{},\"scene\":\"{}\",\"control\":{},\"sample_id\":{id},\"sample_serial\":{},\"sha256\":{hash}}}", engine.ticks, index[engine.scene.resource()].name, engine.control.enabled, engine.sample_player.serial));
                previous = Some(state);
            }
        }
        std::fs::write(
            directory.join("arch-dialogue-timeline.json"),
            format!("[{}]\n", timeline.join(",")),
        )
        .unwrap();
        if arch_idle {
            assert!(
                timeline.iter().any(|row| row.contains("closeup.scn")),
                "idle cutscene must run"
            );
            return;
        }
        let control_tick = control_tick.expect("entry dialogue did not finish");
        assert!(engine.control.enabled);
        assert!(
            engine
                .interact_actor(&resources, 7, PolygonInteraction::WalkTo)
                .unwrap()
        );
        for _ in 0..24 {
            engine
                .tick_content(&resources, Some(&bank), Some(&tracks), Some(&strings))
                .unwrap();
            let surface = engine.render(&resources).unwrap();
            let rgb: Vec<_> = surface
                .rgba
                .chunks_exact(4)
                .flat_map(|pixel| pixel[..3].iter().copied())
                .collect();
            std::fs::write(
                directory.join(format!("tick-{:06}.rgb", engine.ticks)),
                &rgb,
            )
            .unwrap();
            rows.push(format!(
                "{{\"tick\":{},\"scene\":{},\"rgb_sha256\":\"{:x}\"}}",
                engine.ticks,
                engine.scene.0,
                Sha256::digest(&rgb)
            ));
        }
        assert_eq!(engine.active_inventory(), Some(2));
        let overlay = engine.render(&resources).unwrap();
        let rgb: Vec<_> = overlay
            .rgba
            .chunks_exact(4)
            .flat_map(|pixel| pixel[..3].iter().copied())
            .collect();
        std::fs::write(directory.join("arch-inventory.rgb"), &rgb).unwrap();
        let bytes = crate::save::Snapshot::capture(&engine, [7; 32])
            .to_bytes()
            .unwrap();
        let mut restored = crate::save::Snapshot::from_bytes(&bytes, [7; 32])
            .unwrap()
            .restore_with_strings(&resources, &bank, &tracks, &strings)
            .unwrap();
        assert_eq!(
            restored.render(&resources).unwrap(),
            engine.render(&resources).unwrap()
        );
        engine
            .tick_content(&resources, Some(&bank), Some(&tracks), Some(&strings))
            .unwrap();
        restored
            .tick_content(&resources, Some(&bank), Some(&tracks), Some(&strings))
            .unwrap();
        assert_eq!(engine, restored);
        std::fs::write(directory.join("arch-inventory.json"), format!("{{\"control_return_tick\":{control_tick},\"inventory_tick\":{},\"inventory\":2,\"save_continuation\":true,\"reference_compared\":false}}\n", engine.ticks)).unwrap();
    }
    if door_exit {
        assert_eq!(
            index[engine.scene.resource()].name,
            if arch_entry { "arch.scn" } else { "unseen.scn" }
        );
    }
    if university_walk {
        assert_ne!(
            Some(engine.movers[&7].placement.position),
            university_walk_start,
            "university floor click must move Rincewind"
        );
    }
    if door_action {
        assert!(!door_calls.is_empty(), "original door script must execute");
        assert!(
            door_control_off && (arch_entry || engine.control.enabled),
            "door action must take and restore control"
        );
        std::fs::write(
            directory.join("door-library-calls.json"),
            format!("{door_calls:?}\n"),
        )
        .unwrap();
    }
    if walk_from_actor {
        assert_eq!(engine.active_inventory(), Some(2));
    }
    if look_mode && frame_limit >= 144 {
        let source = bank.sample(3636).unwrap().unwrap();
        let expected: Vec<i16> = source.iter().map(|&s| (i16::from(s) - 128) * 256).collect();
        assert_eq!(
            look_pcm, expected,
            "complete emitted speech matches original unsigned PCM"
        );
        let completed = look_completion.expect("LOOK speech did not finish");
        let bytes: Vec<u8> = look_pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
        std::fs::write(directory.join("speech-completion.json"), format!(
            "{{\"sample_id\":3636,\"source_samples\":{},\"emitted_samples\":{},\"pcm_s16le_sha256\":\"{:x}\",\"completion_tick\":{completed},\"restored_cursor\":[50,120],\"control_restored\":true,\"final_block_tail_silent\":true}}\n",
            source.len(), look_pcm.len(), Sha256::digest(&bytes))).unwrap();
    }

    std::fs::write(
        directory.join("audio-states.json"),
        format!("[{}]\n", audio_states.join(",")),
    )
    .unwrap();
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
        "Captured {} hover frames beginning at tick {}",
        rows.len(),
        start.unwrap()
    );
}
