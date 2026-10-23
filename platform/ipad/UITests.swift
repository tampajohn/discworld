import XCTest

final class DiscworldLayoutTests: XCTestCase {
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
