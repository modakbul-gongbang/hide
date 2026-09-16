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
