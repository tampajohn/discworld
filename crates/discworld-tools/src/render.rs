use discworld_core::{
    animation::Animation,
    compositor::Surface,
    film::Film,
    graphics::{Handle, Resources},
    iso::Iso,
    resource::parse_index,
};

#[cfg(test)]
use discworld_tools::replay::InteractionEvent;
use discworld_tools::replay::interactions;

pub fn headless(
    iso_path: std::ffi::OsString,
    mut args: impl Iterator<Item = std::ffi::OsString>,
) -> Result<(), Box<dyn std::error::Error>> {
    let ticks: u32 = args
        .next()
        .ok_or("missing tick count")?
        .into_string()
        .map_err(|_| "invalid ticks")?
        .parse()?;
    let output = args.next().ok_or("missing output path")?;
    let wave_output = args.next().filter(|path| path != "-");
    // Preserve create_new at write time too; this early check avoids replaying
    // hundreds of thousands of ticks for a destination that already exists.
    for path in std::iter::once(&output).chain(wave_output.iter()) {
        if std::path::Path::new(path).exists() {
            return Err(format!(
                "output already exists: {}",
                std::path::Path::new(path).display()
            )
            .into());
        }
    }
    let soundfont = args.next();
    let press_tick: Option<u32> = args
        .next()
        .map(|s| {
            s.into_string()
                .map_err(|_| "invalid input tick")?
                .parse::<u32>()
                .map_err(|_| "invalid input tick")
        })
        .transpose()?;
    let interaction_trace = args.next();
    let mut snapshot_audit = false;
    let mut session_load = None;
    let mut session_save = None;
    while let Some(flag) = args.next() {
        if flag == "--snapshot-audit" {
            snapshot_audit = true;
        } else if flag == "--load" {
            session_load = Some(args.next().ok_or("missing load file")?);
        } else if flag == "--save" {
            session_save = Some(args.next().ok_or("missing save file")?);
        } else {
            return Err("unexpected headless argument".into());
        }
    }
    let interactions = interaction_trace
        .map(|path| interactions(&std::fs::read(path)?, ticks))
        .transpose()?
        .unwrap_or_default();
    let mut interaction_log = Vec::new();
    let mut text_calls = std::collections::VecDeque::new();
    let data = std::fs::read(iso_path)?;
    let asset_id: [u8; 32] = {
        use sha2::Digest;
        sha2::Sha256::digest(&data).into()
    };
    let mut snapshot_checks = 0u64;
    let mut snapshot_max_bytes = 0usize;
    let mut audio_snapshot_checks = 0u64;
    let mut audio_snapshot_max_bytes = 0usize;
    let iso = Iso::parse(&data)?;
    let index = parse_index(iso.file("/DISCWLD/INDEX").ok_or("missing index")?)?;
    discworld_tools::replay::validate_scenes(&interactions, &index)?;
    let resources = Resources(
        index
            .iter()
            .map(|e| {
                iso.file(&format!("/DISCWLD/{}", e.name))
                    .ok_or("missing resource")
            })
            .collect::<Result<Vec<_>, _>>()?,
    );
    let mut engine = discworld_core::engine::Engine::bootstrap(&resources)?;
    let bank = discworld_core::audio::SampleBank::new(
        iso.file("/DISCWLD/ENGLISH.IDX")
            .ok_or("missing audio index")?,
        iso.file("/DISCWLD/ENGLISH.SMP")
            .ok_or("missing audio samples")?,
    )?;
    let tracks = discworld_core::music::archive(
        iso.file("/DISCWLD/MIDI.DAT")
            .ok_or("missing MIDI archive")?,
    )?;
    let soundfont_bytes = soundfont.map(std::fs::read).transpose()?;
    let mut synth = soundfont_bytes
        .as_deref()
        .map(|bytes| discworld_audio::MusicOutput::new(bytes, 22050))
        .transpose()?;
    let mut midi_count = 0;
    let strings = discworld_core::text::Strings::new(
        iso.file("/DISCWLD/ENGLISH.TXT")
            .ok_or("missing text archive")?,
    )?;
    if let Some(path) = session_load {
        let bytes = discworld_tools::save_file::read(std::path::Path::new(&path))?;
        let font = soundfont_bytes
            .as_deref()
            .ok_or("--load requires a SoundFont")?;
        let restored = discworld_audio::session::decode(
            &bytes, asset_id, &resources, &bank, &tracks, font, &strings,
        )?;
        engine = restored.engine;
        synth = Some(restored.audio);
    }
    let start_tick = u32::try_from(engine.ticks)?;
    if start_tick > ticks {
        return Err("loaded save is beyond the requested final tick".into());
    }
    // Bound captured duration, including when resuming a late-game save.
    if wave_output.is_some() && ticks - start_tick > 100_000 {
        return Err("WAV capture is limited to 100000 elapsed ticks".into());
    }
    let mut pcm = Vec::new();
    let mut status = "ready".to_owned();
    let mut state_changes = std::collections::VecDeque::new();
    let mut movement_history = std::collections::VecDeque::new();
    let mut previous_movers = Vec::new();
    let mut previous_mover_scene = engine.scene;
    let mut previous_globals = engine.globals.clone();
    let mut previous_control = engine.control.enabled;
    for tick in start_tick..ticks {
        if tick % 100_000 == 0 {
            eprintln!(
                "Headless replay {tick}/{ticks}: {}",
                index[engine.scene.resource()].name
            );
        }
        if press_tick == Some(tick) {
            engine.user_event()?;
        }
        if let Some(input) = interactions.iter().find(|input| input.tick == tick) {
            let scene = &index[engine.scene.resource()].name;
            let result = discworld_tools::replay::apply(input, &mut engine, &resources, scene);
            interaction_log.push(serde_json::json!({"tick":tick,"scene":input.scene,"target":input.target,"event":format!("{:?}", input.event),"result":format!("{result:?}")}));
            match result {
                Ok(true) => {
                    engine.scene_calls = Some(std::collections::VecDeque::new());
                }
                Ok(false) => {
                    status = "unsupported_or_error: interaction was not accepted".to_owned();
                    break;
                }
                Err(error) => {
                    status = format!("unsupported_or_error: {error}");
                    break;
                }
            }
        }
        match engine.tick_content(
            &resources,
            Some(&bank),
            synth.as_ref().map(|_| tracks.as_slice()),
            Some(&strings),
        ) {
            Ok(s) => {
                if snapshot_audit
                    && (engine.ticks % 1000 == 0
                        || engine.ticks == u64::from(ticks)
                        || engine.scene != previous_mover_scene
                        || interactions.iter().any(|input| input.tick == tick))
                {
                    let captured = discworld_core::save::Snapshot::capture(&engine, asset_id);
                    let bytes = captured.to_bytes()?;
                    let decoded = discworld_core::save::Snapshot::from_bytes(&bytes, asset_id)?;
                    decoded.verify_scripts(&resources)?;
                    decoded.verify_audio(&bank, &tracks)?;
                    if decoded.preview_with_strings(&resources, &strings)?
                        != engine.render(&resources)?
                    {
                        return Err(format!("snapshot preview differs at {}", engine.ticks).into());
                    }
                    if decoded.to_bytes()? != bytes {
                        return Err(
                            format!("snapshot round trip failed at {}", engine.ticks).into()
                        );
                    }
                    let restored =
                        decoded.restore_with_strings(&resources, &bank, &tracks, &strings)?;
                    if restored != engine {
                        return Err(
                            format!("restored snapshot state differs at {}", engine.ticks).into(),
                        );
                    }
                    let mut continued = restored.clone();
                    let mut uninterrupted = engine.clone();
                    let a = continued.tick_content(
                        &resources,
                        Some(&bank),
                        synth.as_ref().map(|_| tracks.as_slice()),
                        Some(&strings),
                    )?;
                    let b = uninterrupted.tick_content(
                        &resources,
                        Some(&bank),
                        synth.as_ref().map(|_| tracks.as_slice()),
                        Some(&strings),
                    )?;
                    if a != b
                        || continued != uninterrupted
                        || continued.render(&resources)? != uninterrupted.render(&resources)?
                    {
                        return Err(
                            format!("snapshot continuation differs at {}", engine.ticks).into()
                        );
                    }
                    if let (Some(audio), Some(font)) = (&mut synth, soundfont_bytes.as_deref()) {
                        let encoded = audio.checkpoint_bytes()?;
                        let rebuilt =
                            discworld_audio::MusicOutput::from_checkpoint(font, &encoded)?;
                        if rebuilt.checkpoint_bytes()? != encoded {
                            return Err("audio snapshot round trip differs".into());
                        }
                        let mut trial = rebuilt.clone();
                        let mut baseline = audio.clone();
                        for (clock, messages) in [
                            (engine.music.clock, &engine.music_frame),
                            (continued.music.clock, &continued.music_frame),
                        ] {
                            let a = trial.render(clock, messages)?;
                            let b = baseline.render(clock, messages)?;
                            if a.len() != b.len()
                                || a.iter().zip(&b).any(|(a, b)| a.to_bits() != b.to_bits())
                            {
                                return Err(format!(
                                    "synth snapshot continuation differs at {}",
                                    engine.ticks
                                )
                                .into());
                            }
                        }
                        *audio = rebuilt;
                        audio_snapshot_checks += 1;
                        audio_snapshot_max_bytes = audio_snapshot_max_bytes.max(encoded.len());
                    }
                    engine = restored;
                    snapshot_checks += 1;
                    snapshot_max_bytes = snapshot_max_bytes.max(bytes.len());
                }
                let movers: Vec<_> = engine
                    .movers
                    .iter()
                    .map(|(actor, mover)| {
                        (
                            *actor,
                            mover.placement.position,
                            mover.walk_target,
                            mover.direct_leg.map(|leg| leg.target),
                            mover.placement.path,
                        )
                    })
                    .collect();
                if movers != previous_movers || engine.scene != previous_mover_scene {
                    if movement_history.len() == 256 {
                        movement_history.pop_front();
                    }
                    movement_history.push_back(serde_json::json!({"tick":engine.ticks,"scene":index[engine.scene.resource()].name,"movers":movers}));
                    previous_movers = movers;
                    previous_mover_scene = engine.scene;
                }

                let globals: Vec<_> = previous_globals.iter().zip(&engine.globals).enumerate()
                    .filter(|(_, (before, after))| before != after)
                    .map(|(index, (before, after))| serde_json::json!({"index":index,"before":before,"after":after})).collect();
                if !globals.is_empty() || previous_control != engine.control.enabled {
                    if state_changes.len() == 256 {
                        state_changes.pop_front();
                    }
                    state_changes.push_back(serde_json::json!({"tick":engine.ticks,"scene":index[engine.scene.resource()].name,"globals":globals,"control_before":previous_control,"control_after":engine.control.enabled}));
                    previous_globals.clone_from(&engine.globals);
                    previous_control = engine.control.enabled;
                }

                // Copy only calls committed this tick before ambient scripts
                // can evict them from the engine's general-purpose ring.
                for entry in engine.scene_calls.iter().flatten().filter(|entry| {
                    entry.tick == engine.ticks
                        && matches!(entry.call.id, 53 | 54 | 55 | 77 | 98 | 104)
                }) {
                    if text_calls.len() == 256 {
                        text_calls.pop_front();
                    }
                    text_calls.push_back(serde_json::json!({"tick":entry.tick,"scene":index[engine.scene.resource()].name,"script":entry.script.0,"instruction":entry.call.instruction,"operation":discworld_core::library::name(entry.call.id),"operands":entry.operands}));
                }
                if let Some(synth) = &mut synth {
                    let music = synth.render(engine.music.clock, &engine.music_frame)?;
                    midi_count += engine.music_frame.len();
                    if music.len() != engine.audio_frame.len() * 2 {
                        return Err("audio clock mismatch".into());
                    }
                    if wave_output.is_some() {
                        for (sample, stereo) in engine.audio_frame.iter().zip(music.chunks_exact(2))
                        {
                            let mixed =
                                f32::from(*sample) + (stereo[0] + stereo[1]) * 0.5 * 32767.0;
                            pcm.push(mixed.round().clamp(-32768.0, 32767.0) as i16);
                        }
                    }
                } else if wave_output.is_some() {
                    pcm.extend_from_slice(&engine.audio_frame);
                }
                status = format!("{s:?}");
                if s != discworld_core::engine::Status::Running {
                    break;
                }
            }
            Err(e) => {
                status = format!("unsupported_or_error: {e}");
                break;
            }
        }
    }
    eprintln!(
        "Headless replay stopped at {}/{}: {} ({status})",
        engine.ticks,
        ticks,
        index[engine.scene.resource()].name
    );
    if let Some(path) = session_save {
        if engine.ticks != u64::from(ticks) {
            return Err("not saving an incomplete headless run".into());
        }
        let audio = synth.as_ref().ok_or("--save requires a SoundFont")?;
        let bytes = discworld_audio::session::encode(&engine, audio, asset_id)?;
        discworld_tools::save_file::write_atomic(std::path::Path::new(&path), &bytes)?;
    }
    let surface = engine.render(&resources)?;
    if let Some(path) = wave_output {
        crate::audio::write_wav(std::path::Path::new(&path), &pcm)?;
    }
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), 320, 200);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&surface.rgba)?;
    let mut actor_hotspots = std::collections::BTreeMap::<u32, (usize, [i32; 2])>::new();
    for y in 0..200 {
        for x in 0..320 {
            if let Some(actor) = engine.actor_at(&resources, [x, y])? {
                let entry = actor_hotspots.entry(actor).or_insert((0, [x, y]));
                entry.0 += 1;
            }
        }
    }
    let mut report = serde_json::json!({"movement_history_scope":"Last 256 tick boundaries with changed mover state or scene; mover tuples are actor, position, ultimate target, direct-leg target, path index; intra-tick changes not observed", "movement_history":movement_history,"state_change_history_scope":"Last 256 tick boundaries with changed globals or control; intra-tick writes not observed", "state_change_history":state_changes,"text_call_history_scope":"Latest 256 committed scene PRINT/PRINTOBJ/PRINTTAG/TALK/TALKAT/TALKATS dispatches observed at tick boundaries; master calls excluded; a tick with more than 256 dispatches may truncate earlier calls","text_call_history":text_calls,"globals":engine.globals,"world_origin":engine.world_origin,"control_enabled":engine.control.enabled,"audio_snapshot_checks":audio_snapshot_checks,"audio_snapshot_max_bytes":audio_snapshot_max_bytes,"snapshot_checks":snapshot_checks,"snapshot_max_bytes":snapshot_max_bytes,"has_restarted":engine.has_restarted(),"movers":engine.movers.iter().map(|(actor,m)| serde_json::json!({"actor":actor,"position":m.placement.position,"path":m.placement.path,"target":m.walk_target,"constrained":m.constrained_walk,"node_travel":format!("{:?}",m.node_travel),"node_walk":format!("{:?}",m.node_walk),"direct_leg":format!("{:?}",m.direct_leg),"footprint":format!("{:?}",m.footprint(&resources))})).collect::<Vec<_>>(),"actor_hotspots":actor_hotspots,"conversation":{"owner":engine.conversation.polygon,"hidden":engine.conversation.hidden,"icon":engine.conversation.icon,"topics":engine.declarations.windows[0].as_ref().map(|w| &w.contents)},"scene_call_trace_scope":"Last 256 committed scene library dispatches since the most recent accepted interaction; master calls excluded", "scene_call_trace":engine.scene_calls.iter().flatten().map(|entry|serde_json::json!({"tick":entry.tick,"script":entry.script.0,"id":entry.call.id,"instruction":entry.call.instruction,"operation":discworld_core::library::name(entry.call.id),"operands":entry.operands})).collect::<Vec<_>>(),"active_inventory":engine.active_inventory(),"inventory_geometry":engine.declarations.windows.iter().map(|w|w.as_ref().map(|w|serde_json::json!({"min":w.min,"size":w.size,"max":w.max,"position":w.position,"capacity":w.capacity}))).collect::<Vec<_>>(),"inventory_first_display":engine.declarations.windows.iter().map(|w|w.as_ref().map(|w|w.first_display)).collect::<Vec<_>>(),"inventory_contents":engine.declarations.windows.iter().map(|w|w.as_ref().map(|w|&w.contents)).collect::<Vec<_>>(),"inventory_objects":engine.inventory_objects.values().map(|o|serde_json::json!({"id":o.id,"film":o.film.0,"script":o.script.0,"attributes":o.attributes})).collect::<Vec<_>>(),"held_object":engine.held_object,"interactions":interaction_log,"polygons":engine.polygons.active.iter().enumerate().map(|(index,p)|serde_json::json!({"index":index,"kind":format!("{:?}",p.definition.kind),"id":p.definition.id,"enabled":p.enabled,"script":p.definition.script.0,"corners":p.definition.corners,"text":p.definition.tag_text.0})).collect::<Vec<_>>(),"ticks":engine.ticks,"midi_messages":midi_count,"music_offset":engine.music.offset,"scene":engine.scene.0,"scene_name":index[engine.scene.resource()].name,"status":status,"standing_requests":engine.pending_calls().iter().filter(|(_,call,a)|call.id==72 && a.len()>=4).map(|(_,_,a)|{let a=&a[a.len()-4..];format!("{:?}",discworld_core::mover::Placement::initial(&engine.declarations,&engine.polygons,a[0],[a[1],a[2]],Handle(a[3] as u32)))}).collect::<Vec<_>>(),"pending_calls":engine.pending_calls().iter().map(|(code, call, operands)| serde_json::json!({"script":code.0,"operation":discworld_core::library::name(call.id),"id":call.id,"instruction":call.instruction,"operands":operands})).collect::<Vec<_>>(),"film_reels":engine.films.len(),"animations":engine.films.iter().map(|a|format!("{a:?}")).collect::<Vec<_>>(),"scope":"Queued scene/entrance runtime; full scheduling and pixels not yet compared with ScummVM"});
    report["start_tick"] = start_tick.into();
    println!("{}", serde_json::to_string_pretty(&report)?);
    if status.starts_with("Unsupported") || status.starts_with("unsupported_or_error") {
        return Err("runtime stopped at unsupported operation; see report".into());
    }
    Ok(())
}

pub fn reel(
    iso_path: std::ffi::OsString,
    mut args: impl Iterator<Item = std::ffi::OsString>,
) -> Result<(), Box<dyn std::error::Error>> {
    let handle: u32 = args
        .next()
        .ok_or("missing decimal film handle")?
        .into_string()
        .map_err(|_| "invalid handle")?
        .parse()?;
    let ordinal: usize = args
        .next()
        .ok_or("missing reel ordinal")?
        .into_string()
        .map_err(|_| "invalid ordinal")?
        .parse()?;
    let ticks: u32 = args
        .next()
        .ok_or("missing tick count")?
        .into_string()
        .map_err(|_| "invalid ticks")?
        .parse()?;
    let output = args.next().ok_or("missing output path")?;
    if args.next().is_some() {
        return Err("unexpected argument".into());
    }
    if ticks > 10000 {
        return Err("preview tick limit exceeded".into());
    }
    let data = std::fs::read(iso_path)?;
    let iso = Iso::parse(&data)?;
    let index = parse_index(iso.file("/DISCWLD/INDEX").ok_or("missing index")?)?;
    let resources = Resources(
        index
            .iter()
            .map(|e| {
                iso.file(&format!("/DISCWLD/{}", e.name))
                    .ok_or("missing resource")
            })
            .collect::<Result<Vec<_>, _>>()?,
    );
    let film = Film::read(&resources, Handle(handle))?;
    let reel = film.reels.get(ordinal).ok_or("invalid reel ordinal")?;
    let mut animation = Animation::new(&resources, reel, 24 / film.rate as u32)?;
    for _ in 0..ticks {
        animation.tick(&resources)?;
    }
    let mut surface = Surface::new(320, 200)?;
    surface.draw_animation(&resources, &animation)?;
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), 320, 200);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&surface.rgba)?;
    eprintln!(
        "Rendered reel {ordinal} of film {handle} at tick {ticks}; frame={}",
        animation.frame.0
    );
    Ok(())
}

#[cfg(test)]
mod interaction_tests {
    use super::*;
    #[test]
    fn trace_requires_ordered_ticks_and_explicit_scene_target() {
        let valid = br#"[{"tick":8600,"scene":"rinceroo.scn","polygon":3,"event":"look"}]"#;
        let trace = interactions(valid, 10000).unwrap();
        assert_eq!(
            trace[0].event,
            InteractionEvent::Polygon(discworld_core::engine::PolygonInteraction::Look)
        );
        assert_eq!(trace[0].target, 3);
        let index = [discworld_core::resource::IndexEntry {
            name: "rinceroo.scn".to_owned(),
            size: 0,
            flags: 0,
        }];
        assert!(discworld_tools::replay::validate_scenes(&trace, &index).is_ok());
        let error = discworld_tools::replay::validate_scenes(&trace, &[])
            .unwrap_err()
            .to_string();
        assert!(error.contains("rinceroo.scn") && error.contains("8600"));

        let actors =
            interactions(br#"[{"tick":1,"scene":"a","actor":7,"event":"action"}]"#, 2).unwrap();
        assert_eq!(
            actors[0].event,
            InteractionEvent::Actor(discworld_core::engine::PolygonInteraction::Action)
        );
        let points = interactions(
            br#"[{"tick":1,"scene":"a","point":[175,180],"event":"walkto"}]"#,
            2,
        )
        .unwrap();
        assert_eq!(
            points[0].event,
            InteractionEvent::World(
                [175, 180],
                discworld_core::engine::PolygonInteraction::WalkTo
            )
        );
        for invalid in [
            br#"[{"tick":1,"scene":"a","point":[320,0],"event":"walkto"}]"#.as_slice(),
            br#"[{"tick":1,"scene":"a","point":[0],"event":"walkto"}]"#.as_slice(),
        ] {
            assert!(interactions(invalid, 2).is_err());
        }
        let items = interactions(
            br#"[{"tick":1,"scene":"a","inventory_slot":0,"event":"walkto"}]"#,
            2,
        )
        .unwrap();
        assert_eq!(
            items[0].event,
            InteractionEvent::Inventory(discworld_core::engine::PolygonInteraction::WalkTo)
        );
        let topics = interactions(br#"[{"tick":1,"scene":"library.scn","topic":4,"event":"topic"},{"tick":2,"scene":"library.scn","event":"close_conversation"}]"#, 3).unwrap();
        assert_eq!(topics[0].event, InteractionEvent::Topic);
        assert_eq!(topics[0].target, 4);
        assert_eq!(topics[1].event, InteractionEvent::CloseConversation);
        let key = interactions(br#"[{"tick":1,"scene":"closeup.scn","event":"key"}]"#, 2).unwrap();
        assert_eq!(key[0].event, InteractionEvent::Key);
        let close =
            interactions(br#"[{"tick":1,"scene":"a","event":"close_inventory"}]"#, 2).unwrap();
        assert_eq!(close[0].event, InteractionEvent::CloseInventory);
        assert!(
            interactions(
                br#"[{"tick":1,"scene":"a","inventory_slot":0,"event":"close_inventory"}]"#,
                2
            )
            .is_err()
        );
        assert!(interactions(br#"[{"tick":1,"scene":"a","polygon":0,"event":"key"}]"#, 2).is_err());
        assert!(interactions(valid, 8600).is_err());
        for invalid in [
            r#"[{"tick":1,"scene":"a","actor":0,"event":"action"}]"#,
            r#"[{"tick":1,"scene":"a","actor":513,"event":"action"}]"#,
            r#"[{"tick":1,"scene":"a","actor":7,"polygon":0,"event":"action"}]"#,
            r#"[{"tick":1,"scene":"a","polygon":4,"event":"topic"}]"#,
            r#"[{"tick":1,"scene":"a","topic":-1,"event":"topic"}]"#,
            r#"[{"tick":1,"scene":"a","topic":0,"event":"close_conversation"}]"#,
            r#"{}"#,
            r#"[{"tick":1,"scene":"","polygon":0,"event":"look"}]"#,
            r#"[{"tick":1,"scene":"a","polygon":-1,"event":"look"}]"#,
            r#"[{"tick":1,"scene":"a","polygon":0,"event":"click"}]"#,
            r#"[{"tick":1,"scene":"a","polygon":0,"event":"look","typo":1}]"#,
            r#"[{"tick":2,"scene":"a","polygon":0,"event":"look"},{"tick":2,"scene":"a","polygon":1,"event":"action"}]"#,
        ] {
            assert!(interactions(invalid.as_bytes(), 10).is_err(), "{invalid}");
        }
    }
}
