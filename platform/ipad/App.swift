import UIKit
import AVFoundation

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

final class GameController: UIViewController {
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
    var action = "walk"
    let pauseButton = UIButton(type: .system)

    override var supportedInterfaceOrientations: UIInterfaceOrientationMask { .all }
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
        picture.addGestureRecognizer(UITapGestureRecognizer(target: self, action: #selector(tap(_:))))
        status.textColor = .white
        status.numberOfLines = 2
        status.text = "Loading Discworld…"
        let modes = UISegmentedControl(items: ["Walk", "Action", "Look"])
        modes.selectedSegmentIndex = 0
        modes.addAction(UIAction { [weak self] _ in self?.action = ["walk", "action", "look"][modes.selectedSegmentIndex] }, for: .valueChanged)
        let row = UIStackView()
        row.axis = .horizontal
        row.spacing = 8
        row.distribution = .fillEqually
        for (label, command) in [("Start", "start"), ("Continue", "continue"), ("Pockets", "pockets"), ("Close", "close"), ("Escape", "escape")] {
            row.addArrangedSubview(button(label) { [weak self] in self?.send(["op": "input", "command": command]) })
        }
        let tools = UIStackView()
        tools.axis = .horizontal
        tools.spacing = 8
        tools.distribution = .fillProportionally
        tools.addArrangedSubview(modes)
        tools.addArrangedSubview(button("Save") { [weak self] in self?.send(["op": "save"]) })
        tools.addArrangedSubview(button("Load") { [weak self] in self?.resetAudio(); self?.send(["op": "load"]) })
        let mute = UIButton(type: .system)
        mute.setTitle("Sound off", for: .normal)
        mute.addAction(UIAction { [weak self] _ in
            guard let self else { return }
            self.muted.toggle()
            if !self.muted { self.startAudio() }
            self.player.volume = self.muted ? 0 : 1
            mute.setTitle(self.muted ? "Sound off" : "Sound on", for: .normal)
        }, for: .touchUpInside)
        tools.addArrangedSubview(mute)
        let pause = pauseButton
        pause.setTitle("Pause", for: .normal)
        pause.addAction(UIAction { [weak self] _ in
            guard let self else { return }
            self.paused.toggle(); self.resetAudio()
            pause.setTitle(self.paused ? "Resume" : "Pause", for: .normal)
        }, for: .touchUpInside)
        tools.addArrangedSubview(pause)
        let stack = UIStackView(arrangedSubviews: [picture, row, tools, status])
        stack.axis = .vertical
        stack.spacing = 6
        stack.translatesAutoresizingMaskIntoConstraints = false
        view.addSubview(stack)
        NSLayoutConstraint.activate([
            stack.leadingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.leadingAnchor, constant: 8),
            stack.trailingAnchor.constraint(equalTo: view.safeAreaLayoutGuide.trailingAnchor, constant: -8),
            stack.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor),
            stack.bottomAnchor.constraint(equalTo: view.safeAreaLayoutGuide.bottomAnchor, constant: -8),
            row.heightAnchor.constraint(equalToConstant: 44), tools.heightAnchor.constraint(equalToConstant: 44),
            status.heightAnchor.constraint(equalToConstant: 40)
        ])
        audioEngine.attach(player)
        audioEngine.connect(player, to: audioEngine.mainMixerNode, format: format)
        NotificationCenter.default.addObserver(self, selector: #selector(suspended), name: UIApplication.willResignActiveNotification, object: nil)
        NotificationCenter.default.addObserver(self, selector: #selector(resumed), name: UIApplication.didBecomeActiveNotification, object: nil)
        NotificationCenter.default.addObserver(self, selector: #selector(suspended), name: AVAudioSession.interruptionNotification, object: nil)
        DispatchQueue.main.async { self.openGame() }
    }

    func button(_ title: String, _ body: @escaping () -> Void) -> UIButton {
        let button = UIButton(type: .system)
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
        let result = dw_ipad_call()
        let response = Data(bytes: dw_ipad_response()!, count: dw_ipad_response_len())
        if result != 0 { throw failure(String(decoding: response, as: UTF8.self)) }
        return try JSONSerialization.jsonObject(with: response) as! [String: Any]
    }
    func failure(_ message: String) -> NSError { NSError(domain: "Discworld", code: 1, userInfo: [NSLocalizedDescriptionKey: message]) }
    func openGame() {
        do {
            let bundle = Bundle.main.bundleURL.appendingPathComponent("GameData.bundle")
            let saves = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
            var result = try call(["op": "open", "bundle": bundle.path, "saves": saves.path])
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
            status.text = ""
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
        if let requests = result["menu_requests"] as? [String], !requests.isEmpty {
            throw failure("Original menu requests are not yet connected in this development host.")
        }
    }
    func render() {
        guard dw_ipad_frame_len() == 320 * 200 * 4 else { return }
        let data = Data(bytes: dw_ipad_frame()!, count: dw_ipad_frame_len())
        guard let provider = CGDataProvider(data: data as CFData),
              let image = CGImage(width: 320, height: 200, bitsPerComponent: 8, bitsPerPixel: 32, bytesPerRow: 1280,
                                  space: CGColorSpaceCreateDeviceRGB(), bitmapInfo: CGBitmapInfo(rawValue: CGImageAlphaInfo.last.rawValue),
                                  provider: provider, decode: nil, shouldInterpolate: false, intent: .defaultIntent) else { return }
        picture.image = UIImage(cgImage: image)
    }
    @objc func tap(_ gesture: UITapGestureRecognizer) {
        let scale = min(picture.bounds.width / 320, picture.bounds.height / 200)
        guard scale > 0 else { return }
        let origin = CGPoint(x: (picture.bounds.width - 320 * scale) / 2, y: (picture.bounds.height - 200 * scale) / 2)
        let point = gesture.location(in: picture)
        let x = (point.x - origin.x) / scale, y = (point.y - origin.y) / scale
        guard x >= 0, x < 320, y >= 0, y < 200 else { return }
        send(["op": "input", "command": action, "x": Int(x), "y": Int(y)])
    }
    func startAudio() {
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
            DispatchQueue.main.async { if let self, self.generation == current { self.queued = max(0, self.queued - 1) } }
        }
    }
    @objc func step(_ link: CADisplayLink) {
        guard ready, !paused, UIApplication.shared.applicationState == .active else { previous = 0; return }
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
    @objc func suspended() { resetAudio(); audioEngine.pause(); audioReady = false }
    @objc func resumed() { previous = 0; accumulator = 0; if !muted { startAudio() } }
}
