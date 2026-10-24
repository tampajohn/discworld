import UIKit
import AVFoundation
import CryptoKit
import GameController
import UniformTypeIdentifiers

@main
final class AppDelegate: UIResponder, UIApplicationDelegate {
    func application(_ application: UIApplication, configurationForConnecting session: UISceneSession, options: UIScene.ConnectionOptions) -> UISceneConfiguration {
        let configuration = UISceneConfiguration(name: "Game", sessionRole: session.role)
        configuration.delegateClass = GameScene.self
        return configuration
    }
}

final class GameScene: UIResponder, UIWindowSceneDelegate {
    var window: UIWindow?
    func scene(_ scene: UIScene, willConnectTo session: UISceneSession, options: UIScene.ConnectionOptions) {
        guard let scene = scene as? UIWindowScene else { return }
        let window = UIWindow(windowScene: scene)
        window.rootViewController = GameController()
        window.makeKeyAndVisible()
        self.window = window
    }
}

// Labels are rasterized from the ISO's tag font during the private build.
final class GameButton: UIButton {
    override func setTitle(_ title: String?, for state: UIControl.State) {
        guard state == .normal, let title else { super.setTitle(title, for: state); return }
        accessibilityLabel = title
        let root = Bundle.main.bundleURL.appendingPathComponent("Controls")
        let normal = UIImage(contentsOfFile: root.appendingPathComponent("\(title)-normal.png").path)
        let selected = UIImage(contentsOfFile: root.appendingPathComponent("\(title)-selected.png").path)
        super.setTitle(normal == nil ? title : nil, for: .normal)
        setImage(normal, for: .normal)
        setImage(selected, for: .selected)
        setImage(selected, for: .highlighted)
        imageView?.contentMode = .scaleAspectFit
        imageView?.layer.magnificationFilter = .nearest
        imageView?.layer.minificationFilter = .nearest
    }
    override func layoutSubviews() {
        super.layoutSubviews()
        imageView?.frame = bounds.insetBy(dx: 2, dy: 4)
    }
}

// Preserve the down position before UIKit crosses its pan-recognition threshold.
final class InventoryPan: UIPanGestureRecognizer {
    var start: CGPoint?
    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent) {
        if start == nil, let touch = touches.first { start = touch.location(in: view) }
        super.touchesBegan(touches, with: event)
    }
    override func reset() {
        super.reset()
        start = nil
    }
}

final class GameController: UIViewController, UIKeyInput, UIDocumentPickerDelegate {
    override var supportedInterfaceOrientations: UIInterfaceOrientationMask {
        UIDevice.current.userInterfaceIdiom == .phone ? .landscape : .all
    }

    let picture = UIImageView()
    let status = UILabel()
    let audioEngine = AVAudioEngine()
    let player = AVAudioPlayerNode()
    let format = AVAudioFormat(standardFormatWithSampleRate: 22050, channels: 2)!
    var link: CADisplayLink?
    var previous = 0.0
    var accumulator = 0.0
    var queued = 0
    var generation = 0
    var ready = false
    var paused = false
    var muted = true
    var audioReady = false
    var interrupted = false
    var suspensionTick: Int?
    var resumptionTick: Int?
    var suspensionCount = 0
    var resumptionCount = 0
    var dragging = false
    var modeButtons: [UIButton] = []
    var action = "walk"
    var snapshot: [String: Any] = [:]
    var keyboardFrame: [Double] = []
    var keyboardChanges = 0
    var importingSave = false
    var exportedSaveDirectory: URL?
    var completedAudioBuffers = 0
    let pauseButton = GameButton(type: .custom)

    override var canBecomeFirstResponder: Bool { true }
    var hasText: Bool { true }
    var keyboardType: UIKeyboardType = .asciiCapable
    var autocorrectionType: UITextAutocorrectionType = .no
    var autocapitalizationType: UITextAutocapitalizationType = .none
    func insertText(_ text: String) {
        let printable = String(text.unicodeScalars.filter { $0.value >= 32 && $0.value < 127 })
        guard !printable.isEmpty else { return }
        for chunk in stride(from: 0, to: printable.utf8.count, by: 80) {
            let bytes = Array(printable.utf8.dropFirst(chunk).prefix(80))
            send(["op": "edit-menu", "text": String(decoding: bytes, as: UTF8.self)])
        }
    }
    func deleteBackward() { send(["op": "edit-menu", "backspace": true]) }

    override func viewDidLayoutSubviews() {
        super.viewDidLayoutSubviews()
        DispatchQueue.main.async { [weak self] in self?.recordGeometry() }
    }
    func recordGeometry() {
        guard ProcessInfo.processInfo.arguments.contains(where: { $0.hasPrefix("--smoke-") }),
              let scene = view.window?.windowScene else { return }
        let orientation: UIInterfaceOrientation
        if #available(iOS 26.0, *) { orientation = scene.effectiveGeometry.interfaceOrientation }
        else { orientation = scene.interfaceOrientation }
        let result: [String: Any] = [
            "view_width": view.bounds.width, "view_height": view.bounds.height,
            "picture_width": picture.bounds.width, "picture_height": picture.bounds.height,
            "interface_orientation": orientation.rawValue,
            "safe_top": view.safeAreaInsets.top, "safe_bottom": view.safeAreaInsets.bottom
        ]
        if let bytes = try? JSONSerialization.data(withJSONObject: result),
           let documents = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first {
            try? bytes.write(to: documents.appendingPathComponent("geometry-probe.json"), options: .atomic)
        }
    }
    override func viewDidLoad() {
        super.viewDidLoad()
        overrideUserInterfaceStyle = .dark
        view.backgroundColor = .black
        picture.contentMode = .scaleAspectFit
        picture.layer.magnificationFilter = .nearest
        picture.isUserInteractionEnabled = true
        picture.isAccessibilityElement = true
        picture.accessibilityIdentifier = "game-frame"
        picture.accessibilityLabel = "Discworld game"
        let tap = UITapGestureRecognizer(target: self, action: #selector(tap(_:)))
        let doubleTap = UITapGestureRecognizer(target: self, action: #selector(doubleTap(_:)))
        doubleTap.numberOfTapsRequired = 2
        tap.require(toFail: doubleTap)
        let pan = InventoryPan(target: self, action: #selector(drag(_:)))
        pan.maximumNumberOfTouches = 1
        tap.require(toFail: pan)
        doubleTap.require(toFail: pan)
        picture.addGestureRecognizer(doubleTap)
        picture.addGestureRecognizer(tap)
        picture.addGestureRecognizer(pan)
        status.textColor = .white
        status.numberOfLines = 2
        status.text = "Loading Discworld…"
        let modes = UIStackView()
        modes.axis = .horizontal
        modes.distribution = .fillEqually
        for (index, label) in ["Walk", "Action", "Look"].enumerated() {
            let mode = GameButton(type: .custom)
            mode.setTitle(label, for: .normal)
            mode.isSelected = index == 0
            mode.addAction(UIAction { [weak self, weak mode] _ in
                self?.action = ["walk", "action", "look"][index]
                for button in self?.modeButtons ?? [] { button.isSelected = button === mode }
            }, for: .touchUpInside)
            modeButtons.append(mode)
            modes.addArrangedSubview(mode)
        }
        let row = UIStackView()
        row.axis = .horizontal
        row.spacing = 8
        row.distribution = .fillEqually
        for (label, command) in [("Start", "start"), ("Continue", "continue"), ("Pockets", "pockets"), ("Close", "close"), ("Escape", "escape")] {
            row.addArrangedSubview(button(label) { [weak self] in self?.send(["op": "input", "command": command]) })
        }
        row.addArrangedSubview(button("Menu") { [weak self] in
            guard let self else { return }
            self.releaseDrag()
            self.paused = false
            self.resetAudio()
            self.pauseButton.setTitle("Pause", for: .normal)
            self.send(["op": "menu", "page": "main"])
        })
        row.addArrangedSubview(button("Files") { [weak self] in self?.showSaveFiles() })
        let tools = UIStackView()
        tools.axis = .horizontal
        tools.spacing = 8
        tools.distribution = .fillProportionally
        tools.addArrangedSubview(modes)
        tools.addArrangedSubview(button("Save") { [weak self] in self?.send(["op": "save"]) })
        tools.addArrangedSubview(button("Load") { [weak self] in self?.releaseDrag(); self?.resetAudio(); self?.send(["op": "load"]) })
        let mute = GameButton(type: .custom)
        mute.setTitle("Sound off", for: .normal)
        mute.addAction(UIAction { [weak self] _ in
            guard let self else { return }
            self.muted.toggle()
            if !self.muted { self.startAudio() }
            self.player.volume = self.muted ? 0 : 1
            mute.setTitle(self.muted ? "Sound off" : "Sound on", for: .normal)
            self.render()
        }, for: .touchUpInside)
        tools.addArrangedSubview(mute)
        let pause = pauseButton
        pause.setTitle("Pause", for: .normal)
        pause.addAction(UIAction { [weak self] _ in
            guard let self else { return }
            self.releaseDrag()
            self.paused.toggle(); self.resetAudio()
            if !self.paused && !self.muted && !self.audioReady { self.startAudio() }
            pause.setTitle(self.paused ? "Resume" : "Pause", for: .normal)
            self.render()
        }, for: .touchUpInside)
        tools.addArrangedSubview(pause)
        let stack: UIStackView
        if UIDevice.current.userInterfaceIdiom == .phone {
            // Landscape phones keep the original scene tall, with controls in
            // two side columns rather than consuming its vertical space.
            row.axis = .vertical
            row.spacing = 4
            tools.axis = .vertical
            tools.spacing = 4
            tools.distribution = .fillEqually
            tools.removeArrangedSubview(modes)
            modes.removeFromSuperview()
            for (index, mode) in modes.arrangedSubviews.enumerated() {
                modes.removeArrangedSubview(mode)
                mode.removeFromSuperview()
                tools.insertArrangedSubview(mode, at: index)
            }
            let gameRow = UIStackView(arrangedSubviews: [row, picture, tools])
            gameRow.axis = .horizontal
            gameRow.spacing = 6
            stack = UIStackView(arrangedSubviews: [gameRow, status])
            status.numberOfLines = 1
            NSLayoutConstraint.activate([
                row.widthAnchor.constraint(equalToConstant: 90),
                tools.widthAnchor.constraint(equalToConstant: 90),
                status.heightAnchor.constraint(equalToConstant: 18)
            ])
        } else {
            stack = UIStackView(arrangedSubviews: [picture, row, tools, status])
            NSLayoutConstraint.activate([
                row.heightAnchor.constraint(equalToConstant: 44),
                tools.heightAnchor.constraint(equalToConstant: 44),
                status.heightAnchor.constraint(equalToConstant: 40)
            ])
        }
        stack.axis = .vertical
        stack.spacing = 6
        stack.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.leadingAnchor, constant: 8),
            stack.trailingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.trailingAnchor, constant: -8),
            stack.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor),
            stack.bottomAnchor.constraint(equalTo: view.keyboardLayoutGuide.topAnchor, constant: -8)
        ])
        audioEngine.attach(player)
        audioEngine.connect(player, to: audioEngine.mainMixerNode, format: format)
        NotificationCenter.default.addObserver(self, selector: #selector(keyboardChanged(_:)), name: UIResponder.keyboardWillChangeFrameNotification, object: nil)
        NotificationCenter.default.addObserver(self, selector: #selector(suspended), name: UIApplication.willResignActiveNotification, object: nil)
        NotificationCenter.default.addObserver(self, selector: #selector(resumed), name: UIApplication.didBecomeActiveNotification, object: nil)
        NotificationCenter.default.addObserver(self, selector: #selector(audioInterrupted(_:)), name: AVAudioSession.interruptionNotification, object: nil)
        DispatchQueue.main.async { self.openGame() }
    }

    @objc func keyboardChanged(_ notification: Notification) {
        if let rect = notification.userInfo?[UIResponder.keyboardFrameEndUserInfoKey] as? CGRect {
            keyboardFrame = [rect.origin.x, rect.origin.y, rect.width, rect.height].map(Double.init)
            keyboardChanges += 1
        }
    }

    func button(_ title: String, _ body: @escaping () -> Void) -> UIButton {
        let button = GameButton(type: .custom)
        button.setTitle(title, for: .normal)
        button.addAction(UIAction { _ in body() }, for: .touchUpInside)
        return button
    }
    func call(_ request: [String: Any]) throws -> [String: Any] {
        dispatchPrecondition(condition: .onQueue(.main))
        let bytes = try JSONSerialization.data(withJSONObject: request)
        dw_ipad_request_clear()
        for byte in bytes {
            guard dw_ipad_request_byte(byte) == 0 else { throw failure("Request is too large") }
        }
        let statusCode = dw_ipad_call()
        let response = Data(bytes: dw_ipad_response()!, count: dw_ipad_response_len())
        if statusCode != 0 { throw failure(String(decoding: response, as: UTF8.self)) }
        let result = try JSONSerialization.jsonObject(with: response) as! [String: Any]
        snapshot = result
        snapshot["last_operation"] = request["op"]
        return result
    }
    func showSaveFiles() {
        guard ready, presentedViewController == nil else { return }
        guard snapshot["menu"] as? Bool != true else {
            status.text = "Close the game menu before transferring saves."
            return
        }
        releaseDrag()
        paused = true
        resetAudio()
        pauseButton.setTitle("Resume", for: .normal)
        resignFirstResponder()
        render()
        let sheet = UIAlertController(title: "Save files", message: nil, preferredStyle: .actionSheet)
        sheet.addAction(UIAlertAction(title: "Export current game", style: .default) { [weak self] _ in self?.exportSave() })
        sheet.addAction(UIAlertAction(title: "Import saved game", style: .default) { [weak self] _ in self?.importSave() })
        sheet.addAction(UIAlertAction(title: "Cancel", style: .cancel))
        sheet.popoverPresentationController?.sourceView = view
        sheet.popoverPresentationController?.sourceRect = CGRect(x: view.bounds.midX, y: view.bounds.maxY - 60, width: 1, height: 1)
        present(sheet, animated: true)
    }
    func exportSave() {
        do {
            let directory = FileManager.default.temporaryDirectory.appendingPathComponent("discworld-export-" + UUID().uuidString)
            try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
            exportedSaveDirectory = directory
            let file = directory.appendingPathComponent("Discworld.dws")
            _ = try call(["op": "export", "path": file.path])
            importingSave = false
            let picker = UIDocumentPickerViewController(forExporting: [file], asCopy: true)
            picker.delegate = self
            present(picker, animated: true)
        } catch { cleanupExport(); status.text = "Export failed: \(error.localizedDescription)" }
    }
    func importSave() {
        importingSave = true
        let picker = UIDocumentPickerViewController(forOpeningContentTypes: [.data], asCopy: true)
        picker.allowsMultipleSelection = false
        picker.delegate = self
        present(picker, animated: true)
    }
    func cleanupExport() {
        if let directory = exportedSaveDirectory { try? FileManager.default.removeItem(at: directory) }
        exportedSaveDirectory = nil
    }
    func documentPicker(_ controller: UIDocumentPickerViewController, didPickDocumentsAt urls: [URL]) {
        defer { cleanupExport() }
        if importingSave {
            guard let file = urls.first else { return }
            let scoped = file.startAccessingSecurityScopedResource()
            defer { if scoped { file.stopAccessingSecurityScopedResource() } }
            do {
                resetAudio()
                _ = try call(["op": "import", "path": file.path])
                render()
                status.text = "Imported game loaded. Save to keep it on this device; Resume to play."
            } catch { status.text = "Import failed: \(error.localizedDescription)" }
        } else { status.text = "Save exported. Tap Resume to play." }
    }
    func documentPickerWasCancelled(_ controller: UIDocumentPickerViewController) {
        cleanupExport()
        status.text = "Transfer cancelled. Tap Resume to play."
    }
    func failure(_ message: String) -> NSError { NSError(domain: "Discworld", code: 1, userInfo: [NSLocalizedDescriptionKey: message]) }
    func openGame() {
        do {
            let bundle = Bundle.main.bundleURL.appendingPathComponent("GameData.bundle")
            let saves = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
            var result = try call(["op": "open", "bundle": bundle.path, "saves": saves.path])
            let startupNotice = result["notice"] as? String
            if ProcessInfo.processInfo.arguments.contains("--smoke-room") {
                for tick in 0..<8500 {
                    if tick == 500 { _ = try call(["op": "input", "command": "start"]) }
                    result = try call(["op": "advance"])
                }
                let frame = Data(bytes: dw_ipad_frame()!, count: dw_ipad_frame_len())
                _ = try call(["op": "save"])
                _ = try call(["op": "advance"])
                result = try call(["op": "load"])
                guard frame == Data(bytes: dw_ipad_frame()!, count: dw_ipad_frame_len()) else { throw failure("Quick-load changed the saved frame") }
                result["quick_save_reload_frame_equal"] = true
                try JSONSerialization.data(withJSONObject: result).write(to: saves.appendingPathComponent("startup-probe.json"))
                paused = true
                pauseButton.setTitle("Resume", for: .normal)
            }
            if ProcessInfo.processInfo.arguments.contains("--smoke-load") {
                result = try call(["op": "load"])
                try JSONSerialization.data(withJSONObject: result).write(to: saves.appendingPathComponent("reload-probe.json"))
                paused = true
                pauseButton.setTitle("Resume", for: .normal)
            }
            ready = true
            render()
            status.text = startupNotice ?? ""
            link = CADisplayLink(target: self, selector: #selector(step(_:)))
            link?.add(to: .main, forMode: .common)
        } catch { status.text = error.localizedDescription }
    }
    func send(_ request: [String: Any]) {
        guard ready else { return }
        do { try handle(call(request)); render() }
        catch { status.text = error.localizedDescription; paused = true; resetAudio() }
    }
    func handle(_ result: [String: Any]) throws {
        if result["reset_audio"] as? Bool == true { dragging = false; resetAudio() }
        if let notice = result["notice"] as? String { status.text = notice }
        if result["quit"] as? Bool == true {
            paused = true
            resetAudio()
            pauseButton.setTitle("Resume", for: .normal)
            status.text = "Game ended. Load a saved game to continue."
        }
        if result["editing"] as? Bool == true {
            if !isFirstResponder { becomeFirstResponder() }
        } else if isFirstResponder { resignFirstResponder() }
    }
    func render() {
        guard dw_ipad_frame_len() == 320 * 200 * 4 else { return }
        let data = Data(bytes: dw_ipad_frame()!, count: dw_ipad_frame_len())
        guard let provider = CGDataProvider(data: data as CFData),
              let image = CGImage(width: 320, height: 200, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: 1280,
                                  space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
                                  provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent) else { return }
        picture.image = UIImage(cgImage: image)
        // Only diagnostic launches expose runtime counters to XCTest. Normal
        // accessibility describes the game without internal implementation data.
        if ProcessInfo.processInfo.arguments.contains(where: { $0.hasPrefix("--smoke-") }) {
            var state = snapshot
            state["frame_sha256"] = SHA256.hash(data: data).map { String(format: "%02x", $0) }.joined()
            state["keyboard_first_responder"] = isFirstResponder
            state["hardware_keyboard_connected"] = GCKeyboard.coalesced != nil
            state["keyboard_frame"] = keyboardFrame
            state["keyboard_changes"] = keyboardChanges
            state["status_text"] = status.text
            state["paused"] = paused
            state["muted"] = muted
            state["audio_ready"] = audioReady
            state["audio_interrupted"] = interrupted
            state["suspension_tick"] = suspensionTick
            state["resumption_tick"] = resumptionTick
            state["suspension_count"] = suspensionCount
            state["resumption_count"] = resumptionCount
            state["completed_audio_buffers"] = completedAudioBuffers
            if let bytes = try? JSONSerialization.data(withJSONObject: state) {
                picture.accessibilityValue = String(decoding: bytes, as: UTF8.self)
            }
        }
    }
    func gamePoint(_ point: CGPoint, clamp: Bool = false) -> [Int]? {
        let scale = min(picture.bounds.width / 320, picture.bounds.height / 200)
        guard scale > 0 else { return nil }
        let origin = CGPoint(x: (picture.bounds.width - 320 * scale) / 2, y: (picture.bounds.height - 200 * scale) / 2)
        let x = (point.x - origin.x) / scale, y = (point.y - origin.y) / scale
        guard clamp || (x >= 0 && x < 320 && y >= 0 && y < 200) else { return nil }
        return [Int(max(0, min(319, x))), Int(max(0, min(199, y)))]
    }
    @objc func tap(_ gesture: UITapGestureRecognizer) {
        guard (!paused || snapshot["menu"] as? Bool == true), let point = gamePoint(gesture.location(in: picture)) else { return }
        send(["op": "input", "command": action, "x": point[0], "y": point[1]])
    }
    @objc func doubleTap(_ gesture: UITapGestureRecognizer) {
        guard (!paused || snapshot["menu"] as? Bool == true), let point = gamePoint(gesture.location(in: picture)) else { return }
        send(["op": "input", "command": "action", "x": point[0], "y": point[1]])
    }
    func releaseDrag() {
        guard dragging else { return }
        dragging = false
        send(["op": "input", "command": "left-up"])
    }
    @objc func drag(_ gesture: InventoryPan) {
        if gesture.state == .ended || gesture.state == .cancelled || gesture.state == .failed {
            if gesture.state == .ended, dragging, !paused,
               let point = gamePoint(gesture.location(in: picture), clamp: true) {
                send(["op": "pointer", "x": point[0], "y": point[1]])
            }
            releaseDrag()
            return
        }
        guard ready, !paused else { releaseDrag(); return }
        let location = gesture.location(in: picture)
        if gesture.state == .began {
            guard let origin = gesture.start, let start = gamePoint(origin) else { return }
            // The engine owns inventory and configuration hit regions and drag rules.
            send(["op": "overlay-press", "x": start[0], "y": start[1]])
            dragging = true
        }
        if dragging, let point = gamePoint(location, clamp: true) {
            send(["op": "pointer", "x": point[0], "y": point[1]])
        }
    }
    func startAudio() {
        guard !interrupted, UIApplication.shared.applicationState == .active else { return }
        do {
            try AVAudioSession.sharedInstance().setCategory(.playback)
            try AVAudioSession.sharedInstance().setActive(true)
            try audioEngine.start()
            player.play(); audioReady = true
        } catch { audioReady = false; status.text = "Audio: \(error.localizedDescription)" }
    }
    func resetAudio() {
        generation += 1; queued = 0; player.stop()
        previous = 0; accumulator = 0
        if audioReady { player.play() }
    }
    func queueAudio() {
        guard audioReady else { return }
        let count = dw_ipad_audio_len() / 2
        guard count > 0, let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(count)),
              let channels = buffer.floatChannelData, let samples = dw_ipad_audio() else { return }
        buffer.frameLength = AVAudioFrameCount(count)
        for i in 0..<count { channels[0][i] = samples[2*i]; channels[1][i] = samples[2*i+1] }
        let current = generation
        queued += 1
        player.scheduleBuffer(buffer, completionCallbackType: .dataPlayedBack) { [weak self] _ in
            DispatchQueue.main.async { if let self, self.generation == current { self.queued = max(0, self.queued - 1); self.completedAudioBuffers += 1 } }
        }
    }
    @objc func step(_ link: CADisplayLink) {
        guard ready, !paused, !interrupted, UIApplication.shared.applicationState == .active else { previous = 0; return }
        if previous == 0 { previous = link.timestamp; return }
        accumulator = min(0.125, accumulator + link.timestamp - previous)
        previous = link.timestamp
        do {
            while accumulator >= 1.0/24 && (!audioReady || queued < 3) {
                try handle(call(["op": "advance"]))
                queueAudio(); accumulator -= 1.0/24
            }
            render()
        } catch { paused = true; resetAudio(); status.text = error.localizedDescription }
    }
    @objc func suspended() {
        releaseDrag()
        audioReady = false
        resetAudio()
        audioEngine.pause()
        suspensionTick = snapshot["tick"] as? Int
        suspensionCount += 1
        render()
    }
    @objc func resumed() {
        previous = 0; accumulator = 0
        resumptionTick = snapshot["tick"] as? Int
        resumptionCount += 1
        if !muted { startAudio() }
        render()
    }
    @objc func audioInterrupted(_ notification: Notification) {
        if !Thread.isMainThread {
            DispatchQueue.main.async { [weak self] in self?.audioInterrupted(notification) }
            return
        }
        guard let value = notification.userInfo?[AVAudioSessionInterruptionTypeKey] as? UInt,
              let type = AVAudioSession.InterruptionType(rawValue: value) else { return }
        switch type {
        case .began:
            interrupted = true
            suspended()
        case .ended:
            interrupted = false
            previous = 0; accumulator = 0
            let options = AVAudioSession.InterruptionOptions(rawValue:
                notification.userInfo?[AVAudioSessionInterruptionOptionKey] as? UInt ?? 0)
            if options.contains(.shouldResume) {
                if !muted { startAudio() }
            } else {
                paused = true
                pauseButton.setTitle("Resume", for: .normal)
            }
            render()
        @unknown default: break
        }
    }
}
