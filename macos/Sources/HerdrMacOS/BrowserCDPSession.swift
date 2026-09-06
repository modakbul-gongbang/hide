import Foundation

enum BrowserConnectionError: Error, LocalizedError, Sendable {
    case invalidEndpoint
    case disconnected
    case timedOut(String)
    case protocolFailure(String)

    var errorDescription: String? {
        switch self {
        case .invalidEndpoint: "The browser host did not provide a valid local CDP target."
        case .disconnected: "The browser connection closed. Reconnect to resume viewing."
        case .timedOut(let method): "Chromium did not answer \(method)."
        case .protocolFailure(let message): "Chromium: \(message)"
        }
    }
}

/// The viewer connects to a page target, never the browser-wide endpoint.
/// Commands have bounded lifetimes; disconnect resumes every waiting caller.
/// Frame traffic bypasses the core snapshot and is bounded to the latest event.
actor BrowserCDPSession {
    nonisolated let events: AsyncThrowingStream<Data, any Error>
    private let eventContinuation: AsyncThrowingStream<Data, any Error>.Continuation
    private let session: URLSession
    private let socket: URLSessionWebSocketTask
    private var receiver: Task<Void, Never>?
    private var sequence = 0
    private var closed = false
    private var pending: [Int: CheckedContinuation<Data, any Error>] = [:]
    private var deadlines: [Int: Task<Void, Never>] = [:]

    init(binding: BrowserPaneBinding) throws {
        guard binding.cdpPort > 0,
              !binding.targetID.isEmpty,
              binding.targetID.utf8.allSatisfy({
                  (48...57).contains($0) || (65...90).contains($0) || (97...122).contains($0) || $0 == 45 || $0 == 95
              }),
              let endpoint = URL(string: "ws://127.0.0.1:\(binding.cdpPort)/devtools/page/\(binding.targetID)")
        else { throw BrowserConnectionError.invalidEndpoint }
        let channel = AsyncThrowingStream<Data, any Error>.makeStream(bufferingPolicy: .bufferingNewest(1))
        events = channel.stream
        eventContinuation = channel.continuation
        let configuration = URLSessionConfiguration.ephemeral
        configuration.timeoutIntervalForRequest = 8
        configuration.httpCookieStorage = nil
        configuration.urlCache = nil
        session = URLSession(configuration: configuration)
        socket = session.webSocketTask(with: endpoint)
        socket.maximumMessageSize = 16 * 1024 * 1024
    }

    func start() {
        guard receiver == nil, !closed else { return }
        socket.resume()
        receiver = Task { await receiveMessages() }
    }

    /// JSON crosses actor boundaries as Data, not an unchecked Sendable Any map.
    func command(_ method: String, parameters: Data = Data("{}".utf8)) async throws -> Data {
        guard !closed else { throw BrowserConnectionError.disconnected }
        let params = try JSONSerialization.jsonObject(with: parameters)
        sequence += 1
        let id = sequence
        let request = try JSONSerialization.data(withJSONObject: ["id": id, "method": method, "params": params])
        return try await withTaskCancellationHandler {
            try await withCheckedThrowingContinuation { continuation in
                pending[id] = continuation
                deadlines[id] = Task { [weak self] in
                    do { try await Task.sleep(for: .seconds(8)) } catch { return }
                    await self?.reject(id, error: BrowserConnectionError.timedOut(method))
                }
                Task { [weak self, socket] in
                    do { try await socket.send(.string(String(decoding: request, as: UTF8.self))) }
                    catch { await self?.reject(id, error: error) }
                }
            }
        } onCancel: {
            Task { await self.reject(id, error: CancellationError()) }
        }
    }

    func close() {
        finish(error: nil)
    }

    private func reject(_ id: Int, error: any Error) {
        deadlines.removeValue(forKey: id)?.cancel()
        pending.removeValue(forKey: id)?.resume(throwing: error)
    }

    private func finish(error: (any Error)?) {
        guard !closed else { return }
        closed = true
        receiver?.cancel()
        receiver = nil
        socket.cancel(with: .goingAway, reason: nil)
        session.invalidateAndCancel()
        for deadline in deadlines.values { deadline.cancel() }
        deadlines.removeAll()
        let waiters = pending.values
        pending.removeAll()
        for waiter in waiters { waiter.resume(throwing: error ?? BrowserConnectionError.disconnected) }
        if let error { eventContinuation.finish(throwing: error) }
        else { eventContinuation.finish() }
    }

    private func receiveMessages() async {
        do {
            while !Task.isCancelled {
                let message = try await socket.receive()
                let data: Data
                switch message {
                case .data(let bytes): data = bytes
                case .string(let text): data = Data(text.utf8)
                @unknown default: throw BrowserConnectionError.protocolFailure("Unsupported WebSocket message")
                }
                guard let object = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
                    throw BrowserConnectionError.protocolFailure("Invalid CDP response")
                }
                if let id = object["id"] as? Int {
                    deadlines.removeValue(forKey: id)?.cancel()
                    guard let continuation = pending.removeValue(forKey: id) else { continue }
                    if let error = object["error"] as? [String: Any] {
                        continuation.resume(throwing: BrowserConnectionError.protocolFailure(
                            error["message"] as? String ?? "CDP request failed"
                        ))
                    } else if let result = object["result"] {
                        do { continuation.resume(returning: try JSONSerialization.data(withJSONObject: result)) }
                        catch { continuation.resume(throwing: error) }
                    } else {
                        continuation.resume(throwing: BrowserConnectionError.protocolFailure("Missing CDP result"))
                    }
                } else {
                    // Acknowledge every received frame even if the consumer is
                    // slower and replaces its buffered frame. Otherwise the
                    // browser stops producing frames after its in-flight cap.
                    if object["method"] as? String == "Page.screencastFrame",
                       let params = object["params"] as? [String: Any],
                       let frameID = params["sessionId"] as? Int {
                        sequence += 1
                        let ack = try JSONSerialization.data(withJSONObject: [
                            "id": sequence, "method": "Page.screencastFrameAck",
                            "params": ["sessionId": frameID],
                        ])
                        try await socket.send(.string(String(decoding: ack, as: UTF8.self)))
                    }
                    eventContinuation.yield(data)
                }
            }
        } catch {
            finish(error: error)
        }
    }
}
