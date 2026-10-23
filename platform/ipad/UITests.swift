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
    func testCanvasWalkingAndPockets() throws {
        let app = XCUIApplication(bundleIdentifier: "org.tampajohn.discworld.dev")
        app.launchArguments = ["--smoke-room"]
        app.launch()
        XCTAssertTrue(app.buttons["Resume"].waitForExistence(timeout: 40))
        XCUIDevice.shared.orientation = .landscapeLeft
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
        app.buttons["Close"].tap()
        XCTAssertTrue((state(app)["inspection"] as? [String: Any])?["inventory"] is NSNull)
        let results = try JSONSerialization.data(withJSONObject: ["before": before, "walked": walked, "pockets": pockets, "dragged": dragged, "closed": state(app)], options: [.prettyPrinted, .sortedKeys])
        let evidence = XCTAttachment(string: String(decoding: results, as: UTF8.self))
        evidence.name = "canvas-input-evidence"
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
        func tapGame(_ x: Int, _ y: Int) {
            let size = canvas.frame.size
            let scale = min(size.width / 320, size.height / 200)
            canvas.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(
                dx: (size.width - 320 * scale) / 2 + (CGFloat(x) + 0.5) * scale,
                dy: (size.height - 200 * scale) / 2 + (CGFloat(y) + 0.5) * scale)).tap()
        }
        func waitItem(_ condition: @escaping ([String: Any]) -> Bool) {
            let predicate = NSPredicate { _, _ in
                guard let inspection = self.state(app)["inspection"] as? [String: Any] else { return false }
                return condition(inspection)
            }
            wait(for: [expectation(for: predicate, evaluatedWith: app)], timeout: 70)
        }
        app.segmentedControls.buttons["Action"].tap()
        tapGame(150, 110) // Closed wardrobe, original polygon 8.
        waitItem { ($0["enabled_tags"] as? [Int] ?? []).contains(551) && $0["control"] as? Bool == true }
        let opened = state(app)
        tapGame(133, 136) // Revealed pouch, original polygon 7.
        waitItem { $0["held_object"] as? Int == 284 && $0["control"] as? Bool == true }
        let held = state(app)
        app.buttons["Pockets"].tap()
        waitItem { $0["inventory"] as? Int == 2 }
        let cell = try XCTUnwrap((state(app)["inspection"] as? [String: Any])?["inventory_first_cell"] as? [Int])
        app.segmentedControls.buttons["Walk"].tap()
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
            for title in ["Start", "Continue", "Pockets", "Close", "Escape", "Save", "Load", "Sound off", "Resume"] {
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
