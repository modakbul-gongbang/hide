import Foundation

enum ConversationProvider: String, Equatable {
    case claude
    case codex

    init?(agentKind: String) {
        switch agentKind.lowercased() {
        case "claude", "claude-code", "claude_code": self = .claude
        case "codex": self = .codex
        default: return nil
        }
    }
}

struct ConversationMessage: Equatable, Identifiable {
    enum Role: String, Equatable {
        case user
        case assistant
    }

    let line: Int
    let role: Role
    let text: String
    let timestamp: Date?

    var id: Int { line }
}

enum ConversationReadResult: Equatable {
    case empty(path: String)
    case loaded(path: String, messages: [ConversationMessage])
    case failed(path: String, line: Int, reason: String)

    var path: String {
        switch self {
        case let .empty(path), let .loaded(path, _), let .failed(path, _, _): path
        }
    }
}

private struct ConversationFileSignature: Equatable, Sendable {
    let byteCount: Int64
    let modificationDate: Date?
    let fileNumber: UInt64?
}

private struct ConversationReadCheckpoint: @unchecked Sendable {
    let signature: ConversationFileSignature
    let result: ConversationReadResult
    let scanOffset: UInt64
    let nextLine: Int
}

private final class ConversationReadCache: @unchecked Sendable {
    private struct Entry {
        let checkpoint: ConversationReadCheckpoint
    }

    private let lock = NSLock()
    private var entries: [String: Entry] = [:]

    func checkpoint(for path: String) -> ConversationReadCheckpoint? {
        lock.lock()
        defer { lock.unlock() }
        return entries[path]?.checkpoint
    }

    func store(_ checkpoint: ConversationReadCheckpoint, for path: String) {
        lock.lock()
        defer { lock.unlock() }
        entries[path] = Entry(checkpoint: checkpoint)
        if entries.count > 64, let oldest = entries.keys.sorted().first {
            entries.removeValue(forKey: oldest)
        }
    }
}

private final class ConversationPathCache: @unchecked Sendable {
    private let lock = NSLock()
    private var entries: [String: URL] = [:]

    func path(for key: String) -> URL? {
        lock.lock()
        defer { lock.unlock() }
        guard let path = entries[key] else { return nil }
        guard FileManager.default.fileExists(atPath: path.path) else {
            entries.removeValue(forKey: key)
            return nil
        }
        return path
    }

    func store(_ path: URL, for key: String) {
        lock.lock()
        defer { lock.unlock() }
        entries[key] = path
        if entries.count > 64, let first = entries.keys.sorted().first {
            entries.removeValue(forKey: first)
        }
    }
}

/// Reads the two local provider ledgers without crossing the core runtime
/// mutex. It deliberately produces only operator-visible user and assistant
/// messages; provider progress, tool calls, and results never enter the view.
struct ConversationReader: Sendable {
    let homeDirectory: URL

    private static let cache = ConversationReadCache()
    private static let pathCache = ConversationPathCache()

    init(homeDirectory: URL = FileManager.default.homeDirectoryForCurrentUser) {
        self.homeDirectory = homeDirectory
    }

    func read(provider: ConversationProvider, sessionID: String?, cwd: String) -> ConversationReadResult {
        guard let path = resolvePath(provider: provider, sessionID: sessionID, cwd: cwd) else {
            return .empty(path: expectedPath(provider: provider, sessionID: sessionID, cwd: cwd).path)
        }
        guard let signature = fileSignature(for: path) else {
            return .failed(path: path.path, line: 0, reason: "Conversation file could not be read")
        }

        if let checkpoint = Self.cache.checkpoint(for: path.path) {
            if checkpoint.signature == signature {
                return checkpoint.result
            }
            if checkpoint.signature.fileNumber != nil,
               checkpoint.signature.fileNumber == signature.fileNumber,
               signature.byteCount > checkpoint.signature.byteCount,
               checkpoint.scanOffset <= UInt64(signature.byteCount)
            {
                return readAppended(
                    from: path,
                    provider: provider,
                    signature: signature,
                    checkpoint: checkpoint
                )
            }
        }

        return readFromBeginning(path: path, provider: provider, signature: signature)
    }

    private struct ParsedConversation {
        let messages: [ConversationMessage]
        let scanOffset: UInt64
        let nextLine: Int
    }

    private enum ParseOutcome {
        case success(ParsedConversation)
        case failure(line: Int)
    }

    private enum LineOutcome {
        case message(ConversationMessage.Role, String, Date?)
        case ignored
        case invalid
    }

    private func readFromBeginning(
        path: URL,
        provider: ConversationProvider,
        signature: ConversationFileSignature
    ) -> ConversationReadResult {
        guard let data = readData(from: path, offset: 0) else {
            return .failed(path: path.path, line: 0, reason: "Conversation file could not be read")
        }

        switch parse(data: data, provider: provider, baseOffset: 0, startLine: 1, messages: []) {
        case let .failure(line):
            return .failed(path: path.path, line: line, reason: "Invalid JSONL record")
        case let .success(parsed):
            let result = result(from: parsed.messages, path: path)
            Self.cache.store(
                ConversationReadCheckpoint(
                    signature: signature,
                    result: result,
                    scanOffset: parsed.scanOffset,
                    nextLine: parsed.nextLine
                ),
                for: path.path
            )
            return result
        }
    }

    private func readAppended(
        from path: URL,
        provider: ConversationProvider,
        signature: ConversationFileSignature,
        checkpoint: ConversationReadCheckpoint
    ) -> ConversationReadResult {
        guard let data = readData(from: path, offset: checkpoint.scanOffset) else {
            return .failed(path: path.path, line: 0, reason: "Conversation file could not be read")
        }
        let messages: [ConversationMessage]
        switch checkpoint.result {
        case .empty:
            messages = []
        case let .loaded(_, existing):
            messages = existing
        case .failed:
            return readFromBeginning(path: path, provider: provider, signature: signature)
        }

        switch parse(
            data: data,
            provider: provider,
            baseOffset: checkpoint.scanOffset,
            startLine: checkpoint.nextLine,
            messages: messages
        ) {
        case let .failure(line):
            return .failed(path: path.path, line: line, reason: "Invalid JSONL record")
        case let .success(parsed):
            let result = result(from: parsed.messages, path: path)
            Self.cache.store(
                ConversationReadCheckpoint(
                    signature: signature,
                    result: result,
                    scanOffset: parsed.scanOffset,
                    nextLine: parsed.nextLine
                ),
                for: path.path
            )
            return result
        }
    }

    private func result(from messages: [ConversationMessage], path: URL) -> ConversationReadResult {
        messages.isEmpty
            ? .empty(path: path.path)
            : .loaded(path: path.path, messages: messages)
    }

    private func parse(
        data: Data,
        provider: ConversationProvider,
        baseOffset: UInt64,
        startLine: Int,
        messages: [ConversationMessage]
    ) -> ParseOutcome {
        var messages = messages
        var lineStart = 0
        var line = startLine
        var cursor = 0

        while let newline = data[cursor...].firstIndex(of: 0x0A) {
            switch parseLine(data[lineStart..<newline], provider: provider) {
            case let .message(role, text, timestamp):
                messages.append(ConversationMessage(line: line, role: role, text: text, timestamp: timestamp))
            case .ignored:
                break
            case .invalid:
                return .failure(line: line)
            }
            cursor = data.index(after: newline)
            lineStart = cursor
            line += 1
        }

        if lineStart < data.count {
            switch parseLine(data[lineStart..<data.count], provider: provider) {
            case let .message(role, text, timestamp):
                messages.append(ConversationMessage(line: line, role: role, text: text, timestamp: timestamp))
                lineStart = data.count
                line += 1
            case .ignored:
                lineStart = data.count
                line += 1
            case .invalid:
                // A record without its terminating newline may still be in
                // flight. Keep its beginning so the next refresh retries it.
                break
            }
        }

        return .success(ParsedConversation(
            messages: messages,
            scanOffset: baseOffset + UInt64(lineStart),
            nextLine: line
        ))
    }

    private func parseLine(_ data: Data, provider: ConversationProvider) -> LineOutcome {
        guard let rawLine = String(data: data, encoding: .utf8) else { return .invalid }
        let value = rawLine.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !value.isEmpty else { return .ignored }
        guard let json = value.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: json) as? [String: Any]
        else { return .invalid }
        guard let visible = visibleMessage(object, provider: provider) else { return .ignored }
        return .message(visible.role, visible.text, timestamp(in: object))
    }

    private func readData(from path: URL, offset: UInt64) -> Data? {
        do {
            let handle = try FileHandle(forReadingFrom: path)
            defer { try? handle.close() }
            try handle.seek(toOffset: offset)
            return try handle.readToEnd() ?? Data()
        } catch {
            return nil
        }
    }

    private func rootURL(for provider: ConversationProvider) -> URL {
        switch provider {
        case .claude: homeDirectory.appendingPathComponent(".claude/projects", isDirectory: true)
        case .codex: homeDirectory.appendingPathComponent(".codex/sessions", isDirectory: true)
        }
    }

    private func resolvePath(provider: ConversationProvider, sessionID: String?, cwd: String) -> URL? {
        let fileManager = FileManager.default
        let root = rootURL(for: provider)
        let identifier = sessionID?.trimmingCharacters(in: .whitespacesAndNewlines)
        switch provider {
        case .claude:
            let project = projectDirectory(for: cwd)
            let directory = root.appendingPathComponent(project, isDirectory: true)
            if let identifier, isSafeSessionIdentifier(identifier) {
                let exact = directory.appendingPathComponent("\(identifier).jsonl")
                if fileManager.fileExists(atPath: exact.path) { return exact }
            }
            return nil
        case .codex:
            guard let identifier, isSafeSessionIdentifier(identifier) else { return nil }
            let cacheKey = "\(root.path)\u{0}\(identifier)"
            if let cached = Self.pathCache.path(for: cacheKey) { return cached }
            guard let exact = findJSONL(named: identifier, under: root) else { return nil }
            Self.pathCache.store(exact, for: cacheKey)
            return exact
        }
    }

    private func expectedPath(provider: ConversationProvider, sessionID: String?, cwd: String) -> URL {
        let identifier = sessionID
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .flatMap { isSafeSessionIdentifier($0) ? $0 : nil }
        switch provider {
        case .claude:
            let name = identifier.flatMap { $0.isEmpty ? nil : $0 } ?? "unknown-session"
            return rootURL(for: provider)
                .appendingPathComponent(projectDirectory(for: cwd), isDirectory: true)
                .appendingPathComponent("\(name).jsonl")
        case .codex:
            let name = identifier.flatMap { $0.isEmpty ? nil : $0 } ?? "unknown-session"
            return rootURL(for: provider).appendingPathComponent("\(name).jsonl")
        }
    }

    private func isSafeSessionIdentifier(_ identifier: String) -> Bool {
        guard !identifier.isEmpty, identifier != ".", identifier != ".." else { return false }
        guard !identifier.contains("/"), !identifier.contains("\\"), !identifier.contains("\0") else {
            return false
        }
        return URL(fileURLWithPath: identifier).lastPathComponent == identifier
    }

    private func projectDirectory(for cwd: String) -> String {
        cwd
            .replacingOccurrences(of: "/", with: "-")
            .replacingOccurrences(of: ".", with: "-")
            .replacingOccurrences(of: "_", with: "-")
    }

    private func findJSONL(named identifier: String, under root: URL) -> URL? {
        let fileManager = FileManager.default
        guard let enumerator = fileManager.enumerator(
            at: root,
            includingPropertiesForKeys: [.isRegularFileKey, .contentModificationDateKey],
            options: [.skipsHiddenFiles]
        ) else { return nil }
        return enumerator.compactMap { item -> URL? in
            guard let url = item as? URL,
                  url.pathExtension == "jsonl"
            else { return nil }
            let basename = url.deletingPathExtension().lastPathComponent
            guard basename == identifier || basename.hasSuffix("-\(identifier)") else { return nil }
            return url
        }.first
    }

    private func fileSignature(for path: URL) -> ConversationFileSignature? {
        guard let attributes = try? FileManager.default.attributesOfItem(atPath: path.path),
              let byteCount = attributes[.size] as? NSNumber
        else { return nil }
        return ConversationFileSignature(
            byteCount: byteCount.int64Value,
            modificationDate: attributes[.modificationDate] as? Date,
            fileNumber: (attributes[.systemFileNumber] as? NSNumber).map(\.uint64Value)
        )
    }

    private func visibleMessage(_ object: [String: Any], provider: ConversationProvider)
        -> (role: ConversationMessage.Role, text: String)?
    {
        let body: [String: Any]
        switch provider {
        case .claude:
            guard let type = object["type"] as? String,
                  type == "user" || type == "assistant",
                  object["isMeta"] as? Bool != true
            else { return nil }
            body = object["message"] as? [String: Any] ?? object
            guard body["isMeta"] as? Bool != true else { return nil }
        case .codex:
            guard object["type"] as? String == "response_item",
                  let payload = object["payload"] as? [String: Any],
                  payload["type"] as? String == "message"
            else { return nil }
            body = payload
        }
            guard let role = body["role"] as? String,
                  let parsedRole = ConversationMessage.Role(rawValue: role),
                  parsedRole != .user || provider != .codex || isCodexOperatorMessage(body),
                  provider != .codex || parsedRole != .assistant || isCodexFinalMessage(body),
                  let text = visibleText(body["content"]),
              !text.isEmpty,
              !isGeneratedLocalCommand(text)
        else { return nil }
        return (parsedRole, text)
    }

    private func isCodexFinalMessage(_ body: [String: Any]) -> Bool {
        guard let phase = body["phase"] as? String else { return true }
        return phase == "final" || phase == "final_answer"
    }

    /// Codex stores startup context as a `user` message alongside the text the
    /// operator actually entered. The metadata distinguishes those records;
    /// without this guard AGENTS.md and other injected context would appear as
    /// a human turn in the viewer.
    private func isCodexOperatorMessage(_ body: [String: Any]) -> Bool {
        guard let metadata = body["internal_chat_message_metadata_passthrough"] as? [String: Any],
              let kinds = metadata["content_item_kinds"] as? [Any]
        else { return false }
        return kinds.contains { ($0 as? String) == "user.text" }
    }

    private func isGeneratedLocalCommand(_ text: String) -> Bool {
        [
            "<local-command-caveat>",
            "<command-name>",
            "<command-message>",
            "<command-args>",
            "<local-command-stdout>"
        ].contains { text.contains($0) }
    }

    private func visibleText(_ value: Any?) -> String? {
        if let text = value as? String {
            return text.trimmingCharacters(in: .whitespacesAndNewlines)
        }
        guard let blocks = value as? [[String: Any]] else { return nil }
        let text = blocks.compactMap { block -> String? in
            guard let type = block["type"] as? String,
                  type == "text" || type == "input_text" || type == "output_text"
            else { return nil }
            return block["text"] as? String
        }.joined(separator: "\n").trimmingCharacters(in: .whitespacesAndNewlines)
        return text.isEmpty ? nil : text
    }

    private func timestamp(in object: [String: Any]) -> Date? {
        guard let value = object["timestamp"] as? String else { return nil }
        let fractional = ISO8601DateFormatter()
        fractional.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        return fractional.date(from: value) ?? ISO8601DateFormatter().date(from: value)
    }
}
