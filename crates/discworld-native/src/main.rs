// SPDX-License-Identifier: GPL-3.0-or-later
//! Development host. Game logic and clocks remain in discworld-core.
use discworld_core::{
    audio::SampleBank,
    engine::{Engine, PolygonInteraction, Status},
    graphics::Resources,
    iso::Iso,
    music,
    resource::parse_index,
    text::Strings,
};
use sdl2::{
    event::{Event, WindowEvent},
    keyboard::Keycode as Key,
    pixels::PixelFormatEnum,
};
mod audio_output;
mod input;
mod menu;
use std::time::{Duration, Instant};

fn replace_audio_sink(sink: &mut rodio::Sink, mixer: &rodio::mixer::Mixer, paused: bool) {
    // clear() waits for the audio callback, which may be suspended after sleep
    // or a device interruption. Dropping a sink only signals its source to stop.
    // Use a fresh sink: appending to a stopped sink can also wait for that callback.
    let replacement = audio_output::connect(mixer);
    replacement.set_volume(sink.volume());
    if paused {
        replacement.pause();
    }
    *sink = replacement;
}

fn main() {
    if let Err(error) = run() {
        eprintln!("discworld-native: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let path = args
        .next()
        .ok_or("usage: discworld-native ISO SOUNDFONT [--room | --replay TRACE [--from-tick N]] [--load SAVE] [--save-file SAVE] [--mute] [--paused] [--frames N]")?;
    let soundfont = args.next().ok_or("missing SoundFont")?;
    let mut from_tick = 0u64;
    let mut replay_path = None;
    let mut muted = false;
    let mut start_paused = false;
    let mut room = false;
    let mut frame_limit = None;
    let mut load_path = None;
    let mut save_path = None;
    while let Some(arg) = args.next() {
        if arg == "--load" {
            load_path = Some(std::path::PathBuf::from(
                args.next().ok_or("missing load path")?,
            ));
        } else if arg == "--save-file" {
            save_path = Some(std::path::PathBuf::from(
                args.next().ok_or("missing save path")?,
            ));
        } else if arg == "--replay" {
            replay_path = Some(args.next().ok_or("missing replay path")?);
        } else if arg == "--from-tick" {
            from_tick = args
                .next()
                .ok_or("missing start tick")?
                .to_str()
                .ok_or("invalid start tick")?
                .parse()?;
        } else if arg == "--paused" {
            start_paused = true;
        } else if arg == "--mute" {
            muted = true;
        } else if arg == "--room" {
            room = true;
        } else if arg == "--frames" {
            frame_limit = Some(
                args.next()
                    .ok_or("missing frame count")?
                    .to_str()
                    .ok_or("invalid frame count")?
                    .parse::<u64>()?,
            );
        } else {
            return Err("unknown option".into());
        }
    }
    let replay = replay_path
        .map(|p| -> Result<_, Box<dyn std::error::Error>> {
            discworld_tools::replay::interactions(&std::fs::read(p)?, u32::MAX)
        })
        .transpose()?;
    if replay.is_some() && room {
        return Err("--room and --replay cannot be combined".into());
    }
    if load_path.is_some() && (room || from_tick != 0) {
        return Err("--load cannot be combined with --room or --from-tick".into());
    }
    let save_path = save_path
        .or_else(|| load_path.clone())
        .unwrap_or_else(|| std::path::PathBuf::from("discworld-save.dws"));
    let mut replay_position = 0usize;
    let data = std::fs::read(path)?;
    let asset_id = discworld_audio::session::asset_identity(&data);
    let iso = Iso::parse(&data)?;
    let index = parse_index(iso.file("/DISCWLD/INDEX").ok_or("missing index")?)?;
    if let Some(inputs) = &replay {
        discworld_tools::replay::validate_scenes(inputs, &index)?;
    }
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
        iso.file("/DISCWLD/ENGLISH.SMP").ok_or("missing samples")?,
    )?;
    let tracks = music::archive(iso.file("/DISCWLD/MIDI.DAT").ok_or("missing music")?)?;
    let strings = Strings::new(iso.file("/DISCWLD/ENGLISH.TXT").ok_or("missing text")?)?;
    let mut engine = Engine::bootstrap(&resources)?;
    let soundfont_bytes = std::fs::read(soundfont)?;
    let mut synth = discworld_audio::MusicOutput::new(&soundfont_bytes, 22050)?;
    if let Some(path) = &load_path {
        let bytes = discworld_tools::save_file::read(path)?;
        let restored = discworld_audio::session::decode(
            &bytes,
            asset_id,
            &resources,
            &bank,
            &tracks,
            &soundfont_bytes,
            &strings,
        )?;
        engine = restored.engine;
        synth = restored.audio;
        replay_position = replay.as_ref().map_or(0, |inputs| {
            inputs.partition_point(|i| u64::from(i.tick) < engine.ticks)
        });
        eprintln!("Loaded {} at tick {}", path.display(), engine.ticks);
    }
    let menu_root = save_path.with_extension("slots");
    let mut menus = menu::Menu::new(
        menu_root,
        discworld_tools::menu_sessions::Assets {
            identity: asset_id,
            resources: &resources,
            bank: &bank,
            tracks: &tracks,
            strings: &strings,
            soundfont: &soundfont_bytes,
        },
    )?;
    let step = |engine: &mut Engine,
                synth: &mut discworld_audio::MusicOutput,
                replay_position: &mut usize,
                saves: &[discworld_core::save_menu::Entry]|
     -> Result<
        (Status, Vec<f32>, discworld_core::menu_controller::Dispatch),
        Box<dyn std::error::Error>,
    > {
        if let Some(inputs) = &replay {
            if engine.ticks == 500 {
                engine.user_event()?;
            }
            if let Some(input) = inputs
                .get(*replay_position)
                .filter(|i| u64::from(i.tick) == engine.ticks)
            {
                let scene = &index[engine.scene.resource()].name;
                if !discworld_tools::replay::apply(input, engine, &resources, scene)? {
                    return Err(format!("Replay input rejected at tick {}", engine.ticks).into());
                }
                eprintln!("Replay tick {}: {} {:?}", input.tick, scene, input.event);
                *replay_position += 1;
            }
        }
        let (status, dispatch) = engine.tick_with_configuration(
            &resources,
            Some(&bank),
            Some(&tracks),
            &strings,
            saves,
        )?;
        let mut output = synth.render(engine.music.clock, &engine.music_frame)?;
        if output.len() != engine.audio_frame.len() * 2 {
            return Err("audio clock mismatch".into());
        }
        for (stereo, speech) in output.chunks_exact_mut(2).zip(&engine.audio_frame) {
            for sample in stereo {
                *sample = (*sample + f32::from(*speech) / 32768.0).clamp(-1.0, 1.0);
            }
        }
        Ok((status, output, dispatch))
    };
    if room {
        eprintln!("Replaying startup to Rincewind's room…");
        for tick in 0..8500 {
            if tick == 500 {
                engine.user_event()?;
            }
            let (status, _, _) = step(
                &mut engine,
                &mut synth,
                &mut replay_position,
                &menus.catalog.entries,
            )?;
            if status != Status::Running {
                return Err(format!("startup stopped: {status:?}").into());
            }
        }
    }
    if from_tick > 0 {
        if replay.is_none() {
            return Err("--from-tick requires --replay".into());
        }
        eprintln!("Preparing replay at tick {from_tick}…");
        while engine.ticks < from_tick {
            let (status, _, _) = step(
                &mut engine,
                &mut synth,
                &mut replay_position,
                &menus.catalog.entries,
            )?;
            if status != Status::Running {
                return Err(format!("replay preparation stopped: {status:?}").into());
            }
        }
    }
    menu::Menu::initialize(&mut engine)?;
    let mut stream = rodio::OutputStreamBuilder::open_default_stream()?;
    stream.log_on_drop(false);
    let mut sink = audio_output::connect(stream.mixer());
    sink.set_volume(if muted { 0.0 } else { 1.0 });
    let mut speed = 1u32;
    let mut replay_end = replay
        .as_ref()
        .and_then(|v| v.last())
        .map(|i| u64::from(i.tick) + 2400);
    let sdl = sdl2::init()?;
    let video = sdl.video()?;
    sdl2::hint::set("SDL_RENDER_SCALE_QUALITY", "0");
    let window = video
        .window("Discworld Rust — development host", 640, 400)
        .position_centered()
        .allow_highdpi()
        .build()?;
    let mut canvas = window.into_canvas().present_vsync().build()?;
    canvas.set_logical_size(320, 200)?;
    let creator = canvas.texture_creator();
    let mut texture = creator.create_texture_streaming(PixelFormatEnum::RGBA32, 320, 200)?;
    let mut events = sdl.event_pump()?;
    video.text_input().start();
    let mut menu_requests = std::collections::VecDeque::new();
    let mut buttons = input::Buttons::restored_engine(&engine);
    sdl.mouse().capture(buttons.any());
    sdl.mouse().show_cursor(false);
    if replay.is_some() {
        eprintln!(
            "Watching recorded inputs. P: pause/resume. F: 1x/4x/16x speed (accelerated playback is silent). M: mute/unmute. Esc: exit. Manual game inputs are enabled after the recorded route finishes."
        );
    } else {
        eprintln!(
            "Mouse moves cursor. Single left click walks/selects, double left click acts, right click looks. Space or click during speech advances dialogue. I: pockets. Click inventory cells to pick up/put down. Point at an actor/tag and press A: action, W: walk to, L: look. P: pause. F1: options, F5: save menu, F7: load menu. Escape: close menu. Close window to exit. Click conversation icons to select; Backspace closes the menu. Drag inventory title bars to move windows, scrollbar thumbs to scroll, or edges/corners to resize. Double-click a title bar to maximize/restore. Complete input compatibility remains unfinished."
        );
        eprintln!(
            "M: mute/unmute sound. Inventory: wheel or Up/Down scrolls rows; Page Up/Page Down, Home/End navigate; click scrollbar arrows or track."
        );
    }
    let mut previous = Instant::now();
    let mut elapsed = Duration::ZERO;
    let interval = Duration::from_nanos(1_000_000_000 / 24);
    let mut paused = start_paused;
    if paused {
        sink.pause();
    }
    let mut stopped = false;
    let mut frames = 0u64;
    let mut save_notice = String::new();
    eprintln!(
        "F6: quick-save. F9: load. Save file: {}",
        save_path.display()
    );
    'running: while frame_limit.is_none_or(|limit| frames < limit) {
        let frame_started = Instant::now();
        let now = Instant::now();
        elapsed = (elapsed + now.duration_since(previous)).min(interval * 4);
        previous = now;
        for event in events.poll_iter() {
            let manual = (replay.is_none() || replay_end.is_none()) && !stopped;
            match event {
                Event::Quit { .. } => break 'running,
                Event::TextInput { text, .. }
                    if manual && !paused && menu::Menu::active(&engine) =>
                {
                    // Original English font indices are byte values, not UTF-8.
                    for character in text
                        .chars()
                        .filter(|c| c.is_ascii() && !c.is_ascii_control())
                    {
                        engine.edit_configuration(&resources, character as u8, false)?;
                    }
                }
                Event::KeyDown {
                    keycode: Some(key),
                    repeat: false,
                    ..
                } if manual
                    && !paused
                    && matches!(key, Key::F1 | Key::F5 | Key::F7 | Key::Escape) =>
                {
                    use discworld_core::menu_navigation::Key as MenuKey;
                    if key == Key::Escape {
                        engine.user_event()?;
                        engine.escape_configuration(
                            &resources,
                            &strings,
                            &menus.catalog.entries,
                        )?;
                    } else {
                        let key = match key {
                            Key::F5 => MenuKey::Save,
                            Key::F7 => MenuKey::Load,
                            _ => MenuKey::Main,
                        };
                        if let Err(error) = menus.open(&mut engine, key) {
                            save_notice = format!("menu failed: {error}");
                            eprintln!("{save_notice}");
                        }
                    }
                }
                Event::KeyDown {
                    keycode: Some(key), ..
                } if manual
                    && !paused
                    && menu::Menu::active(&engine)
                    && !matches!(key, Key::F6 | Key::F9) =>
                {
                    use discworld_core::save_menu::Scroll;
                    let scroll = match key {
                        Key::Up => Some(Scroll::RowUp),
                        Key::Down => Some(Scroll::RowDown),
                        Key::PageUp => Some(Scroll::PageUp),
                        Key::PageDown => Some(Scroll::PageDown),
                        Key::Home => Some(Scroll::Home),
                        Key::End => Some(Scroll::End),
                        _ => None,
                    };
                    if key == Key::Backspace {
                        engine.edit_configuration(&resources, 0, true)?;
                    }
                    if let Some(scroll) = scroll {
                        engine.scroll_configuration(scroll)?;
                    }
                }
                Event::KeyDown {
                    keycode: Some(Key::Escape),
                    ..
                } if !manual => break 'running,
                Event::KeyDown {
                    keycode: Some(Key::F6),
                    repeat: false,
                    ..
                } => {
                    let result = discworld_audio::session::encode(&engine, &synth, asset_id)
                        .and_then(|bytes| {
                            discworld_tools::save_file::write_atomic(&save_path, &bytes)
                                .map_err(Into::into)
                        });
                    save_notice = match result {
                        Ok(()) => format!("saved tick {}", engine.ticks),
                        Err(error) => format!("save failed: {error}"),
                    };
                    eprintln!("{save_notice}: {}", save_path.display());
                    elapsed = Duration::ZERO;
                    previous = Instant::now();
                }
                Event::KeyDown {
                    keycode: Some(Key::F9),
                    repeat: false,
                    ..
                } => {
                    let result = discworld_tools::save_file::read(&save_path)
                        .map_err(Box::<dyn std::error::Error>::from)
                        .and_then(|bytes| {
                            discworld_audio::session::decode(
                                &bytes,
                                asset_id,
                                &resources,
                                &bank,
                                &tracks,
                                &soundfont_bytes,
                                &strings,
                            )
                        });
                    match result {
                        Ok(restored) => {
                            replace_audio_sink(&mut sink, stream.mixer(), paused);
                            engine = restored.engine;
                            menu::Menu::initialize(&mut engine)?;
                            synth = restored.audio;
                            stopped = false;
                            menu_requests.clear();
                            buttons = input::Buttons::restored_engine(&engine);
                            sdl.mouse().capture(buttons.any());
                            replay_position = replay.as_ref().map_or(0, |inputs| {
                                inputs.partition_point(|i| u64::from(i.tick) < engine.ticks)
                            });
                            replay_end = replay
                                .as_ref()
                                .and_then(|v| v.last())
                                .map(|i| u64::from(i.tick) + 2400);
                            save_notice = format!("loaded tick {}", engine.ticks);
                        }
                        Err(error) => save_notice = format!("load failed: {error}"),
                    }
                    eprintln!("{save_notice}: {}", save_path.display());
                    elapsed = Duration::ZERO;
                    previous = Instant::now();
                }
                Event::KeyDown {
                    keycode: Some(Key::M),
                    repeat: false,
                    ..
                } => {
                    muted = !muted;
                    sink.set_volume(if muted { 0.0 } else { 1.0 });
                }
                Event::KeyDown {
                    keycode: Some(Key::F),
                    repeat: false,
                    ..
                } if replay.is_some() => {
                    speed = match speed {
                        1 => 4,
                        4 => 16,
                        _ => 1,
                    };
                    replace_audio_sink(&mut sink, stream.mixer(), paused || stopped);
                }
                Event::KeyDown {
                    keycode: Some(Key::P),
                    repeat: false,
                    ..
                } => {
                    paused = !paused;
                    if paused {
                        sink.pause();
                    } else {
                        sink.play();
                    }
                }
                Event::MouseMotion { x, y, .. } if manual && !paused => {
                    engine.move_cursor_with_configuration([x, y])?
                }
                Event::MouseButtonDown {
                    mouse_btn, x, y, ..
                } if manual && !paused => {
                    if let Some(button) = buttons.press(mouse_btn) {
                        engine.move_cursor_with_configuration([x, y])?;
                        menu_requests.push_back(
                            engine.mouse_button_with_configuration(&resources, button, true)?,
                        );
                        sdl.mouse().capture(true);
                    }
                }
                Event::MouseButtonUp {
                    mouse_btn, x, y, ..
                } if manual => {
                    if let Some(button) = buttons.release(mouse_btn) {
                        if !paused {
                            engine.move_cursor_with_configuration([x, y])?;
                        }
                        menu_requests.push_back(
                            engine.mouse_button_with_configuration(&resources, button, false)?,
                        );
                        sdl.mouse().capture(buttons.any());
                    }
                }
                Event::Window {
                    win_event: WindowEvent::FocusLost,
                    ..
                } => {
                    for button in buttons.release_all() {
                        if manual {
                            menu_requests.push_back(
                                engine
                                    .mouse_button_with_configuration(&resources, button, false)?,
                            );
                        }
                    }
                    sdl.mouse().capture(false);
                }
                Event::MouseWheel { y, direction, .. } if manual && !paused => {
                    use discworld_core::inventory::Scroll;
                    use sdl2::mouse::MouseWheelDirection;
                    let up = (y > 0) != (direction == MouseWheelDirection::Flipped);
                    if y != 0 {
                        if menu::Menu::active(&engine) {
                            use discworld_core::save_menu::Scroll as MenuScroll;
                            engine.scroll_configuration(if up {
                                MenuScroll::RowUp
                            } else {
                                MenuScroll::RowDown
                            })?;
                        } else {
                            engine.scroll_inventory(if up { Scroll::Up } else { Scroll::Down });
                        }
                    }
                }
                Event::KeyDown {
                    keycode: Some(key),
                    repeat,
                    ..
                } if manual && !paused => {
                    use discworld_core::inventory::Scroll;
                    let scroll = match key {
                        Key::Up => Some(Scroll::Up),
                        Key::Down => Some(Scroll::Down),
                        Key::PageUp => Some(Scroll::PageUp),
                        Key::PageDown => Some(Scroll::PageDown),
                        Key::Home => Some(Scroll::Home),
                        Key::End => Some(Scroll::End),
                        _ => None,
                    };
                    if let Some(action) = scroll {
                        engine.scroll_inventory(action);
                    }
                    if repeat {
                        continue;
                    }
                    match key {
                        Key::I => {
                            if matches!(engine.active_inventory(), Some(1 | 2)) {
                                engine.close_inventory();
                            } else if let Some(lead) = &engine.declarations.lead {
                                engine.interact_actor(
                                    &resources,
                                    lead.id as u32,
                                    PolygonInteraction::WalkTo,
                                )?;
                            }
                        }
                        Key::Space => engine.left_event()?,
                        Key::Backspace => {
                            if matches!(engine.active_inventory(), Some(1 | 2)) {
                                engine.close_inventory();
                            } else {
                                engine.interact_conversation(&resources, None)?;
                            }
                        }
                        Key::A | Key::W | Key::L => {
                            let event = match key {
                                Key::A => PolygonInteraction::Action,
                                Key::L => PolygonInteraction::Look,
                                _ => PolygonInteraction::WalkTo,
                            };
                            if let Some(slot) = engine.inventory_slot_at(engine.control.cursor) {
                                engine.interact_inventory(&resources, slot, event)?;
                            } else {
                                engine.interact_world(&resources, engine.control.cursor, event)?;
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        if !paused && !stopped {
            let playback_interval = interval / speed;
            while elapsed >= playback_interval
                && (speed > 1 || sink.len() < 6)
                && !menu_requests
                    .iter()
                    .any(|dispatch| dispatch.outcome.request.is_some())
            {
                elapsed -= playback_interval;
                match step(
                    &mut engine,
                    &mut synth,
                    &mut replay_position,
                    &menus.catalog.entries,
                ) {
                    Ok((Status::Running, audio, dispatch)) => {
                        let has_request = dispatch.outcome.request.is_some();
                        menu_requests.push_back(dispatch);
                        if speed == 1 {
                            sink.append(rodio::buffer::SamplesBuffer::new(2, 22050, audio));
                        }
                        // Control may return while the new background is still
                        // black. Finish both the cutscene and its fade first.
                        if engine.control.enabled
                            && !engine.scene_transition_active()
                            && replay_end.is_some_and(|end| engine.ticks >= end)
                        {
                            replay_end = None;
                            eprintln!(
                                "Recorded route finished. P resumes with manual game controls enabled."
                            );
                            paused = true;
                            sink.pause();
                            elapsed = Duration::ZERO;
                            break;
                        }
                        if has_request {
                            break;
                        }
                    }
                    result => {
                        let reason = match result {
                            Ok((status, _, _)) => format!("{status:?}"),
                            Err(error) => error.to_string(),
                        };
                        eprintln!(
                            "Stopped at tick {}, scene {}: {reason}",
                            engine.ticks,
                            index[engine.scene.resource()].name
                        );
                        canvas.window_mut().set_title(&format!(
                            "Discworld Rust — stopped: {reason} — Esc to exit"
                        ))?;
                        sink.stop();
                        stopped = true;
                        break;
                    }
                }
            }
        } else {
            elapsed = Duration::ZERO;
        }
        while let Some(dispatch) = menu_requests.pop_front() {
            match menus.dispatch(&dispatch, &engine, &synth) {
                Ok(menu::Effect::None) => continue,
                Ok(menu::Effect::Quit) => break 'running,
                Ok(menu::Effect::Saved(id)) => save_notice = format!("saved slot {id}"),
                Ok(menu::Effect::Loaded(restored)) => {
                    replace_audio_sink(&mut sink, stream.mixer(), paused);
                    engine = restored.engine;
                    synth = restored.audio;
                    menu::Menu::initialize(&mut engine)?;
                    stopped = false;
                    buttons = input::Buttons::restored_engine(&engine);
                    sdl.mouse().capture(buttons.any());
                    replay_position = replay.as_ref().map_or(0, |inputs| {
                        inputs.partition_point(|i| u64::from(i.tick) < engine.ticks)
                    });
                    replay_end = replay
                        .as_ref()
                        .and_then(|v| v.last())
                        .map(|i| u64::from(i.tick) + 2400);
                    save_notice = format!("loaded tick {}", engine.ticks);
                    menu_requests.clear();
                }
                Err(error) => {
                    save_notice = format!("menu failed: {error}");
                    eprintln!("{save_notice}");
                }
            }
            elapsed = Duration::ZERO;
            previous = Instant::now();
        }
        if !stopped {
            canvas.window_mut().set_title(&format!(
                "Discworld Rust — {} — {} — {} — F1 menu — M sound — F6 quick-save/F9 quick-load {}",
                index[engine.scene.resource()].name,
                if paused && replay.is_some() && replay_end.is_none() {
                    "recorded route finished (P continues)"
                } else if paused {
                    "paused (P resumes)"
                } else {
                    "P pause"
                },
                if let Some(inputs) = &replay {
                    let next = inputs.partition_point(|input| u64::from(input.tick) < engine.ticks);
                    let countdown = inputs.get(next).map_or_else(
                        || " · no recorded inputs remaining".to_string(),
                        |input| {
                            let seconds = (u64::from(input.tick) - engine.ticks) as f64
                                / 24.0
                                / f64::from(speed);
                            format!(
                                " · next input in {seconds:.1}s{}",
                                if paused { " after resume" } else { "" }
                            )
                        },
                    );
                    format!(
                        "Replay {speed}x · F speed · tick {}{}{}",
                        engine.ticks,
                        countdown,
                        if muted || speed > 1 { " · silent" } else { "" }
                    )
                } else {
                    format!(
                        "A action · W walk · L look{}",
                        if muted { " · muted" } else { "" }
                    )
                },
                save_notice
            ))?;
        }
        let surface = engine.render(&resources)?;
        texture.update(None, &surface.rgba, 320 * 4)?;
        canvas.clear();
        canvas.copy(&texture, None, None)?;
        canvas.present();
        // Also bound CPU use when the selected renderer does not honor vsync.
        if let Some(rest) =
            Duration::from_nanos(1_000_000_000 / 60).checked_sub(frame_started.elapsed())
        {
            std::thread::sleep(rest);
        }
        frames += 1;
    }
    eprintln!(
        "Window closed after {frames} frames, tick {}, scene {}, stopped={stopped}",
        engine.ticks,
        index[engine.scene.resource()].name
    );
    if stopped {
        return Err("runtime reached an unsupported operation".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacing_audio_without_a_device_callback_does_not_wait() {
        let (done, result) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let (mixer, _undrained_output) = rodio::mixer::mixer(2, 22050);
            let mut sink = audio_output::connect(&mixer);
            for paused in [false, true, false] {
                sink.set_volume(0.0);
                sink.append(rodio::buffer::SamplesBuffer::new(2, 22050, vec![0.5; 100]));
                assert_eq!(sink.len(), 1);
                replace_audio_sink(&mut sink, &mixer, paused);
                assert!(sink.empty());
                assert_eq!(sink.volume(), 0.0);
                assert_eq!(sink.is_paused(), paused);
                // A new queue accepts samples without draining the abandoned one.
                sink.append(rodio::buffer::SamplesBuffer::new(2, 22050, vec![0.0; 100]));
                replace_audio_sink(&mut sink, &mixer, paused);
            }
            done.send(()).unwrap();
        });
        result
            .recv_timeout(Duration::from_secs(2))
            .expect("audio replacement waited for the absent device callback");
        worker.join().unwrap();
    }
}
