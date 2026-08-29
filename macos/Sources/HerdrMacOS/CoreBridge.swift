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
    let zoomed: String?
    let paneLayout: CorePaneLayoutSnapshot?
    let terminal: CoreTerminalSnapshot
    let editor: CoreEditorSnapshot
    let uiState: CoreUIStateSnapshot
    let status: CoreStatusSnapshot

    enum CodingKeys: String, CodingKey {
        case schemaVersion = "schema_version"
        case navigator
        case zoomed
        case paneLayout = "pane_layout"
        case terminal
        case editor
        case uiState = "ui_state"
        case status
    }
}

struct CorePaneLayoutSnapshot: Decodable {
    let workspaceID: String
    let tabID: String
    let focusedPaneID: String
    let zoomed: Bool
    let root: CorePaneLayoutNode

    enum CodingKeys: String, CodingKey {
        case workspaceID = "workspace_id"
        case tabID = "tab_id"
        case focusedPaneID = "focused_pane_id"
        case zoomed
        case root
    }
}

indirect enum CorePaneLayoutNode: Decodable {
    case pane(paneID: String)
    case split(
        direction: PaneSplitDirection,
        ratio: Double,
        first: CorePaneLayoutNode,
        second: CorePaneLayoutNode
    )

    private enum CodingKeys: String, CodingKey {
        case type
        case paneID = "pane_id"
        case direction
        case ratio
        case first
        case second
    }

    private enum NodeType: String, Decodable {
        case pane
        case split
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        switch try container.decode(NodeType.self, forKey: .type) {
        case .pane:
            self = .pane(paneID: try container.decode(String.self, forKey: .paneID))
        case .split:
            self = .split(
                direction: try container.decode(PaneSplitDirection.self, forKey: .direction),
                ratio: try container.decode(Double.self, forKey: .ratio),
                first: try container.decode(CorePaneLayoutNode.self, forKey: .first),
                second: try container.decode(CorePaneLayoutNode.self, forKey: .second)
            )
        }
    }

    var paneIDs: [String] {
        switch self {
        case let .pane(paneID):
            [paneID]
        case let .split(_, _, first, second):
            first.paneIDs + second.paneIDs
        }
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
    let panes: [CoreTerminalPaneSnapshot]

    enum CodingKeys: String, CodingKey {
        case paneID = "pane_id"
        case sequence
        case chunks
        case closed
        case exitCode = "exit_code"
        case panes
    }
}

struct CoreTerminalChunk: Decodable {
    let paneID: String
    let sequence: UInt64
    let bytesBase64: String

    enum CodingKeys: String, CodingKey {
        case paneID = "pane_id"
        case sequence
        case bytesBase64 = "bytes_base64"
    }
}

struct CoreTerminalPaneSnapshot: Decodable, Identifiable {
    var id: String { paneID }
    let paneID: String
    let closed: Bool
    let exitCode: Int32?

    enum CodingKeys: String, CodingKey {
        case paneID = "pane_id"
        case closed
        case exitCode = "exit_code"
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
    let shortcutBindings: [String: String]

    enum CodingKeys: String, CodingKey {
        case expandedPaths = "expanded_paths"
        case selectedPath = "selected_path"
        case selectedPaneID = "selected_pane_id"
        case shortcutBindings = "shortcut_bindings"
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        expandedPaths = try container.decode([String].self, forKey: .expandedPaths)
        selectedPath = try container.decodeIfPresent(String.self, forKey: .selectedPath)
        selectedPaneID = try container.decodeIfPresent(String.self, forKey: .selectedPaneID)
        shortcutBindings = try container.decodeIfPresent(
            [String: String].self,
            forKey: .shortcutBindings
        ) ?? [:]
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
    let herdr: CoreHerdrStatus
    let chromux: CoreChromuxStatus
    let environment: [CoreEnvironmentStatus]
    let diagnostics: [CoreDiagnostic]
    let lastError: CoreLastError?

    enum CodingKeys: String, CodingKey {
        case herdr
        case chromux
        case environment
        case diagnostics
        case lastError = "last_error"
    }
}

struct CoreHerdrStatus: Decodable {
    let state: String
    let socketPath: String?
    let message: String?

    enum CodingKeys: String, CodingKey {
        case state
        case socketPath = "socket_path"
        case message
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

    let workspaceRoot: URL
    let isRemoteWorkspace: Bool

    nonisolated(unsafe) private var core: OpaquePointer?
    private var lastTerminalSequence: UInt64 = 0
    private var pendingTerminalBytes: [String: [[UInt8]]] = [:]
    private var terminalRegistrations: [String: TerminalRegistration] = [:]
    private var restoredPaneSelection = false

    private struct TerminalRegistration {
        let id: UUID
        let receive: ([UInt8]) -> Void
        let focus: () -> Void
    }

    init(arguments: [String] = CommandLine.arguments) {
        isRemoteWorkspace = arguments.contains("--remote-workspace")
        workspaceRoot = LaunchArguments.value("--workspace-root", in: arguments)
            .map { URL(fileURLWithPath: $0, isDirectory: true) }
            ?? URL(fileURLWithPath: FileManager.default.currentDirectoryPath, isDirectory: true)
        let statePath = LaunchArguments.value("--state-path", in: arguments)
            ?? "/tmp/herdr-ide-verify-ui-state.json"
        // The verification fixture runs without any live herdr connection;
        // every other launch talks to the local herdr socket.
        let fixtureMode = arguments.contains("--verification-ui-fixture")
        let options: [String: Any] = [
            "schema_version": coreSchemaVersion,
            "herdr_socket_path": fixtureMode ? NSNull() : Self.defaultHerdrSocketPath() as Any,
            "herdr_bin_path": Self.resolveHerdrBinaryPath().map { $0 as Any } ?? NSNull(),
            "remote_targets": [[
                "id": "mini",
                "label": "Mac mini",
                "ssh_alias": "mini",
            ]],
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

    func sendTerminalInput(_ bytes: [UInt8], paneID: String? = nil) {
        // Input goes to the pane the terminal is attached to; the loopback
        // pane only exists for the verification fixture.
        let target = paneID ?? snapshot?.terminal.paneID ?? "local-loopback"
        dispatch(kind: "key", payload: [
            "pane_id": target,
            "bytes_base64": Data(bytes).base64EncodedString(),
        ])
    }

    func resizeTerminal(paneID: String, cols: Int, rows: Int) {
        dispatch(kind: "terminal_resize", payload: [
            "pane_id": paneID,
            "cols": cols,
            "rows": rows,
        ])
    }

    func focusPane(_ paneID: String) {
        dispatch(kind: "focus_pane", payload: ["pane_id": paneID])
    }

    @discardableResult
    func registerTerminal(
        paneID: String,
        receive: @escaping ([UInt8]) -> Void,
        focus: @escaping () -> Void
    ) -> UUID {
        let registrationID = UUID()
        terminalRegistrations[paneID] = TerminalRegistration(
            id: registrationID,
            receive: receive,
            focus: focus
        )
        drainPendingTerminalBytes(for: paneID)
        if snapshot?.terminal.paneID == paneID {
            DispatchQueue.main.async { [weak self] in
                guard self?.terminalRegistrations[paneID]?.id == registrationID else { return }
                self?.terminalRegistrations[paneID]?.focus()
            }
        }
        return registrationID
    }

    func unregisterTerminal(paneID: String, registrationID: UUID) {
        guard terminalRegistrations[paneID]?.id == registrationID else { return }
        terminalRegistrations.removeValue(forKey: paneID)
    }

    func splitCurrentPane(direction: PaneSplitDirection) {
        guard let paneID = snapshot?.terminal.paneID else {
            bridgeError = "pane.no_current_pane: Select a terminal pane before splitting"
            return
        }
        dispatch(kind: "create_pane", payload: [
            "tab_id": paneID,
            "cwd": workspaceRoot.path,
            "command": NSNull(),
            "direction": direction.rawValue,
        ])
    }

    func toggleCurrentPaneZoom() {
        guard let paneID = snapshot?.terminal.paneID else {
            bridgeError = "pane.no_current_pane: Select a terminal pane before toggling zoom"
            return
        }
        dispatch(kind: "toggle_zoom", payload: ["pane_id": paneID])
    }

    func closePane(_ paneID: String, confirmed: Bool) {
        dispatch(kind: "close_pane", payload: [
            "pane_id": paneID,
            "confirmed": confirmed,
        ])
    }

    func recordBrowserStatus(_ receipt: BrowserRuntimeReceipt) {
        let checkedMilliseconds = UInt64(
            (ISO8601DateFormatter().date(from: receipt.checkedAt)?.timeIntervalSince1970 ?? Date().timeIntervalSince1970) * 1_000
        )
        dispatch(kind: "browser_status", payload: [
            "state": receipt.phase.rawValue,
            "profile": receipt.profile,
            "current_url": receipt.currentURL.map { $0 as Any } ?? NSNull(),
            "current_title": receipt.currentTitle.map { $0 as Any } ?? NSNull(),
            "message": receipt.message,
            "last_checked_at_unix_ms": NSNumber(value: checkedMilliseconds),
        ])
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
        selectedPaneID: String? = nil,
        shortcutBindings: [String: String]? = nil
    ) {
        let current = snapshot?.uiState
        let effectivePath = selectedPath ?? current?.selectedPath
        let effectivePaneID = selectedPaneID ?? current?.selectedPaneID
        dispatch(kind: "ui_state_update", payload: [
            "expanded_paths": expandedPaths ?? current?.expandedPaths ?? [],
            "selected_path": effectivePath.map { $0 as Any } ?? NSNull(),
            "selected_pane_id": effectivePaneID.map { $0 as Any } ?? NSNull(),
            "shortcut_bindings": shortcutBindings ?? current?.shortcutBindings ?? [:],
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

    func environmentState(for key: String) -> String? {
        snapshot?.status.environment.first(where: { $0.key == key })?.state
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
            let previousFocusedPaneID = snapshot?.paneLayout?.focusedPaneID
                ?? snapshot?.terminal.paneID
            snapshot = decoded
            bridgeError = decoded.status.lastError.map { "\($0.kind): \($0.message)" }
            restorePaneSelectionIfNeeded(decoded)
            let authoritativeFocusedPaneID = decoded.paneLayout?.focusedPaneID
                ?? decoded.terminal.paneID
            if authoritativeFocusedPaneID != previousFocusedPaneID,
               let authoritativeFocusedPaneID {
                DispatchQueue.main.async { [weak self] in
                    self?.terminalRegistrations[authoritativeFocusedPaneID]?.focus()
                }
            }
            for chunk in decoded.terminal.chunks
                .filter({ $0.sequence > lastTerminalSequence })
                .sorted(by: { $0.sequence < $1.sequence })
            {
                lastTerminalSequence = chunk.sequence
                guard let data = Data(base64Encoded: chunk.bytesBase64) else {
                    bridgeError = "terminal.invalid_base64: sequence \(chunk.sequence)"
                    continue
                }
                pendingTerminalBytes[chunk.paneID, default: []].append([UInt8](data))
                drainPendingTerminalBytes(for: chunk.paneID)
            }
        } catch {
            bridgeError = "Snapshot decode failed: \(error.localizedDescription)"
        }
    }

    /// Restores the persisted pane selection once on launch so the terminal
    /// reattaches to the pane the user last worked in. A pane that no longer
    /// exists fails through the normal attach error path.
    private func restorePaneSelectionIfNeeded(_ decoded: CoreSnapshot) {
        guard !restoredPaneSelection else { return }
        restoredPaneSelection = true
        guard decoded.terminal.paneID == nil,
              let persisted = decoded.uiState.selectedPaneID else { return }
        focusPane(persisted)
    }

    private func drainPendingTerminalBytes(for paneID: String) {
        guard let registration = terminalRegistrations[paneID],
              let pending = pendingTerminalBytes.removeValue(forKey: paneID)
        else { return }
        for bytes in pending {
            registration.receive(bytes)
        }
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

    /// The environment registry in herdr-core applies HERDR_SOCKET_PATH to
    /// this configured default without exposing the raw value to Swift.
    static func defaultHerdrSocketPath() -> String {
        return NSHomeDirectory() + "/.config/herdr/herdr.sock"
    }

    /// Finder launches carry no shell PATH, so the herdr binary is resolved
    /// from its known install locations. A missing binary stays nil and pane
    /// attach fails with explicit guidance instead of a silent no-op.
    static func resolveHerdrBinaryPath() -> String? {
        let candidates = [
            NSHomeDirectory() + "/.local/bin/herdr",
            "/opt/homebrew/bin/herdr",
            "/usr/local/bin/herdr",
        ]
        return candidates.first { FileManager.default.isExecutableFile(atPath: $0) }
    }
}
