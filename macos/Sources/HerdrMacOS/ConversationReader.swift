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
    /// False when the first read opened a tail window, so `nextLine` counts
    /// from that window and a failure is reported by byte offset instead.
    let linesAreAbsolute: Bool
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

    /// Bytes an initial read takes from the end of a ledger. A long session's
    /// file grows past this; the operator sees its tail, and later refreshes
    /// read only what was appended, so the bound is paid once per file.
    static let initialReadLimit: UInt64 = 2 * 1024 * 1024

    /// Codex names a rollout by date, `sessions/YYYY/MM/DD/rollout-<ts>-<id>.jsonl`.
    /// A lookup visits the newest day directories only, so a years-old
    /// tree is never walked to answer for a session started today.
    static let codexLookupDayLimit = 14

    func read(provider: ConversationProvider, sessionID: String?, cwd: String) -> ConversationReadResult {
        let path: URL
        switch resolvePath(provider: provider, sessionID: sessionID, cwd: cwd) {
        case let .found(url):
            path = url
        case .missing:
            return .empty(path: expectedPath(provider: provider, sessionID: sessionID, cwd: cwd).path)
        case let .unreadable(url, reason):
            return .failed(path: url.path, line: 0, reason: reason)
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
        /// `line` counts from the window the parse started in; it is absolute
        /// only when that window began at byte 0. `offset` is absolute always.
        case failure(line: Int, offset: UInt64)
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
        let byteCount = UInt64(max(0, signature.byteCount))
        let tailed = byteCount > Self.initialReadLimit
        let startOffset = tailed ? byteCount - Self.initialReadLimit : 0
        guard var data = readData(from: path, offset: startOffset) else {
            return .failed(path: path.path, line: 0, reason: "Conversation file could not be read")
        }
        var baseOffset = startOffset
        if tailed {
            // The window opens mid-record; everything up to the first newline
            // belongs to a record whose beginning was not read.
            guard let newline = data.firstIndex(of: 0x0A) else {
                return .failed(
                    path: path.path,
                    line: 0,
                    reason: "No complete record in the last \(Self.initialReadLimit) bytes"
                )
            }
            let skipped = data.distance(from: data.startIndex, to: data.index(after: newline))
            data = Data(data[data.index(after: newline)...])
            baseOffset += UInt64(skipped)
        }

        switch parse(data: data, provider: provider, baseOffset: baseOffset, startLine: 1, messages: []) {
        case let .failure(line, offset):
            return invalidRecord(path: path, line: line, offset: offset, linesAreAbsolute: !tailed)
        case let .success(parsed):
            let result = result(from: parsed.messages, path: path)
            Self.cache.store(
                ConversationReadCheckpoint(
                    signature: signature,
                    result: result,
                    scanOffset: parsed.scanOffset,
                    nextLine: parsed.nextLine,
                    linesAreAbsolute: !tailed
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
        case let .failure(line, offset):
            return invalidRecord(
                path: path,
                line: line,
                offset: offset,
                linesAreAbsolute: checkpoint.linesAreAbsolute
            )
        case let .success(parsed):
            let result = result(from: parsed.messages, path: path)
            Self.cache.store(
                ConversationReadCheckpoint(
                    signature: signature,
                    result: result,
                    scanOffset: parsed.scanOffset,
                    nextLine: parsed.nextLine,
                    linesAreAbsolute: checkpoint.linesAreAbsolute
                ),
                for: path.path
            )
            return result
        }
    }

    private func invalidRecord(path: URL, line: Int, offset: UInt64, linesAreAbsolute: Bool) -> ConversationReadResult {
        linesAreAbsolute
            ? .failed(path: path.path, line: line, reason: "Invalid JSONL record")
            : .failed(path: path.path, line: 0, reason: "Invalid JSONL record at byte \(offset)")
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
                return .failure(line: line, offset: baseOffset + UInt64(lineStart))
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

    private enum ResolvedPath {
        case found(URL)
        case missing
        case unreadable(URL, reason: String)
    }

    private func resolvePath(provider: ConversationProvider, sessionID: String?, cwd: String) -> ResolvedPath {
        let root = rootURL(for: provider)
        guard let identifier = sessionID?.trimmingCharacters(in: .whitespacesAndNewlines),
              Self.isSafeSessionIdentifier(identifier)
        else { return .missing }
        switch provider {
        case .claude:
            let exact = root
                .appendingPathComponent(Self.projectDirectory(for: cwd), isDirectory: true)
                .appendingPathComponent("\(identifier).jsonl")
            switch probe(exact) {
            case .present: return .found(exact)
            case .missing: return .missing
            case let .unreadable(reason): return .unreadable(exact, reason: reason)
            }
        case .codex:
            let cacheKey = "\(root.path)\u{0}\(identifier)"
            if let cached = Self.pathCache.path(for: cacheKey) { return .found(cached) }
            let resolved = findCodexRollout(named: identifier, under: root)
            if case let .found(exact) = resolved {
                Self.pathCache.store(exact, for: cacheKey)
            }
            return resolved
        }
    }

    private func expectedPath(provider: ConversationProvider, sessionID: String?, cwd: String) -> URL {
        let identifier = sessionID
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .flatMap { Self.isSafeSessionIdentifier($0) ? $0 : nil }
        let name = identifier ?? "unknown-session"
        switch provider {
        case .claude:
            return rootURL(for: provider)
                .appendingPathComponent(Self.projectDirectory(for: cwd), isDirectory: true)
                .appendingPathComponent("\(name).jsonl")
        case .codex:
            return rootURL(for: provider).appendingPathComponent("\(name).jsonl")
        }
    }

    /// A session id names one file inside a provider directory and nothing
    /// else: no separators, no `.`/`..`, nothing a path would interpret.
    /// The shell checks the same rule before offering the view, so an id
    /// this refuses never reaches a read.
    static func isSafeSessionIdentifier(_ identifier: String) -> Bool {
        guard !identifier.isEmpty, identifier != ".", identifier != ".." else { return false }
        guard !identifier.contains("/"), !identifier.contains("\\"), !identifier.contains("\0") else {
            return false
        }
        return URL(fileURLWithPath: identifier).lastPathComponent == identifier
    }

    /// Claude Code names a project directory by its working directory with
    /// every character outside `[A-Za-z0-9]` written as `-`, one per UTF-16
    /// unit. `/Users/x/.claude` is `-Users-x--claude`; `Mobile Documents`
    /// is `Mobile-Documents`.
    static func projectDirectory(for cwd: String) -> String {
        String(utf16CodeUnits: cwd.utf16.map { unit -> unichar in
            switch unit {
            case 0x30...0x39, 0x41...0x5A, 0x61...0x7A: unit
            default: 0x2D
            }
        }, count: cwd.utf16.count)
    }

    private enum PathProbe {
        case present
        case missing
        case unreadable(String)
    }

    /// `stat` distinguishes a path that is not there from one the process may
    /// not look at; `fileExists` folds both into `false`.
    private func probe(_ url: URL) -> PathProbe {
        var info = stat()
        if stat(url.path, &info) == 0 { return .present }
        switch errno {
        case ENOENT, ENOTDIR: return .missing
        default: return .unreadable(String(cString: strerror(errno)))
        }
    }

    private enum DirectoryListing {
        case entries([String])
        case missing
        case unreadable(String)
    }

    private func list(_ url: URL) -> DirectoryListing {
        do {
            return .entries(try FileManager.default.contentsOfDirectory(atPath: url.path))
        } catch let error as NSError {
            if error.domain == NSCocoaErrorDomain, error.code == NSFileReadNoSuchFileError {
                return .missing
            }
            if error.domain == NSPOSIXErrorDomain, error.code == Int(ENOENT) || error.code == Int(ENOTDIR) {
                return .missing
            }
            let underlying = error.userInfo[NSUnderlyingErrorKey] as? NSError
            if underlying?.domain == NSPOSIXErrorDomain,
               underlying?.code == Int(ENOENT) || underlying?.code == Int(ENOTDIR)
            {
                return .missing
            }
            return .unreadable(error.localizedDescription)
        }
    }

    private func findCodexRollout(named identifier: String, under root: URL) -> ResolvedPath {
        var pending = Self.codexLookupDayLimit
        func dated(_ entries: [String]) -> [String] {
            entries.filter { !$0.isEmpty && $0.allSatisfy(\.isNumber) }.sorted(by: >)
        }
        let years: [String]
        switch list(root) {
        case let .entries(entries): years = dated(entries)
        case .missing: return .missing
        case let .unreadable(reason): return .unreadable(root, reason: reason)
        }
        for year in years {
            let yearURL = root.appendingPathComponent(year, isDirectory: true)
            let months: [String]
            switch list(yearURL) {
            case let .entries(entries): months = dated(entries)
            case .missing: continue
            case let .unreadable(reason): return .unreadable(yearURL, reason: reason)
            }
            for month in months {
                let monthURL = yearURL.appendingPathComponent(month, isDirectory: true)
                let days: [String]
                switch list(monthURL) {
                case let .entries(entries): days = dated(entries)
                case .missing: continue
                case let .unreadable(reason): return .unreadable(monthURL, reason: reason)
                }
                for day in days {
                    guard pending > 0 else { return .missing }
                    pending -= 1
                    let dayURL = monthURL.appendingPathComponent(day, isDirectory: true)
                    switch list(dayURL) {
                    case let .entries(entries):
                        if let match = entries.sorted(by: >).first(where: { name in
                            guard name.hasSuffix(".jsonl") else { return false }
                            let basename = String(name.dropLast(".jsonl".count))
                            return basename == identifier || basename.hasSuffix("-\(identifier)")
                        }) {
                            return .found(dayURL.appendingPathComponent(match))
                        }
                    case .missing:
                        continue
                    case let .unreadable(reason):
                        return .unreadable(dayURL, reason: reason)
                    }
                }
            }
        }
        return .missing
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
