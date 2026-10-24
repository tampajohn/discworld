import XCTest

final class DiscworldLayoutTests: XCTestCase {
    func state(_ app: XCUIApplication) -> [String: Any] {
        let frame = app.descendants(matching: .any).matching(identifier: "game-frame").firstMatch
        guard let value = frame.value as? String,
              let bytes = value.data(using: .utf8),
              let result = try? JSONSerialization.jsonObject(with: bytes) as? [String: Any] else { return [:] }
        return result
    }
    func waitFor(_ app: XCUIApplication, _ condition: @escaping ([String: Any]) -> Bool) {
        let predicate = NSPredicate { _, _ in condition(self.state(app)) }
        wait(for: [expectation(for: predicate, evaluatedWith: app)], timeout: 15)
    }
    func testAudioPrerollAndSteadyPlayback() throws {
        let app = XCUIApplication(bundleIdentifier: "org.tampajohn.discworld.dev")
        app.launchArguments = ["--smoke-room"]
        app.launch()
        XCTAssertTrue(app.buttons["Resume"].waitForExistence(timeout: 40))
        app.buttons["Sound off"].tap()
        XCTAssertEqual(state(app)["audio_playing"] as? Bool, false)
        XCTAssertEqual(state(app)["audio_queued_buffers"] as? Int, 0)
        app.buttons["Resume"].tap()
        waitFor(app) { ($0["completed_audio_buffers"] as? Int ?? 0) >= 120 }
        let steady = state(app)
        XCTAssertEqual(steady["audio_playing"] as? Bool, true)
        XCTAssertEqual(steady["audio_underruns"] as? Int, 0)
        app.buttons["Pause"].tap()
        XCTAssertEqual(state(app)["audio_playing"] as? Bool, false)
        XCTAssertEqual(state(app)["audio_queued_buffers"] as? Int, 0)
        let completed = try XCTUnwrap(steady["completed_audio_buffers"] as? Int)
        app.buttons["Resume"].tap()
        waitFor(app) { ($0["completed_audio_buffers"] as? Int ?? 0) > completed + 24 }
        XCTAssertEqual(state(app)["audio_underruns"] as? Int, 0)
        let evidence = XCTAttachment(string: String(decoding: try JSONSerialization.data(withJSONObject: ["steady": steady, "resumed": state(app)], options: [.prettyPrinted, .sortedKeys]), as: UTF8.self))
        evidence.name = "audio-preroll-evidence"
        evidence.lifetime = .keepAlways
        add(evidence)
    }

    func testBackgroundResumePreservesPauseAndRestartsAudio() throws {
        let app = XCUIApplication(bundleIdentifier: "org.tampajohn.discworld.dev")
        app.launchArguments = ["--smoke-room", "--smoke-interruption-on-background"]
        app.launch()
        XCTAssertTrue(app.buttons["Resume"].waitForExistence(timeout: 40))
        XCUIDevice.shared.orientation = .landscapeRight
        Thread.sleep(forTimeInterval: 1)
        let paused = state(app)
        func backgroundAndReturn() {
            XCUIDevice.shared.press(.home)
            XCTAssertTrue(app.wait(for: .runningBackground, timeout: 5))
            Thread.sleep(forTimeInterval: 3)
            app.activate()
            XCTAssertTrue(app.wait(for: .runningForeground, timeout: 5))
            // Foreground state precedes SpringBoard's landscape transition.
            // A tap during that animation can land at the old portrait location.
            Thread.sleep(forTimeInterval: 1)
            XCTAssertTrue(app.buttons["Sound off"].exists || app.buttons["Sound on"].exists)
        }
        backgroundAndReturn()
        let restoredPause = state(app)
        XCTAssertEqual(restoredPause["tick"] as? Int, paused["tick"] as? Int)
        XCTAssertEqual(restoredPause["frame_sha256"] as? String, paused["frame_sha256"] as? String)
        XCTAssertEqual(restoredPause["paused"] as? Bool, true)
        XCTAssertEqual(restoredPause["muted"] as? Bool, true)
        XCTAssertEqual(restoredPause["audio_ready"] as? Bool, false)
        app.buttons["Sound off"].tap()
        app.buttons["Resume"].tap()
        waitFor(app) { ($0["completed_audio_buffers"] as? Int ?? 0) >= 4 }
        let playing = state(app)
        backgroundAndReturn()
        let completions = try XCTUnwrap(playing["completed_audio_buffers"] as? Int)
        waitFor(app) { $0["audio_ready"] as? Bool == true && ($0["completed_audio_buffers"] as? Int ?? 0) > completions + 4 }
        let resumed = state(app)
        XCTAssertEqual(resumed["audio_interrupted"] as? Bool, false)
        XCTAssertEqual(resumed["paused"] as? Bool, false)
        XCTAssertEqual(resumed["muted"] as? Bool, false)
        XCTAssertEqual(resumed["suspension_tick"] as? Int, resumed["resumption_tick"] as? Int)
        XCTAssertGreaterThan(try XCTUnwrap(resumed["suspension_count"] as? Int), try XCTUnwrap(playing["suspension_count"] as? Int))
        XCTAssertGreaterThan(try XCTUnwrap(resumed["resumption_count"] as? Int), try XCTUnwrap(playing["resumption_count"] as? Int))
        XCTAssertGreaterThan(try XCTUnwrap(resumed["tick"] as? Int), try XCTUnwrap(resumed["resumption_tick"] as? Int))
        let results = try JSONSerialization.data(withJSONObject: ["paused": paused, "restored_pause": restoredPause,
            "playing": playing, "resumed": resumed], options: [.prettyPrinted, .sortedKeys])
        let evidence = XCTAttachment(string: String(decoding: results, as: UTF8.self))
        evidence.name = "background-resume-evidence"
        evidence.lifetime = .keepAlways
        add(evidence)
    }

    func testCanvasWalkingAndPockets() throws {
        let app = XCUIApplication(bundleIdentifier: "org.tampajohn.discworld.dev")
        app.launchArguments = ["--smoke-room"]
        app.launch()
        XCTAssertTrue(app.buttons["Resume"].waitForExistence(timeout: 40))
        XCUIDevice.shared.orientation = .landscapeRight
        app.buttons["Resume"].tap()
        waitFor(app) { ($0["inspection"] as? [String: Any])?["control"] as? Bool == true && ($0["tick"] as? Int ?? 0) > 8520 }
        let before = state(app)
        XCTAssertNotEqual((before["inspection"] as? [String: Any])?["lead_position"] as? [Int], [160, 185])
        let canvas = app.descendants(matching: .any).matching(identifier: "game-frame").firstMatch
        let size = canvas.frame.size
        let scale = min(size.width / 320, size.height / 200)
        let point = CGVector(dx: (size.width - 320 * scale) / 2 + 160.5 * scale,
                             dy: (size.height - 200 * scale) / 2 + 185.5 * scale)
        canvas.coordinate(withNormalizedOffset: .zero).withOffset(point).tap()
        waitFor(app) {
            guard let inspection = $0["inspection"] as? [String: Any] else { return false }
            return inspection["lead_position"] as? [Int] == [160, 185] && inspection["walk_target"] is NSNull
        }
        app.buttons["Pause"].tap()
        let walked = state(app)
        XCTAssertEqual((walked["inspection"] as? [String: Any])?["cursor"] as? [Int], [160, 185])
        app.buttons["Resume"].tap()
        app.buttons["Pockets"].tap()
        waitFor(app) { ($0["inspection"] as? [String: Any])?["inventory"] as? Int == 2 }
        app.buttons["Pause"].tap()
        let pockets = state(app)
        let shot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        shot.name = "touch-opened-pockets"
        shot.lifetime = .keepAlways
        add(shot)
        app.buttons["Resume"].tap()
        let position = try XCTUnwrap((pockets["inspection"] as? [String: Any])?["inventory_position"] as? [Int])
        func coordinate(_ x: Int, _ y: Int) -> XCUICoordinate {
            canvas.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(
                dx: (size.width - 320 * scale) / 2 + (CGFloat(x) + 0.5) * scale,
                dy: (size.height - 200 * scale) / 2 + (CGFloat(y) + 0.5) * scale))
        }
        coordinate(position[0] + 30, position[1] + 8).press(forDuration: 0.1,
            thenDragTo: coordinate(position[0] + 60, position[1] + 28))
        waitFor(app) {
            let inspection = $0["inspection"] as? [String: Any]
            return inspection?["inventory_position"] as? [Int] == [position[0] + 30, position[1] + 20]
                && inspection?["inventory_dragging"] as? Bool == false
        }
        let dragged = state(app)
        // A later pointer action must not keep moving the released window.
        coordinate(160, 185).tap()
        XCTAssertEqual((state(app)["inspection"] as? [String: Any])?["inventory_position"] as? [Int],
                       [position[0] + 30, position[1] + 20])
        app.buttons["Pause"].tap()
        coordinate(position[0] + 60, position[1] + 28).press(forDuration: 0.1,
            thenDragTo: coordinate(position[0] + 80, position[1] + 38))
        XCTAssertEqual((state(app)["inspection"] as? [String: Any])?["inventory_position"] as? [Int],
                       [position[0] + 30, position[1] + 20])
        XCTAssertEqual((state(app)["inspection"] as? [String: Any])?["inventory_dragging"] as? Bool, false)
        app.buttons["Escape"].tap()
        XCTAssertTrue((state(app)["inspection"] as? [String: Any])?["inventory"] is NSNull)
        let results = try JSONSerialization.data(withJSONObject: ["before": before, "walked": walked, "pockets": pockets, "dragged": dragged, "closed": state(app)], options: [.prettyPrinted, .sortedKeys])
        let evidence = XCTAttachment(string: String(decoding: results, as: UTF8.self))
        evidence.name = "canvas-input-evidence"
        evidence.lifetime = .keepAlways
        add(evidence)
    }
    func testFilesDialogCancelPreservesPausedGame() throws {
        let app = XCUIApplication(bundleIdentifier: "org.tampajohn.discworld.dev")
        app.launchArguments = ["--smoke-room"]
        app.launch()
        XCTAssertTrue(app.buttons["Resume"].waitForExistence(timeout: 40))
        XCUIDevice.shared.orientation = .landscapeLeft
        let before = state(app)
        app.buttons["Files"].tap()
        XCTAssertTrue(app.buttons["Export current game"].waitForExistence(timeout: 5))
        XCTAssertTrue(app.buttons["Import saved game"].exists)
        app.buttons["Cancel"].tap()
        XCTAssertTrue(app.buttons["Resume"].exists)
        let after = state(app)
        XCTAssertEqual(after["tick"] as? Int, before["tick"] as? Int)
        XCTAssertEqual(after["frame_sha256"] as? String, before["frame_sha256"] as? String)
        app.buttons["Files"].tap()
        app.buttons["Import saved game"].tap()
        XCTAssertTrue(app.buttons["Cancel"].waitForExistence(timeout: 10))
        let shot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        shot.name = "native-import-picker"
        shot.lifetime = .keepAlways
        add(shot)
        app.buttons["Cancel"].tap()
        XCTAssertTrue(app.buttons["Resume"].waitForExistence(timeout: 5))
        XCTAssertEqual(state(app)["tick"] as? Int, before["tick"] as? Int)
        XCTAssertEqual(state(app)["frame_sha256"] as? String, before["frame_sha256"] as? String)
    }

    func testOriginalMenuNamedSaveAndRelaunchLoad() throws {
        let app = XCUIApplication(bundleIdentifier: "org.tampajohn.discworld.dev")
        app.launchArguments = ["--smoke-room"]
        app.launch()
        XCTAssertTrue(app.buttons["Resume"].waitForExistence(timeout: 40))
        XCUIDevice.shared.orientation = .landscapeRight
        Thread.sleep(forTimeInterval: 1)
        app.buttons["Resume"].tap()
        waitFor(app) { ($0["inspection"] as? [String: Any])?["control"] as? Bool == true }
        app.buttons["Pause"].tap()
        func tapGame(_ x: Int, _ y: Int) {
            let canvas = app.descendants(matching: .any).matching(identifier: "game-frame").firstMatch
            let size = canvas.frame.size
            let scale = min(size.width / 320, size.height / 200)
            canvas.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(
                dx: (size.width - 320 * scale) / 2 + (CGFloat(x) + 0.5) * scale,
                dy: (size.height - 200 * scale) / 2 + (CGFloat(y) + 0.5) * scale)).tap()
        }
        let before = state(app)
        app.buttons["Menu"].tap()
        XCTAssertEqual(state(app)["menu_page"] as? String, "Main")
        tapGame(90, 59)
        waitFor(app) { $0["menu_page"] as? String == "Save" }
        XCTAssertTrue(app.keyboards.firstMatch.waitForExistence(timeout: 5))
        // Tap the real software keys: this canvas-backed UIKeyInput has no
        // UITextField for XCTest's typeText focus heuristic.
        let keyboardShot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        keyboardShot.name = "keyboard-before-typing"
        keyboardShot.lifetime = .keepAlways
        add(keyboardShot)
        let keyboardReady = NSPredicate { _, _ in
            let key = app.keyboards.keys["i"]
            return key.exists && key.isHittable && app.windows.firstMatch.frame.contains(key.frame)
        }
        wait(for: [expectation(for: keyboardReady, evaluatedWith: app)], timeout: 15)
        let keyboardState = try JSONSerialization.data(withJSONObject: state(app), options: [.prettyPrinted, .sortedKeys])
        let keyboardEvidence = XCTAttachment(string: String(decoding: keyboardState, as: UTF8.self))
        keyboardEvidence.name = "keyboard-state-before-typing"
        keyboardEvidence.lifetime = .keepAlways
        add(keyboardEvidence)
        for character in "ipad tesx" { app.keyboards.keys[String(character)].tap() }
        app.keyboards.keys["delete"].tap()
        app.keyboards.keys["t"].tap()
        let named = state(app)
        let shot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        shot.name = "original-save-name-keyboard"
        shot.lifetime = .keepAlways
        add(shot)
        tapGame(262, 69)
        waitFor(app) { $0["status_text"] as? String == "Game saved on this device." && $0["menu"] as? Bool == false }
        app.buttons["Pause"].tap()
        let saved = state(app)
        app.terminate()
        app.launchArguments = ["--smoke-load"]
        app.launch()
        XCTAssertTrue(app.buttons["Resume"].waitForExistence(timeout: 30))
        app.buttons["Resume"].tap()
        waitFor(app) { ($0["inspection"] as? [String: Any])?["control"] as? Bool == true }
        app.buttons["Menu"].tap()
        waitFor(app) { $0["menu_page"] as? String == "Main" }
        tapGame(90, 44)
        waitFor(app) { $0["menu_page"] as? String == "Load" }
        tapGame(262, 69)
        waitFor(app) { $0["status_text"] as? String == "Saved game loaded." && $0["menu"] as? Bool == false }
        app.buttons["Pause"].tap()
        let loaded = state(app)
        XCTAssertEqual(loaded["status_text"] as? String, "Saved game loaded.")
        XCTAssertEqual(loaded["menu"] as? Bool, false)
        XCTAssertGreaterThan(try XCTUnwrap(loaded["tick"] as? Int), try XCTUnwrap(before["tick"] as? Int))
        XCTAssertEqual(loaded["scene"] as? String, before["scene"] as? String)
        XCTAssertEqual((loaded["inspection"] as? [String: Any])?["lead_position"] as? [Int],
                       (before["inspection"] as? [String: Any])?["lead_position"] as? [Int])
        let results = try JSONSerialization.data(withJSONObject: ["before": before, "named": named,
            "saved": saved, "loaded": loaded], options: [.prettyPrinted, .sortedKeys])
        let evidence = XCTAttachment(string: String(decoding: results, as: UTF8.self))
        evidence.name = "original-menu-save-evidence"
        evidence.lifetime = .keepAlways
        add(evidence)
    }

    func testPouchPickupStorageAndRetrieval() throws {
        let app = XCUIApplication(bundleIdentifier: "org.tampajohn.discworld.dev")
        app.launchArguments = ["--smoke-room"]
        app.launch()
        XCTAssertTrue(app.buttons["Resume"].waitForExistence(timeout: 40))
        XCUIDevice.shared.orientation = .landscapeLeft
        app.buttons["Resume"].tap()
        waitFor(app) { ($0["inspection"] as? [String: Any])?["control"] as? Bool == true }
        let canvas = app.descendants(matching: .any).matching(identifier: "game-frame").firstMatch
        func tapGame(_ x: Int, _ y: Int, twice: Bool = false) {
            let size = canvas.frame.size
            let scale = min(size.width / 320, size.height / 200)
            let point = canvas.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(
                dx: (size.width - 320 * scale) / 2 + (CGFloat(x) + 0.5) * scale,
                dy: (size.height - 200 * scale) / 2 + (CGFloat(y) + 0.5) * scale))
            if twice { point.doubleTap() } else { point.tap() }
        }
        func waitItem(_ condition: @escaping ([String: Any]) -> Bool) {
            let predicate = NSPredicate { _, _ in
                guard let inspection = self.state(app)["inspection"] as? [String: Any] else { return false }
                return condition(inspection)
            }
            wait(for: [expectation(for: predicate, evaluatedWith: app)], timeout: 70)
        }
        // Default Walk mode: double taps act, single taps select/store items.
        tapGame(150, 110, twice: true) // Closed wardrobe, original polygon 8.
        waitItem { ($0["enabled_tags"] as? [Int] ?? []).contains(551) && $0["control"] as? Bool == true }
        let opened = state(app)
        tapGame(133, 136, twice: true) // Revealed pouch, original polygon 7.
        waitItem { $0["held_object"] as? Int == 284 && $0["control"] as? Bool == true }
        let held = state(app)
        app.buttons["Pockets"].tap()
        waitItem { $0["inventory"] as? Int == 2 }
        let cell = try XCTUnwrap((state(app)["inspection"] as? [String: Any])?["inventory_first_cell"] as? [Int])
        app.buttons["Walk"].tap()
        tapGame(cell[0], cell[1])
        waitItem { $0["held_object"] as? Int == -1 && ($0["pockets"] as? [Int] ?? []).contains(284) }
        app.buttons["Pause"].tap()
        let stored = state(app)
        let shot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        shot.name = "touch-stored-pouch"
        shot.lifetime = .keepAlways
        add(shot)
        app.buttons["Save"].tap()
        app.buttons["Resume"].tap()
        tapGame(cell[0], cell[1])
        waitItem { $0["held_object"] as? Int == 284 }
        let retrieved = state(app)
        app.buttons["Pause"].tap()
        app.buttons["Load"].tap()
        let restored = state(app)
        XCTAssertEqual((restored["inspection"] as? [String: Any])?["held_object"] as? Int, -1)
        XCTAssertEqual((restored["inspection"] as? [String: Any])?["pockets"] as? [Int], [284])
        XCTAssertEqual(restored["frame_sha256"] as? String, stored["frame_sha256"] as? String)
        let results = try JSONSerialization.data(withJSONObject: ["opened": opened, "held": held,
            "stored": stored, "retrieved": retrieved, "restored": restored], options: [.prettyPrinted, .sortedKeys])
        let evidence = XCTAttachment(string: String(decoding: results, as: UTF8.self))
        evidence.name = "pouch-touch-evidence"
        evidence.lifetime = .keepAlways
        add(evidence)
    }

    func testSoundSliderPersistsAcrossRelaunch() throws {
        let app = XCUIApplication(bundleIdentifier: "org.tampajohn.discworld.dev")
        app.launchArguments = ["--smoke-room"]
        app.launch()
        XCTAssertTrue(app.buttons["Resume"].waitForExistence(timeout: 40))
        XCUIDevice.shared.orientation = .landscapeLeft
        app.buttons["Resume"].tap()
        waitFor(app) { ($0["inspection"] as? [String: Any])?["control"] as? Bool == true }
        func point(_ x: Int, _ y: Int) -> XCUICoordinate {
            let canvas = app.descendants(matching: .any).matching(identifier: "game-frame").firstMatch
            let size = canvas.frame.size
            let scale = min(size.width / 320, size.height / 200)
            return canvas.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(
                dx: (size.width - 320 * scale) / 2 + (CGFloat(x) + 0.5) * scale,
                dy: (size.height - 200 * scale) / 2 + (CGFloat(y) + 0.5) * scale))
        }
        func volumes() -> [Int]? {
            ((state(app)["inspection"] as? [String: Any])?["settings"] as? [String: Any])?["volumes"] as? [Int]
        }
        app.buttons["Menu"].tap()
        point(90, 89).tap() // Main -> Sound.
        waitFor(app) { $0["menu_page"] as? String == "Sound" }
        let original = try XCTUnwrap(volumes())
        let initialX = 170 + 81 * original[0] / 255 + 4
        // Drag to the low end regardless of the previous stored preference.
        point(initialX, 53).press(forDuration: 0.1, thenDragTo: point(174, 53))
        XCTAssertEqual(volumes()?[0], 0)
        // Then move to a known intermediate position, exercising nonzero audio gain.
        point(174, 53).press(forDuration: 0.1, thenDragTo: point(214, 53))
        let changed = try XCTUnwrap(volumes())
        XCTAssertEqual(changed[0], 125) // floor(40 * 255 / 81), original slider quantization.
        XCTAssertEqual(Array(changed.dropFirst()), Array(original.dropFirst()))
        XCTAssertEqual((state(app)["inspection"] as? [String: Any])?["inventory_dragging"] as? Bool, false)
        let adjusted = state(app)
        let shot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        shot.name = "original-sound-slider"
        shot.lifetime = .keepAlways
        add(shot)
        app.buttons["Escape"].tap() // Persists original configuration settings.
        waitFor(app) { $0["menu_page"] as? String == "Main" }
        app.terminate()
        // Bootstrap without restoring the old quick-save configuration.
        app.launchArguments = ["--smoke-room"]
        app.launch()
        XCTAssertTrue(app.buttons["Resume"].waitForExistence(timeout: 40))
        XCTAssertEqual(volumes(), changed)
        let restored = state(app)
        let results = try JSONSerialization.data(withJSONObject: ["adjusted": adjusted, "restored": restored], options: [.prettyPrinted, .sortedKeys])
        let evidence = XCTAttachment(string: String(decoding: results, as: UTF8.self))
        evidence.name = "sound-slider-evidence"
        evidence.lifetime = .keepAlways
        add(evidence)
    }

    func testStairsRoundTripAndSave() throws {
        let app = XCUIApplication(bundleIdentifier: "org.tampajohn.discworld.dev")
        app.launchArguments = ["--smoke-room"]
        app.launch()
        XCTAssertTrue(app.buttons["Resume"].waitForExistence(timeout: 40))
        XCUIDevice.shared.orientation = .landscapeLeft
        app.buttons["Resume"].tap()
        waitFor(app) { ($0["inspection"] as? [String: Any])?["control"] as? Bool == true }
        let canvas = app.descendants(matching: .any).matching(identifier: "game-frame").firstMatch
        func tapGame(_ x: Int, _ y: Int) {
            XCTAssertTrue((0..<320).contains(x) && (0..<200).contains(y))
            let size = canvas.frame.size
            let scale = min(size.width / 320, size.height / 200)
            canvas.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(
                dx: (size.width - 320 * scale) / 2 + (CGFloat(x) + 0.5) * scale,
                dy: (size.height - 200 * scale) / 2 + (CGFloat(y) + 0.5) * scale)).tap()
        }
        func waitScene(_ name: String) {
            let predicate = NSPredicate { _, _ in
                let value = self.state(app)
                let inspection = value["inspection"] as? [String: Any]
                return value["scene"] as? String == name && inspection?["control"] as? Bool == true
                    && inspection?["walk_target"] is NSNull
            }
            wait(for: [expectation(for: predicate, evaluatedWith: app)], timeout: 70)
        }
        app.buttons["Action"].tap()
        let started = try XCTUnwrap(state(app)["tick"] as? Int)
        tapGame(55, 125)
        waitFor(app) { ($0["tick"] as? Int ?? 0) > started + 80 && ($0["inspection"] as? [String: Any])?["control"] as? Bool == true }
        app.buttons["Walk"].tap()
        tapGame(55, 125)
        waitScene("unseen.scn")
        let landing = state(app)
        let landingPosition = try XCTUnwrap((landing["inspection"] as? [String: Any])?["lead_position"] as? [Int])
        let landingTick = try XCTUnwrap(landing["tick"] as? Int)
        tapGame(175, 180) // Walk along the stairs after the scripted room exit.
        let walked = NSPredicate { _, _ in
            let value = self.state(app)
            let inspection = value["inspection"] as? [String: Any]
            return value["scene"] as? String == "unseen.scn"
                && (value["tick"] as? Int ?? 0) > landingTick + 10
                && inspection?["lead_position"] as? [Int] != landingPosition
                && inspection?["control"] as? Bool == true
                && inspection?["walk_target"] is NSNull
        }
        wait(for: [expectation(for: walked, evaluatedWith: app)], timeout: 70)
        app.buttons["Pause"].tap()
        let downstairs = state(app)
        app.buttons["Save"].tap()
        let shot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        shot.name = "touch-downstairs"
        shot.lifetime = .keepAlways
        add(shot)
        let origin = try XCTUnwrap((downstairs["inspection"] as? [String: Any])?["world_origin"] as? [Int])
        app.buttons["Resume"].tap()
        // Original university exit 48 returns to Rincewind's room.
        tapGame(786 - origin[0], 65 - origin[1])
        waitScene("rinceroo.scn")
        app.buttons["Pause"].tap()
        let returned = state(app)
        app.buttons["Load"].tap()
        let loaded = state(app)
        XCTAssertEqual(loaded["scene"] as? String, "unseen.scn")
        XCTAssertEqual(loaded["frame_sha256"] as? String, downstairs["frame_sha256"] as? String)
        XCTAssertEqual(loaded["tick"] as? Int, downstairs["tick"] as? Int)
        app.buttons["Resume"].tap()
        tapGame(786 - origin[0], 65 - origin[1])
        waitScene("rinceroo.scn")
        app.buttons["Pause"].tap()
        let results = try JSONSerialization.data(withJSONObject: ["landing": landing, "downstairs": downstairs,
            "returned": returned, "loaded": loaded, "returned_after_load": state(app)], options: [.prettyPrinted, .sortedKeys])
        let evidence = XCTAttachment(string: String(decoding: results, as: UTF8.self))
        evidence.name = "stairs-touch-evidence"
        evidence.lifetime = .keepAlways
        add(evidence)
    }

    func testTappedPlaybackAudioAndPersistentSave() throws {
        let app = XCUIApplication(bundleIdentifier: "org.tampajohn.discworld.dev")
        app.launchArguments = ["--smoke-room"]
        app.launch()
        XCTAssertTrue(app.buttons["Resume"].waitForExistence(timeout: 40))
        XCUIDevice.shared.orientation = .landscapeLeft
        app.buttons["Save"].tap()
        let saved = state(app)
        XCTAssertEqual(saved["last_operation"] as? String, "save")
        let tick = try XCTUnwrap(saved["tick"] as? Int)
        app.buttons["Sound off"].tap()
        XCTAssertTrue(app.buttons["Sound on"].exists)
        app.buttons["Resume"].tap()
        waitFor(app) { ($0["tick"] as? Int ?? 0) > tick + 8 && ($0["completed_audio_buffers"] as? Int ?? 0) >= 3 }
        let playing = state(app)
        XCTAssertEqual(playing["audio_ready"] as? Bool, true)
        app.buttons["Sound on"].tap()
        XCTAssertEqual(state(app)["muted"] as? Bool, true)
        let mutedAt = try XCTUnwrap(state(app)["tick"] as? Int)
        waitFor(app) { ($0["tick"] as? Int ?? 0) > mutedAt + 5 }
        let mutedState = state(app)
        app.buttons["Pause"].tap()
        XCTAssertEqual(state(app)["paused"] as? Bool, true)
        app.buttons["Load"].tap()
        let loaded = state(app)
        XCTAssertEqual(loaded["last_operation"] as? String, "load")
        XCTAssertEqual(loaded["tick"] as? Int, tick)
        XCTAssertEqual(loaded["frame_sha256"] as? String, saved["frame_sha256"] as? String)
        app.terminate()
        app.launchArguments = ["--smoke-load"]
        app.launch()
        XCTAssertTrue(app.buttons["Resume"].waitForExistence(timeout: 20))
        XCTAssertEqual(state(app)["tick"] as? Int, tick)
        XCTAssertEqual(state(app)["frame_sha256"] as? String, saved["frame_sha256"] as? String)
        let results = try JSONSerialization.data(withJSONObject: ["saved": saved, "playing": playing, "muted": mutedState, "loaded": loaded, "relaunched": state(app)], options: [.prettyPrinted, .sortedKeys])
        let evidence = XCTAttachment(string: String(decoding: results, as: UTF8.self))
        evidence.name = "tap-control-state-evidence"
        evidence.lifetime = .keepAlways
        add(evidence)
        let shot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
        shot.name = "tap-save-relaunch-load"
        shot.lifetime = .keepAlways
        add(shot)
    }
    func testLandscapeAndPortraitKeepControlsReachable() {
        let app = XCUIApplication(bundleIdentifier: "org.tampajohn.discworld.dev")
        app.launchArguments = ["--smoke-room"]
        app.launch()
        XCTAssertTrue(app.buttons["Resume"].waitForExistence(timeout: 40))
        for orientation in [UIDeviceOrientation.landscapeLeft, .portrait, .landscapeRight] {
            XCUIDevice.shared.orientation = orientation
            let landscape = orientation != .portrait
            let predicate = NSPredicate { _, _ in
                let frame = app.windows.firstMatch.frame
                return landscape ? frame.width > frame.height : frame.height > frame.width
            }
            let changed = expectation(for: predicate, evaluatedWith: app)
            wait(for: [changed], timeout: 15)
            for title in ["Start", "Continue", "Pockets", "Close", "Escape", "Menu", "Files", "Walk", "Action", "Look", "Save", "Load", "Sound off", "Resume"] {
                XCTAssertTrue(app.buttons[title].isHittable, "\(title), orientation \(orientation.rawValue)")
                XCTAssertTrue(app.windows.firstMatch.frame.contains(app.buttons[title].frame), "Clipped \(title)")
            }
            let shot = XCTAttachment(screenshot: XCUIScreen.main.screenshot())
            shot.name = "orientation-\(orientation.rawValue)"
            shot.lifetime = .keepAlways
            add(shot)
        }
    }
}
