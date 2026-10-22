use discworld_core::{
    compositor::Surface,
    conversation_window::{Content, Frame},
    engine::Engine,
    graphics::Resources,
    inventory,
    iso::Iso,
    resource::parse_index,
    text::{Font, Strings},
};

pub fn preview(
    path: std::ffi::OsString,
    mut args: impl Iterator<Item = std::ffi::OsString>,
    menu: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let output = args.next().ok_or("missing output PNG")?;
    let slot = args
        .next()
        .map(|s| {
            s.to_str()
                .ok_or("invalid window slot")?
                .parse::<usize>()
                .map_err(|_| "invalid window slot")
        })
        .transpose()?
        .unwrap_or(0);
    if slot > 2 || args.next().is_some() {
        return Err("invalid window slot or extra argument".into());
    }
    let data = std::fs::read(path)?;
    let iso = Iso::parse(&data)?;
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
    if let Some(kind) = menu {
        if kind == "preview-menu" && slot != 0 {
            return Err("main-menu preview takes no image state".into());
        }
        let strings = Strings::new(iso.file("/DISCWLD/ENGLISH.TXT").ok_or("missing text")?)?;
        let bank = discworld_core::audio::SampleBank::new(
            iso.file("/DISCWLD/ENGLISH.IDX")
                .ok_or("missing audio index")?,
            iso.file("/DISCWLD/ENGLISH.SMP").ok_or("missing samples")?,
        )?;
        let tracks =
            discworld_core::music::archive(iso.file("/DISCWLD/MIDI.DAT").ok_or("missing MIDI")?)?;
        for _ in 0..500 {
            if let discworld_core::engine::Status::Unsupported(call) =
                engine.tick_content(&resources, Some(&bank), Some(&tracks), Some(&strings))?
            {
                return Err(format!("menu preview stopped: {call:?}").into());
            }
        }
        use discworld_core::configuration_menu::{
            ButtonImage, Choice, Confirmation, ConfirmationMenu, MainMenu, SoundMenu,
        };
        let mut surface = engine.render(&resources)?;
        let mut sound_audit = None;
        let mut settings_audit = None;
        match kind {
            "preview-menu" => MainMenu::new(&resources, &strings, &engine.declarations)?.draw(
                &resources,
                &mut surface,
                Some([100, 43]),
            )?,
            "preview-save-menu" | "preview-load-menu" => {
                use discworld_core::save_menu::{Entry, List, Mode, Scroll, View};
                let mode = if kind == "preview-save-menu" {
                    Mode::Save
                } else {
                    Mode::Load
                };
                let mut list = List::new(
                    mode,
                    (0..100)
                        .map(|id| Entry {
                            id,
                            description: format!("Checkpoint {id:02}").into_bytes(),
                        })
                        .take(if slot == 0 { 12 } else { 100 })
                        .collect(),
                )?;
                if slot == 1 {
                    list.scroll(Scroll::PageDown);
                } else if slot == 2 {
                    list.scroll(Scroll::End);
                }
                View::new(&resources, &strings, &engine.declarations, mode)?.draw(
                    &resources,
                    &mut surface,
                    &list,
                    Some([262, 69]),
                    None,
                )?;
            }
            "preview-controls-menu" | "preview-subtitles-menu" => {
                use discworld_core::settings_menu::{Kind, SettingsMenu};
                let controls = kind == "preview-controls-menu";
                let page = if controls {
                    Kind::Controls
                } else {
                    Kind::Subtitles
                };
                let mut menu = SettingsMenu::new(
                    &resources,
                    &strings,
                    &engine.declarations,
                    page,
                    if controls { 10 } else { 0 },
                    false,
                    false,
                )?;
                let point = if controls { [185, 120] } else { [172, 85] };
                menu.point_at(Some(point));
                if !menu.click(point, false) || menu.click(point, false) {
                    return Err("toggle busy audit failed".into());
                }
                let changes = (0..5)
                    .map(|_| menu.step_toggle(true, true))
                    .collect::<Vec<_>>();
                if changes != [None, None, Some(true), None, None] || !menu.enabled() {
                    return Err("toggle timing audit failed".into());
                }
                if controls
                    && (menu.click([172, 80], false)
                        || !menu.click([172, 80], true)
                        || !menu.flipped())
                {
                    return Err("double click test audit failed".into());
                }
                let thumb = if controls { [205, 53] } else { [178, 48] };
                if !menu.begin_drag(&resources, thumb)?
                    || menu.drag(0)?.is_some()
                    || menu.drag(1)? != Some(if controls { 10 } else { 1 })
                {
                    return Err("settings slider audit failed".into());
                }
                menu.end_drag()?;
                if menu.drag(1)?.is_some() {
                    return Err("settings slider remained grabbed".into());
                }
                settings_audit = Some(
                    serde_json::json!({"toggle_changes": changes, "enabled": menu.enabled(), "flipped": menu.flipped(), "slider_value": menu.value(), "slider_released": true}),
                );
                menu.draw(&resources, &mut surface)?;
            }
            "preview-sound-menu" => {
                let mut menu =
                    SoundMenu::new(&resources, &strings, &engine.declarations, [255, 128, 0])?;
                let initial = menu.clone();
                if !menu.begin_drag(&resources, [218, 93])? {
                    return Err("original effects slider thumb did not accept drag".into());
                }
                for (dx, expected) in [
                    (0, None),
                    (1, Some((1, 129))),
                    (100, Some((1, 255))),
                    (-60, None),
                    (-1, Some((1, 251))),
                ] {
                    if menu.drag(dx)? != expected {
                        return Err("original slider drag audit mismatch".into());
                    }
                }
                menu.end_drag()?;
                if menu.values() != [255, 251, 0] || menu.drag(1)?.is_some() {
                    return Err("original slider release audit mismatch".into());
                }
                sound_audit = Some(
                    serde_json::json!({"before": initial.values(), "after": menu.values(), "drag_origin": [218,93], "deltas": [0,1,100,-60,-1], "released": true}),
                );
                if slot == 0 {
                    menu = initial;
                }
                menu.draw(&resources, &mut surface)?;
            }
            "preview-restart-menu" | "preview-quit-menu" => {
                let confirmation = if kind == "preview-restart-menu" {
                    Confirmation::Restart
                } else {
                    Confirmation::Quit
                };
                ConfirmationMenu::new(&resources, &strings, &engine.declarations, confirmation)?
                    .draw(
                        &resources,
                        &mut surface,
                        Some([180, 90]),
                        match slot {
                            0 => None,
                            1 => Some((Choice::Accept, ButtonImage::Normal)),
                            _ => Some((Choice::Accept, ButtonImage::Down)),
                        },
                    )?;
            }
            _ => return Err("unknown menu preview".into()),
        }
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(output)?;
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), 320, 200);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(&surface.rgba)?;
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "tick": engine.ticks, "menu": kind, "preview_state": slot, "sound_drag_audit": sound_audit, "settings_audit": settings_audit,
                "strings": engine.declarations.config_strings.unwrap()[..8].iter()
                    .map(|h| strings.get(h.0).map(String::from_utf8_lossy)).collect::<Result<Vec<_>, _>>()?,
                "scope": "Original menu assets and palette rendering over startup; menu activation and actions not connected"
            }))?
        );
        return Ok(());
    }
    let declarations = &engine.declarations;
    let mut window = declarations.windows[slot]
        .clone()
        .ok_or("missing conversation window")?;
    if slot == 0 {
        window.contents.clone_from(&declarations.permanent_topics);
    }
    let parts = declarations.window_parts.ok_or("missing window parts")?;
    let frame = if slot == 0 {
        Frame::new(&resources, parts, &window)?
    } else {
        Frame::inventory(&resources, parts, &window)?
    };
    let strings = Strings::new(iso.file("/DISCWLD/ENGLISH.TXT").ok_or("missing text")?)?;
    let font = Font::read(&resources, declarations.tag_font.ok_or("missing tag font")?)?;
    let objects = inventory::read(&resources)?;
    let content = Content::new(
        &resources,
        &window,
        &frame.layout,
        &font,
        &strings,
        &objects,
        -1,
    )?;
    let mut surface = Surface::new(320, 200)?;
    frame.draw(&resources, &mut surface)?;
    content.draw(&resources, &mut surface)?;
    // Exercise every defined icon, including topics added by later scene scripts.
    let mut audited = 0;
    let mut indexed_frames = 0;
    for &id in objects.keys() {
        let mut one = window.clone();
        one.contents = vec![id];
        let mut icon = Content::new(
            &resources,
            &one,
            &frame.layout,
            &font,
            &strings,
            &objects,
            -1,
        )?;
        for _ in 0..24 {
            icon.tick(&resources)?;
            icon.draw(&resources, &mut Surface::new(320, 200)?)?;
            for topic in &icon.icons {
                if topic.animation.frame.0 == 0 {
                    continue;
                }
                for handle in discworld_core::film::frame(&resources, topic.animation.frame)? {
                    audit_indexed_image(&resources, handle)?;
                    indexed_frames += 1;
                }
            }
        }
        audited += 1;
    }
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), 320, 200);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&surface.rgba)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "window_slot": slot, "window_size": window.size, "title": window.title.0,
            "permanent_topics": window.contents, "frame_parts": frame.parts.len(),
        "inventory_icons_audited": audited, "ticks_per_icon": 24,
        "indexed_image_comparisons": indexed_frames,
            "scope": "Isolated declared window frame/title/contents; black canvas, no palette translucency, scroll thumb, repositioning, engine lifecycle, or independent reference comparison"
        }))?
    );
    Ok(())
}

fn audit_indexed_image(
    resources: &Resources<'_>,
    handle: discworld_core::graphics::Handle,
) -> Result<(), Box<dyn std::error::Error>> {
    use discworld_core::{
        graphics::{Image, decode_tiles, read_palette},
        indexed_surface::IndexedSurface,
    };
    let image = Image::read(resources, handle)?;
    let decoded = decode_tiles(resources, &image)?;
    let colors = read_palette(resources, image.palette)?;
    let rgba = decoded.rgba(&colors)?;
    let mut palette = [[0; 3]; 256];
    for (destination, source) in palette[1..].iter_mut().zip(colors) {
        *destination = source;
    }
    let mapping = std::array::from_fn(|i| i as u8);
    for flip in [[false, false], [true, true]] {
        let mut indexed = IndexedSurface::new(decoded.width, decoded.height)?;
        let mut direct = Surface::new(decoded.width, decoded.height)?;
        indexed.blit(&decoded, &mapping, [-1, 0], flip)?;
        direct.blit(decoded.width, decoded.height, &rgba, [-1, 0], flip)?;
        if indexed.present(&palette)? != direct {
            return Err("indexed and RGBA icon composition differ".into());
        }
    }
    Ok(())
}
