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
