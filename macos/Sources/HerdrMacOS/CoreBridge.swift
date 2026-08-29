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
    let pet: CorePetSnapshot

    enum CodingKeys: String, CodingKey {
        case schemaVersion = "schema_version"
        case navigator
        case zoomed
        case paneLayout = "pane_layout"
        case terminal
        case editor
        case uiState = "ui_state"
        case status
        case pet
    }
}

struct CorePetSnapshot: Decodable, Equatable {
    let visible: Bool
    let connection: String
    let connectionMessage: String?
    let pose: String
    let sleepPhase: String
    let roamAllowed: Bool
    let badges: CorePetBadges
    let attentionPaneIDs: [String]
    let origin: CorePetOrigin?
    let shortcut: String?
    let shortcutError: String?
    let themeID: String
    let lastClick: CorePetClick?

    /// True while herdr is answering. Every other connection value is an
    /// explicit failure the pet shows rather than posing idle through.
    var isConnected: Bool { connection == "connected" }

    enum CodingKeys: String, CodingKey {
        case visible
        case connection
        case connectionMessage = "connection_message"
        case pose
        case sleepPhase = "sleep_phase"
        case roamAllowed = "roam_allowed"
        case badges
        case attentionPaneIDs = "attention_pane_ids"
        case origin
        case shortcut
        case shortcutError = "shortcut_error"
        case themeID = "theme_id"
        case lastClick = "last_click"
    }
}

struct CorePetBadges: Decodable, Equatable {
    let working: Int
    let done: Int
    let attention: Int
    let error: Int
    let disconnected: Int
    let subagentsActive: UInt32
    let backgroundRunning: UInt32
    let backgroundFailed: UInt32

    enum CodingKeys: String, CodingKey {
        case working
        case done
        case attention
        case error
        case disconnected
        case subagentsActive = "subagents_active"
        case backgroundRunning = "background_running"
        case backgroundFailed = "background_failed"
    }

    static let none = CorePetBadges(
        working: 0,
        done: 0,
        attention: 0,
        error: 0,
        disconnected: 0,
        subagentsActive: 0,
        backgroundRunning: 0,
        backgroundFailed: 0
    )
}

struct CorePetOrigin: Decodable, Equatable {
    let x: Double
    let y: Double

    var point: CGPoint { CGPoint(x: x, y: y) }
}

struct CorePetClick: Decodable, Equatable {
    let selectedPaneID: String?
    let atUnixMilliseconds: UInt64

    enum CodingKeys: String, CodingKey {
        case selectedPaneID = "selected_pane_id"
        case atUnixMilliseconds = "at_unix_ms"
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
    let focusedDeviceID: String?
    let focusedWorkspaceID: String?
    let focusedCheckoutID: String?
    let devices: [CoreDeviceSnapshot]
    let workspaces: [CoreWorkspaceSnapshot]
    let agents: [SidebarAgent]

    enum CodingKeys: String, CodingKey {
        case rootPath = "root_path"
        case focusedDeviceID = "focused_device_id"
        case focusedWorkspaceID = "focused_workspace_id"
        case focusedCheckoutID = "focused_checkout_id"
        case devices
        case workspaces
        case agents
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        rootPath = try container.decodeIfPresent(String.self, forKey: .rootPath)
        focusedDeviceID = try container.decodeIfPresent(String.self, forKey: .focusedDeviceID)
        focusedWorkspaceID = try container.decodeIfPresent(String.self, forKey: .focusedWorkspaceID)
        focusedCheckoutID = try container.decodeIfPresent(String.self, forKey: .focusedCheckoutID)
        devices = try container.decodeIfPresent([CoreDeviceSnapshot].self, forKey: .devices) ?? []
        workspaces = try container.decodeIfPresent([CoreWorkspaceSnapshot].self, forKey: .workspaces) ?? []
        agents = try container.decodeIfPresent([SidebarAgent].self, forKey: .agents) ?? []
    }
}

struct CoreDeviceSnapshot: Decodable, Identifiable {
    let id: String
    let label: String
    let kind: String
    let state: String
    let sshAlias: String?
    let agentCount: UInt32

    enum CodingKeys: String, CodingKey {
        case id
        case label
        case kind
        case state
        case sshAlias = "ssh_alias"
        case agentCount = "agent_count"
    }
}

struct CoreWorkspaceSnapshot: Decodable, Identifiable {
    let id: String
    let label: String
    let path: String
    let remoteTargetID: String?
    let expanded: Bool
    let deviceID: String
    let repoName: String
    let isGit: Bool
    let defaultBranch: String?
    let registered: Bool
    let temporary: Bool
    let checkouts: [CoreCheckoutSnapshot]

    enum CodingKeys: String, CodingKey {
        case id
        case label
        case path
        case remoteTargetID = "remote_target_id"
        case expanded
        case deviceID = "device_id"
        case repoName = "repo_name"
        case isGit = "is_git"
        case defaultBranch = "default_branch"
        case registered
        case temporary
        case checkouts
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(String.self, forKey: .id)
        label = try container.decode(String.self, forKey: .label)
        path = try container.decode(String.self, forKey: .path)
        remoteTargetID = try container.decodeIfPresent(String.self, forKey: .remoteTargetID)
        expanded = try container.decodeIfPresent(Bool.self, forKey: .expanded) ?? true
        deviceID = try container.decodeIfPresent(String.self, forKey: .deviceID) ?? "local"
        repoName = try container.decodeIfPresent(String.self, forKey: .repoName) ?? label
        isGit = try container.decodeIfPresent(Bool.self, forKey: .isGit) ?? false
        defaultBranch = try container.decodeIfPresent(String.self, forKey: .defaultBranch)
        registered = try container.decodeIfPresent(Bool.self, forKey: .registered) ?? true
        temporary = try container.decodeIfPresent(Bool.self, forKey: .temporary) ?? false
        checkouts = try container.decodeIfPresent([CoreCheckoutSnapshot].self, forKey: .checkouts) ?? []
    }
}

struct CoreCheckoutSnapshot: Decodable, Identifiable {
    let id: String
    let workspaceID: String
    let label: String
    let path: String
    let branch: String?
    let isWorktree: Bool
    let exists: Bool
    let temporary: Bool
    let tabs: [CoreTabSnapshot]

    enum CodingKeys: String, CodingKey {
        case id
        case workspaceID = "workspace_id"
        case label
        case path
        case branch
        case isWorktree = "is_worktree"
        case exists
        case temporary
        case tabs
    }
}

struct CoreTabSnapshot: Decodable, Identifiable {
    let id: String?
    let workspaceID: String?
    let checkoutID: String?
    let label: String?
    let empty: Bool
    let panes: [CorePaneSnapshot]

    var stableID: String {
        id ?? "empty-\(checkoutID ?? workspaceID ?? label ?? "checkout")"
    }

    enum CodingKeys: String, CodingKey {
        case id
        case workspaceID = "workspace_id"
        case checkoutID = "checkout_id"
        case label
        case empty
        case panes
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decodeIfPresent(String.self, forKey: .id)
        workspaceID = try container.decodeIfPresent(String.self, forKey: .workspaceID)
        checkoutID = try container.decodeIfPresent(String.self, forKey: .checkoutID)
        label = try container.decodeIfPresent(String.self, forKey: .label)
        empty = try container.decodeIfPresent(Bool.self, forKey: .empty) ?? true
        panes = try container.decodeIfPresent([CorePaneSnapshot].self, forKey: .panes) ?? []
    }
}

struct CorePaneSnapshot: Decodable, Identifiable {
    let id: String
    let label: String
    let cwd: String
    let state: String
    let summary: String?
    let activityAt: UInt64?

    enum CodingKeys: String, CodingKey {
        case id
        case label
        case cwd
        case state
        case summary
        case activityAt = "activity_at_unix_ms"
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
    let ambient: CoreAmbientSignal?

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
        case ambient
    }
}

struct CoreAmbientSignal: Decodable, Equatable {
    let subagentsActive: UInt32
    let backgroundRunning: UInt32
    let backgroundFailed: UInt32

    enum CodingKeys: String, CodingKey {
        case subagentsActive = "subagents_active"
        case backgroundRunning = "background_running"
        case backgroundFailed = "background_failed"
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
    let focusedDeviceID: String?
    let focusedCheckoutID: String?
    let workspaceRegistrations: [CoreWorkspaceRegistration]
    let deviceRegistrations: [CoreDeviceRegistration]
    let accentHex: String
    let fontSize: Double
    let bypassWarnings: Bool

    enum CodingKeys: String, CodingKey {
        case expandedPaths = "expanded_paths"
        case selectedPath = "selected_path"
        case selectedPaneID = "selected_pane_id"
        case shortcutBindings = "shortcut_bindings"
        case focusedDeviceID = "focused_device_id"
        case focusedCheckoutID = "focused_checkout_id"
        case workspaceRegistrations = "workspace_registrations"
        case deviceRegistrations = "device_registrations"
        case accentHex = "accent_hex"
        case fontSize = "font_size"
        case bypassWarnings = "bypass_warnings"
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
        focusedDeviceID = try container.decodeIfPresent(String.self, forKey: .focusedDeviceID)
        focusedCheckoutID = try container.decodeIfPresent(String.self, forKey: .focusedCheckoutID)
        workspaceRegistrations = try container.decodeIfPresent(
            [CoreWorkspaceRegistration].self,
            forKey: .workspaceRegistrations
        ) ?? []
        deviceRegistrations = try container.decodeIfPresent(
            [CoreDeviceRegistration].self,
            forKey: .deviceRegistrations
        ) ?? []
        accentHex = try container.decodeIfPresent(String.self, forKey: .accentHex) ?? "#B9FF66"
        fontSize = try container.decodeIfPresent(Double.self, forKey: .fontSize) ?? 13
        bypassWarnings = try container.decodeIfPresent(Bool.self, forKey: .bypassWarnings) ?? false
    }
}

struct CoreWorkspaceRegistration: Decodable, Identifiable {
    let id: String
    let label: String
    let path: String
    let deviceID: String

    enum CodingKeys: String, CodingKey {
        case id
        case label
        case path
        case deviceID = "device_id"
    }
}

struct CoreDeviceRegistration: Decodable, Identifiable {
    let id: String
    let label: String
    let sshAlias: String?

    enum CodingKeys: String, CodingKey {
        case id
        case label
        case sshAlias = "ssh_alias"
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

private struct RuntimeStartupPreparation: Sendable {
    let selection: HerdrRuntimeSelection?
    let environment: [String: String]
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
    @Published private(set) var runtimeSelection: HerdrRuntimeSelection?

    let workspaceRoot: URL
    let isRemoteWorkspace: Bool

    nonisolated(unsafe) private var core: OpaquePointer?
    private var launchedHerdrServer: Process?
    private let statePath: String
    private let fixtureMode: Bool
    private var startupDiagnostic: String?
    private var runtimeInitializationStarted = false
    private var lastLoggedHerdrState: String?
    private var lastLoggedErrorKind: String?
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
        let initStarted = Date()
        HideLaunchTrace.mark("core_bridge.init.begin")
        isRemoteWorkspace = arguments.contains("--remote-workspace")
        workspaceRoot = LaunchArguments.value("--workspace-root", in: arguments)
            .map { URL(fileURLWithPath: $0, isDirectory: true) }
            ?? URL(fileURLWithPath: FileManager.default.currentDirectoryPath, isDirectory: true)
        let resolvedStatePath = LaunchArguments.value("--state-path", in: arguments)
            ?? (arguments.contains("--verification-ui-fixture")
                ? "/tmp/herdr-ide-verify-ui-state.json"
                : Self.defaultStatePath())
        // The verification fixture runs without any live herdr connection;
        // every other launch talks to the local herdr socket.
        let resolvedFixtureMode = arguments.contains("--verification-ui-fixture")
        statePath = resolvedStatePath
        fixtureMode = resolvedFixtureMode
        runtimeSelection = nil
        guard let created = Self.createCore(
            herdrBinaryPath: nil,
            fixtureMode: resolvedFixtureMode,
            statePath: resolvedStatePath
        ) else {
            bridgeError = "herdr_core_create returned null"
            HideLaunchTrace.mark(
                "core_bridge.init.failed",
                detail: "core_create_null",
                durationMilliseconds: Int(Date().timeIntervalSince(initStarted) * 1_000)
            )
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
        if !resolvedFixtureMode {
            startupDiagnostic = HideStartupDiagnostic.initializing
            bridgeError = startupDiagnostic
        }
        HideLaunchTrace.mark(
            "core_bridge.init.ready",
            detail: resolvedFixtureMode ? "fixture" : "initial_core_without_runtime",
            durationMilliseconds: Int(Date().timeIntervalSince(initStarted) * 1_000)
        )
    }

    /// Starts all external runtime discovery after the application has made
    /// its first window visible. Finder launches therefore cannot lose their
    /// first window to a slow shell or CLI subprocess.
    func startRuntimeInitialization() {
        guard !fixtureMode, !runtimeInitializationStarted else { return }
        runtimeInitializationStarted = true
        let bundlePath = Bundle.main.path(
            forResource: "herdr",
            ofType: nil,
            inDirectory: "herdr-runtime"
        )
        let socketPath = Self.defaultHerdrSocketPath()
        let startedAt = Date()
        HideLaunchTrace.mark("runtime_initialization.begin")
        Task { @MainActor [weak self] in
            let preparation = await Task.detached(priority: .userInitiated) {
                RuntimeStartupPreparation(
                    selection: HerdrRuntimeResolver.resolve(bundlePath: bundlePath),
                    environment: HideRuntimeEnvironment.childEnvironment()
                )
            }.value
            guard let self else { return }
            let detail = preparation.selection.map {
                "selected_\($0.source)_v\($0.version)"
            } ?? "no_runtime"
            HideLaunchTrace.mark(
                "runtime_initialization.resolved",
                detail: detail,
                durationMilliseconds: Int(Date().timeIntervalSince(startedAt) * 1_000)
            )

            let socketExists = FileManager.default.fileExists(atPath: socketPath)
            guard self.replaceCore(with: preparation.selection) else {
                HideLaunchTrace.mark("runtime_initialization.failed", detail: "core_replace_failed")
                return
            }
            guard preparation.selection != nil || socketExists else {
                self.setStartupDiagnostic(HideStartupDiagnostic.runtimeUnavailable)
                HideLaunchTrace.mark("runtime_initialization.failed", detail: "runtime_unavailable")
                return
            }

            self.setStartupDiagnostic(nil)
            switch HerdrRuntimeResolver.startServerIfNeeded(
                selection: preparation.selection,
                socketPath: socketPath,
                environment: preparation.environment
            ) {
            case .notNeeded:
                HideLaunchTrace.mark("runtime_initialization.server_not_needed")
            case .started(let process):
                self.launchedHerdrServer = process
                process.terminationHandler = { [weak self] process in
                    Task { @MainActor [weak self] in
                        guard let self, self.launchedHerdrServer != nil else { return }
                        self.launchedHerdrServer = nil
                        self.setStartupDiagnostic(
                            HideStartupDiagnostic.serverExited(status: process.terminationStatus)
                        )
                        HideLaunchTrace.mark(
                            "runtime_initialization.server_exited",
                            detail: "status_\(process.terminationStatus)"
                        )
                    }
                }
                HideLaunchTrace.mark("runtime_initialization.server_started")
            case .failed(let message):
                self.setStartupDiagnostic(message)
                HideLaunchTrace.mark("runtime_initialization.server_failed", detail: "launch_error")
            }
        }
    }

    private static func createCore(
        herdrBinaryPath: String?,
        fixtureMode: Bool,
        statePath: String
    ) -> OpaquePointer? {
        let options: [String: Any] = [
            "schema_version": coreSchemaVersion,
            "herdr_socket_path": fixtureMode ? NSNull() : Self.defaultHerdrSocketPath() as Any,
            "herdr_bin_path": herdrBinaryPath.map { $0 as Any } ?? NSNull(),
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
            return nil
        }
        return created
    }

    @discardableResult
    private func replaceCore(with selection: HerdrRuntimeSelection?) -> Bool {
        if let current = core {
            herdr_core_on_change(current, nil, nil)
            herdr_core_destroy(current)
        }
        core = nil
        guard let created = Self.createCore(
            herdrBinaryPath: selection?.path,
            fixtureMode: fixtureMode,
            statePath: statePath
        ) else {
            runtimeSelection = selection
            setStartupDiagnostic("Hide could not initialize its Herdr connection. Reopen the app to retry.")
            HideLaunchTrace.mark("core_bridge.replace.failed", detail: "core_create_null")
            return false
        }
        core = created
        herdr_core_on_change(
            created,
            coreChangeCallback,
            Unmanaged.passUnretained(self).toOpaque()
        )
        runtimeSelection = selection
        lastTerminalSequence = 0
        pendingTerminalBytes.removeAll()
        restoredPaneSelection = false
        startupDiagnostic = nil
        refreshSnapshot()
        HideLaunchTrace.mark(
            "core_bridge.replace.ready",
            detail: selection.map { "runtime_\($0.source)" } ?? "socket_only"
        )
        return true
    }

    private func setStartupDiagnostic(_ message: String?) {
        startupDiagnostic = message
        bridgeError = message
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

    var pet: CorePetSnapshot? { snapshot?.pet }

    /// The click the core resolves: it picks the oldest unseen pane and
    /// focuses it, or reports none so the shell only raises its window.
    func petClicked() {
        dispatch(kind: "pet_click", payload: [:])
    }

    func setPetVisible(_ visible: Bool) {
        dispatch(kind: "pet_set_visible", payload: ["visible": visible])
    }

    func togglePetVisible() {
        dispatch(kind: "pet_toggle_visible", payload: [:])
    }

    func movePet(to origin: CGPoint) {
        dispatch(kind: "pet_move", payload: ["x": origin.x, "y": origin.y])
    }

    func setPetDragging(_ dragging: Bool) {
        dispatch(kind: "pet_drag", payload: ["dragging": dragging])
    }

    func notePetActivity() {
        dispatch(kind: "pet_activity", payload: [:])
    }

    func updatePetShortcut(accelerator: String?, error: String?) {
        dispatch(kind: "pet_shortcut_update", payload: [
            "accelerator": accelerator.map { $0 as Any } ?? NSNull(),
            "error": error.map { $0 as Any } ?? NSNull(),
        ])
    }

    func focusCheckout(workspaceID: String, checkoutID: String) {
        dispatch(kind: "focus_checkout", payload: [
            "workspace_id": workspaceID,
            "checkout_id": checkoutID,
        ])
    }

    func focusTab(workspaceID: String, checkoutID: String, tabID: String) {
        dispatch(kind: "focus_tab", payload: [
            "workspace_id": workspaceID,
            "checkout_id": checkoutID,
            "tab_id": tabID,
        ])
    }

    func focusDevice(_ deviceID: String) {
        dispatch(kind: "focus_device", payload: ["device_id": deviceID])
    }

    func createWorkspace(path: URL, label: String, initializeGit: Bool) {
        dispatch(kind: "create_workspace", payload: [
            "path": path.path,
            "label": label,
            "initialize_git": initializeGit,
        ])
    }

    func removeWorkspace(_ workspaceID: String) {
        dispatch(kind: "remove_workspace", payload: ["workspace_id": workspaceID])
    }

    func registerDevice(id: String, label: String, sshAlias: String) {
        dispatch(kind: "register_device", payload: [
            "id": id,
            "label": label,
            "ssh_alias": sshAlias,
        ])
    }

    func removeDevice(_ deviceID: String) {
        dispatch(kind: "remove_device", payload: ["device_id": deviceID])
    }

    func testDevice(_ deviceID: String) {
        dispatch(kind: "test_device", payload: ["device_id": deviceID])
    }

    func createTab(workspaceID: String, checkoutID: String? = nil, label: String = "New tab") {
        dispatch(kind: "create_tab", payload: [
            "workspace_id": workspaceID,
            "checkout_id": checkoutID.map { $0 as Any } ?? NSNull(),
            "label": label,
        ])
    }

    func startAgent(agent: String, checkoutPath: String, bypassWarnings: Bool) {
        guard let runtimeSelection else {
            bridgeError = "The verified bundled Herdr runtime is not available for this launch."
            return
        }
        let paneID = paneID(for: checkoutPath)
        let herdrPath = runtimeSelection.path
        Task { @MainActor [weak self] in
            let result = await Task.detached {
                HerdrAgentLauncher.launch(
                    herdrPath: herdrPath,
                    agent: agent,
                    checkoutPath: checkoutPath,
                    paneID: paneID,
                    bypassWarnings: bypassWarnings
                )
            }.value
            guard let self else { return }
            bridgeError = result.message
        }
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
        shortcutBindings: [String: String]? = nil,
        accentHex: String? = nil,
        fontSize: Double? = nil,
        bypassWarnings: Bool? = nil
    ) {
        let current = snapshot?.uiState
        let effectivePath = selectedPath ?? current?.selectedPath
        let effectivePaneID = selectedPaneID ?? current?.selectedPaneID
        dispatch(kind: "ui_state_update", payload: [
            "expanded_paths": expandedPaths ?? current?.expandedPaths ?? [],
            "selected_path": effectivePath.map { $0 as Any } ?? NSNull(),
            "selected_pane_id": effectivePaneID.map { $0 as Any } ?? NSNull(),
            "shortcut_bindings": shortcutBindings ?? current?.shortcutBindings ?? [:],
            "focused_device_id": current?.focusedDeviceID.map { $0 as Any } ?? NSNull(),
            "focused_checkout_id": current?.focusedCheckoutID.map { $0 as Any } ?? NSNull(),
            "workspace_registrations": current?.workspaceRegistrations.map {
                [
                    "id": $0.id,
                    "label": $0.label,
                    "path": $0.path,
                    "device_id": $0.deviceID,
                ]
            } ?? [],
            "device_registrations": current?.deviceRegistrations.map {
                [
                    "id": $0.id,
                    "label": $0.label,
                    "ssh_alias": $0.sshAlias.map { $0 as Any } ?? NSNull(),
                ]
            } ?? [],
            "accent_hex": accentHex ?? current?.accentHex ?? "#B9FF66",
            "font_size": fontSize ?? current?.fontSize ?? 13,
            "bypass_warnings": bypassWarnings ?? current?.bypassWarnings ?? false,
        ])
    }

    func dispatch(kind: String, payload: [String: Any]) {
        guard let core else {
            bridgeError = "Hide is still starting. Try again when the Herdr status is available."
            return
        }
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

    private func paneID(for checkoutPath: String) -> String? {
        let normalizedCheckout = URL(fileURLWithPath: checkoutPath, isDirectory: true)
            .standardizedFileURL
            .path
            .trimmingCharacters(in: CharacterSet(charactersIn: "/"))
        return snapshot?.navigator.workspaces
            .flatMap { $0.checkouts }
            .first(where: { checkout in
                checkout.path.trimmingCharacters(in: CharacterSet(charactersIn: "/")) == normalizedCheckout
            })?
            .tabs
            .flatMap { $0.panes }
            .first?
            .id
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
            if decoded.status.herdr.state != lastLoggedHerdrState {
                lastLoggedHerdrState = decoded.status.herdr.state
                HideLaunchTrace.mark("herdr.status", detail: decoded.status.herdr.state)
            }
            if let lastError = decoded.status.lastError {
                if lastError.kind != lastLoggedErrorKind {
                    lastLoggedErrorKind = lastError.kind
                    HideLaunchTrace.mark("core.error", detail: lastError.kind)
                }
            } else {
                lastLoggedErrorKind = nil
            }
            let previousFocusedPaneID = snapshot?.paneLayout?.focusedPaneID
                ?? snapshot?.terminal.paneID
            snapshot = decoded
            bridgeError = decoded.status.lastError.map { "\($0.kind): \($0.message)" } ?? startupDiagnostic
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
        dispatch(kind: "create_workspace", payload: [
            "path": workspaceRoot.path,
            "label": "hide rebrand",
            "initialize_git": false,
        ])
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

    static func defaultStatePath() -> String {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)
            .first ?? URL(fileURLWithPath: NSHomeDirectory()).appendingPathComponent("Library/Application Support")
        return base.appendingPathComponent("hide/state.json").path
    }

    /// Compatibility entry point for existing callers. Runtime selection
    /// uses the live-socket/install/bundle chain; child tools use the
    /// login-shell PATH separately.
    static func resolveHerdrBinaryPath() -> String? {
        HerdrRuntimeResolver.resolve()?.path
    }
}
