import AppKit
import Foundation

struct BrowserFrameGeometry: Equatable, Sendable {
    let deviceWidth: Double
    let deviceHeight: Double
    let pageScaleFactor: Double
    let offsetTop: Double

    /// The view letterboxes the actual browser viewport. It never changes a
    /// borrowed page's device emulation just because its pane was resized.
    func imageRect(in bounds: CGRect) -> CGRect {
        guard deviceWidth > 0, deviceHeight > 0 else { return .zero }
        let scale = min(bounds.width / deviceWidth, bounds.height / deviceHeight)
        let size = CGSize(width: deviceWidth * scale, height: deviceHeight * scale)
        return CGRect(x: bounds.midX - size.width / 2, y: bounds.midY - size.height / 2, width: size.width, height: size.height)
    }

    func pagePoint(_ point: CGPoint, in bounds: CGRect) -> CGPoint? {
        let rect = imageRect(in: bounds)
        guard rect.width > 0, rect.height > 0, pageScaleFactor > 0, rect.contains(point) else { return nil }
        let x = (point.x - rect.minX) / rect.width * deviceWidth / pageScaleFactor
        let y = ((point.y - rect.minY) / rect.height * deviceHeight - offsetTop) / pageScaleFactor
        guard y >= 0 else { return nil }
        return CGPoint(x: x, y: y)
    }
}

@MainActor
final class BrowserPaneController: ObservableObject {
    @Published private(set) var image: NSImage?
    @Published private(set) var geometry: BrowserFrameGeometry?
    @Published private(set) var notice: String?
    @Published private(set) var connected = false
    @Published private(set) var address = ""
    private var connection: BrowserCDPSession?
    private var generation = UUID()
    private var inputTail: Task<Void, Never>?
    private var viewportSize = CGSize.zero
    private var ownsTarget = false
    private var resizeTask: Task<Void, Never>?

    func resizeViewport(to size: CGSize) {
        let pixels = CGSize(width: size.width.rounded(.down), height: size.height.rounded(.down))
        guard pixels.width > 0, pixels.height > 0, pixels != viewportSize else { return }
        viewportSize = pixels
        guard ownsTarget, connected else { return }
        resizeTask?.cancel()
        resizeTask = Task { [weak self] in
            do { try await Task.sleep(for: .milliseconds(100)) } catch { return }
            guard let self else { return }
            self.send("Emulation.setDeviceMetricsOverride", parameters: self.viewportParameters)
        }
    }

    private var viewportParameters: [String: Any] {
        ["width": Int(viewportSize.width), "height": Int(viewportSize.height),
         "deviceScaleFactor": 1, "mobile": false]
    }

    func run(binding: BrowserPaneBinding) async {
        let attempt = UUID()
        generation = attempt
        resizeTask?.cancel()
        ownsTarget = binding.ownsTarget
        connected = false
        notice = nil
        image = nil
        geometry = nil
        do {
            let client = try BrowserCDPSession(binding: binding)
            connection = client
            await client.start()
            do {
                _ = try await client.command("Page.enable")
                if ownsTarget, viewportSize.width > 0, viewportSize.height > 0 {
                    _ = try await client.command("Emulation.setDeviceMetricsOverride", parameters:
                        JSONSerialization.data(withJSONObject: viewportParameters))
                }
                let tree = try await client.command("Page.getFrameTree")
                if let payload = try JSONSerialization.jsonObject(with: tree) as? [String: Any],
                   let frameTree = payload["frameTree"] as? [String: Any],
                   let frame = frameTree["frame"] as? [String: Any] {
                    address = frame["url"] as? String ?? ""
                }
                _ = try await client.command("Page.startScreencast", parameters: Data(
                    #"{"format":"jpeg","quality":85,"maxWidth":1920,"maxHeight":1200,"everyNthFrame":1}"#.utf8
                ))
                let firstFrameDeadline = Task { [weak self] in
                    do { try await Task.sleep(for: .seconds(8)) } catch { return }
                    guard let self, self.generation == attempt, self.image == nil else { return }
                    self.notice = "Chromium has not produced a frame. Reconnect or inspect the selected tab."
                }
                defer { firstFrameDeadline.cancel() }
                for try await event in client.events {
                    try Task.checkCancellation()
                    guard generation == attempt else { break }
                    try consume(event)
                }
            } catch {
                if generation == attempt, !Task.isCancelled { notice = error.localizedDescription }
            }
            await client.close()
        } catch {
            if generation == attempt, !Task.isCancelled { notice = error.localizedDescription }
        }
        if generation == attempt {
            connection = nil
            connected = false
            inputTail?.cancel()
            inputTail = nil
            resizeTask?.cancel()
        }
    }

    func send(_ method: String, parameters: [String: Any] = [:]) {
        guard let client = connection, connected else { return }
        do {
            let data = try JSONSerialization.data(withJSONObject: parameters)
            let previous = inputTail
            let attempt = generation
            inputTail = Task { [weak self] in
                await previous?.value
                guard !Task.isCancelled, self?.generation == attempt else { return }
                do {
                    let result = try await client.command(method, parameters: data)
                    if method == "Page.navigate",
                       let object = try JSONSerialization.jsonObject(with: result) as? [String: Any],
                       let error = object["errorText"] as? String, !error.isEmpty {
                        throw BrowserConnectionError.protocolFailure(error)
                    }
                }
                catch {
                    guard self?.generation == attempt, !Task.isCancelled else { return }
                    self?.notice = error.localizedDescription
                }
            }
        } catch { notice = error.localizedDescription }
    }

    func navigate(_ value: String) {
        guard let url = URL(string: value), ["http", "https", "about"].contains(url.scheme?.lowercased() ?? "") else {
            notice = "Enter an http:// or https:// address."
            return
        }
        send("Page.navigate", parameters: ["url": url.absoluteString])
    }

    private func consume(_ data: Data) throws {
        guard let event = try JSONSerialization.jsonObject(with: data) as? [String: Any],
              let method = event["method"] as? String,
              let params = event["params"] as? [String: Any] else { return }
        switch method {
        case "Page.screencastFrame":
            guard let encoded = params["data"] as? String,
                  let bytes = Data(base64Encoded: encoded),
                  let frame = NSImage(data: bytes),
                  let metadata = params["metadata"] as? [String: Any],
                  let width = metadata["deviceWidth"] as? Double,
                  let height = metadata["deviceHeight"] as? Double,
                  let scale = metadata["pageScaleFactor"] as? Double,
                  let offset = metadata["offsetTop"] as? Double,
                  width > 0, height > 0, scale > 0 else {
                throw BrowserConnectionError.protocolFailure("Invalid screencast frame")
            }
            geometry = BrowserFrameGeometry(deviceWidth: width, deviceHeight: height, pageScaleFactor: scale, offsetTop: offset)
            image = frame
            if !connected { notice = nil }
            connected = true
        case "Page.frameNavigated":
            if let frame = params["frame"] as? [String: Any], frame["parentId"] == nil,
               let url = frame["url"] as? String { address = url }
        case "Page.navigatedWithinDocument":
            if let url = params["url"] as? String { address = url }
        case "Inspector.detached":
            throw BrowserConnectionError.disconnected
        default: break
        }
    }
}
