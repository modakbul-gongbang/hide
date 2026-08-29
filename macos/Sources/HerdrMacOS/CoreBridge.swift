import CHerdrCore
import Foundation

private let coreSchemaVersion = 1

private let coreChangeCallback: @convention(c) (UnsafeMutableRawPointer?) -> Void = { context in
    guard let context else { return }
    let bridge = Unmanaged<CoreBridge>.fromOpaque(context).takeUnretainedValue()
    bridge.receiveCoreChange()
}

struct CoreSnapshot: Decodable {
    let schemaVersion: UInt32
    let navigator: CoreNavigatorSnapshot
    let terminal: CoreTerminalSnapshot
    let editor: CoreEditorSnapshot
    let uiState: CoreUIStateSnapshot
    let status: CoreStatusSnapshot

    enum CodingKeys: String, CodingKey {
        case schemaVersion = "schema_version"
        case navigator
        case terminal
        case editor
        case uiState = "ui_state"
        case status
    }
}

struct CoreNavigatorSnapshot: Decodable {
    let rootPath: String?
    let agents: [SidebarAgent]

    enum CodingKeys: String, CodingKey {
        case rootPath = "root_path"
        case agents
    }
}

struct SidebarAgent: Decodable, Identifiable {
    let id: String
    let paneID: String
    let workspaceLabel: String
    let agentKind: String
    let state: String
    let symbol: String
    let summary: String
    let elapsed: String
    let sortRank: String
    let activity: String

    enum CodingKeys: String, CodingKey {
        case id
        case paneID = "pane_id"
        case workspaceLabel = "workspace_label"
        case agentKind = "agent_kind"
        case state
        case symbol
        case summary
        case elapsed
        case sortRank = "sort_rank"
        case activity
    }
}

struct CoreTerminalSnapshot: Decodable {
    let paneID: String?
    let sequence: UInt64
    let chunks: [CoreTerminalChunk]
    let closed: Bool
    let exitCode: Int32?

    enum CodingKeys: String, CodingKey {
        case paneID = "pane_id"
        case sequence
        case chunks
        case closed
        case exitCode = "exit_code"
    }
}

struct CoreTerminalChunk: Decodable {
    let sequence: UInt64
    let bytesBase64: String

    enum CodingKeys: String, CodingKey {
        case sequence
        case bytesBase64 = "bytes_base64"
    }
}

struct CoreEditorSnapshot: Decodable {
    let path: String?
    let language: String?
    let contentsUTF8: String?
    let openedModifiedAt: UInt64?
    let dirty: Bool
    let readonlyReason: String?
    let conflict: CoreEditorConflict?
    let diff: CoreDiffSnapshot?

    enum CodingKeys: String, CodingKey {
        case path
        case language
        case contentsUTF8 = "contents_utf8"
        case openedModifiedAt = "opened_modified_at_unix_ms"
        case dirty
        case readonlyReason = "readonly_reason"
        case conflict
        case diff
    }
}

struct CoreUIStateSnapshot: Decodable {
    let expandedPaths: [String]
    let selectedPath: String?
    let selectedPaneID: String?

    enum CodingKeys: String, CodingKey {
        case expandedPaths = "expanded_paths"
        case selectedPath = "selected_path"
        case selectedPaneID = "selected_pane_id"
    }
}

struct CoreEditorConflict: Decodable {
    let diskModifiedAt: UInt64
    let openedModifiedAt: UInt64

    enum CodingKeys: String, CodingKey {
        case diskModifiedAt = "disk_modified_at_unix_ms"
        case openedModifiedAt = "opened_modified_at_unix_ms"
    }
}

struct CoreDiffSnapshot: Decodable {
    let addedLines: [UInt32]
    let removedLines: [UInt32]

    enum CodingKeys: String, CodingKey {
        case addedLines = "added_lines"
        case removedLines = "removed_lines"
    }
}

struct CoreStatusSnapshot: Decodable {
    let chromux: CoreChromuxStatus
    let environment: [CoreEnvironmentStatus]
    let diagnostics: [CoreDiagnostic]
    let lastError: CoreLastError?

    enum CodingKeys: String, CodingKey {
        case chromux
        case environment
        case diagnostics
        case lastError = "last_error"
    }
}

struct CoreChromuxStatus: Decodable {
    let state: String
    let profile: String
    let message: String?
}

struct CoreEnvironmentStatus: Decodable, Identifiable {
    var id: String { key }
    let key: String
    let required: Bool
    let format: String
    let state: String
    let absentBehavior: String
    let message: String

    enum CodingKeys: String, CodingKey {
        case key
        case required
        case format
        case state
        case absentBehavior = "absent_behavior"
        case message
    }
}

struct CoreDiagnostic: Decodable, Identifiable {
    var id: String { "\(kind)-\(occurredAt)" }
    let kind: String
    let message: String
    let occurredAt: UInt64

    enum CodingKeys: String, CodingKey {
        case kind
        case message
        case occurredAt = "occurred_at"
    }
}

struct CoreLastError: Decodable {
    let kind: String
    let message: String
    let retryable: Bool
    let occurredAt: UInt64

    enum CodingKeys: String, CodingKey {
        case kind
        case message
        case retryable
        case occurredAt = "occurred_at"
    }
}

@MainActor
final class CoreBridge: ObservableObject, @unchecked Sendable {
    @Published private(set) var snapshot: CoreSnapshot?
    @Published private(set) var bridgeError: String?

    var onTerminalBytes: (([UInt8]) -> Void)? {
        didSet { drainPendingTerminalBytes() }
    }

    let workspaceRoot: URL

    nonisolated(unsafe) private var core: OpaquePointer?
    private var lastTerminalSequence: UInt64 = 0
    private var pendingTerminalBytes: [[UInt8]] = []

    init(arguments: [String] = CommandLine.arguments) {
        workspaceRoot = Self.argumentValue("--workspace-root", in: arguments)
            .map { URL(fileURLWithPath: $0, isDirectory: true) }
            ?? URL(fileURLWithPath: FileManager.default.currentDirectoryPath, isDirectory: true)
        let statePath = Self.argumentValue("--state-path", in: arguments)
            ?? "/tmp/herdr-ide-verify-ui-state.json"
        let options: [String: Any] = [
            "schema_version": coreSchemaVersion,
            "herdr_socket_path": NSNull(),
            "remote_targets": [],
            "app_state_path": statePath,
        ]
        guard
            let data = try? JSONSerialization.data(withJSONObject: options),
            let created = data.withUnsafeBytes({ buffer in
                herdr_core_create(buffer.bindMemory(to: UInt8.self).baseAddress, data.count)
            })
        else {
            bridgeError = "herdr_core_create returned null"
            return
        }
        core = created
        herdr_core_on_change(
            created,
            coreChangeCallback,
            Unmanaged.passUnretained(self).toOpaque()
        )
        refreshSnapshot()

        #if DEBUG
        if arguments.contains("--verification-ui-fixture") {
            seedVerificationFixture()
        }
        #endif
    }

    deinit {
        if let core {
            herdr_core_on_change(core, nil, nil)
            herdr_core_destroy(core)
        }
    }

    nonisolated func receiveCoreChange() {
        DispatchQueue.main.async { [weak self] in
            self?.refreshSnapshot()
        }
    }

    func sendTerminalInput(_ bytes: [UInt8], paneID: String = "local-loopback") {
        dispatch(kind: "key", payload: [
            "pane_id": paneID,
            "bytes_base64": Data(bytes).base64EncodedString(),
        ])
    }

    func focusPane(_ paneID: String) {
        dispatch(kind: "focus_pane", payload: ["pane_id": paneID])
        persistUIState(selectedPaneID: paneID)
    }

    func openFile(_ url: URL) {
        dispatch(kind: "file_open", payload: ["path": url.path])
        persistUIState(selectedPath: url.path)
    }

    func updateDraft(_ contents: String) {
        dispatch(kind: "file_draft", payload: ["contents_utf8": contents])
    }

    func saveFile(_ contents: String) {
        guard let editor = snapshot?.editor, let path = editor.path else { return }
        dispatch(kind: "file_save", payload: [
            "path": path,
            "contents_utf8": contents,
            "expected_modified_at_unix_ms": editor.openedModifiedAt.map { NSNumber(value: $0) as Any } ?? NSNull(),
        ])
    }

    func resolveConflict(_ action: String) {
        dispatch(kind: "file_conflict", payload: ["action": action])
    }

    func persistUIState(
        expandedPaths: [String]? = nil,
        selectedPath: String? = nil,
        selectedPaneID: String? = nil
    ) {
        let current = snapshot?.uiState
        let effectivePath = selectedPath ?? current?.selectedPath
        let effectivePaneID = selectedPaneID ?? current?.selectedPaneID
        dispatch(kind: "ui_state_update", payload: [
            "expanded_paths": expandedPaths ?? current?.expandedPaths ?? [],
            "selected_path": effectivePath.map { $0 as Any } ?? NSNull(),
            "selected_pane_id": effectivePaneID.map { $0 as Any } ?? NSNull(),
        ])
    }

    func dispatch(kind: String, payload: [String: Any]) {
        guard let core else { return }
        let envelope: [String: Any] = [
            "schema_version": coreSchemaVersion,
            "kind": kind,
            "payload": payload,
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: envelope) else {
            bridgeError = "Could not encode \(kind) event"
            return
        }
        data.withUnsafeBytes { buffer in
            herdr_core_dispatch(core, buffer.bindMemory(to: UInt8.self).baseAddress, data.count)
        }
    }

    private func refreshSnapshot() {
        guard let core else { return }
        let owned = herdr_core_snapshot(core)
        defer { herdr_core_free_bytes(owned) }
        guard let pointer = owned.ptr, owned.len > 0 else {
            bridgeError = "herdr_core_snapshot returned empty bytes"
            return
        }
        do {
            let data = Data(bytes: pointer, count: owned.len)
            let decoded = try JSONDecoder().decode(CoreSnapshot.self, from: data)
            snapshot = decoded
            bridgeError = decoded.status.lastError.map { "\($0.kind): \($0.message)" }
            for chunk in decoded.terminal.chunks
                .filter({ $0.sequence > lastTerminalSequence })
                .sorted(by: { $0.sequence < $1.sequence })
            {
                lastTerminalSequence = chunk.sequence
                guard let data = Data(base64Encoded: chunk.bytesBase64) else {
                    bridgeError = "terminal.invalid_base64: sequence \(chunk.sequence)"
                    continue
                }
                pendingTerminalBytes.append([UInt8](data))
            }
            drainPendingTerminalBytes()
        } catch {
            bridgeError = "Snapshot decode failed: \(error.localizedDescription)"
        }
    }

    private func drainPendingTerminalBytes() {
        guard let onTerminalBytes else { return }
        for bytes in pendingTerminalBytes {
            onTerminalBytes(bytes)
        }
        pendingTerminalBytes.removeAll(keepingCapacity: true)
    }

    #if DEBUG
    private func seedVerificationFixture() {
        let agents: [[String: Any]] = [
            Self.fixtureAgent("error", "×", "00", "1755000007000", "Build failed", "12s", "Core", "codex", "status_error_new"),
            Self.fixtureAgent("question", "?", "01", "1755000006000", "Choose persistence scope", "4m", "UI", "claude", "status_question_new"),
            Self.fixtureAgent("approval", "!", "02", "1755000005000", "Approve local save", "8m", "Workbench", "codex", "status_approval_new"),
            Self.fixtureAgent("done", "●", "04", "1755000004000", "Sidebar contract complete", "2h", "Agents", "claude", "status_done_new"),
            Self.fixtureAgent("working", "●", "05", "1755000003000", "Connecting Rust bytes", "18s", "Terminal", "codex", "status_working"),
            Self.fixtureAgent("idle", "○", "10", "1755000002000", "Reviewed fixture", "3d", "Verify", "claude", "status_idle"),
            Self.fixtureAgent("unknown", "~", "10", "1755000001000", "Awaiting lifecycle token", "9m", "Other", "unknown", "status_unknown"),
        ]
        dispatch(kind: "session_snapshot", payload: ["agents": agents])
        let banner = "\u{001B}[1;36mherdr-core ↔ SwiftTerm\u{001B}[0m\r\nLocal byte bridge ready. IME V9 remains blocked.\r\n\r\n"
        dispatch(kind: "terminal_output", payload: [
            "pane_id": "local-loopback",
            "bytes_base64": Data(banner.utf8).base64EncodedString(),
        ])
    }

    private static func fixtureAgent(
        _ id: String,
        _ symbol: String,
        _ rank: String,
        _ activity: String,
        _ summary: String,
        _ elapsed: String,
        _ workspace: String,
        _ agent: String,
        _ statusToken: String
    ) -> [String: Any] {
        [
            "id": id,
            "pane_id": "fixture-\(id)",
            "workspace_label": workspace,
            "agent": agent,
            "agent_status": id == "working" ? "working" : id == "idle" ? "idle" : "unknown",
            "tokens": [
                statusToken: symbol,
                "sort_rank": rank,
                "activity": activity,
                "summary": summary,
                "elapsed": elapsed,
            ],
        ]
    }
    #endif

    private static func argumentValue(_ flag: String, in arguments: [String]) -> String? {
        guard let index = arguments.firstIndex(of: flag), arguments.indices.contains(index + 1) else {
            return nil
        }
        return arguments[index + 1]
    }
}
