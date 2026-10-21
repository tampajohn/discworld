//! Opt-in integration checks using locally supplied original assets.
use super::BrowserGame;

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
    assert_eq!(u32::from_le_bytes(pending[60..64].try_into().unwrap()), 5);
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
    assert_eq!(u32::from_le_bytes(continued[60..64].try_into().unwrap()), 4);
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
    assert_eq!(u32::from_le_bytes(pending[60..64].try_into().unwrap()), 6);
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
        u32::from_le_bytes(game.save().unwrap()[60..64].try_into().unwrap()),
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
}
