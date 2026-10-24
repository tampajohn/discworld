//! Opt-in integration checks using locally supplied original assets.
use super::BrowserGame;

// Preserve assertions about the older inventory/menu payload while checking
// the new independent cursor-process wrapper when it is present.
fn without_cursor_process(core: &[u8]) -> &[u8] {
    assert!(core.len() >= 80);
    if u32::from_le_bytes(core[8..12].try_into().unwrap()) != 27 {
        return core;
    }
    let mut length = 0usize;
    let mut offset = 80;
    for shift in (0..35).step_by(7) {
        let byte = core[offset];
        offset += 1;
        length |= usize::from(byte & 127) << shift;
        if byte & 128 == 0 {
            let inner = &core[offset..offset + length];
            assert!(inner.len() >= 80);
            assert!(u32::from_le_bytes(inner[8..12].try_into().unwrap()) < 27);
            return inner;
        }
    }
    panic!("invalid nested snapshot length");
}

fn inventory_schema(session: &[u8]) -> u32 {
    let length = u32::from_le_bytes(session[12..16].try_into().unwrap()) as usize;
    u32::from_le_bytes(without_cursor_process(&session[52..52 + length])[8..12].try_into().unwrap())
}

#[test]
#[ignore = "requires DISCWORLD_TEST_ISO and DISCWORLD_TEST_SOUNDFONT"]
fn original_menu_controller_transitions_render_and_resolve_live_selection() {
    use discworld_core::{
        menu_controller::{Assets, Controller, Settings},
        menu_navigation::{Page, Request},
        save_menu::{Entry, Target},
        text::Strings,
    };
    let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
    let mut game =
        BrowserGame::new(read("DISCWORLD_TEST_ISO"), read("DISCWORLD_TEST_SOUNDFONT")).unwrap();
    while game.tick() < 500 {
        game.advance((500 - game.tick()).min(24) as u32).unwrap();
    }
    let resources = game.assets.resources();
    let strings = Strings::new(&game.assets.iso[game.assets.text.clone()]).unwrap();
    let saves = [Entry {
        id: 7,
        description: b"Library".to_vec(),
    }];
    let assets = Assets {
        resources: &resources,
        strings: &strings,
        declarations: &game.engine.declarations,
        saves: &saves,
    };
    let settings = Settings {
        volumes: [255, 128, 0],
        double_click: 10,
        swap_buttons: false,
        text_speed: 50,
        subtitles: true,
        flipped: false,
    };
    let baseline = game.engine.render(&resources).unwrap();
    let bank = game.assets.bank().unwrap();
    let tracks =
        discworld_core::music::archive(&game.assets.iso[game.assets.music.clone()]).unwrap();
    let restore_menu = |original: &Controller| {
        let bytes = original.checkpoint().unwrap();
        let restored = Controller::from_checkpoint(&bytes, &assets).unwrap();
        assert_eq!(*original, restored);
        assert_eq!(restored.checkpoint().unwrap(), bytes);
        let mut before = baseline.clone();
        let mut after = baseline.clone();
        original.draw(&resources, &mut before).unwrap();
        restored.draw(&resources, &mut after).unwrap();
        assert_eq!(before.rgba, after.rgba);
        let mut engine = game.engine.clone();
        engine.configuration = Some(original.clone());
        let snapshot_bytes = discworld_core::save::Snapshot::capture(&engine, game.assets.identity)
            .to_bytes()
            .unwrap();
        assert_eq!(
            u32::from_le_bytes(without_cursor_process(&snapshot_bytes)[8..12].try_into().unwrap()),
            25 // Cursor trails wrap the world-hover, intro and menu checkpoints.
        );
        let snapshot =
            discworld_core::save::Snapshot::from_bytes(&snapshot_bytes, game.assets.identity)
                .unwrap();
        assert_eq!(snapshot.to_bytes().unwrap(), snapshot_bytes);
        assert!(snapshot.preview(&resources).is_err()); // never silently drop a menu
        assert_eq!(
            snapshot
                .preview_with_strings(&resources, &strings)
                .unwrap()
                .rgba,
            engine.render(&resources).unwrap().rgba
        );
        let bytes =
            discworld_audio::session::encode(&engine, &game.audio, game.assets.identity).unwrap();
        let session = discworld_audio::session::decode(
            &bytes,
            game.assets.identity,
            &resources,
            &bank,
            &tracks,
            &game.font,
            &strings,
        )
        .unwrap();
        assert_eq!(engine, session.engine);
        assert_eq!(
            session.engine.render(&resources).unwrap().rgba,
            engine.render(&resources).unwrap().rgba
        );
        assert_eq!(
            discworld_audio::session::encode(&session.engine, &session.audio, game.assets.identity)
                .unwrap(),
            bytes
        );
        restored
    };
    for (page, point) in [
        (Page::Main, [90, 44]),
        (Page::Save, [262, 69]),
        (Page::Load, [262, 69]),
        (Page::Restart, [170, 85]),
        (Page::Quit, [170, 85]),
        (Page::Controls, [176, 112]),
        (Page::Subtitles, [163, 77]),
        (Page::Sound, [218, 93]),
    ] {
        let mut original = Controller::new(settings.clone()).unwrap();
        original.open(page, &assets).unwrap();
        original.point_at(Some(point));
        original.click(point, false).unwrap();
        if page == Page::Sound {
            assert!(original.begin_drag(&resources, point).unwrap());
            original.drag_by([100, 0]).unwrap();
        }
        for step in 0..6 {
            let mut restored = restore_menu(&original);
            assert_eq!(
                restored.advance(&assets).unwrap(),
                original.advance(&assets).unwrap()
            );
            assert_eq!(restored, original);
            if step == 0 && matches!(page, Page::Subtitles | Page::Save) {
                original.escape(&assets).unwrap();
            }
        }
        restore_menu(&original);
    }
    for (page, point) in [
        (Page::Sound, [218, 93]),
        (Page::Controls, [205, 53]),
        (Page::Subtitles, [218, 48]),
        (Page::Save, [224, 50]),
        (Page::Load, [224, 50]),
    ] {
        let mut original = Controller::new(settings.clone()).unwrap();
        original.open(page, &assets).unwrap();
        original.point_at(Some(point));
        if !matches!(page, Page::Save | Page::Load) {
            let before = original.clone();
            assert!(
                original
                    .begin_drag(&discworld_core::graphics::Resources(vec![]), point)
                    .is_err()
            );
            assert_eq!(original, before);
        }
        assert!(!original.begin_drag(&resources, [0, 0]).unwrap());
        if page == Page::Controls {
            // The previous monitor pass can still own a toggle highlight when
            // a press arrives at the thumb; freeze it throughout the drag.
            original.point_at(Some([176, 112]));
        }
        assert!(original.begin_drag(&resources, point).unwrap());
        assert!(original.dragging());
        assert!(!original.begin_drag(&resources, point).unwrap());
        let frozen_hover = original.clone();
        original.point_at(None);
        assert_eq!(original, frozen_hover);
        for delta in [[1, 1], [300, 300], [-100, -100], [-250, -250], [20, 20]] {
            let mut restored = restore_menu(&original);
            restored.drag_by(delta).unwrap();
            original.drag_by(delta).unwrap();
            assert_eq!(restored, original);
        }
        let mut restored = restore_menu(&original);
        original.end_drag().unwrap();
        restored.end_drag().unwrap();
        assert_eq!(original, restored);
        assert!(!original.dragging());
        let released = original.clone();
        original.end_drag().unwrap();
        assert_eq!(original, released);
        restore_menu(&original);
    }
    let mut editing = Controller::new(settings.clone()).unwrap();
    editing.open(Page::Save, &assets).unwrap();
    assert!(!editing.edit(&resources, b' ', false).unwrap());
    assert!(!editing.edit(&resources, 0, false).unwrap());
    for byte in b"Rincewind's room" {
        assert!(editing.edit(&resources, *byte, false).unwrap());
    }
    assert!(editing.edit(&resources, 8, true).unwrap());
    assert_eq!(editing.list_mut().unwrap().draft(), b"Rincewind's roo");
    let before = editing.clone();
    assert!(
        editing
            .edit(&discworld_core::graphics::Resources(vec![]), b'W', false)
            .is_err()
    );
    assert_eq!(editing, before);
    restore_menu(&editing);
    for _ in 0..80 {
        editing.edit(&resources, b'W', false).unwrap();
    }
    assert!(editing.list_mut().unwrap().draft().len() <= 40);
    restore_menu(&editing);
    editing.close();
    editing.open(Page::Load, &assets).unwrap();
    let before = editing.clone();
    assert!(!editing.edit(&resources, b'X', false).unwrap());
    assert!(!editing.edit(&resources, 8, true).unwrap());
    assert_eq!(editing, before);
    assert!(editing.scroll(discworld_core::save_menu::Scroll::End));
    restore_menu(&editing);
    assert!(editing.scroll(discworld_core::save_menu::Scroll::Home));
    restore_menu(&editing);
    let mut checkpoint_probe = Controller::new(settings.clone()).unwrap();
    checkpoint_probe.open(Page::Main, &assets).unwrap();
    let bytes = checkpoint_probe.checkpoint().unwrap();
    for length in 0..bytes.len() {
        assert!(Controller::from_checkpoint(&bytes[..length], &assets).is_err());
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(Controller::from_checkpoint(&trailing, &assets).is_err());
    checkpoint_probe.close();
    checkpoint_probe.open(Page::Save, &assets).unwrap();
    checkpoint_probe.list_mut().unwrap().select(1).unwrap();
    if let Some(path) = std::env::var_os("DISCWORLD_TEST_MENU_SAVE_OUTPUT") {
        use std::io::Write;
        let mut engine = game.engine.clone();
        engine.configuration = Some(checkpoint_probe.clone());
        let bytes =
            discworld_audio::session::encode(&engine, &game.audio, game.assets.identity).unwrap();
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap()
            .write_all(&bytes)
            .unwrap();
        let mut rgba = path.clone();
        rgba.push(".rgba");
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(rgba)
            .unwrap()
            .write_all(&engine.render(&resources).unwrap().rgba)
            .unwrap();
    }
    let empty_catalog = Assets {
        saves: &[],
        resources: &resources,
        strings: &strings,
        declarations: assets.declarations,
    };
    assert_eq!(
        Controller::from_checkpoint(&checkpoint_probe.checkpoint().unwrap(), &empty_catalog)
            .unwrap(),
        checkpoint_probe
    );
    let invalid_declarations = discworld_core::startup::Declarations::default();
    let invalid_assets = Assets {
        declarations: &invalid_declarations,
        ..empty_catalog
    };
    assert!(
        Controller::from_checkpoint(&checkpoint_probe.checkpoint().unwrap(), &invalid_assets)
            .is_err()
    );
    // Rebuild logical widget checkpoints against the real font/window film,
    // including unsnapped drag positions and a toggle redrawing during a drag.
    for (kind, value, thumb, toggle) in [
        (
            discworld_core::settings_menu::Kind::Controls,
            10,
            [205, 53],
            [176, 112],
        ),
        (
            discworld_core::settings_menu::Kind::Subtitles,
            50,
            [218, 48],
            [163, 77],
        ),
    ] {
        use discworld_core::settings_menu::SettingsMenu;
        let mut widget = SettingsMenu::new(
            &resources,
            &strings,
            assets.declarations,
            kind,
            value,
            false,
            false,
        )
        .unwrap();
        let restore = |original: &SettingsMenu| {
            let state = original.state();
            state.validate().unwrap();
            let restored =
                SettingsMenu::from_state(&resources, &strings, assets.declarations, state).unwrap();
            assert_eq!(*original, restored);
            let mut before = baseline.clone();
            let mut after = baseline.clone();
            original.draw(&resources, &mut before).unwrap();
            restored.draw(&resources, &mut after).unwrap();
            assert_eq!(before.rgba, after.rgba);
            restored
        };
        assert!(widget.begin_drag(&resources, thumb).unwrap());
        for delta in [1, 100, -60, -1] {
            let mut copy = restore(&widget);
            assert_eq!(copy.drag(delta).unwrap(), widget.drag(delta).unwrap());
            assert_eq!(copy, widget);
            widget = restore(&widget);
        }
        widget.point_at(Some(toggle));
        assert!(widget.click(toggle, false));
        for _ in 0..5 {
            let mut copy = restore(&widget);
            assert_eq!(copy.step_toggle(true, true), widget.step_toggle(true, true));
            assert_eq!(copy, widget);
        }
        widget = restore(&widget);
        widget.end_drag().unwrap();
        restore(&widget);
    }
    {
        use discworld_core::configuration_menu::SoundMenu;
        let mut widget =
            SoundMenu::new(&resources, &strings, assets.declarations, [255, 128, 0]).unwrap();
        assert!(widget.begin_drag(&resources, [218, 93]).unwrap());
        for delta in [0, 1, 100, -60, -1] {
            let mut restored =
                SoundMenu::from_state(&resources, &strings, assets.declarations, widget.state())
                    .unwrap();
            assert_eq!(restored, widget);
            assert_eq!(restored.drag(delta).unwrap(), widget.drag(delta).unwrap());
            assert_eq!(restored, widget);
            let mut before = baseline.clone();
            let mut after = baseline.clone();
            widget.draw(&resources, &mut before).unwrap();
            restored.draw(&resources, &mut after).unwrap();
            assert_eq!(before.rgba, after.rgba);
        }
    }
    for (row, page) in [
        Some(Page::Load),
        Some(Page::Save),
        Some(Page::Restart),
        Some(Page::Sound),
        Some(Page::Controls),
        Some(Page::Subtitles),
        Some(Page::Quit),
        None,
    ]
    .into_iter()
    .enumerate()
    {
        let mut menu = Controller::new(settings.clone()).unwrap();
        assert!(menu.open(Page::Main, &assets).unwrap());
        assert!(menu.click_button([90, 44 + row as i32 * 15]));
        menu.step(&assets, true).unwrap();
        assert_eq!(menu.page(), page);
        assert_eq!(
            menu.take_cursor_warp(),
            match page {
                Some(Page::Save) => Some([262, 91]),
                Some(Page::Restart | Page::Quit) => Some([180, 90]),
                _ => None,
            }
        );
        assert_eq!(menu.take_cursor_warp(), None);
        let mut surface = baseline.clone();
        menu.draw(&resources, &mut surface).unwrap();
        assert_eq!(menu.take_cursor_warp(), None); // redraw never warps again
        assert_eq!(surface.rgba == baseline.rgba, page.is_none());
    }
    let mut menu = Controller::new(settings).unwrap();
    menu.open(Page::Save, &assets).unwrap();
    assert_eq!(menu.take_cursor_warp(), Some([262, 91]));
    assert!(menu.click_button([262, 69]));
    menu.step(&assets, true).unwrap();
    menu.click_row([50, 63], false).unwrap();
    for _ in 0..3 {
        assert_eq!(menu.step(&assets, true).unwrap().outcome.request, None);
    }
    let result = menu.step(&assets, true).unwrap();
    assert_eq!(result.outcome.request, Some(Request::Save));
    assert_eq!(
        result.selection,
        Some((Target::Existing(7), b"Library".to_vec()))
    );
    assert_eq!(menu.page(), None);

    // Reopening Main resets the shared selBox. The old queued function still
    // runs, but it must not accidentally save to a stale slot from its old page.
    menu.open(Page::Save, &assets).unwrap();
    menu.click_row([50, 63], false).unwrap();
    menu.click_button([262, 69]);
    menu.step(&assets, true).unwrap();
    assert!(menu.escape(&assets).unwrap().write_settings);
    assert_eq!(menu.take_cursor_warp(), None);
    let result = menu.step(&assets, false).unwrap();
    assert_eq!(result.outcome.request, Some(Request::Save));
    assert_eq!(result.selection, None);
    // Exercise controller-owned HL1, without an externally supplied existence
    // flag. A stationary pointer preserves effects; another box replaces them.
    let mut live = Controller::new(menu.settings()).unwrap();
    live.open(Page::Restart, &assets).unwrap();
    live.click_button([170, 85]);
    live.close();
    live.advance(&assets).unwrap(); // ButtonPress does not check inventoryActive
    let mut closed_effect = baseline.clone();
    live.draw(&resources, &mut closed_effect).unwrap();
    assert_ne!(closed_effect.rgba, baseline.rgba);
    live.point_at(None);
    assert_eq!(
        live.advance(&assets).unwrap().outcome.request,
        Some(Request::Restart)
    );
    live.open(Page::Restart, &assets).unwrap();
    live.point_at(Some([170, 85]));
    live.click_button([170, 85]);
    live.advance(&assets).unwrap();
    let mut normal = baseline.clone();
    live.draw(&resources, &mut normal).unwrap();
    live.point_at(Some([170, 85]));
    let mut stationary = baseline.clone();
    live.draw(&resources, &mut stationary).unwrap();
    assert_eq!(stationary.rgba, normal.rgba);
    live.point_at(Some([130, 85]));
    let mut other = baseline.clone();
    live.draw(&resources, &mut other).unwrap();
    assert_ne!(other.rgba, normal.rgba);
    assert_eq!(live.advance(&assets).unwrap().outcome.request, None);
    live.point_at(None);
    assert_eq!(live.advance(&assets).unwrap().outcome.request, None); // second depressed tick
    assert_eq!(
        live.advance(&assets).unwrap().outcome.request,
        Some(Request::Restart)
    );

    live.open(Page::Subtitles, &assets).unwrap();
    let enabled = live.settings().subtitles;
    live.point_at(Some([163, 77]));
    live.click_settings([163, 77], false);
    live.advance(&assets).unwrap();
    live.escape(&assets).unwrap();
    live.point_at(Some([90, 44]));
    live.advance(&assets).unwrap(); // retained toggle replaces Main's text highlight
    live.point_at(Some([90, 59])); // a different text highlight replaces the effect
    live.advance(&assets).unwrap(); // any HL1 suffices: the original setting flips
    assert_eq!(live.settings().subtitles, !enabled);
    assert_eq!(live.page(), Some(Page::Main));
    live.point_at(None);
    live.advance(&assets).unwrap();
    assert!(live.click_button([90, 44]));
    live.advance(&assets).unwrap();
    assert_eq!(live.page(), Some(Page::Load));
    menu.open(Page::Controls, &assets).unwrap();
    let view = menu.settings_mut().unwrap();
    view.point_at(Some([176, 112]));
    view.click([176, 112], false);
    view.step_toggle(true, true); // removes highlight
    view.point_at(Some([163, 72])); // move to non-highlighted FLIP box
    view.step_toggle(true, false); // installs depressed toggle graphic
    let mut before = baseline.clone();
    view.draw(&resources, &mut before).unwrap();
    view.point_at(Some([163, 72])); // stationary FLIP must not remove that graphic
    let mut after = baseline.clone();
    view.draw(&resources, &mut after).unwrap();
    assert_eq!(before.rgba, after.rgba);
    view.point_at(None); // outside always deletes HL1
    let mut outside = baseline.clone();
    view.draw(&resources, &mut outside).unwrap();
    assert_ne!(outside.rgba, after.rgba);
    assert_eq!(view.step_toggle(true, false), None); // abort without changing value
    menu.close();
    // A toggle retains its original setting pointer across Escape-to-Main.
    // It blocks new button functions until the inventory coroutine completes.
    menu.open(Page::Subtitles, &assets).unwrap();
    let enabled = menu.settings().subtitles;
    assert!(menu.click_settings([163, 77], false));
    menu.step(&assets, true).unwrap(); // normal image, first sleep
    menu.escape(&assets).unwrap();
    assert!(!menu.click_button([90, 44]));
    let mut normal = baseline.clone();
    menu.draw(&resources, &mut normal).unwrap();
    menu.step(&assets, false).unwrap(); // Main active: creates depressed image
    let mut depressed = baseline.clone();
    menu.draw(&resources, &mut depressed).unwrap();
    assert_ne!(normal.rgba, depressed.rgba);
    menu.step(&assets, true).unwrap(); // HL1 exists: changes original subtitles
    assert_eq!(menu.settings().subtitles, !enabled);
    assert_eq!(menu.page(), Some(Page::Main));
    menu.step(&assets, true).unwrap();
    menu.step(&assets, false).unwrap();
    assert!(menu.click_button([90, 44]));
    menu.step(&assets, true).unwrap();
    assert_eq!(menu.page(), Some(Page::Load));
    // Fully closing the window before the first sleep completes aborts it.
    menu.close();
    menu.open(Page::Subtitles, &assets).unwrap();
    let enabled = menu.settings().subtitles;
    menu.click_settings([163, 77], false);
    menu.step(&assets, true).unwrap();
    menu.close();
    menu.step(&assets, false).unwrap();
    assert_eq!(menu.settings().subtitles, enabled);

    // A close before the host consumes the warp must not move the world cursor
    // to a now-hidden save window on its next presentation update.
    menu.open(Page::Save, &assets).unwrap();
    menu.click_row([50, 48], true).unwrap();
    assert_eq!(menu.take_cursor_warp(), None);

    menu.open(Page::Controls, &assets).unwrap();
    assert!(menu.click_settings([176, 112], false));
    assert!(!menu.click_settings([176, 112], false)); // shared effect already busy
    assert!(menu.click_settings([163, 72], true)); // FLIP remains immediate
    for _ in 0..3 {
        menu.step(&assets, true).unwrap();
    }
    menu.escape(&assets).unwrap();
    assert!(menu.settings().swap_buttons && menu.settings().flipped);
    assert!(!menu.click_button([90, 44])); // still owns the shared effect slot
    menu.step(&assets, false).unwrap(); // removed HL1 ends the remaining effect
    menu.close();
    menu.open(Page::Controls, &assets).unwrap();
    assert!(menu.settings_mut().unwrap().enabled());

    // Failed resource construction must preserve both navigation and view.
    menu.close();
    menu.open(Page::Main, &assets).unwrap();
    menu.click_button([90, 44]);
    let before = menu.clone();
    let invalid = discworld_core::startup::Declarations::default();
    let bad_assets = Assets {
        declarations: &invalid,
        ..assets
    };
    assert!(menu.step(&bad_assets, true).is_err());
    assert_eq!(menu, before);
    menu.step(&assets, true).unwrap();
    assert_eq!(menu.page(), Some(Page::Load));

    // A pending graphic function can survive closure and opening another page.
    // It owns the same effect slot as settings toggles, but not immediate FLIP.
    menu.close();
    menu.open(Page::Save, &assets).unwrap();
    menu.click_button([262, 69]);
    menu.step(&assets, true).unwrap();
    menu.close();
    menu.open(Page::Controls, &assets).unwrap();
    assert!(!menu.click_settings([176, 112], false));
    assert!(menu.click_settings([163, 72], true));
    let result = menu.step(&assets, false).unwrap();
    assert_eq!(result.outcome.request, Some(Request::Save));
    assert_eq!(result.selection, None);
}

#[test]
#[ignore = "requires DISCWORLD_TEST_ISO and DISCWORLD_TEST_SOUNDFONT"]
fn startup_menu_key_restores_original_cursor_and_resumes_deterministically() {
    use discworld_core::menu_navigation::Key;
    let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
    let mut game =
        BrowserGame::new(read("DISCWORLD_TEST_ISO"), read("DISCWORLD_TEST_SOUNDFONT")).unwrap();
    while game.tick() < 500 {
        game.advance((500 - game.tick()).min(24) as u32).unwrap();
    }
    assert!(game.engine.menu_enabled);
    assert!(game.engine.declarations.cursor.is_some());
    let before = game.save().unwrap();
    assert!(
        game.engine
            .configuration_key(&game.assets.resources(), Key::Main, false)
            .unwrap()
    );
    assert!(game.engine.control.enabled);
    assert!(!game.engine.menu_enabled);
    let activated = game.save().unwrap();
    let frame = game.frame().unwrap();
    game.load(&before).unwrap();
    assert!(
        game.engine
            .configuration_key(&game.assets.resources(), Key::Main, false)
            .unwrap()
    );
    assert_eq!(game.save().unwrap(), activated);
    assert_eq!(game.frame().unwrap(), frame);
    roundtrip_pending(&mut game, 24);
}

#[test]
#[ignore = "requires DISCWORLD_TEST_ISO and DISCWORLD_TEST_SOUNDFONT"]
fn engine_menu_opening_preserves_gates_warps_and_atomicity() {
    use discworld_core::{
        menu_controller::{Controller, Settings},
        menu_navigation::Key,
        text::Strings,
    };
    let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
    let mut game =
        BrowserGame::new(read("DISCWORLD_TEST_ISO"), read("DISCWORLD_TEST_SOUNDFONT")).unwrap();
    while game.tick() < 500 {
        game.advance((500 - game.tick()).min(24) as u32).unwrap();
    }
    game.engine.configuration = Some(
        Controller::new(Settings {
            volumes: [255; 3],
            double_click: 10,
            swap_buttons: false,
            text_speed: 50,
            subtitles: true,
            flipped: false,
        })
        .unwrap(),
    );
    let resources = game.assets.resources();
    let strings = Strings::new(&game.assets.iso[game.assets.text.clone()]).unwrap();
    let initial = game.engine.clone();
    let mut broken = initial.clone();
    assert!(
        broken
            .open_configuration(
                &discworld_core::graphics::Resources(vec![]),
                &strings,
                &[],
                Key::Main
            )
            .is_err()
    );
    assert_eq!(broken, initial);
    let mut checkpoints = Vec::new();
    for key in [Key::Main, Key::Save, Key::Load, Key::Quit] {
        let mut engine = initial.clone();
        if key != Key::Main {
            // Only Main can turn control on at the ENABLEMENU startup gate.
            assert!(
                !engine
                    .open_configuration(&resources, &strings, &[], key)
                    .unwrap()
            );
            assert!(engine.configuration.as_ref().unwrap().page().is_none());
            engine
                .configuration_key(&resources, Key::Main, false)
                .unwrap();
        }
        let cursor = engine.control.cursor;
        assert!(
            engine
                .open_configuration(&resources, &strings, &[], key)
                .unwrap()
        );
        assert_eq!(
            engine.configuration.as_ref().unwrap().page(),
            Some(key.page())
        );
        assert!(!engine.control.tags_enabled);
        assert_eq!(
            engine.control.cursor,
            match key {
                Key::Save => [262, 91],
                Key::Quit => [180, 90],
                _ => cursor,
            }
        );
        assert_eq!(
            engine.configuration.as_mut().unwrap().take_cursor_warp(),
            None
        );
        let controller = engine.configuration.clone();
        assert!(
            !engine
                .open_configuration(&resources, &strings, &[], Key::Main)
                .unwrap()
        );
        assert_eq!(engine.configuration, controller);
        game.engine = engine;
        let saved = game.save().unwrap();
        let frame = game.frame().unwrap();
        checkpoints.push((saved, frame));
    }
    for (saved, frame) in checkpoints {
        game.load(&saved).unwrap();
        assert_eq!(game.save().unwrap(), saved);
        assert_eq!(game.frame().unwrap(), frame);
        let settings = game.engine.configuration.as_ref().unwrap().settings();
        game.engine.close_inventory();
        assert!(game.engine.configuration.as_ref().unwrap().page().is_none());
        assert!(game.engine.control.tags_enabled);
        assert_eq!(
            game.engine.configuration.as_ref().unwrap().settings(),
            settings
        );
        let closed = game.save().unwrap();
        game.load(&closed).unwrap();
        assert_eq!(game.save().unwrap(), closed);
    }
    let resources = game.assets.resources();
    let strings = Strings::new(&game.assets.iso[game.assets.text.clone()]).unwrap();
    let mut engine = initial;
    engine
        .open_configuration(&resources, &strings, &[], Key::Main)
        .unwrap();
    assert!(
        engine
            .configuration_click([90, 59], discworld_core::engine::PolygonInteraction::Look)
            .unwrap()
            .outcome
            .request
            .is_none()
    );
    let pending = engine.clone();
    let mut scheduled = pending.clone();
    let mut manual = pending.clone();
    let bank = game.assets.bank().unwrap();
    let tracks =
        discworld_core::music::archive(&game.assets.iso[game.assets.music.clone()]).unwrap();
    let expected_dispatch = manual
        .advance_configuration(&resources, &strings, &[])
        .unwrap();
    let expected_status = manual
        .tick_content(&resources, Some(&bank), Some(&tracks), Some(&strings))
        .unwrap();
    let result = scheduled
        .tick_with_configuration(&resources, Some(&bank), Some(&tracks), &strings, &[])
        .unwrap();
    assert_eq!(result, (expected_status, expected_dispatch));
    assert_eq!(scheduled, manual);
    // A later failure rolls back the menu transition together with the cycle.
    let before = pending.clone();
    let mut failed = before.clone();
    assert!(
        failed
            .tick_with_configuration(
                &discworld_core::graphics::Resources(vec![]),
                Some(&bank),
                Some(&tracks),
                &strings,
                &[]
            )
            .is_err()
    );
    assert_eq!(failed, before);
    assert!(
        engine
            .advance_configuration(&discworld_core::graphics::Resources(vec![]), &strings, &[])
            .is_err()
    );
    assert_eq!(engine, pending);
    let opened = engine
        .advance_configuration(&resources, &strings, &[])
        .unwrap();
    assert!(opened.outcome.changed);
    assert_eq!(
        engine.configuration.as_ref().unwrap().page(),
        Some(discworld_core::menu_navigation::Page::Save)
    );
    assert_eq!(engine.control.cursor, [262, 91]);
    assert!(!engine.control.tags_enabled);
    let before_escape = engine.clone();
    assert!(
        engine
            .escape_input(&discworld_core::graphics::Resources(vec![]), &strings, &[])
            .is_err()
    );
    assert_eq!(engine, before_escape);
    let escaped = engine.escape_input(&resources, &strings, &[]).unwrap();
    assert!(escaped.changed && escaped.write_settings);
    assert!(!engine.control.tags_enabled);
    assert_eq!(
        engine.configuration.as_ref().unwrap().page(),
        Some(discworld_core::menu_navigation::Page::Main)
    );
    let escaped = engine.escape_input(&resources, &strings, &[]).unwrap();
    assert!(escaped.changed && !escaped.write_settings);
    assert!(engine.control.tags_enabled);
    assert!(engine.configuration.as_ref().unwrap().page().is_none());

    // A button function remains pending even when CLOSEINVENTORY removes its window.
    engine
        .open_configuration(&resources, &strings, &[], Key::Save)
        .unwrap();
    assert!(
        engine
            .configuration
            .as_mut()
            .unwrap()
            .click_button([262, 69])
    );
    let mut scene_changed = engine.clone();
    let scene = scene_changed.capture_scene().unwrap();
    let tracks =
        discworld_core::music::archive(&game.assets.iso[game.assets.music.clone()]).unwrap();
    scene_changed
        .rebuild_scene(&resources, &tracks, &scene)
        .unwrap();
    assert!(
        scene_changed
            .configuration
            .as_ref()
            .unwrap()
            .page()
            .is_none()
    );
    assert!(scene_changed.control.tags_enabled);
    engine.close_inventory();
    assert_eq!(scene_changed.configuration, engine.configuration);
    let mut requests = Vec::new();
    for _ in 0..6 {
        let snapshot = discworld_core::save::Snapshot::capture(&engine, game.assets.identity)
            .to_bytes()
            .unwrap();
        let mut restored =
            discworld_core::save::Snapshot::from_bytes(&snapshot, game.assets.identity)
                .unwrap()
                .restore_with_strings(
                    &resources,
                    &game.assets.bank().unwrap(),
                    &discworld_core::music::archive(&game.assets.iso[game.assets.music.clone()])
                        .unwrap(),
                    &strings,
                )
                .unwrap();
        let dispatch = engine
            .advance_configuration(&resources, &strings, &[])
            .unwrap();
        assert_eq!(
            scene_changed
                .advance_configuration(&resources, &strings, &[])
                .unwrap(),
            dispatch
        );
        assert_eq!(scene_changed.configuration, engine.configuration);
        assert_eq!(
            restored
                .advance_configuration(&resources, &strings, &[])
                .unwrap(),
            dispatch
        );
        assert_eq!(restored, engine);
        if let Some(request) = dispatch.outcome.request {
            requests.push(request);
            assert_eq!(
                dispatch.selection,
                Some((discworld_core::save_menu::Target::New, Vec::new()))
            );
        }
        assert!(engine.control.tags_enabled);
    }
    assert_eq!(
        requests,
        vec![discworld_core::menu_navigation::Request::Save]
    );
    use discworld_core::{
        engine::PolygonInteraction as Gesture,
        menu_navigation::{Page, Request},
        save_menu::{Entry, Target},
    };
    let catalog = [Entry {
        id: 7,
        description: b"Library".to_vec(),
    }];
    engine
        .open_configuration(&resources, &strings, &catalog, Key::Save)
        .unwrap();
    engine.configuration_click([50, 63], Gesture::Look).unwrap();
    let save = engine
        .configuration_click([50, 48], Gesture::Action)
        .unwrap();
    assert_eq!(save.outcome.request, Some(Request::Save));
    assert_eq!(
        save.selection,
        Some((Target::Existing(7), b"Library".to_vec()))
    );
    assert!(engine.control.tags_enabled);
    assert!(engine.configuration.as_ref().unwrap().page().is_none());
    assert!(
        engine
            .advance_configuration(&resources, &strings, &catalog)
            .unwrap()
            .outcome
            .request
            .is_none()
    );
    engine
        .open_configuration(&resources, &strings, &catalog, Key::Load)
        .unwrap();
    engine
        .configuration_click([224, 181], Gesture::WalkTo)
        .unwrap();
    assert_eq!(
        engine
            .configuration
            .as_mut()
            .unwrap()
            .list_mut()
            .unwrap()
            .first(),
        1
    );
    engine
        .configuration_click([224, 43], Gesture::Action)
        .unwrap();
    assert_eq!(
        engine
            .configuration
            .as_mut()
            .unwrap()
            .list_mut()
            .unwrap()
            .first(),
        0
    );
    engine.configuration_click([0, 0], Gesture::Action).unwrap();
    assert_eq!(
        engine.configuration.as_ref().unwrap().page(),
        Some(Page::Load)
    );
    engine.control.enabled = false;
    engine.configuration_click([0, 0], Gesture::Look).unwrap();
    assert_eq!(
        engine.configuration.as_ref().unwrap().page(),
        Some(Page::Load)
    );
    engine.control.enabled = true;
    let closed = engine.configuration_click([0, 0], Gesture::Look).unwrap();
    assert!(closed.outcome.changed && !closed.outcome.write_settings);
    assert!(engine.configuration.as_ref().unwrap().page().is_none());
    assert!(engine.control.tags_enabled);
    use discworld_core::mouse::Button;
    engine
        .open_configuration(&resources, &strings, &catalog, Key::Main)
        .unwrap();
    engine
        .move_cursor_with_configuration(&resources, [90, 59])
        .unwrap();
    engine
        .mouse_button_with_configuration(&resources, Button::Right, true)
        .unwrap();
    engine
        .mouse_button_with_configuration(&resources, Button::Right, false)
        .unwrap();
    engine
        .advance_configuration(&resources, &strings, &catalog)
        .unwrap();
    assert_eq!(
        engine.configuration.as_ref().unwrap().page(),
        Some(Page::Save)
    );
    engine
        .move_cursor_with_configuration(&resources, [50, 63])
        .unwrap();
    assert!(
        engine
            .mouse_button_with_configuration(&resources, Button::Left, true)
            .unwrap()
            .outcome
            .request
            .is_none()
    );
    engine
        .mouse_button_with_configuration(&resources, Button::Left, false)
        .unwrap();
    engine
        .move_cursor_with_configuration(&resources, [50, 48])
        .unwrap();
    let saved = engine
        .mouse_button_with_configuration(&resources, Button::Left, true)
        .unwrap();
    assert_eq!(saved.outcome.request, Some(Request::Save));
    assert_eq!(
        saved.selection,
        Some((Target::Existing(7), b"Library".to_vec()))
    );
    engine
        .mouse_button_with_configuration(&resources, Button::Left, false)
        .unwrap();
    engine
        .open_configuration(&resources, &strings, &catalog, Key::Main)
        .unwrap();
    engine
        .configuration_click([90, 104], Gesture::WalkTo)
        .unwrap();
    engine
        .advance_configuration(&resources, &strings, &catalog)
        .unwrap();
    assert_eq!(
        engine.configuration.as_ref().unwrap().page(),
        Some(Page::Controls)
    );
    engine
        .move_cursor_with_configuration(&resources, [205, 53])
        .unwrap();
    engine
        .mouse_button_with_configuration(&resources, Button::Left, true)
        .unwrap();
    assert!(engine.configuration.as_ref().unwrap().dragging());
    let before = engine.clone();
    assert!(
        engine
            .move_cursor_with_configuration(&resources, [i32::MIN, 53])
            .is_err()
    );
    assert_eq!(engine, before);
    engine
        .move_cursor_with_configuration(&resources, [230, 53])
        .unwrap();
    assert!(
        engine
            .configuration
            .as_ref()
            .unwrap()
            .settings()
            .double_click
            > 10
    );
    let held = discworld_core::save::Snapshot::capture(&engine, game.assets.identity)
        .to_bytes()
        .unwrap();
    let mut restored = discworld_core::save::Snapshot::from_bytes(&held, game.assets.identity)
        .unwrap()
        .restore_with_strings(&resources, &game.assets.bank().unwrap(), &tracks, &strings)
        .unwrap();
    // Release is admitted even if a script took control during the drag.
    for state in [&mut engine, &mut restored] {
        state.control.enabled = false;
        state
            .mouse_button_with_configuration(&resources, Button::Left, false)
            .unwrap();
        assert!(!state.configuration.as_ref().unwrap().dragging());
    }
    assert_eq!(engine, restored);
    engine.control.enabled = true;
    engine
        .move_cursor_with_configuration(&resources, [176, 112])
        .unwrap();
    engine
        .configuration_click([176, 112], Gesture::WalkTo)
        .unwrap();
    for _ in 0..6 {
        engine
            .advance_configuration(&resources, &strings, &catalog)
            .unwrap();
    }
    let settings = engine.configuration.as_ref().unwrap().settings();
    assert!(settings.swap_buttons);
    let thumb = [
        178 + discworld_core::menu_slider::offset(settings.double_click, 30).unwrap(),
        53,
    ];
    engine
        .move_cursor_with_configuration(&resources, thumb)
        .unwrap();
    engine
        .mouse_button_with_configuration(&resources, Button::Right, true)
        .unwrap();
    assert!(engine.configuration.as_ref().unwrap().dragging());
    engine
        .mouse_button_with_configuration(&resources, Button::Right, false)
        .unwrap();
    assert!(!engine.configuration.as_ref().unwrap().dragging());
    // A complete cycle dispatches a queued confirmation once, including across saves.
    engine.close_inventory();
    engine
        .open_configuration(&resources, &strings, &catalog, Key::Save)
        .unwrap();
    engine
        .move_cursor_with_configuration(&resources, [262, 69])
        .unwrap();
    engine
        .configuration_click([262, 69], Gesture::WalkTo)
        .unwrap();
    let mut requests = Vec::new();
    for _ in 0..6 {
        let snapshot = discworld_core::save::Snapshot::capture(&engine, game.assets.identity)
            .to_bytes()
            .unwrap();
        let mut restored =
            discworld_core::save::Snapshot::from_bytes(&snapshot, game.assets.identity)
                .unwrap()
                .restore_with_strings(&resources, &bank, &tracks, &strings)
                .unwrap();
        let actual = engine
            .tick_with_configuration(&resources, Some(&bank), Some(&tracks), &strings, &catalog)
            .unwrap();
        assert_eq!(
            restored
                .tick_with_configuration(&resources, Some(&bank), Some(&tracks), &strings, &catalog)
                .unwrap(),
            actual
        );
        assert_eq!(engine, restored);
        if let Some(request) = actual.1.outcome.request {
            requests.push(request);
        }
    }
    assert_eq!(requests, vec![Request::Save]);
    let mut subtitle_probe = engine.clone();
    subtitle_probe.control.enabled = true;
    subtitle_probe
        .open_configuration(&resources, &strings, &catalog, Key::Main)
        .unwrap();
    subtitle_probe
        .configuration_click([90, 119], Gesture::WalkTo)
        .unwrap();
    subtitle_probe
        .advance_configuration(&resources, &strings, &catalog)
        .unwrap();
    assert_eq!(
        subtitle_probe.configuration.as_ref().unwrap().page(),
        Some(Page::Subtitles)
    );
    let before_subtitles = subtitle_probe.subtitles;
    subtitle_probe
        .move_cursor_with_configuration(&resources, [163, 77])
        .unwrap();
    subtitle_probe
        .configuration_click([163, 77], Gesture::WalkTo)
        .unwrap();
    for _ in 0..6 {
        subtitle_probe
            .advance_configuration(&resources, &strings, &catalog)
            .unwrap();
        assert_eq!(
            subtitle_probe.subtitles,
            subtitle_probe
                .configuration
                .as_ref()
                .unwrap()
                .settings()
                .subtitles
        );
    }
    assert_ne!(subtitle_probe.subtitles, before_subtitles);
    engine.close_inventory();
    engine.control.enabled = true;
    engine
        .open_configuration(&resources, &strings, &catalog, Key::Main)
        .unwrap();
    engine
        .configuration_click([90, 74], Gesture::WalkTo)
        .unwrap();
    engine
        .tick_with_configuration(&resources, Some(&bank), Some(&tracks), &strings, &catalog)
        .unwrap();
    assert_eq!(
        engine.configuration.as_ref().unwrap().page(),
        Some(Page::Restart)
    );
    // This diagnostic runs alongside startup scripts, which may take control
    // during navigation; isolate confirmation dispatch from that cutscene gate.
    engine.control.enabled = true;
    engine
        .move_cursor_with_configuration(&resources, [170, 85])
        .unwrap();
    engine
        .configuration_click([170, 85], Gesture::WalkTo)
        .unwrap();
    let settings = engine.configuration.as_ref().unwrap().settings();
    let before_tick = engine.ticks;
    let before_clock = engine.music.clock;
    let mut restarts = 0;
    for _ in 0..6 {
        let snapshot = discworld_core::save::Snapshot::capture(&engine, game.assets.identity)
            .to_bytes()
            .unwrap();
        let mut restored =
            discworld_core::save::Snapshot::from_bytes(&snapshot, game.assets.identity)
                .unwrap()
                .restore_with_strings(&resources, &bank, &tracks, &strings)
                .unwrap();
        let actual = engine
            .tick_with_configuration(&resources, Some(&bank), Some(&tracks), &strings, &catalog)
            .unwrap();
        assert_eq!(
            restored
                .tick_with_configuration(&resources, Some(&bank), Some(&tracks), &strings, &catalog)
                .unwrap(),
            actual
        );
        assert_eq!(engine, restored);
        if actual.1.outcome.request == Some(Request::Restart) {
            restarts += 1;
            assert!(engine.has_restarted());
            assert!(engine.configuration.as_ref().unwrap().page().is_none());
            assert!(!engine.music.playing());
            assert_eq!(engine.sample_player.id, None);
        }
        assert_eq!(engine.configuration.as_ref().unwrap().settings(), settings);
    }
    assert_eq!(restarts, 1);
    assert_eq!(engine.ticks, before_tick + 6);
    assert_eq!(engine.music.clock, before_clock + 30);
}

fn roundtrip_pending(game: &mut BrowserGame, ticks: u32) {
    let pending = game.save().unwrap();
    game.load(&pending).unwrap();
    assert_eq!(game.save().unwrap(), pending);
    let audio = game.advance(ticks).unwrap();
    let frame = game.frame().unwrap();
    let continued = game.save().unwrap();
    game.load(&pending).unwrap();
    assert_eq!(game.advance(ticks).unwrap(), audio);
    assert_eq!(game.frame().unwrap(), frame);
    assert_eq!(game.save().unwrap(), continued);
}

#[test]
#[ignore = "requires DISCWORLD_TEST_ISO, DISCWORLD_TEST_SOUNDFONT and DISCWORLD_TEST_SAVE (tick 33200 inventory checkpoint)"]
fn inventory_pickup_and_drop_resume_pending_physical_clicks() {
    let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
    let mut game =
        BrowserGame::new(read("DISCWORLD_TEST_ISO"), read("DISCWORLD_TEST_SOUNDFONT")).unwrap();
    game.load(&read("DISCWORLD_TEST_SAVE")).unwrap();
    assert_eq!(game.tick(), 33200);
    assert_eq!(game.engine.held_object, -1);
    let slot = game.engine.active_inventory().unwrap() as usize;
    let item = game.engine.declarations.windows[slot]
        .as_ref()
        .unwrap()
        .contents[0];
    let [x, y] = game.engine.inventory_cell_position(0).unwrap();
    println!("Inventory {slot}, item {item}, cell center ({x}, {y})");
    game.move_pointer(x, y);
    game.input("left-down").unwrap();
    game.input("left-up").unwrap();
    assert_eq!(game.engine.held_object, -1);
    // Exercise a queued script and then its suspended double-click wait.
    roundtrip_pending(&mut game, 1);
    assert_eq!(game.engine.held_object, -1);
    roundtrip_pending(&mut game, 24);
    assert_eq!(game.engine.held_object, item);

    game.input("left-down").unwrap();
    game.input("left-up").unwrap();
    assert_eq!(game.engine.held_object, item);
    // Ordinary putdown uses a native task rather than an inventory script.
    roundtrip_pending(&mut game, 1);
    assert_eq!(game.engine.held_object, item);
    roundtrip_pending(&mut game, 24);
    assert_eq!(game.engine.held_object, -1);
    assert!(
        game.engine.declarations.windows[slot]
            .as_ref()
            .unwrap()
            .contents
            .contains(&item)
    );
}

#[test]
#[ignore = "requires DISCWORLD_TEST_ISO, DISCWORLD_TEST_SOUNDFONT and DISCWORLD_TEST_SAVE (tick 33200 inventory checkpoint)"]
fn inventory_frame_edge_does_not_close_window() {
    let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
    let mut game =
        BrowserGame::new(read("DISCWORLD_TEST_ISO"), read("DISCWORLD_TEST_SOUNDFONT")).unwrap();
    game.load(&read("DISCWORLD_TEST_SAVE")).unwrap();
    let slot = game.engine.active_inventory().unwrap();
    let frame = discworld_core::conversation_window::Frame::inventory(
        &game.assets.resources(),
        game.engine.declarations.window_parts.unwrap(),
        game.engine.declarations.windows[slot as usize]
            .as_ref()
            .unwrap(),
    )
    .unwrap();
    let [x, y, width, height] = frame.layout.background;
    let initial = game.save().unwrap();
    for point in [[x + width + 1, y + 30], [x + 20, y + height + 1]] {
        game.load(&initial).unwrap();
        assert!(game.engine.inventory_contains_point(point));
        game.move_pointer(point[0], point[1]);
        game.input("left-down").unwrap();
        game.input("left-up").unwrap();
        game.advance(24).unwrap();
        assert_eq!(game.engine.active_inventory(), Some(slot));
        assert_eq!(game.engine.held_object, -1);
    }
    game.load(&initial).unwrap();
    game.move_pointer(x + width + 2, y + 30);
    game.input("left-down").unwrap();
    assert_eq!(game.engine.active_inventory(), None);
}

#[test]
#[ignore = "requires DISCWORLD_TEST_ISO, DISCWORLD_TEST_SOUNDFONT and DISCWORLD_TEST_SAVE (tick 33200 inventory checkpoint)"]
fn inventory_header_drag_moves_immediately_and_survives_save() {
    let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
    let mut game =
        BrowserGame::new(read("DISCWORLD_TEST_ISO"), read("DISCWORLD_TEST_SOUNDFONT")).unwrap();
    game.load(&read("DISCWORLD_TEST_SAVE")).unwrap();
    game.move_pointer(140, 110);
    let before = game.frame().unwrap();
    let contents = game.engine.declarations.windows[1]
        .as_ref()
        .unwrap()
        .contents
        .clone();
    game.input("left-down").unwrap();
    assert!(game.inventory_dragging());
    game.move_pointer(170, 130);
    assert_eq!(
        game.engine.declarations.windows[1]
            .as_ref()
            .unwrap()
            .position,
        [130, 120]
    );
    assert_eq!(game.engine.inventory_cell_position(0), Some([148, 152]));
    // Moving back before a tick restores the complete frame, including all
    // original text, icon frames and background. No reconstruction occurred.
    game.move_pointer(140, 110);
    assert_eq!(game.frame().unwrap(), before);
    game.move_pointer(170, 130);
    let pending = game.save().unwrap();
    assert_eq!(inventory_schema(&pending), 5);
    game.load(&pending).unwrap();
    assert_eq!(game.save().unwrap(), pending);
    assert!(game.inventory_dragging());
    game.move_pointer(180, 120);
    game.input("left-up").unwrap();
    assert!(!game.inventory_dragging());
    assert_eq!(
        game.engine.declarations.windows[1]
            .as_ref()
            .unwrap()
            .position,
        [140, 110]
    );
    let audio = game.advance(24).unwrap();
    let frame = game.frame().unwrap();
    let continued = game.save().unwrap();
    assert_eq!(inventory_schema(&continued), 4);
    game.load(&pending).unwrap();
    game.move_pointer(180, 120);
    game.input("left-up").unwrap();
    assert_eq!(game.advance(24).unwrap(), audio);
    assert_eq!(game.frame().unwrap(), frame);
    assert_eq!(game.save().unwrap(), continued);
    game.move_pointer(20, 20);
    assert_eq!(
        game.engine.declarations.windows[1]
            .as_ref()
            .unwrap()
            .position,
        [140, 110]
    );
    assert_eq!(
        game.engine.declarations.windows[1]
            .as_ref()
            .unwrap()
            .contents,
        contents
    );
    assert_eq!(game.engine.held_object, -1);

    game.load(&pending).unwrap();
    game.input("close").unwrap();
    assert!(!game.inventory_dragging());
    game.save().unwrap();
}

#[test]
#[ignore = "requires DISCWORLD_TEST_ISO, DISCWORLD_TEST_SOUNDFONT and DISCWORLD_TEST_SAVE (tick 33200 inventory checkpoint)"]
fn held_inventory_item_drops_through_one_pixel_cell_gaps() {
    let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
    let mut game =
        BrowserGame::new(read("DISCWORLD_TEST_ISO"), read("DISCWORLD_TEST_SOUNDFONT")).unwrap();
    game.load(&read("DISCWORLD_TEST_SAVE")).unwrap();
    let initial = game.save().unwrap();
    let window = game.engine.declarations.windows[1].as_ref().unwrap();
    assert!(window.size[0] >= 2 && window.size[1] >= 2);
    let columns = window.size[0] as usize;
    let item = window.contents[0];
    let original_items = window.contents.clone();
    let [x, y] = game.engine.inventory_cell_position(0).unwrap();
    // Each cell has 25 pixels followed by one dead pixel. These are the
    // vertical gap, horizontal gap, and intersection of the two gaps.
    let gaps = [
        ([x + 13, y], 1),
        ([x, y + 13], columns),
        ([x + 13, y + 13], columns + 1),
    ];
    for (point, destination) in gaps {
        game.load(&initial).unwrap();
        assert_eq!(game.engine.inventory_slot_at(point), None);
        // Empty-hand clicks must not acquire an adjacent item.
        game.move_pointer(point[0], point[1]);
        game.input("left-down").unwrap();
        game.input("left-up").unwrap();
        game.advance(24).unwrap();
        assert_eq!(game.engine.held_object, -1);
        assert_eq!(
            game.engine.declarations.windows[1]
                .as_ref()
                .unwrap()
                .contents,
            original_items
        );

        game.load(&initial).unwrap();
        game.move_pointer(x, y);
        game.input("left-down").unwrap();
        game.input("left-up").unwrap();
        game.advance(24).unwrap();
        assert_eq!(game.engine.held_object, item);
        game.move_pointer(point[0], point[1]);
        let held = game.save().unwrap();
        game.input("right-down").unwrap();
        game.input("right-up").unwrap();
        game.advance(24).unwrap();
        assert_eq!(game.engine.held_object, item);
        assert_eq!(
            game.engine.declarations.windows[1]
                .as_ref()
                .unwrap()
                .contents,
            original_items
        );

        game.load(&held).unwrap();
        game.input("left-down").unwrap();
        game.input("left-up").unwrap();
        assert_eq!(game.engine.held_object, item);
        // Both queued and suspended drop tasks must survive an exact save/load.
        roundtrip_pending(&mut game, 1);
        assert_eq!(game.engine.held_object, item);
        roundtrip_pending(&mut game, 24);
        assert_eq!(game.engine.held_object, -1, "gap {point:?}");
        let mut expected = original_items.clone();
        expected.remove(0);
        expected.insert(destination.min(expected.len()), item);
        assert_eq!(
            game.engine.declarations.windows[1]
                .as_ref()
                .unwrap()
                .contents,
            expected
        );
    }
}

#[test]
#[ignore = "requires DISCWORLD_TEST_ISO, DISCWORLD_TEST_SOUNDFONT and DISCWORLD_TEST_SLIDER_SAVE (tick 434050 scrollable inventory checkpoint)"]
fn inventory_slider_preserves_continuous_motion_and_active_save() {
    let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
    let mut game =
        BrowserGame::new(read("DISCWORLD_TEST_ISO"), read("DISCWORLD_TEST_SOUNDFONT")).unwrap();
    game.load(&read("DISCWORLD_TEST_SLIDER_SAVE")).unwrap();
    assert_eq!(game.tick(), 434050);
    assert_eq!(game.engine.active_inventory(), Some(1));
    let window = game.engine.declarations.windows[1].as_ref().unwrap();
    assert_eq!(window.first_display, 0);
    let contents = window.contents.clone();
    let position = window.position;
    let frame = discworld_core::conversation_window::Frame::inventory(
        &game.assets.resources(),
        game.engine.declarations.window_parts.unwrap(),
        window,
    )
    .unwrap();
    let bar = frame.layout.scrollbar.unwrap();
    println!("Original slider: {bar:?}");
    let [min, max] = bar.range;
    let x = bar.thumb[0] + 2;
    game.move_pointer(x, min + 2);
    game.input("left-down").unwrap();
    assert!(game.inventory_dragging());
    let middle = min + (max - min) / 2 + 1;
    game.move_pointer(x, middle + 2);
    assert_eq!(
        game.engine.declarations.windows[1]
            .as_ref()
            .unwrap()
            .position,
        position
    );
    assert!(
        game.engine.declarations.windows[1]
            .as_ref()
            .unwrap()
            .first_display
            > 0
    );
    // A tick rebuilds icons, while retaining continuous thumb placement.
    game.advance(1).unwrap();
    assert_eq!(game.engine.inventory_scroll_at([x, middle]), None);
    assert_eq!(
        game.engine.inventory_scroll_at([x, middle - 1]),
        Some(discworld_core::inventory::Scroll::PageUp)
    );
    let pending = game.save().unwrap();
    assert_eq!(inventory_schema(&pending), 6);
    game.load(&pending).unwrap();
    assert_eq!(game.save().unwrap(), pending);
    assert!(game.inventory_dragging());
    roundtrip_pending(&mut game, 3);

    game.move_pointer(x, max + 42);
    let bottom_first = game.engine.declarations.windows[1]
        .as_ref()
        .unwrap()
        .first_display;
    assert!(bottom_first > 0);
    let overshoot = game.save().unwrap();
    game.load(&overshoot).unwrap();
    game.move_pointer(x, max + 32);
    assert_eq!(
        game.engine.declarations.windows[1]
            .as_ref()
            .unwrap()
            .first_display,
        bottom_first
    );
    game.move_pointer(x, min + 2);
    assert_eq!(
        game.engine.declarations.windows[1]
            .as_ref()
            .unwrap()
            .first_display,
        0
    );
    // Horizontal pointer motion neither moves the window nor changes the page.
    game.move_pointer(x + 100, min + 2);
    assert_eq!(
        game.engine.declarations.windows[1]
            .as_ref()
            .unwrap()
            .position,
        position
    );
    assert_eq!(
        game.engine.declarations.windows[1]
            .as_ref()
            .unwrap()
            .first_display,
        0
    );
    game.input("left-up").unwrap();
    assert!(!game.inventory_dragging());
    game.advance(1).unwrap();
    assert_eq!(
        inventory_schema(&game.save().unwrap()),
        4
    );
    assert_eq!(
        game.engine.declarations.windows[1]
            .as_ref()
            .unwrap()
            .contents,
        contents
    );
    assert_eq!(game.engine.held_object, -1);
    game.load(&pending).unwrap();
    game.input("left-up").unwrap();
    game.advance(1).unwrap();
    // Middle row snaps from y144 to its table position y143 on release.
    assert_eq!(game.engine.inventory_scroll_at([x, middle - 1]), None);
    game.load(&pending).unwrap();
    game.input("close").unwrap();
    assert!(!game.inventory_dragging());
    game.save().unwrap();
    // Once geometry has been recorded, an active scrollbar uses the combined
    // version-7 payload instead of version 6, retaining both kinds of state.
    game.load(&read("DISCWORLD_TEST_SLIDER_SAVE")).unwrap();
    for _ in 0..2 {
        let [wx, wy] = game.engine.declarations.windows[1]
            .as_ref()
            .unwrap()
            .position;
        game.move_pointer(wx + 20, wy + 10);
        for _ in 0..2 {
            game.input("left-down").unwrap();
            game.input("left-up").unwrap();
        }
        game.advance(24).unwrap();
    }
    game.move_pointer(x, min + 2);
    game.input("left-down").unwrap();
    game.move_pointer(x, middle + 2);
    assert!(game.inventory_dragging());
    assert_eq!(
        inventory_schema(&game.save().unwrap()),
        7
    );
    roundtrip_pending(&mut game, 3);
    assert!(game.inventory_dragging());
    game.input("left-up").unwrap();
    assert!(!game.inventory_dragging());
    game.save().unwrap();
}

#[test]
#[ignore = "requires DISCWORLD_TEST_ISO, DISCWORLD_TEST_SOUNDFONT and DISCWORLD_TEST_SAVE (tick 33200 inventory checkpoint)"]
fn inventory_header_double_click_preserves_both_geometries_and_saves() {
    fn double_header(game: &mut BrowserGame) {
        let slot = game.engine.active_inventory().unwrap() as usize;
        let [x, y] = game.engine.declarations.windows[slot]
            .as_ref()
            .unwrap()
            .position;
        game.move_pointer(x + 20, y + 10);
        for _ in 0..2 {
            game.input("left-down").unwrap();
            game.input("left-up").unwrap();
        }
        assert!(!game.inventory_dragging());
    }
    let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
    let mut game =
        BrowserGame::new(read("DISCWORLD_TEST_ISO"), read("DISCWORLD_TEST_SOUNDFONT")).unwrap();
    game.load(&read("DISCWORLD_TEST_SAVE")).unwrap();
    let contents = game.engine.declarations.windows[1]
        .as_ref()
        .unwrap()
        .contents
        .clone();
    double_header(&mut game);
    let window = game.engine.declarations.windows[1].as_ref().unwrap();
    assert_eq!((window.size, window.position), ([10, 6], [21, 15]));
    let pixels = game.frame().unwrap();
    let maximized = game.save().unwrap();
    assert_eq!(inventory_schema(&maximized), 7);
    game.load(&maximized).unwrap();
    assert_eq!(game.save().unwrap(), maximized);
    assert_eq!(game.frame().unwrap(), pixels);
    roundtrip_pending(&mut game, 24);

    // Move the maximized window and save the ongoing header drag with geometry.
    game.move_pointer(41, 25);
    game.input("left-down").unwrap();
    assert!(game.inventory_dragging());
    game.move_pointer(51, 35);
    let dragging = game.save().unwrap();
    assert_eq!(inventory_schema(&dragging), 7);
    game.load(&dragging).unwrap();
    assert!(game.inventory_dragging());
    game.input("left-up").unwrap();
    game.advance(24).unwrap();
    double_header(&mut game);
    let window = game.engine.declarations.windows[1].as_ref().unwrap();
    assert_eq!((window.size, window.position), ([3, 2], [100, 100]));
    game.advance(24).unwrap();
    double_header(&mut game);
    let window = game.engine.declarations.windows[1].as_ref().unwrap();
    assert_eq!((window.size, window.position), ([10, 6], [31, 25]));
    game.input("close").unwrap();
    let closed = game.save().unwrap();
    game.load(&closed).unwrap();
    game.engine
        .interact_actor(
            &game.assets.resources(),
            8,
            discworld_core::engine::PolygonInteraction::WalkTo,
        )
        .unwrap();
    game.advance(24).unwrap();
    assert_eq!(game.engine.active_inventory(), Some(1));
    let window = game.engine.declarations.windows[1].as_ref().unwrap();
    assert_eq!((window.size, window.position), ([10, 6], [31, 25]));
    double_header(&mut game);
    let window = game.engine.declarations.windows[1].as_ref().unwrap();
    assert_eq!((window.size, window.position), ([3, 2], [100, 100]));
    assert_eq!(window.contents, contents);
    assert_eq!(game.engine.held_object, -1);
    // Geometry remains version 7 after unmaximizing to retain the other position.
    assert_eq!(
        inventory_schema(&game.save().unwrap()),
        7
    );
    // FULL construction on reopen restores the original reachable-header limit.
    game.advance(24).unwrap();
    game.move_pointer(120, 110);
    game.input("left-down").unwrap();
    game.engine.move_cursor([500, 500]);
    game.input("left-up").unwrap();
    game.input("close").unwrap();
    game.engine
        .interact_actor(
            &game.assets.resources(),
            8,
            discworld_core::engine::PolygonInteraction::WalkTo,
        )
        .unwrap();
    game.advance(24).unwrap();
    assert_eq!(
        game.engine.declarations.windows[1]
            .as_ref()
            .unwrap()
            .position,
        [314, 194]
    );
}

#[test]
#[ignore = "requires DISCWORLD_TEST_ISO, DISCWORLD_TEST_SOUNDFONT and DISCWORLD_TEST_SAVE"]
fn inventory_resize_outline_rounding_and_active_save() {
    let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
    let mut game =
        BrowserGame::new(read("DISCWORLD_TEST_ISO"), read("DISCWORLD_TEST_SOUNDFONT")).unwrap();
    let initial = read("DISCWORLD_TEST_SAVE");
    // Exercise every edge and corner, on both sides of the rounding threshold.
    for (point, direction) in [
        ([100, 135], [-1, 0]),
        ([195, 135], [1, 0]),
        ([145, 100], [0, -1]),
        ([145, 173], [0, 1]),
        ([100, 100], [-1, -1]),
        ([195, 100], [1, -1]),
        ([100, 173], [-1, 1]),
        ([195, 173], [1, 1]),
    ] {
        for distance in [12, 13] {
            game.load(&initial).unwrap();
            let contents = game.engine.declarations.windows[1]
                .as_ref()
                .unwrap()
                .contents
                .clone();
            game.move_pointer_checked(point[0], point[1]).unwrap();
            game.input("left-down").unwrap();
            assert!(game.inventory_dragging(), "{point:?}");
            let before_outline = game.frame().unwrap();
            game.move_pointer_checked(
                point[0] + direction[0] * distance,
                point[1] + direction[1] * distance,
            )
            .unwrap();
            let frame = game.frame().unwrap();
            // EMPTY draws only the outline: old contents and translucent
            // backdrop remain pixel-identical before the next animation tick.
            for y in 125..160 {
                for x in 110..180 {
                    let pixel = (y * 320 + x) * 4;
                    assert_eq!(&frame[pixel..pixel + 4], &before_outline[pixel..pixel + 4]);
                }
            }
            let active = game.save().unwrap();
            assert_eq!(inventory_schema(&active), 8);
            game.load(&active).unwrap();
            assert_eq!(game.frame().unwrap(), frame);
            assert_eq!(game.save().unwrap(), active);
            roundtrip_pending(&mut game, 3);
            game.input("left-up").unwrap();
            assert!(!game.inventory_dragging());
            let window = game.engine.declarations.windows[1].as_ref().unwrap();
            for axis in 0..2 {
                assert_eq!(
                    window.size[axis],
                    [3, 2][axis] + i32::from(distance == 13 && direction[axis] != 0),
                    "{point:?}, {distance}"
                );
                assert_eq!(
                    window.position[axis],
                    100 - if direction[axis] < 0 { distance } else { 0 }
                );
            }
            assert_eq!(window.contents, contents);
            assert_eq!(game.engine.held_object, -1);
            roundtrip_pending(&mut game, 3);
        }
    }
    // Resizing the maximized window makes the result the new normal size.
    game.load(&initial).unwrap();
    game.move_pointer_checked(120, 110).unwrap();
    for _ in 0..2 {
        game.input("left-down").unwrap();
        game.input("left-up").unwrap();
    }
    game.advance(24).unwrap();
    game.move_pointer_checked(298, 192).unwrap();
    game.input("left-down").unwrap();
    assert!(game.inventory_dragging());
    game.move_pointer_checked(272, 166).unwrap();
    roundtrip_pending(&mut game, 3);
    game.input("left-up").unwrap();
    assert_eq!(
        game.engine.declarations.windows[1].as_ref().unwrap().size,
        [9, 5]
    );
    game.advance(24).unwrap();
    for expected in [[10, 6], [9, 5]] {
        let [x, y] = game.engine.declarations.windows[1]
            .as_ref()
            .unwrap()
            .position;
        game.move_pointer_checked(x + 20, y + 10).unwrap();
        for _ in 0..2 {
            game.input("left-down").unwrap();
            game.input("left-up").unwrap();
        }
        assert_eq!(
            game.engine.declarations.windows[1].as_ref().unwrap().size,
            expected
        );
        roundtrip_pending(&mut game, 24);
    }
}

#[test]
#[ignore = "requires DISCWORLD_TEST_ISO, DISCWORLD_TEST_SOUNDFONT and DISCWORLD_TEST_SLIDER_SAVE"]
fn inventory_changes_during_resize_preserve_frame_and_use_current_icon_grid() {
    use discworld_core::inventory::Scroll;
    let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
    let mut game =
        BrowserGame::new(read("DISCWORLD_TEST_ISO"), read("DISCWORLD_TEST_SOUNDFONT")).unwrap();
    game.load(&read("DISCWORLD_TEST_SLIDER_SAVE")).unwrap();
    game.move_pointer(100, 135);
    game.input("left-down").unwrap();
    game.move_pointer(87, 135);
    assert!(game.inventory_dragging());
    // The logical window has moved left even though the old full frame remains.
    assert_eq!(game.engine.inventory_slot_at([105, 132]), Some(0));
    assert_eq!(game.engine.inventory_cell_position(0), Some([105, 132]));
    let before = game.frame().unwrap();
    assert!(game.engine.scroll_inventory(Scroll::Down));
    game.advance(1).unwrap();
    let after = game.frame().unwrap();
    // Title and header remain exactly where they were before the item redraw.
    for y in 103..115 {
        for x in 110..180 {
            let pixel = (y * 320 + x) * 4;
            assert_eq!(&before[pixel..pixel + 4], &after[pixel..pixel + 4]);
        }
    }
    assert_eq!(
        game.engine.declarations.windows[1]
            .as_ref()
            .unwrap()
            .first_display,
        3
    );
    // The original thumb range (124..162) remains, with its new midpoint at143.
    assert_eq!(
        game.engine.inventory_scroll_at([189, 142]),
        Some(Scroll::PageUp)
    );
    assert_eq!(game.engine.inventory_scroll_at([189, 143]), None);
    assert_eq!(
        game.engine.inventory_scroll_at([189, 148]),
        Some(Scroll::PageDown)
    );
    roundtrip_pending(&mut game, 3);
    game.input("left-up").unwrap();
    let window = game.engine.declarations.windows[1].as_ref().unwrap();
    assert_eq!((window.size, window.position), ([4, 2], [87, 100]));
    roundtrip_pending(&mut game, 3);
}

#[test]
#[ignore = "requires DISCWORLD_TEST_ISO, DISCWORLD_TEST_SOUNDFONT and DISCWORLD_TEST_SAVE"]
fn inventory_hover_uses_original_arrows_freezes_during_drag_and_restores() {
    use discworld_core::{animation::Animation, film::Film};
    fn original_cursor(game: &mut BrowserGame, reel: Option<usize>) {
        original_cursor_phase(game, reel, 1);
    }
    fn original_cursor_phase(game: &mut BrowserGame, reel: Option<usize>, ticks: u32) {
        let resources = game.assets.resources();
        let film = if reel.is_some() {
            Film::read(&resources, game.engine.declarations.window_parts.unwrap()).unwrap()
        } else {
            game.engine.declarations.cursor.as_ref().unwrap().1.clone()
        };
        let mut cursor = Animation::new(
            &resources,
            &film.reels[reel.unwrap_or(0)],
            if reel.is_some() {
                2
            } else {
                24 / film.rate as u32
            },
        )
        .unwrap();
        for _ in 0..ticks {
            cursor.tick(&resources).unwrap();
        }
        [cursor.object.x, cursor.object.y] = game.engine.control.cursor;
        let was_hidden = game.engine.control.hidden;
        let actual = game.frame().unwrap();
        game.engine.control.hidden = true;
        let mut expected = game.engine.render(&resources).unwrap();
        game.engine.control.hidden = false;
        expected.draw_animation(&resources, &cursor).unwrap();
        assert!(
            actual == expected.rgba,
            "cursor reel {reel:?}, hidden={was_hidden}, tick={}",
            game.tick()
        );
    }
    let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
    let mut game =
        BrowserGame::new(read("DISCWORLD_TEST_ISO"), read("DISCWORLD_TEST_SOUNDFONT")).unwrap();
    let initial = read("DISCWORLD_TEST_SAVE");
    for (point, reel) in [
        ([100, 135], 24),
        ([195, 135], 24),
        ([145, 100], 25),
        ([145, 173], 25),
        ([100, 100], 27),
        ([195, 173], 27),
        ([195, 100], 26),
        ([100, 173], 26),
    ] {
        game.load(&initial).unwrap();
        game.move_pointer(point[0], point[1]);
        game.advance(2).unwrap();
        original_cursor(&mut game, Some(reel));
        roundtrip_pending(&mut game, 3);
        game.input("left-down").unwrap();
        game.move_pointer(300, 10);
        game.advance(2).unwrap();
        original_cursor(&mut game, Some(reel));
        roundtrip_pending(&mut game, 3);
        game.input("left-up").unwrap();
        game.move_pointer_checked(300, 0).unwrap();
        game.advance(2).unwrap();
        // Release or movement outside restores immediately, before these ticks.
        original_cursor_phase(&mut game, None, 3);
        roundtrip_pending(&mut game, 3);
    }
    game.load(&initial).unwrap();
    game.move_pointer(100, 135);
    game.advance(2).unwrap();
    game.input("close").unwrap();
    game.advance(2).unwrap();
    // Player close restores/steps immediately, then both engine ticks advance it.
    original_cursor_phase(&mut game, None, 3);
    game.input("pockets").unwrap();
    game.advance(24).unwrap();
    assert_eq!(game.engine.active_inventory(), Some(2));
    let window = game.engine.declarations.windows[2].as_ref().unwrap();
    assert_eq!(window.min, window.max);
    let [x, y] = window.position;
    game.move_pointer(x, y + 25);
    game.advance(2).unwrap();
    // Initial restore step plus 28 subsequent ticks.
    original_cursor_phase(&mut game, None, 29);
    for (name, reel) in [
        ("DISCWORLD_TEST_HOVER_ARROW", Some(24)),
        ("DISCWORLD_TEST_HOVER_CORNER", Some(27)),
        ("DISCWORLD_TEST_HOVER_NORMAL", None),
    ] {
        if let Some(path) = std::env::var_os(name) {
            game.load(&std::fs::read(path).unwrap()).unwrap();
            original_cursor(&mut game, reel);
            roundtrip_pending(&mut game, 3);
        }
    }
}

#[test]
#[ignore = "requires DISCWORLD_TEST_ISO, DISCWORLD_TEST_SOUNDFONT and DISCWORLD_TEST_SAVE"]
fn inventory_pointed_label_uses_original_font_and_survives_save() {
    use discworld_core::{
        compositor::Surface,
        text::{Font, Strings},
    };
    let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
    let mut game =
        BrowserGame::new(read("DISCWORLD_TEST_ISO"), read("DISCWORLD_TEST_SOUNDFONT")).unwrap();
    game.load(&read("DISCWORLD_TEST_SAVE")).unwrap();
    game.engine.subtitles = false; // POINTED labels ignore speech subtitle preference.
    game.move_pointer(118, 132);
    game.advance(1).unwrap();
    let saved = game.save().unwrap();
    assert_eq!(inventory_schema(&saved), 9);
    let resources = game.assets.resources();
    let strings = Strings::new(&game.assets.iso[game.assets.text.clone()]).unwrap();
    let text = (0..strings.len())
        .map(|i| strings.get(i as u32).unwrap())
        .find(|s| s.eq_ignore_ascii_case(b"banana"))
        .expect("original banana label");
    let font = Font::read(&resources, game.engine.declarations.tag_font.unwrap()).unwrap();
    let mut label = font.centered(&resources, text, [118, 120]).unwrap();
    label.keep_inventory_horizontal(&resources, 320).unwrap();
    let mut expected = Surface::new(320, 200).unwrap();
    for p in expected.rgba.chunks_exact_mut(4) {
        p.copy_from_slice(&[13, 17, 19, 255]);
    }
    label.draw(&resources, &mut expected).unwrap();
    game.engine.control.hidden = true;
    let frame = game.frame().unwrap();
    game.engine.control.hidden = false;
    let pixels: Vec<_> = expected
        .rgba
        .chunks_exact(4)
        .enumerate()
        .filter(|(_, p)| *p != [13, 17, 19, 255])
        .collect();
    assert!(!pixels.is_empty());
    for (i, p) in pixels {
        assert_eq!(&frame[i * 4..i * 4 + 4], p, "original label pixel {i}");
    }
    roundtrip_pending(&mut game, 4);
    game.input("look").unwrap();
    roundtrip_pending(&mut game, 1); // Non-POINTED arbitration's initial sleep.
    roundtrip_pending(&mut game, 1); // Description and hidden hover label.
    let spoken = game.frame().unwrap();
    assert!(
        expected
            .rgba
            .chunks_exact(4)
            .enumerate()
            .any(|(i, p)| { p != [13, 17, 19, 255] && &spoken[i * 4..i * 4 + 4] != p }),
        "hover label must yield to the spoken description"
    );
    for _ in 0..600 {
        game.advance(1).unwrap();
        if game.engine.control.enabled {
            break;
        }
    }
    assert!(game.engine.control.enabled);
    game.advance(2).unwrap();
    game.engine.control.hidden = true;
    let redisplayed = game.frame().unwrap();
    game.engine.control.hidden = false;
    for (i, p) in expected
        .rgba
        .chunks_exact(4)
        .enumerate()
        .filter(|(_, p)| *p != [13, 17, 19, 255])
    {
        assert_eq!(
            &redisplayed[i * 4..i * 4 + 4],
            p,
            "redisplayed original label pixel {i}"
        );
    }
    game.move_pointer(0, 0);
    game.advance(1).unwrap();
    roundtrip_pending(&mut game, 3); // Includes POINTED completion and UNPOINT.
    game.move_pointer(118, 132);
    game.advance(1).unwrap();
    roundtrip_pending(&mut game, 3);
}

#[test]
#[ignore = "requires DISCWORLD_TEST_ISO, DISCWORLD_TEST_SOUNDFONT and DISCWORLD_TEST_NODE_EXIT_SAVE"]
fn right_click_while_leaving_unseen_stairs_preserves_leg_and_recovers() {
    let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
    let mut game =
        BrowserGame::new(read("DISCWORLD_TEST_ISO"), read("DISCWORLD_TEST_SOUNDFONT")).unwrap();
    game.load(&read("DISCWORLD_TEST_NODE_EXIT_SAVE")).unwrap();
    assert_eq!(game.tick(), 133103);
    assert_eq!(
        game.assets.names[game.engine.scene.resource()],
        "unseen.scn"
    );
    roundtrip_pending(&mut game, 1); // Former failing tick, including scripted retarget.
    roundtrip_pending(&mut game, 24);
    while game.tick() < 133800 {
        game.advance((133800 - game.tick()).min(24) as u32).unwrap();
    }
    assert_eq!(game.tick(), 133800);
    assert!(game.engine.control.enabled);
    assert_eq!(game.engine.movers[&7].placement.position, [452, 553]);
    assert_eq!(game.engine.movers[&8].placement.position, [452, 573]);
    assert!(game.engine.movers.values().all(|m| m.walk_target.is_none()));
    roundtrip_pending(&mut game, 5);
}

#[test]
#[ignore = "requires DISCWORLD_TEST_ISO and DISCWORLD_TEST_SOUNDFONT"]
fn disabled_voice_plays_original_intro_with_text_and_restorable_timing() {
    let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
    let mut game =
        BrowserGame::new(read("DISCWORLD_TEST_ISO"), read("DISCWORLD_TEST_SOUNDFONT")).unwrap();
    game.menu_preferences(r#"{"volumes":[255,255,0],"double_click":10,"swap_buttons":false,"text_speed":0,"subtitles":false,"flipped":false}"#).unwrap();
    let mut saw_speech_wait = false;
    while game.tick() < 8500 {
        if game.tick() == 500 {
            game.input("start").unwrap();
        }
        game.advance(1).unwrap();
        assert!(!game.engine.sample_player.is_speech());
        assert!(!game.engine.subtitles);
        saw_speech_wait |= game
            .engine
            .pending_calls()
            .iter()
            .any(|(_, call, _)| matches!(call.id, 53 | 54 | 77 | 98 | 104));
        if game.tick() % 1000 == 0 {
            assert_eq!(game.frame().unwrap().len(), 320 * 200 * 4);
            roundtrip_pending(&mut game, 1);
        }
    }
    assert!(
        saw_speech_wait,
        "the original intro must exercise speech calls"
    );
}

#[test]
#[ignore = "requires DISCWORLD_TEST_ISO, DISCWORLD_TEST_SOUNDFONT and DISCWORLD_TEST_NODE_EXIT_SAVE"]
fn original_walking_talk_reel_preserves_route_and_full_session() {
    use discworld_core::graphics::Handle;
    let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
    let mut game =
        BrowserGame::new(read("DISCWORLD_TEST_ISO"), read("DISCWORLD_TEST_SOUNDFONT")).unwrap();
    game.load(&read("DISCWORLD_TEST_NODE_EXIT_SAVE")).unwrap();
    while game.tick() < 133800 {
        game.advance((133800 - game.tick()).min(24) as u32).unwrap();
    }
    let actor = game.engine.declarations.lead.as_ref().unwrap().id;
    let initial = game.engine.movers[&actor].clone();
    let start = initial.placement.position;
    let resources = game.assets.resources();
    let mut selected = None;
    for delta in [[-40, 0], [40, 0], [0, -20], [0, 20]] {
        let target = [start[0] + delta[0], start[1] + delta[1]];
        let mut mover = initial.clone();
        if mover
            .begin_walk(
                &resources,
                &game.engine.declarations,
                &game.engine.polygons,
                target,
                target,
            )
            .is_ok()
            && mover.walk_target.is_some()
        {
            selected = Some(mover);
            break;
        }
    }
    let mut mover = selected.expect("a nontrivial walk from the original checkpoint");
    let target = mover.walk_target.unwrap();
    let serial = mover.walk_serial;
    let saved = mover
        .push_talk(&resources, &game.engine.declarations, Handle(0))
        .unwrap();
    assert_eq!(saved, Handle(0));
    game.engine.movers.insert(actor, mover);
    // Exercise the actual core mover loop and production engine/DSP session
    // codec. This is a reel-level test; the synthetic core test dispatches TALK.
    roundtrip_pending(&mut game, 1);
    roundtrip_pending(&mut game, 5);
    assert_eq!(game.engine.movers[&actor].placement.position, start);
    assert_eq!(game.engine.movers[&actor].walk_target, Some(target));
    assert_eq!(game.engine.movers[&actor].walk_serial, serial);
    let resources = game.assets.resources();
    game.engine
        .movers
        .get_mut(&actor)
        .unwrap()
        .pop_talk(&resources, &game.engine.declarations, saved)
        .unwrap();
    roundtrip_pending(&mut game, 1);
    for _ in 0..100 {
        if game.engine.movers[&actor].walk_target.is_none() {
            break;
        }
        roundtrip_pending(&mut game, 1);
    }
    assert_eq!(game.engine.movers[&actor].walk_target, None);
    assert_eq!(game.engine.movers[&actor].placement.position, target);
    assert_eq!(game.engine.movers[&actor].walk_serial, serial);
    roundtrip_pending(&mut game, 3);
}

#[test]
#[ignore = "requires DISCWORLD_TEST_ISO, DISCWORLD_TEST_SOUNDFONT and DISCWORLD_TEST_NODE_EXIT_SAVE"]
fn original_custom_walk_film_restores_exact_frames_and_full_sessions() {
    use discworld_core::graphics::Handle;
    let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
    let mut game =
        BrowserGame::new(read("DISCWORLD_TEST_ISO"), read("DISCWORLD_TEST_SOUNDFONT")).unwrap();
    game.load(&read("DISCWORLD_TEST_NODE_EXIT_SAVE")).unwrap();
    while game.tick() < 133800 {
        game.advance((133800 - game.tick()).min(24) as u32).unwrap();
    }
    let actor = game.engine.declarations.lead.as_ref().unwrap().id;
    let initial = game.engine.movers[&actor].clone();
    let start = initial.placement.position;
    let film = game.engine.declarations.movers[&actor][initial.placement.scale - 1][0];
    let resources = game.assets.resources();
    let mut invalid = initial.clone();
    assert!(
        invalid
            .begin_walk_film(
                &resources,
                &game.engine.declarations,
                &game.engine.polygons,
                start,
                start,
                Handle(u32::MAX)
            )
            .is_err()
    );
    assert_eq!(invalid, initial);
    let mut selected = None;
    for delta in [[-40, 0], [40, 0], [0, -20], [0, 20]] {
        let target = [start[0] + delta[0], start[1] + delta[1]];
        let mut mover = initial.clone();
        if mover
            .begin_walk_film(
                &resources,
                &game.engine.declarations,
                &game.engine.polygons,
                target,
                target,
                film,
            )
            .is_ok()
            && mover.walk_target.is_some()
        {
            selected = Some(mover);
            break;
        }
    }
    let mover = selected.expect("nontrivial original custom-film walk");
    let target = mover.walk_target.unwrap();
    let serial = mover.walk_serial;
    assert_eq!(mover.last_film, film);
    assert_eq!(mover.animation.steps, 0);
    game.engine.movers.insert(actor, mover);
    for _ in 0..100 {
        if game.engine.movers[&actor].walk_target.is_none() {
            break;
        }
        assert_eq!(game.engine.movers[&actor].last_film, film);
        assert_eq!(
            game.engine.movers[&actor].placement.direction,
            initial.placement.direction
        );
        roundtrip_pending(&mut game, 1);
    }
    assert_eq!(game.engine.movers[&actor].walk_target, None);
    assert_eq!(game.engine.movers[&actor].placement.position, target);
    assert_eq!(game.engine.movers[&actor].walk_serial, serial);
    let mover = &game.engine.movers[&actor];
    assert_eq!(
        mover.last_film,
        game.engine.declarations.movers[&actor][mover.placement.scale - 1]
            [4 + mover.placement.direction.index()]
    );
    roundtrip_pending(&mut game, 3);
}

#[test]
#[ignore = "requires DISCWORLD_TEST_ISO and DISCWORLD_TEST_SOUNDFONT"]
fn original_intro_headroom_preserves_sessions_and_avoids_measured_clipping() {
    let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
    let mut game =
        BrowserGame::new(read("DISCWORLD_TEST_ISO"), read("DISCWORLD_TEST_SOUNDFONT")).unwrap();
    let mut original_rails = 0;
    let mut reduced_rails = 0;
    let mut reduced_peak = 0.0f32;
    while game.tick() < 8500 {
        if game.tick() == 500 {
            game.engine.user_event().unwrap();
        }
        let boundary = if game.tick() < 500 { 500 } else { 8500 };
        let ticks = (boundary - game.tick()).min(24) as u32;
        let before = game.save().unwrap();
        game.set_audio_headroom(false);
        let original = game.advance(ticks).unwrap();
        let after = game.save().unwrap();
        game.load(&before).unwrap();
        game.set_audio_headroom(true);
        let reduced = game.advance(ticks).unwrap();
        assert_eq!(original.len(), reduced.len());
        assert_eq!(
            game.save().unwrap(),
            after,
            "presentation must not change engine or DSP state"
        );
        for (a, b) in original.iter().zip(&reduced) {
            assert!(a.is_finite() && b.is_finite());
            original_rails += usize::from(a.abs() == 1.0);
            reduced_rails += usize::from(b.abs() == 1.0);
            reduced_peak = reduced_peak.max(b.abs());
            if a.abs() < 1.0 {
                assert_eq!(*b, *a * 0.5);
            }
        }
    }
    assert!(original_rails > 0, "must exercise the measured clipping");
    assert_eq!(reduced_rails, 0);
    assert!(reduced_peak > 0.5 && reduced_peak < 1.0);
    println!(
        "intro headroom: original rails={original_rails}, reduced rails={reduced_rails}, reduced peak={reduced_peak}"
    );
}

#[test]
#[ignore = "requires DISCWORLD_TEST_ISO and DISCWORLD_TEST_SOUNDFONT"]
fn original_intro_escape_reaches_title_with_exact_session_continuation() {
    let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
    let iso_bytes = read("DISCWORLD_TEST_ISO");
    let iso = discworld_core::iso::Iso::parse(&iso_bytes).unwrap();
    let index = discworld_core::resource::parse_index(iso.file("/DISCWLD/INDEX").unwrap()).unwrap();
    let title = discworld_core::intro::title_scene(&index).unwrap().unwrap();
    let mut game = BrowserGame::new(iso_bytes, read("DISCWORLD_TEST_SOUNDFONT")).unwrap();
    assert!(game.engine.intro_active());
    game.advance(1).unwrap();
    assert_ne!(game.engine.scene, title, "test must escape before title");
    game.input("escape").unwrap();
    for _ in 0..500 {
        roundtrip_pending(&mut game, 24);
        if game.engine.scene == title {
            break;
        }
    }
    assert_eq!(game.engine.scene, title);
    roundtrip_pending(&mut game, 24);
}

#[test]
#[ignore = "requires DISCWORLD_TEST_ISO and DISCWORLD_TEST_SOUNDFONT"]
fn user_load_ends_intro_but_exact_restore_retains_it_atomically() {
    use discworld_core::{engine::Engine, iso::Iso, resource::parse_index};
    let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
    let bytes = read("DISCWORLD_TEST_ISO");
    let iso = Iso::parse(&bytes).unwrap();
    let index = parse_index(iso.file("/DISCWLD/INDEX").unwrap()).unwrap();
    let title = discworld_core::intro::title_scene(&index).unwrap().unwrap();
    let mut game = BrowserGame::new(bytes, read("DISCWORLD_TEST_SOUNDFONT")).unwrap();
    game.engine = Engine::bootstrap_with_title(&game.assets.resources(), Some(title)).unwrap();
    game.advance(1).unwrap();
    let saved = game.save().unwrap();
    assert!(game.engine.intro_active());
    game.load(&saved).unwrap();
    assert_eq!(game.save().unwrap(), saved);
    let audio = game.audio.checkpoint_bytes().unwrap();
    let tick = game.tick();
    let frame = game.frame().unwrap();
    game.load_game(&saved).unwrap();
    let loaded = game.save().unwrap();
    assert!(!game.engine.intro_active());
    assert_eq!(game.tick(), tick);
    assert_eq!(game.frame().unwrap(), frame);
    assert_eq!(game.audio.checkpoint_bytes().unwrap(), audio);
    let mut invalid = saved.clone();
    invalid.pop();
    assert!(game.load_game(&invalid).is_err());
    assert_eq!(game.save().unwrap(), loaded);
    game.load(&saved).unwrap();
    assert!(game.engine.intro_active());
}

#[test]
#[ignore = "requires DISCWORLD_TEST_ISO and DISCWORLD_TEST_SOUNDFONT"]
fn scripted_quit_returns_host_request_without_fault_or_extra_ticks() {
    let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
    let mut game =
        BrowserGame::new(read("DISCWORLD_TEST_ISO"), read("DISCWORLD_TEST_SOUNDFONT")).unwrap();
    inject_startup_quit(&mut game);
    let before = game.tick();
    let output = game.advance(24).unwrap();
    assert_eq!(game.tick(), before + 1);
    assert!(output.iter().all(|v| v.is_finite()));
    assert!(game.fault.is_none());
    assert!(game.advance(24).unwrap().is_empty());
    assert_eq!(game.tick(), before + 1);
    let request: serde_json::Value =
        serde_json::from_str(&game.take_menu_request().unwrap().unwrap()).unwrap();
    assert_eq!(request["request"], "quit");
    assert!(game.take_menu_request().unwrap().is_none());
    assert!(game.fault.is_none());
    // Completing the DSP block before delivering Quit keeps export valid.
    let bytes = game.save().unwrap();
    game.load(&bytes).unwrap();
    assert_eq!(game.save().unwrap(), bytes);
}

fn inject_startup_quit(game: &mut BrowserGame) {
    // Inject QUITGAME into private copies of initial scene scripts. Original
    // files and original resource bytes stay untouched. This tests host wiring,
    // not whether an unmodified original-game interaction reaches QUITGAME.
    let scene = discworld_core::scene::read(&game.assets.resources(), game.engine.scene.resource())
        .unwrap()
        .unwrap();
    let scripts =
        std::iter::once(scene.script).chain(scene.entrances.iter().map(|(_, code)| *code));
    for code in scripts.filter(|code| code.0 != 0) {
        let mut copy = game.assets.iso[game.assets.resources[code.resource()].clone()].to_vec();
        copy[code.offset()..code.offset() + 3].copy_from_slice(&[0x4e, 115, 0]);
        let start = game.assets.iso.len();
        game.assets.iso.extend(copy);
        game.assets.resources[code.resource()] = start..game.assets.iso.len();
    }
}

#[test]
#[ignore = "requires DISCWORLD_TEST_ISO and DISCWORLD_TEST_SOUNDFONT"]
fn scripted_quit_preserves_same_cycle_save_and_load_before_host_shutdown() {
    use discworld_core::{
        menu_controller::{Assets, Controller, Settings},
        menu_navigation::{Page, Request},
        save_menu::{Entry, Target},
        text::Strings,
    };
    let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
    for page in [Page::Save, Page::Load] {
        let mut game =
            BrowserGame::new(read("DISCWORLD_TEST_ISO"), read("DISCWORLD_TEST_SOUNDFONT")).unwrap();
        inject_startup_quit(&mut game);
        game.menu_entries.push(Entry {
            id: 7,
            description: b"Library".to_vec(),
        });
        let expected = if page == Page::Save {
            Request::Save
        } else {
            Request::Load
        };
        let settings = Settings {
            volumes: [255; 3],
            double_click: 10,
            swap_buttons: false,
            text_speed: 50,
            subtitles: true,
            flipped: false,
        };
        let mut menu = Controller::new(settings).unwrap();
        {
            let resources = game.assets.resources();
            let strings = Strings::new(&game.assets.iso[game.assets.text.clone()]).unwrap();
            let assets = Assets {
                resources: &resources,
                strings: &strings,
                declarations: &game.engine.declarations,
                saves: &game.menu_entries,
            };
            menu.open(page, &assets).unwrap();
            menu.click_row([50, if page == Page::Save { 63 } else { 48 }], false)
                .unwrap();
            assert!(menu.click_button([262, 69]));
            // Stop just before the real controller dispatches, so its request
            // and QUITGAME both occur in the next production engine cycle.
            let mut ready = false;
            for _ in 0..12 {
                let mut next = menu.clone();
                let dispatch = next.advance(&assets).unwrap();
                if dispatch.outcome.request == Some(expected) {
                    assert_eq!(
                        dispatch.selection,
                        Some((Target::Existing(7), b"Library".to_vec()))
                    );
                    ready = true;
                    break;
                }
                assert!(dispatch.outcome.request.is_none());
                menu = next;
            }
            assert!(ready);
        }
        game.engine.configuration = Some(menu);
        let before = game.tick();
        game.advance(24).unwrap();
        assert_eq!(game.tick(), before + 1);
        assert!(game.fault.is_none());
        let request: serde_json::Value =
            serde_json::from_str(&game.take_menu_request().unwrap().unwrap()).unwrap();
        assert_eq!(
            request["request"],
            if page == Page::Save { "save" } else { "load" }
        );
        assert_eq!(request["selection"]["id"], 7);
        assert_eq!(
            request["selection"]["description"],
            serde_json::json!(b"Library".to_vec())
        );
        // Neither draining the first request nor loading can lose the queued
        // host quit or permit another simulation tick before it is delivered.
        let bytes = game.save().unwrap();
        if page == Page::Load {
            game.load_game(&bytes).unwrap();
        }
        assert!(game.advance(24).unwrap().is_empty());
        assert_eq!(game.tick(), before + 1);
        let request: serde_json::Value =
            serde_json::from_str(&game.take_menu_request().unwrap().unwrap()).unwrap();
        assert_eq!(request["request"], "quit");
        assert!(game.take_menu_request().unwrap().is_none());
        assert!(game.fault.is_none());
    }
}

#[test]
#[ignore = "requires DISCWORLD_TEST_ISO and DISCWORLD_TEST_SOUNDFONT"]
fn original_title_prompt_matches_independent_reference_palette() {
    use sha2::{Digest, Sha256};
    let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
    let mut game =
        BrowserGame::new(read("DISCWORLD_TEST_ISO"), read("DISCWORLD_TEST_SOUNDFONT")).unwrap();
    while game.tick() < 420 {
        game.advance((420 - game.tick()).min(24) as u32).unwrap();
    }
    assert_eq!(game.scene(), "title.scn");
    let rgb: Vec<_> = game
        .frame()
        .unwrap()
        .chunks_exact(4)
        .flat_map(|p| p[..3].iter().copied())
        .collect();
    // Pinned reference frame 420/cycle 421; complete framebuffer, not a
    // value generated by this renderer. See reference-software-frames.json.
    assert_eq!(
        format!("{:x}", Sha256::digest(&rgb)),
        "c506236b08e086d7d8f5fc6c8f9976a3dc64fc3495d81794f4a0b8a40206d970"
    );
    let bytes = game.save().unwrap();
    game.load(&bytes).unwrap();
    let restored: Vec<_> = game
        .frame()
        .unwrap()
        .chunks_exact(4)
        .flat_map(|p| p[..3].iter().copied())
        .collect();
    assert_eq!(rgb, restored);
}

#[test]
#[ignore = "requires DISCWORLD_TEST_ISO and DISCWORLD_TEST_SOUNDFONT"]
fn original_credits_scroll_matches_reference_cycle_and_save_continuation() {
    use sha2::{Digest, Sha256};
    let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
    let mut game =
        BrowserGame::new(read("DISCWORLD_TEST_ISO"), read("DISCWORLD_TEST_SOUNDFONT")).unwrap();
    while game.tick() < 637 {
        game.advance((637 - game.tick()).min(24) as u32).unwrap();
    }
    assert_eq!(game.scene(), "credits.scn");
    let saved = game.save().unwrap();
    for replay in 0..2 {
        if replay != 0 {
            game.load(&saved).unwrap();
        }
        for (tick, y, expected) in [
            (
                639,
                2,
                "ea0787f65f73b0013d03b359490e3125211b28ad5c1502ffb1544c0ded4192f5",
            ),
            (
                650,
                24,
                "082bab1e37a039667506a7b55c6677937eb38436632758275727037acacf6c26",
            ),
            (
                700,
                124,
                "0342a18fb95ba2fcaaf78d644752235285e57424e56feb3597a2c79339915be4",
            ),
        ] {
            while game.tick() < tick {
                game.advance((tick - game.tick()).min(24) as u32).unwrap();
            }
            assert_eq!(game.engine.world_origin, [0, y]);
            let rgb: Vec<_> = game
                .frame()
                .unwrap()
                .chunks_exact(4)
                .flat_map(|p| p[..3].iter().copied())
                .collect();
            // Independent reference cycles 640, 651 and 701, not Rust-generated images.
            assert_eq!(format!("{:x}", Sha256::digest(&rgb)), expected);
        }
    }
}

#[test]
#[ignore = "requires DISCWORLD_TEST_ISO, DISCWORLD_TEST_SOUNDFONT and DISCWORLD_TEST_SAVE"]
fn original_world_hotspot_scripts_create_game_font_labels_and_restore() {
    use discworld_core::{polygon::Kind, text::Font};
    let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
    let mut game =
        BrowserGame::new(read("DISCWORLD_TEST_ISO"), read("DISCWORLD_TEST_SOUNDFONT")).unwrap();
    let legacy = read("DISCWORLD_TEST_SAVE");
    game.load(&legacy).unwrap();
    assert_eq!(
        game.save().unwrap(),
        legacy,
        "exact historical restore remains byte preserving"
    );
    let tick = game.tick();
    let scene = game.engine.scene;
    let globals = game.engine.globals.clone();
    game.load_game(&legacy).unwrap();
    assert_eq!(game.tick(), tick);
    assert_eq!(game.engine.scene, scene);
    assert_eq!(game.engine.globals, globals);
    assert!(game.engine.world_tag_layout().is_none());
    let upgraded = game.save().unwrap();
    game.load_game(&upgraded).unwrap();
    assert_eq!(
        game.save().unwrap(),
        upgraded,
        "current player load is idempotent"
    );
    game.input("close").unwrap();
    game.advance(24).unwrap();
    assert!(game.engine.control.tags_enabled);
    game.engine.scene_calls = Some(Default::default());
    let points: Vec<_> = game
        .engine
        .polygons
        .active
        .iter()
        .filter(|p| p.enabled && matches!(p.definition.kind, Kind::Tag | Kind::Exit))
        .map(|p| {
            std::array::from_fn::<_, 2, _>(|i| {
                p.definition.corners.iter().map(|c| c[i]).sum::<i32>() / 4
                    - game.engine.world_origin[i]
            })
        })
        .filter(|p| (0..320).contains(&p[0]) && (0..200).contains(&p[1]))
        .collect();
    let mut labeled = 0;
    for [x, y] in points {
        if game
            .engine
            .actor_at(&game.assets.resources(), [x, y])
            .unwrap()
            .is_some()
        {
            continue;
        }
        game.move_pointer(x, y);
        game.advance(3).unwrap();
        let Some(layout) = game.engine.world_tag_layout().cloned() else {
            continue;
        };
        if layout.glyphs.is_empty() {
            continue;
        }
        let font = Font::read(
            &game.assets.resources(),
            game.engine.declarations.tag_font.unwrap(),
        )
        .unwrap();
        assert!(
            layout
                .glyphs
                .iter()
                .all(|glyph| font.glyphs.contains(&glyph.image))
        );
        let frame = game.frame().unwrap();
        let saved = game.save().unwrap();
        game.load(&saved).unwrap();
        assert_eq!(game.engine.world_tag_layout(), Some(&layout));
        assert_eq!(game.frame().unwrap(), frame);
        roundtrip_pending(&mut game, 3);
        labeled += 1;
    }
    assert!(labeled > 0, "original room must exercise a world label");
    assert!(
        game.engine
            .scene_calls
            .as_ref()
            .unwrap()
            .iter()
            .any(|call| call.call.id == 55)
    );
    println!(
        "{}: {labeled} original polygon labels checked",
        game.scene()
    );
}

#[cfg(feature = "native-inspection")]
#[test]
#[ignore = "requires DISCWORLD_TEST_ISO, DISCWORLD_TEST_SOUNDFONT and DISCWORLD_TEST_SAVE"]
fn native_touch_drop_accepts_gaps_without_magnetizing_empty_hand() {
    let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
    let mut game = BrowserGame::new(read("DISCWORLD_TEST_ISO"), read("DISCWORLD_TEST_SOUNDFONT")).unwrap();
    game.load(&read("DISCWORLD_TEST_SAVE")).unwrap();
    let [x, y] = game.engine.inventory_cell_position(0).unwrap();
    let item = game.engine.declarations.windows[1].as_ref().unwrap().contents[0];
    let original = game.engine.declarations.windows[1].as_ref().unwrap().contents.clone();
    for point in [[x + 13, y], [x, y + 13], [x + 13, y + 13]] {
        assert!(game.engine.inventory_slot_at(point).is_none());
        game.native_touch_input("walk", point[0], point[1]).unwrap();
        game.advance(24).unwrap();
        assert_eq!(game.engine.held_object, -1);
        game.native_touch_input("walk", x, y).unwrap();
        game.advance(24).unwrap();
        assert_eq!(game.engine.held_object, item);
        game.native_touch_input("walk", point[0], point[1]).unwrap();
        game.advance(24).unwrap();
        assert_eq!(game.engine.held_object, -1);
        let mut actual = game.engine.declarations.windows[1].as_ref().unwrap().contents.clone();
        let mut expected = original.clone();
        actual.sort(); expected.sort();
        assert_eq!(actual, expected, "touch drop must preserve inventory membership");
        game.load(&read("DISCWORLD_TEST_SAVE")).unwrap();
    }
}

#[cfg(feature = "native-inspection")]
#[test]
#[ignore = "requires DISCWORLD_TEST_ISO, DISCWORLD_TEST_SOUNDFONT and DISCWORLD_TEST_SAVE"]
fn native_outside_walk_closes_inventory_but_protects_held_items_and_drags() {
    let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
    let mut game = BrowserGame::new(read("DISCWORLD_TEST_ISO"), read("DISCWORLD_TEST_SOUNDFONT")).unwrap();
    let initial = read("DISCWORLD_TEST_SAVE");
    game.load(&initial).unwrap();
    let destination = [160, 185];
    assert!(!game.engine.inventory_contains_point(destination));
    game.native_touch_input("walk", destination[0], destination[1]).unwrap();
    assert_eq!(game.engine.active_inventory(), None);
    for _ in 0..10 { game.advance(24).unwrap(); }
    let lead = game.engine.declarations.lead.as_ref().unwrap().id;
    assert_eq!(game.engine.movers[&lead].placement.position, destination);

    game.load(&initial).unwrap();
    let cell = game.engine.inventory_cell_position(0).unwrap();
    game.native_touch_input("walk", cell[0], cell[1]).unwrap();
    game.advance(24).unwrap();
    let held = game.engine.held_object;
    assert_ne!(held, -1);
    game.native_touch_input("walk", destination[0], destination[1]).unwrap();
    assert_eq!(game.engine.active_inventory(), Some(1));
    assert_eq!(game.engine.held_object, held);

    game.load(&initial).unwrap();
    let position = game.engine.declarations.windows[1].as_ref().unwrap().position;
    game.move_pointer_checked(position[0] + 30, position[1] + 8).unwrap();
    game.input("left-down").unwrap();
    assert!(game.engine.inventory_dragging());
    game.native_touch_input("walk", destination[0], destination[1]).unwrap();
    assert_eq!(game.engine.active_inventory(), Some(1));
    game.input("left-up").unwrap();
}

#[cfg(feature = "native-inspection")]
#[test]
#[ignore = "requires DISCWORLD_TEST_ISO, DISCWORLD_TEST_SOUNDFONT and DISCWORLD_TEST_CONVERSATION_SAVE"]
fn native_topic_taps_select_conversation_icons_in_every_action_mode() {
    let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
    let mut game = BrowserGame::new(read("DISCWORLD_TEST_ISO"), read("DISCWORLD_TEST_SOUNDFONT")).unwrap();
    let initial = read("DISCWORLD_TEST_CONVERSATION_SAVE");
    for command in ["walk", "action", "look"] {
        game.load(&initial).unwrap();
        assert!(game.engine.conversation.polygon.is_some());
        assert!(!game.engine.conversation.hidden);
        let expected = game.engine.declarations.windows[0].as_ref().unwrap().contents[0];
        let point = game.engine.inventory_cell_position(0).unwrap();
        assert_eq!(game.engine.conversation_topic_at(point), Some(0));
        assert_ne!(game.engine.conversation.icon, expected);
        game.native_touch_input(command, point[0], point[1]).unwrap();
        assert_eq!(game.engine.conversation.icon, expected, "single topic tap in {command} mode");
        game.advance(24).unwrap();
        assert!(game.fault.is_none());
    }
}

#[cfg(feature = "native-inspection")]
#[test]
#[ignore = "requires private ISO, SoundFont, inventory and conversation saves"]
fn native_walk_dismisses_modals_and_cancels_stale_destinations() {
    let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
    let mut game = BrowserGame::new(read("DISCWORLD_TEST_ISO"), read("DISCWORLD_TEST_SOUNDFONT")).unwrap();
    let room = read("DISCWORLD_TEST_SAVE");
    game.load(&room).unwrap();
    game.native_dismiss_overlays().unwrap();
    assert_eq!(game.engine.active_inventory(), None);
    game.native_touch_input("walk",300,185).unwrap();
    for _ in 0..10 { game.advance(24).unwrap(); }
    let lead = game.engine.declarations.lead.as_ref().unwrap().id;
    let unobstructed_destination = game.engine.movers[&lead].placement.position;
    for page in ["main", "save", "load", "quit"] {
        game.load(&room).unwrap();
        game.native_dismiss_overlays().unwrap();
        game.open_menu(page).unwrap();
        game.native_touch_input("walk", 300, 185).unwrap();
        assert!(!game.menu_active(), "{page}");
        while game.take_menu_request().unwrap().is_some() {}
        for _ in 0..10 { game.advance(24).unwrap(); }
        let lead = game.engine.declarations.lead.as_ref().unwrap().id;
        assert_eq!(game.engine.movers[&lead].placement.position, unobstructed_destination, "{page}");
    }
    // Real window artwork/geometry used by the tutorial's BOGUS_WINDOW overlay.
    game.load(&room).unwrap();
    game.native_dismiss_overlays().unwrap();
    game.engine.top_window = Some(discworld_core::conversation_window::Frame::top_window(
        &game.assets.resources(), game.engine.declarations.window_parts.unwrap()).unwrap());
    game.native_touch_input("walk",300,185).unwrap();
    assert!(game.engine.top_window.is_none());
    for _ in 0..10 { game.advance(24).unwrap(); }
    assert_eq!(game.engine.movers[&lead].placement.position, unobstructed_destination);
    game.load(&read("DISCWORLD_TEST_CONVERSATION_SAVE")).unwrap();
    assert!(game.engine.conversation.polygon.is_some());
    let point = [10,190];
    assert!(!game.engine.inventory_contains_point(point));
    game.native_touch_input("walk", point[0], point[1]).unwrap();
    assert!(game.engine.conversation.polygon.is_none());
    assert!(game.pending_touch_walk.is_some());
    game.input("escape").unwrap();
    assert!(game.pending_touch_walk.is_none(), "new input cancels waiting movement");
    game.load(&room).unwrap();
    game.native_touch_input("walk",160,185).unwrap();
    assert!(game.pending_touch_walk.is_some());
    game.load(&room).unwrap();
    assert!(game.pending_touch_walk.is_none(), "loading must not replay a prior walk gesture");
    for kind in ["pointer", "menu", "pause"] {
        game.load(&room).unwrap();
        game.native_touch_input("walk",160,185).unwrap();
        assert!(game.pending_touch_walk.is_some());
        match kind {
            "pointer" => game.move_pointer_checked(100,100).unwrap(),
            "menu" => game.open_menu("main").unwrap(),
            _ => game.native_cancel_pending_walk(),
        }
        assert!(game.pending_touch_walk.is_none(), "{kind} cancels pending movement");
    }

}

#[cfg(feature = "native-inspection")]
#[test]
#[ignore = "requires DISCWORLD_TEST_ISO and DISCWORLD_TEST_SOUNDFONT"]
fn native_tap_advances_startup_without_start_or_continue_buttons() {
    let read = |name| std::fs::read(std::env::var_os(name).expect(name)).unwrap();
    let mut game = BrowserGame::new(read("DISCWORLD_TEST_ISO"), read("DISCWORLD_TEST_SOUNDFONT")).unwrap();
    for _ in 0..20 { game.advance(24).unwrap(); }
    game.advance(20).unwrap();
    assert!(!game.engine.control.enabled);
    game.native_touch_input("walk",160,100).unwrap();
    for _ in 0..333 { game.advance(24).unwrap(); }
    game.advance(8).unwrap();
    assert_eq!(game.scene(), "rinceroo.scn");
    assert!(game.fault.is_none());
}
