import Foundation
import Testing
@testable import HerdrMacOS

@Suite("Conversation reader")
struct ConversationReaderTests {
    @Test func claudeReaderKeepsOnlyVisibleMessagesAndUsesSessionPath() throws {
        let root = try temporaryRoot()
        defer { try? FileManager.default.removeItem(at: root) }
        let directory = root.appendingPathComponent(".claude/projects/-tmp-project-with-suffix", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let path = directory.appendingPathComponent("session-1.jsonl")
        try """
        {"type":"progress","message":{"content":"hidden"}}
        {"type":"user","isMeta":true,"message":{"role":"user","content":[{"type":"text","text":"injected reminder"}]}}
        {"type":"user","message":{"role":"user","isMeta":true,"content":[{"type":"text","text":"nested injected reminder"}]}}
        {"type":"user","message":{"role":"user","content":[{"type":"image","source":{"media_type":"image/png","data":"placeholder"}}]}}
        {"type":"user","timestamp":"2026-09-15T00:00:00.626Z","message":{"role":"user","content":[{"type":"text","text":"<local-command-caveat>generated</local-command-caveat><command-name>/model</command-name><local-command-stdout>hidden</local-command-stdout>"}]}}
        {"type":"user","timestamp":"2026-09-15T00:00:00.626Z","message":{"role":"user","content":[{"type":"text","text":"Inspect this"}]}}
        {"type":"assistant","message":{"role":"assistant","content":[{"type":"text","text":"Done"},{"type":"tool_use","name":"bash"}]}}
        """.write(to: path, atomically: true, encoding: .utf8)

        let result = ConversationReader(homeDirectory: root).read(
            provider: .claude,
            sessionID: "session-1",
            cwd: "/tmp/project.with_suffix"
        )
        guard case let .loaded(readPath, messages) = result else {
            Issue.record("expected loaded conversation, got \(result)")
            return
        }
        #expect(URL(fileURLWithPath: readPath).standardizedFileURL.path == path.standardizedFileURL.path)
        #expect(messages.map(\.text) == ["Inspect this", "Done"])
        #expect(messages.map(\.role) == [.user, .assistant])
        #expect(messages[0].timestamp != nil)
    }

    @Test func codexReaderReportsMalformedLineWithPathAndLine() throws {
        let root = try temporaryRoot()
        defer { try? FileManager.default.removeItem(at: root) }
        let directory = root.appendingPathComponent(".codex/sessions/2026/09/15", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let path = directory.appendingPathComponent("rollout-2026-09-15T00-00-00-session-2.jsonl")
        try "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"Hello\"}}\nnot-json\n".write(to: path, atomically: true, encoding: .utf8)

        let result = ConversationReader(homeDirectory: root).read(
            provider: .codex,
            sessionID: "session-2",
            cwd: "/tmp/project"
        )
        guard case let .failed(readPath, line, reason) = result else {
            Issue.record("expected a failed read, got \(result)")
            return
        }
        #expect(URL(fileURLWithPath: readPath).standardizedFileURL.path == path.standardizedFileURL.path)
        #expect(line == 2)
        #expect(reason == "Invalid JSONL record")

        try "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"Hello\"}}\n{\"type\":\"response_item\"".write(to: path, atomically: true, encoding: .utf8)
        let partial = ConversationReader(homeDirectory: root).read(
            provider: .codex,
            sessionID: "session-2",
            cwd: "/tmp/project"
        )
        guard case let .loaded(_, partialMessages) = partial else {
            Issue.record("expected the incomplete tail to be deferred, got \(partial)")
            return
        }
        #expect(partialMessages.map(\.text) == ["Hello"])
    }

    @Test func codexReaderHidesInjectedStartupContextButKeepsOperatorText() throws {
        let root = try temporaryRoot()
        defer { try? FileManager.default.removeItem(at: root) }
        let directory = root.appendingPathComponent(".codex/sessions/2026/09/15", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let path = directory.appendingPathComponent("rollout-2026-09-15T00-00-00-session-3.jsonl")
        try """
        {"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"# AGENTS.md instructions; injected startup context"}],"internal_chat_message_metadata_passthrough":{"content_item_kinds":["agents_md.instructions"]}}}
        {"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Inspect this"}],"internal_chat_message_metadata_passthrough":{"content_item_kinds":["user.text"]}}}
        {"type":"response_item","payload":{"type":"message","role":"assistant","phase":"commentary","content":[{"type":"output_text","text":"internal progress"}]}}
        {"type":"response_item","payload":{"type":"message","role":"assistant","phase":"final_answer","content":[{"type":"output_text","text":"Done"}]}}
        """.write(to: path, atomically: true, encoding: .utf8)

        let result = ConversationReader(homeDirectory: root).read(
            provider: .codex,
            sessionID: "session-3",
            cwd: "/tmp/project"
        )
        guard case let .loaded(_, messages) = result else {
            Issue.record("expected loaded conversation, got \(result)")
            return
        }
        #expect(messages.map(\.text) == ["Inspect this", "Done"])
        #expect(messages.map(\.role) == [.user, .assistant])
    }

    @Test func readerRejectsSessionPathTraversal() throws {
        let root = try temporaryRoot()
        defer { try? FileManager.default.removeItem(at: root) }
        let directory = root.appendingPathComponent(".claude/projects/-tmp-project", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let escapedPath = root.appendingPathComponent("escaped.jsonl")
        try #"{"type":"user","message":{"role":"user","content":"must not load"}}"#
            .write(to: escapedPath, atomically: true, encoding: .utf8)

        let result = ConversationReader(homeDirectory: root).read(
            provider: .claude,
            sessionID: "../escaped",
            cwd: "/tmp/project"
        )
        guard case let .empty(path) = result else {
            Issue.record("expected traversal to resolve to an empty safe path, got \(result)")
            return
        }
        #expect(path.hasSuffix("/.claude/projects/-tmp-project/unknown-session.jsonl"))
        #expect(!path.contains("escaped.jsonl"))
    }

    @Test func readerTailsAppendedRecordsAndDefersAnIncompleteTail() throws {
        let root = try temporaryRoot()
        defer { try? FileManager.default.removeItem(at: root) }
        let directory = root.appendingPathComponent(".codex/sessions/2026/09/15", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let path = directory.appendingPathComponent("rollout-2026-09-15T00-00-00-session-tail.jsonl")
        let initial = #"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"Inspect this"}],"internal_chat_message_metadata_passthrough":{"content_item_kinds":["user.text"]}}}"# + "\n"
        try initial.write(to: path, atomically: true, encoding: .utf8)

        let reader = ConversationReader(homeDirectory: root)
        let first = reader.read(provider: .codex, sessionID: "tail", cwd: "/tmp/project")
        guard case let .loaded(_, firstMessages) = first else {
            Issue.record("expected the initial record to load, got \(first)")
            return
        }
        #expect(firstMessages.map(\.line) == [1])

        try append(#"{"type":"response_item","payload":{"type":"message","role":"assistant","phase":"final","content":[{"type":"output_text","text":"Done"#, to: path)
        let duringWrite = reader.read(provider: .codex, sessionID: "tail", cwd: "/tmp/project")
        guard case let .loaded(_, duringWriteMessages) = duringWrite else {
            Issue.record("expected an incomplete tail to preserve the previous messages, got \(duringWrite)")
            return
        }
        #expect(duringWriteMessages.map(\.text) == ["Inspect this"])

        try append("\"}]}}\n", to: path)
        let complete = reader.read(provider: .codex, sessionID: "tail", cwd: "/tmp/project")
        guard case let .loaded(_, completeMessages) = complete else {
            Issue.record("expected the completed tail to load, got \(complete)")
            return
        }
        #expect(completeMessages.map(\.line) == [1, 2])
        #expect(completeMessages.map(\.text) == ["Inspect this", "Done"])
    }

    @Test func ledgerFormattingUsesOneProviderGlyphAndInterpolatesElapsedSeconds() {
        let started = Date(timeIntervalSince1970: 0)
        let messages = [
            ConversationMessage(line: 1, role: .user, text: "Inspect this", timestamp: started),
            ConversationMessage(line: 2, role: .assistant, text: "Done", timestamp: started.addingTimeInterval(65))
        ]

        let claude = ConversationLedgerFormatting.markdown(
            messages: messages,
            provider: .claude,
            activity: "idle",
            now: started.addingTimeInterval(65)
        )
        #expect(claude.contains("**>** Inspect this"))
        #expect(!claude.contains("> **"))
        #expect(claude.contains("1m 5s"))
        #expect(ConversationLedgerFormatting.elapsedText(from: started, to: started.addingTimeInterval(65)) == "1m 5s")

        let codex = ConversationLedgerFormatting.markdown(
            messages: messages,
            provider: .codex,
            activity: "idle",
            now: started
        )
        #expect(codex.contains("**›** Inspect this"))
        #expect(!codex.contains("> **"))
        #expect(codex.contains("1m 5s"))
    }

    @Test func ledgerTurnsExposeDistinctHumanAndAssistantMetadata() {
        let started = Date(timeIntervalSince1970: 0)
        let turns = ConversationLedgerFormatting.turns(
            messages: [
                ConversationMessage(line: 1, role: .user, text: "Inspect this", timestamp: started),
                ConversationMessage(line: 2, role: .assistant, text: "Done", timestamp: started.addingTimeInterval(65)),
            ],
            provider: .claude,
            activity: "idle",
            now: started.addingTimeInterval(65)
        )

        #expect(turns.map(\.role) == [.human, .assistant])
        #expect(turns[0].promptGlyph == ">")
        #expect(turns[1].promptGlyph == nil)
        #expect(turns[1].elapsed == "1m 5s")
        #expect(turns[0].isWorking == false)
        #expect(turns[1].isWorking == false)

        let grouped = ConversationLedgerFormatting.turns(
            messages: [
                ConversationMessage(line: 1, role: .user, text: "Inspect this", timestamp: started),
                ConversationMessage(line: 2, role: .assistant, text: "First", timestamp: started.addingTimeInterval(65)),
                ConversationMessage(line: 3, role: .assistant, text: "Second", timestamp: started.addingTimeInterval(70)),
                ConversationMessage(line: 4, role: .user, text: "Continue", timestamp: started.addingTimeInterval(100)),
            ],
            provider: .claude,
            activity: "idle",
            now: started.addingTimeInterval(100)
        )
        #expect(grouped.map(\.role) == [.human, .assistant, .human])
        #expect(grouped[1].text == "First\n\nSecond")
        #expect(grouped[1].timestamp == started.addingTimeInterval(70))

        let working = ConversationLedgerFormatting.turns(
            messages: [
                ConversationMessage(line: 1, role: .user, text: "Inspect this", timestamp: started),
                ConversationMessage(line: 2, role: .assistant, text: "Done", timestamp: started.addingTimeInterval(65)),
                ConversationMessage(line: 3, role: .user, text: "Continue", timestamp: started.addingTimeInterval(100)),
            ],
            provider: .claude,
            activity: "working",
            now: started.addingTimeInterval(130)
        )
        #expect(working.last?.role == .assistant)
        #expect(working.last?.text == "")
        #expect(working.last?.timestamp == nil)
        #expect(working.last?.elapsed == "30s")
        #expect(working.last?.isWorking == true)
    }

    @Test @MainActor func ledgerDocumentKeepsRoleBodiesObservable() {
        let started = Date(timeIntervalSince1970: 0)
        let document = ConversationLedgerFormatting.document(
            messages: [
                ConversationMessage(line: 1, role: .user, text: "Inspect this", timestamp: started),
                ConversationMessage(line: 2, role: .assistant, text: "**Done**", timestamp: started.addingTimeInterval(65)),
            ],
            provider: .claude,
            activity: "idle",
            now: started.addingTimeInterval(65),
            textScale: 1
        )

        #expect(document.attributedString.string.contains("> Inspect this"))
        #expect(document.attributedString.string.contains("Done"))
        #expect(!document.attributedString.string.contains("(turn.text)"))
        #expect(document.metadata.count == 2)
    }

    @Test @MainActor func ledgerDocumentKeepsElapsedVisibleForEmptyWorkingTurn() {
        let started = Date(timeIntervalSince1970: 0)
        let document = ConversationLedgerFormatting.document(
            messages: [
                ConversationMessage(line: 1, role: .user, text: "Inspect this", timestamp: started),
            ],
            provider: .codex,
            activity: "working",
            now: started.addingTimeInterval(30),
            textScale: 1
        )

        let workingMetadata = document.metadata.values.first(where: \.isWorking)
        #expect(workingMetadata?.timestamp == nil)
        #expect(workingMetadata?.elapsed == "30s")
        #expect(workingMetadata?.isWorking == true)
    }

    @Test func claudeProjectSlugWritesEveryNonAlphanumericAsHyphen() {
        // Names observed under ~/.claude/projects on a workstation.
        #expect(ConversationReader.projectDirectory(for: "/Users/x/.claude") == "-Users-x--claude")
        #expect(
            ConversationReader.projectDirectory(for: "/Users/x/Library/Mobile Documents/iCloud~md~obsidian/Documents")
                == "-Users-x-Library-Mobile-Documents-iCloud-md-obsidian-Documents"
        )
        #expect(ConversationReader.projectDirectory(for: "/tmp/herdr-ide.worktrees/a_b") == "-tmp-herdr-ide-worktrees-a-b")
    }

    @Test func claudeReaderResolvesASlugWithSpacesAndUnderscores() throws {
        let root = try temporaryRoot()
        defer { try? FileManager.default.removeItem(at: root) }
        let directory = root.appendingPathComponent(".claude/projects/-tmp-My-Project-v2-0", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try #"{"type":"assistant","message":{"role":"assistant","content":"Hi"}}"#
            .write(to: directory.appendingPathComponent("s.jsonl"), atomically: true, encoding: .utf8)

        let result = ConversationReader(homeDirectory: root).read(
            provider: .claude,
            sessionID: "s",
            cwd: "/tmp/My Project_v2.0"
        )
        guard case let .loaded(_, messages) = result else {
            Issue.record("expected the slug to resolve, got \(result)")
            return
        }
        #expect(messages.map(\.text) == ["Hi"])
    }

    @Test func safeSessionIdentifierRuleRefusesAnythingAPathWouldInterpret() {
        #expect(ConversationReader.isSafeSessionIdentifier("01a0a077-9b97-7042-964a-e7975a95bff2"))
        #expect(ConversationReader.isSafeSessionIdentifier("session_1"))
        for unsafe in ["", ".", "..", "../x", "a/b", "/abs", "a\\b", "a\0b", "x/.."] {
            #expect(!ConversationReader.isSafeSessionIdentifier(unsafe), "accepted \(unsafe.debugDescription)")
        }
    }

    @Test func readerRejectsCodexSessionPathTraversal() throws {
        let root = try temporaryRoot()
        defer { try? FileManager.default.removeItem(at: root) }
        let directory = root.appendingPathComponent(".codex/sessions/2026/09/15", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        try #"{"type":"response_item","payload":{"type":"message","role":"assistant","content":"must not load"}}"#
            .write(to: root.appendingPathComponent("escaped.jsonl"), atomically: true, encoding: .utf8)

        let result = ConversationReader(homeDirectory: root).read(
            provider: .codex,
            sessionID: "../../../escaped",
            cwd: "/tmp/project"
        )
        guard case let .empty(path) = result else {
            Issue.record("expected traversal to resolve to an empty safe path, got \(result)")
            return
        }
        #expect(path.hasSuffix("/.codex/sessions/unknown-session.jsonl"))
    }

    @Test func missingLedgerIsEmptyButAnUnreadableOneIsFailed() throws {
        let root = try temporaryRoot()
        defer { try? FileManager.default.removeItem(at: root) }
        let reader = ConversationReader(homeDirectory: root)

        let missing = reader.read(provider: .claude, sessionID: "s", cwd: "/tmp/project")
        guard case .empty = missing else {
            Issue.record("expected a missing ledger to read as empty, got \(missing)")
            return
        }
        let missingCodex = reader.read(provider: .codex, sessionID: "s", cwd: "/tmp/project")
        guard case .empty = missingCodex else {
            Issue.record("expected a missing Codex tree to read as empty, got \(missingCodex)")
            return
        }

        // Root can read anything, so the permission half has no meaning there.
        guard geteuid() != 0 else { return }
        let fileManager = FileManager.default

        let claudeProject = root.appendingPathComponent(".claude/projects/-tmp-project", isDirectory: true)
        try fileManager.createDirectory(at: claudeProject, withIntermediateDirectories: true)
        try "".write(to: claudeProject.appendingPathComponent("s.jsonl"), atomically: true, encoding: .utf8)
        try fileManager.setAttributes([.posixPermissions: 0], ofItemAtPath: claudeProject.path)
        defer { try? fileManager.setAttributes([.posixPermissions: 0o755], ofItemAtPath: claudeProject.path) }
        let sealedDirectory = reader.read(provider: .claude, sessionID: "s", cwd: "/tmp/project")
        guard case let .failed(path, line, reason) = sealedDirectory else {
            Issue.record("expected an unreadable project directory to fail, got \(sealedDirectory)")
            return
        }
        #expect(path.hasSuffix("/-tmp-project/s.jsonl"))
        #expect(line == 0)
        #expect(reason == "Permission denied")

        let sealedFile = root.appendingPathComponent(".claude/projects/-tmp-other/t.jsonl")
        try fileManager.createDirectory(at: sealedFile.deletingLastPathComponent(), withIntermediateDirectories: true)
        try #"{"type":"assistant","message":{"role":"assistant","content":"Hi"}}"#
            .write(to: sealedFile, atomically: true, encoding: .utf8)
        try fileManager.setAttributes([.posixPermissions: 0], ofItemAtPath: sealedFile.path)
        let unreadableFile = reader.read(provider: .claude, sessionID: "t", cwd: "/tmp/other")
        guard case let .failed(_, _, fileReason) = unreadableFile else {
            Issue.record("expected an unreadable ledger to fail, got \(unreadableFile)")
            return
        }
        #expect(fileReason == "Conversation file could not be read")

        let codexDay = root.appendingPathComponent(".codex/sessions/2026/09/15", isDirectory: true)
        try fileManager.createDirectory(at: codexDay, withIntermediateDirectories: true)
        try fileManager.setAttributes([.posixPermissions: 0], ofItemAtPath: codexDay.path)
        defer { try? fileManager.setAttributes([.posixPermissions: 0o755], ofItemAtPath: codexDay.path) }
        let sealedCodex = reader.read(provider: .codex, sessionID: "u", cwd: "/tmp/project")
        guard case let .failed(codexPath, _, codexReason) = sealedCodex else {
            Issue.record("expected an unreadable Codex day directory to fail, got \(sealedCodex)")
            return
        }
        #expect(codexPath.hasSuffix("/.codex/sessions/2026/09/15"))
        #expect(!codexReason.isEmpty)
    }

    @Test func codexLookupVisitsOnlyTheNewestDayDirectories() throws {
        let root = try temporaryRoot()
        defer { try? FileManager.default.removeItem(at: root) }
        let sessions = root.appendingPathComponent(".codex/sessions", isDirectory: true)
        let record = #"{"type":"response_item","payload":{"type":"message","role":"assistant","content":"Found"}}"#
        // One more day than the lookup visits, newest first; the target sits
        // in the oldest one, just past the window.
        let dayCount = ConversationReader.codexLookupDayLimit + 1
        for index in 0..<dayCount {
            let day = sessions.appendingPathComponent(String(format: "2026/08/%02d", dayCount - index), isDirectory: true)
            try FileManager.default.createDirectory(at: day, withIntermediateDirectories: true)
        }
        let stale = sessions.appendingPathComponent("2026/08/01/rollout-2026-08-01T00-00-00-old.jsonl")
        try record.write(to: stale, atomically: true, encoding: .utf8)
        try "not a date".write(to: sessions.appendingPathComponent("notes.txt"), atomically: true, encoding: .utf8)

        let reader = ConversationReader(homeDirectory: root)
        let outside = reader.read(provider: .codex, sessionID: "old", cwd: "/tmp/project")
        guard case .empty = outside else {
            Issue.record("expected a rollout past the day window to read as empty, got \(outside)")
            return
        }

        let recent = sessions.appendingPathComponent(String(format: "2026/08/%02d/rollout-2026-08-15T00-00-00-new.jsonl", dayCount))
        try record.write(to: recent, atomically: true, encoding: .utf8)
        let inside = reader.read(provider: .codex, sessionID: "new", cwd: "/tmp/project")
        guard case let .loaded(path, messages) = inside else {
            Issue.record("expected the newest day's rollout to load, got \(inside)")
            return
        }
        #expect(URL(fileURLWithPath: path).standardizedFileURL.path == recent.standardizedFileURL.path)
        #expect(messages.map(\.text) == ["Found"])
    }

    @Test func initialReadOfALargeLedgerIsBoundedToItsTail() throws {
        let root = try temporaryRoot()
        defer { try? FileManager.default.removeItem(at: root) }
        let directory = root.appendingPathComponent(".claude/projects/-tmp-project", isDirectory: true)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        let path = directory.appendingPathComponent("big.jsonl")

        let early = #"{"type":"assistant","message":{"role":"assistant","content":"early"}}"# + "\n"
        let filler = #"{"type":"progress","message":{"content":""# + String(repeating: "x", count: 4096) + "\"}}\n"
        var body = early
        while body.utf8.count <= Int(ConversationReader.initialReadLimit) + filler.utf8.count {
            body += filler
        }
        body += #"{"type":"user","message":{"role":"user","content":"late question"}}"# + "\n"
        try body.write(to: path, atomically: true, encoding: .utf8)

        let reader = ConversationReader(homeDirectory: root)
        let first = reader.read(provider: .claude, sessionID: "big", cwd: "/tmp/project")
        guard case let .loaded(_, messages) = first else {
            Issue.record("expected the tail to load, got \(first)")
            return
        }
        #expect(messages.map(\.text) == ["late question"])

        try append(#"{"type":"assistant","message":{"role":"assistant","content":"late answer"}}"# + "\n", to: path)
        let appended = reader.read(provider: .claude, sessionID: "big", cwd: "/tmp/project")
        guard case let .loaded(_, appendedMessages) = appended else {
            Issue.record("expected the appended record to load, got \(appended)")
            return
        }
        #expect(appendedMessages.map(\.text) == ["late question", "late answer"])

        try append("broken\n", to: path)
        let broken = reader.read(provider: .claude, sessionID: "big", cwd: "/tmp/project")
        guard case let .failed(_, line, reason) = broken else {
            Issue.record("expected the malformed tail to fail, got \(broken)")
            return
        }
        #expect(line == 0)
        #expect(reason.hasPrefix("Invalid JSONL record at byte "))
    }

    private func temporaryRoot() throws -> URL {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("conversation-reader-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        return root
    }

    private func append(_ string: String, to path: URL) throws {
        let handle = try FileHandle(forWritingTo: path)
        defer { try? handle.close() }
        try handle.seekToEnd()
        try handle.write(contentsOf: Data(string.utf8))
    }
}
