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
