import CHerdrCore
import Foundation

private let coreSchemaVersion = 2

private let coreChangeCallback: @convention(c) (UnsafeMutableRawPointer?) -> Void = { context in
    guard let context else { return }
    let bridge = Unmanaged<CoreBridge>.fromOpaque(context).takeUnretainedValue()
    bridge.receiveCoreChange()
}

/// Composed view state the shell renders. Assembled from delta responses:
/// rest sections replace wholesale when their revision moves, the editor and
/// the changes view each ride their own revision, and terminal chunks stream
/// past this value to the terminal views, so chunk-only updates leave it
/// untouched.
struct CoreSnapshot {
    let schemaVersion: UInt32
    let navigator: CoreNavigatorSnapshot
    let zoomed: String?
    let paneLayout: CorePaneLayoutSnapshot?
    let terminal: CoreTerminalSnapshot
    let editor: CoreEditorSnapshot
    let changes: CoreChangesSnapshot
    let uiState: CoreUIStateSnapshot
    let status: CoreStatusSnapshot
    let pet: CorePetSnapshot

    /// Rebuilds the snapshot with only the independently revisioned sections
    /// that arrived, so a delta carrying one of them leaves the rest alone.
    func replacing(
        editor: CoreEditorSnapshot?,
        changes: CoreChangesSnapshot?
    ) -> CoreSnapshot {
        CoreSnapshot(
            schemaVersion: schemaVersion,
            navigator: navigator,
            zoomed: zoomed,
            paneLayout: paneLayout,
            terminal: terminal,
            editor: editor ?? self.editor,
            changes: changes ?? self.changes,
            uiState: uiState,
            status: status,
            pet: pet
        )
    }
}

/// One response on the delta snapshot wire: `herdr_core_snapshot` called
/// with the bridge's revision and terminal-sequence cursors. `rest`,
/// `editor`, and `changes` are absent when the cursor already covers them.
struct CoreSnapshotDelta: Decodable {
    let schemaVersion: UInt32
    let revision: UInt64
    let rest: CoreRestSnapshot?
    let editor: CoreEditorSnapshot?
    let changes: CoreChangesSnapshot?
    let terminalSequence: UInt64
    let chunks: [CoreTerminalChunk]
    let chunksDropped: Bool

    enum CodingKeys: String, CodingKey {
        case schemaVersion = "schema_version"
        case revision
        case rest
        case editor
        case changes
        case terminalSequence = "terminal_sequence"
        case chunks
        case chunksDropped = "chunks_dropped"
    }
}

struct CoreRestSnapshot: Decodable {
    let navigator: CoreNavigatorSnapshot
    let zoomed: String?
    let paneLayout: CorePaneLayoutSnapshot?
    let terminal: CoreTerminalSnapshot
    let uiState: CoreUIStateSnapshot
    let status: CoreStatusSnapshot
    let pet: CorePetSnapshot

    enum CodingKeys: String, CodingKey {
        case navigator
        case zoomed
        case paneLayout = "pane_layout"
        case terminal
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
    let providerUsage: [CoreProviderUsageSnapshot]

    enum CodingKeys: String, CodingKey {
        case rootPath = "root_path"
        case focusedDeviceID = "focused_device_id"
        case focusedWorkspaceID = "focused_workspace_id"
        case focusedCheckoutID = "focused_checkout_id"
        case devices
        case workspaces
        case agents
        case providerUsage = "provider_usage"
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
        providerUsage = try container.decodeIfPresent(
            [CoreProviderUsageSnapshot].self,
            forKey: .providerUsage
        ) ?? []
    }
}

struct CoreProviderUsageSnapshot: Decodable, Identifiable {
    var id: String { provider }
    let provider: String
    let label: String
    let windowMinutes: UInt64
    let state: String
    let usedPercent: Double?
    let resetsAtUnixSeconds: UInt64?
    let message: String?
    let lastCheckedAtUnixMilliseconds: UInt64?

    enum CodingKeys: String, CodingKey {
        case provider
        case label
        case windowMinutes = "window_minutes"
        case state
        case usedPercent = "used_percent"
        case resetsAtUnixSeconds = "resets_at_unix_seconds"
        case message
        case lastCheckedAtUnixMilliseconds = "last_checked_at_unix_ms"
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

    init(
        id: String,
        label: String,
        path: String,
        remoteTargetID: String?,
        expanded: Bool,
        deviceID: String,
        repoName: String,
        isGit: Bool,
        defaultBranch: String?,
        registered: Bool,
        temporary: Bool,
        checkouts: [CoreCheckoutSnapshot]
    ) {
        self.id = id
        self.label = label
        self.path = path
        self.remoteTargetID = remoteTargetID
        self.expanded = expanded
        self.deviceID = deviceID
        self.repoName = repoName
        self.isGit = isGit
        self.defaultBranch = defaultBranch
        self.registered = registered
        self.temporary = temporary
        self.checkouts = checkouts
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

    init(
        id: String,
        workspaceID: String,
        label: String,
        path: String,
        branch: String?,
        isWorktree: Bool,
        exists: Bool,
        temporary: Bool,
        tabs: [CoreTabSnapshot]
    ) {
        self.id = id
        self.workspaceID = workspaceID
        self.label = label
        self.path = path
        self.branch = branch
        self.isWorktree = isWorktree
        self.exists = exists
        self.temporary = temporary
        self.tabs = tabs
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

    init(
        id: String?,
        workspaceID: String?,
        checkoutID: String?,
        label: String?,
        empty: Bool,
        panes: [CorePaneSnapshot]
    ) {
        self.id = id
        self.workspaceID = workspaceID
        self.checkoutID = checkoutID
        self.label = label
        self.empty = empty
        self.panes = panes
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
    /// The three names a pane can be shown by. The core ships all three and
    /// `PaneHeaderPresentation` picks; see its ladder for the order.
    let herdrLabel: String?
    let terminalTitle: String?
    let workspaceLabel: String?
    let cwd: String
    let state: String
    let summary: String?
    let activityAt: UInt64?

    enum CodingKeys: String, CodingKey {
        case id
        case herdrLabel = "herdr_label"
        case terminalTitle = "terminal_title"
        case workspaceLabel = "workspace_label"
        case cwd
        case state
        case summary
        case activityAt = "activity_at_unix_ms"
    }

    init(
        id: String,
        herdrLabel: String? = nil,
        terminalTitle: String? = nil,
        workspaceLabel: String? = nil,
        cwd: String,
        state: String,
        summary: String?,
        activityAt: UInt64?
    ) {
        self.id = id
        self.herdrLabel = herdrLabel
        self.terminalTitle = terminalTitle
        self.workspaceLabel = workspaceLabel
        self.cwd = cwd
        self.state = state
        self.summary = summary
        self.activityAt = activityAt
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
    let closed: Bool
    let exitCode: Int32?
    let panes: [CoreTerminalPaneSnapshot]

    enum CodingKeys: String, CodingKey {
        case paneID = "pane_id"
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
    let transportState: String
    let transportMessage: String?
    let transportGeneration: UInt64
    let transportAttempt: UInt64
    let transportExitCategory: String?
    let transportRetryDecision: String

    enum CodingKeys: String, CodingKey {
        case paneID = "pane_id"
        case closed
        case exitCode = "exit_code"
        case transportState = "transport_state"
        case transportMessage = "transport_message"
        case transportGeneration = "transport_generation"
        case transportAttempt = "transport_attempt"
        case transportExitCategory = "transport_exit_category"
        case transportRetryDecision = "transport_retry_decision"
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        paneID = try container.decode(String.self, forKey: .paneID)
        closed = try container.decodeIfPresent(Bool.self, forKey: .closed) ?? false
        exitCode = try container.decodeIfPresent(Int32.self, forKey: .exitCode)
        transportState = try container.decodeIfPresent(String.self, forKey: .transportState) ?? "idle"
        transportMessage = try container.decodeIfPresent(String.self, forKey: .transportMessage)
        transportGeneration = try container.decodeIfPresent(UInt64.self, forKey: .transportGeneration) ?? 0
        transportAttempt = try container.decodeIfPresent(UInt64.self, forKey: .transportAttempt) ?? 0
        transportExitCategory = try container.decodeIfPresent(String.self, forKey: .transportExitCategory)
        transportRetryDecision = try container.decodeIfPresent(
            String.self,
            forKey: .transportRetryDecision
        ) ?? "none"
    }
}

struct CoreEditorSnapshot: Decodable {
    let tabs: [CoreFileTabSnapshot]
    let activeTabID: String?
    let document: CoreEditorDocumentSnapshot?

    var path: String? { document?.path }
    var language: String? { document?.language }
    var contentsUTF8: String? { document?.contentsUTF8 }
    var openedModifiedAt: UInt64? { document?.openedModifiedAt }
    var dirty: Bool { document?.dirty ?? false }
    var readonlyReason: String? { document?.readonlyReason }
    var conflict: CoreEditorConflict? { document?.conflict }

    enum CodingKeys: String, CodingKey {
        case tabs
        case activeTabID = "active_tab_id"
        case document
    }
}

struct CoreFileTabSnapshot: Decodable, Identifiable, Equatable {
    let id: String
    let workspaceID: String
    let checkoutID: String
    let path: String
    let label: String
    let dirty: Bool

    enum CodingKeys: String, CodingKey {
        case id
        case workspaceID = "workspace_id"
        case checkoutID = "checkout_id"
        case path
        case label
        case dirty
    }
}

struct CoreEditorDocumentSnapshot: Decodable {
    let path: String
    let language: String?
    let contentsUTF8: String?
    let openedModifiedAt: UInt64?
    let dirty: Bool
    let readonlyReason: String?
    let conflict: CoreEditorConflict?

    enum CodingKeys: String, CodingKey {
        case path
        case language
        case contentsUTF8 = "contents_utf8"
        case openedModifiedAt = "opened_modified_at_unix_ms"
        case dirty
        case readonlyReason = "readonly_reason"
        case conflict
    }
}

struct CoreUIStateSnapshot: Decodable {
    let leftSidebarVisible: Bool
    let rightPanelVisible: Bool
    let rightPanelSection: RightPanelSection
    let expandedPaths: [String]
    let collapsedWorkspaceIDs: [String]
    let selectedPath: String?
    let selectedPaneID: String?
    let shortcutBindings: [String: String]
    let focusedDeviceID: String?
    let focusedCheckoutID: String?
    let workspaceRegistrations: [CoreWorkspaceRegistration]
    let deviceRegistrations: [CoreDeviceRegistration]
    let accentHex: String
    let fontSize: Double
    /// Per-pane content text scale. A pane the user has not zoomed is absent,
    /// so a lookup miss means the default rather than an error.
    let paneTextScales: [String: Double]

    enum CodingKeys: String, CodingKey {
        case leftSidebarVisible = "left_sidebar_visible"
        case rightPanelVisible = "right_panel_visible"
        case rightPanelSection = "right_panel_section"
        case expandedPaths = "expanded_paths"
        case collapsedWorkspaceIDs = "collapsed_workspace_ids"
        case selectedPath = "selected_path"
        case selectedPaneID = "selected_pane_id"
        case shortcutBindings = "shortcut_bindings"
        case focusedDeviceID = "focused_device_id"
        case focusedCheckoutID = "focused_checkout_id"
        case workspaceRegistrations = "workspace_registrations"
        case deviceRegistrations = "device_registrations"
        case accentHex = "accent_hex"
        case fontSize = "font_size"
        case paneTextScales = "pane_text_scales"
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        leftSidebarVisible = try container.decodeIfPresent(Bool.self, forKey: .leftSidebarVisible) ?? true
        rightPanelVisible = try container.decodeIfPresent(Bool.self, forKey: .rightPanelVisible) ?? true
        rightPanelSection = try container.decodeIfPresent(
            RightPanelSection.self,
            forKey: .rightPanelSection
        ) ?? .explorer
        expandedPaths = try container.decode([String].self, forKey: .expandedPaths)
        collapsedWorkspaceIDs = try container.decodeIfPresent(
            [String].self,
            forKey: .collapsedWorkspaceIDs
        ) ?? []
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
        paneTextScales = try container.decodeIfPresent(
            [String: Double].self,
            forKey: .paneTextScales
        ) ?? [:]
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

/// The right panel's two sections. The core owns which one is showing, so the
/// choice survives hiding and reopening the panel.
enum RightPanelSection: String, Decodable, CaseIterable, Identifiable {
    case explorer
    case changes

    var id: String { rawValue }

    var title: String {
        switch self {
        case .explorer: "Explorer"
        case .changes: "Changes"
        }
    }

    var systemImage: String {
        switch self {
        case .explorer: "doc.text.magnifyingglass"
        case .changes: "arrow.triangle.branch"
        }
    }
}

/// One checkout's Git working-tree state as the core read it.
struct CoreChangesSnapshot: Decodable {
    let rootPath: String?
    let entries: [CoreChangedFile]
    let selectedPath: String?
    let diff: CoreChangedFileDiff?
    /// Why there is nothing to list. An empty list with no reason means the
    /// checkout genuinely has no changes.
    let unavailableReason: String?

    enum CodingKeys: String, CodingKey {
        case rootPath = "root_path"
        case entries
        case selectedPath = "selected_path"
        case diff
        case unavailableReason = "unavailable_reason"
    }

    static let empty = CoreChangesSnapshot(
        rootPath: nil,
        entries: [],
        selectedPath: nil,
        diff: nil,
        unavailableReason: nil
    )
}

struct CoreChangedFile: Decodable, Identifiable, Equatable {
    let path: String
    let relativePath: String
    let status: CoreChangedFileStatus

    var id: String { path }

    enum CodingKeys: String, CodingKey {
        case path
        case relativePath = "relative_path"
        case status
    }
}

enum CoreChangedFileStatus: String, Decodable, Equatable {
    case modified
    case added
    case deleted
    case untracked

    /// The single letter the row shows, which is how Git itself names these.
    var badge: String {
        switch self {
        case .modified: "M"
        case .added: "A"
        case .deleted: "D"
        case .untracked: "U"
        }
    }
}

struct CoreChangedFileDiff: Decodable, Equatable {
    let path: String
    let text: String
    let truncatedReason: String?

    enum CodingKeys: String, CodingKey {
        case path
        case text
        case truncatedReason = "truncated_reason"
    }
}

struct CoreStatusSnapshot: Decodable {
    let herdr: CoreHerdrStatus
    let remote: [CoreRemoteStatus]
    let chromux: CoreChromuxStatus
    let environment: [CoreEnvironmentStatus]
    let diagnostics: [CoreDiagnostic]
    let lastError: CoreLastError?

    enum CodingKeys: String, CodingKey {
        case herdr
        case remote
        case chromux
        case environment
        case diagnostics
        case lastError = "last_error"
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        herdr = try container.decode(CoreHerdrStatus.self, forKey: .herdr)
        remote = try container.decodeIfPresent([CoreRemoteStatus].self, forKey: .remote) ?? []
        chromux = try container.decode(CoreChromuxStatus.self, forKey: .chromux)
        environment = try container.decodeIfPresent([CoreEnvironmentStatus].self, forKey: .environment) ?? []
        diagnostics = try container.decodeIfPresent([CoreDiagnostic].self, forKey: .diagnostics) ?? []
        lastError = try container.decodeIfPresent(CoreLastError.self, forKey: .lastError)
    }
}

struct CoreRemoteStatus: Decodable, Identifiable {
    var id: String { targetID }
    let targetID: String
    let state: String
    let message: String?
    let session: CoreRemoteSessionSnapshot?
    let files: CoreRemoteFileList

    enum CodingKeys: String, CodingKey {
        case targetID = "target_id"
        case state
        case message
        case session
        case files
    }
}

struct CoreRemoteFileList: Decodable {
    let rootPath: String?
    let state: String
    let entries: [CoreRemoteFileEntry]
    let message: String?
    let generation: UInt64

    enum CodingKeys: String, CodingKey {
        case rootPath = "root_path"
        case state
        case entries
        case message
        case generation
    }
}

struct CoreRemoteFileEntry: Decodable {
    let path: String
    let name: String
    let isDirectory: Bool
    let sizeBytes: UInt64

    enum CodingKeys: String, CodingKey {
        case path
        case name
        case isDirectory = "is_directory"
        case sizeBytes = "size_bytes"
    }
}

struct CoreRemoteSessionSnapshot: Decodable {
    let workspaces: [CoreWorkspaceSnapshot]
    let agents: [SidebarAgent]
    let activeTabIDs: [String: String]
    let focusedWorkspaceID: String?
    let focusedCheckoutID: String?
    let focusedTabID: String?
    let focusedPaneID: String?
    let paneLayouts: [RemotePaneLayoutSnapshot]

    enum CodingKeys: String, CodingKey {
        case workspaces
        case agents
        case activeTabIDs = "active_tab_ids"
        case focusedWorkspaceID = "focused_workspace_id"
        case focusedCheckoutID = "focused_checkout_id"
        case focusedTabID = "focused_tab_id"
        case focusedPaneID = "focused_pane_id"
        case paneLayouts = "pane_layouts"
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

/// Blocks only topology events whose Rust handlers still address the local
/// Herdr session directly. Terminal input, resize, and scroll are deliberately
/// absent: the core routes those through the target-scoped terminal session
/// selected by the pane ID for both local and remote panes.
struct CoreDispatchRoutingPolicy {
    private static let localTopologyEventKinds: Set<String> = [
        "reconnect_pane",
        "focus_pane",
        "focus_checkout",
        "focus_tab",
        "create_tab",
        "create_pane",
        "toggle_zoom",
        "close_pane",
    ]

    static func blocks(kind: String, whenDeviceIsRemote isRemote: Bool) -> Bool {
        isRemote && localTopologyEventKinds.contains(kind)
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
    private var lastLoggedProjection: String?
    private var lastLoggedSnapshotRevision: UInt64?
    private var lastTerminalSequence: UInt64 = 0
    private var haveRevision: UInt64 = 0
    private var pendingTerminalBytes: [String: [[UInt8]]] = [:]
    private var terminalRegistrations: [String: TerminalRegistration] = [:]
    private var restoredPaneSelection = false
    private var pendingFileSave: Task<Void, Never>?
    private var commandDevice = CommandDevice.local
    private var routingError: String?

    private struct CommandDevice {
        let id: String
        let label: String
        let isRemote: Bool

        static let local = CommandDevice(id: "local", label: "This Mac", isRemote: false)
    }

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
                "herdr_socket_path": "/Users/grab/.config/herdr/herdr.sock",
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

    /// Herdr owns the pane's history, so the wheel is forwarded to it rather
    /// than moving a local buffer that the rendered stream never fills.
    func scrollTerminal(paneID: String, direction: String, lines: Int) {
        dispatch(kind: "terminal_scroll", payload: [
            "pane_id": paneID,
            "direction": direction,
            "lines": lines,
        ])
    }

    func focusPane(_ paneID: String) {
        dispatch(kind: "focus_pane", payload: ["pane_id": paneID])
    }

    /// The core owns the ladder and its bounds, so the shell sends a direction
    /// rather than a computed size and reads the result back off the snapshot.
    func setPaneTextScale(paneID: String, direction: PaneTextScaleDirection) {
        dispatch(
            kind: "pane_text_scale",
            payload: ["pane_id": paneID, "direction": direction.rawValue]
        )
    }

    var pet: CorePetSnapshot? { snapshot?.pet }

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

    func reconnectPane(_ paneID: String) {
        dispatch(kind: "reconnect_pane", payload: ["pane_id": paneID])
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

    func listRemoteFiles(targetID: String, rootPath: String) {
        dispatch(kind: "remote_file_list", payload: [
            "target_id": targetID,
            "root_path": rootPath,
        ])
    }

    /// Synchronizes the shell's visible device selection with the local-core
    /// write boundary. This is intentionally synchronous so a shortcut pressed
    /// immediately after selecting a remote device cannot race a snapshot.
    func selectCommandDevice(id: String, label: String, isRemote: Bool) {
        commandDevice = CommandDevice(id: id, label: label, isRemote: isRemote)
        routingError = nil
        bridgeError = snapshot?.status.lastError.map { "\($0.kind): \($0.message)" }
            ?? startupDiagnostic
        HideLaunchTrace.mark(
            "core.command_device",
            detail: "id=\(id) remote=\(isRemote)"
        )
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

    func createTab(workspaceID: String, checkoutID: String? = nil, label: String = "Tab 1") {
        dispatch(kind: "create_tab", payload: [
            "workspace_id": workspaceID,
            "checkout_id": checkoutID.map { $0 as Any } ?? NSNull(),
            "label": label,
        ])
    }

    func closeTab(_ tabID: String, confirmed: Bool) {
        dispatch(kind: "close_tab", payload: [
            "tab_id": tabID,
            "confirmed": confirmed,
        ])
    }

    func startAgent(
        agent: String,
        checkoutPath: String,
        checkoutID: String,
        workspaceID: String?,
        bypassWarnings: Bool
    ) {
        guard let runtimeSelection else {
            bridgeError = "The verified bundled Herdr runtime is not available for this launch."
            return
        }
        let herdrPath = runtimeSelection.path
        HideLaunchTrace.mark(
            "agent.launch.requested",
            detail: "kind=\(agent) workspace_id=\(workspaceID ?? "new")"
        )
        Task { @MainActor [weak self] in
            let result = await Task.detached {
                HerdrAgentLauncher.launch(
                    herdrPath: herdrPath,
                    agent: agent,
                    checkoutPath: checkoutPath,
                    workspaceID: workspaceID,
                    bypassWarnings: bypassWarnings
                )
            }.value
            guard let self else { return }
            HideLaunchTrace.mark(
                result.succeeded ? "agent.launch.ready" : "agent.launch.failed",
                detail: result.message
            )
            if let paneID = result.paneID {
                // Match the terminal-launch contract: the CLI-created root
                // pane anchors this checkout until session sync publishes its
                // authoritative layout. Keep the tab visible even when agent
                // startup fails after Herdr has already created it.
                persistUIState(
                    selectedPaneID: paneID,
                    focusedCheckoutID: checkoutID
                )
            }
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

    /// Puts one pane's terminal view in front of the keyboard. The pane's own
    /// registration owns the view, so the focus request goes through it rather
    /// than through a second reference to the same view.
    func focusTerminal(paneID: String) {
        terminalRegistrations[paneID]?.focus()
    }

    func unregisterTerminal(paneID: String, registrationID: UUID) {
        guard terminalRegistrations[paneID]?.id == registrationID else { return }
        terminalRegistrations.removeValue(forKey: paneID)
    }

    func splitCurrentPane(direction: PaneSplitDirection, cwd: String) {
        guard let paneID = snapshot?.terminal.paneID else {
            bridgeError = "pane.no_current_pane: Select a terminal pane before splitting"
            return
        }
        guard !cwd.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            bridgeError = "pane.no_checkout_path: The selected checkout path is empty"
            return
        }
        dispatch(kind: "create_pane", payload: [
            "tab_id": paneID,
            "cwd": cwd,
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

    func resizePane(_ paneID: String, direction: PaneResizeDirection, amount: Double) {
        guard amount.isFinite, amount >= 0.001 else { return }
        dispatch(kind: "resize_pane", payload: [
            "pane_id": paneID,
            "direction": direction.rawValue,
            "amount": min(amount, 0.5),
        ])
    }

    func closePane(_ paneID: String, confirmed: Bool) {
        dispatch(kind: "close_pane", payload: [
            "pane_id": paneID,
            "confirmed": confirmed,
        ])
    }

    func focusRemotePane(targetID: String, paneID: String) {
        dispatchRemoteControl(
            targetID: targetID,
            action: "focus_pane",
            extra: ["pane_id": paneID]
        )
    }

    func splitRemotePane(targetID: String, paneID: String, direction: PaneSplitDirection) {
        dispatchRemoteControl(
            targetID: targetID,
            action: "split_pane",
            extra: [
                "pane_id": paneID,
                "direction": direction.rawValue,
            ]
        )
    }

    func toggleRemotePaneZoom(targetID: String, paneID: String) {
        dispatchRemoteControl(
            targetID: targetID,
            action: "toggle_pane_zoom",
            extra: ["pane_id": paneID]
        )
    }

    func closeRemotePane(targetID: String, paneID: String, confirmed: Bool) {
        dispatchRemoteControl(
            targetID: targetID,
            action: "close_pane",
            extra: [
                "pane_id": paneID,
                "confirmed": confirmed,
            ]
        )
    }

    func focusRemoteWorkspace(targetID: String, workspaceID: String) {
        dispatchRemoteControl(
            targetID: targetID,
            action: "focus_workspace",
            extra: ["workspace_id": workspaceID]
        )
    }

    func focusRemoteTab(targetID: String, tabID: String) {
        dispatchRemoteControl(
            targetID: targetID,
            action: "focus_tab",
            extra: ["tab_id": tabID]
        )
    }

    func createRemoteTab(
        targetID: String,
        workspaceID: String,
        cwd: String,
        label: String
    ) {
        dispatchRemoteControl(
            targetID: targetID,
            action: "create_tab",
            extra: [
                "workspace_id": workspaceID,
                "cwd": cwd,
                "label": label,
            ]
        )
    }

    func closeRemoteTab(targetID: String, tabID: String, confirmed: Bool) {
        dispatchRemoteControl(
            targetID: targetID,
            action: "close_tab",
            extra: [
                "tab_id": tabID,
                "confirmed": confirmed,
            ]
        )
    }

    private func dispatchRemoteControl(
        targetID: String,
        action: String,
        extra: [String: Any] = [:]
    ) {
        var payload: [String: Any] = [
            "target_id": targetID,
            "request_id": UUID().uuidString,
            "action": action,
        ]
        for (key, value) in extra {
            payload[key] = value
        }
        dispatch(kind: "remote_control", payload: payload)
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

    func openFile(_ url: URL, workspaceID: String, checkoutID: String) {
        dispatch(kind: "file_open", payload: [
            "path": url.path,
            "workspace_id": workspaceID,
            "checkout_id": checkoutID,
        ])
    }

    func focusFileTab(_ tabID: String) {
        dispatch(kind: "file_focus", payload: ["tab_id": tabID])
    }

    func closeFileTab(_ tabID: String) {
        if snapshot?.editor.activeTabID == tabID {
            flushPendingFileSave()
        }
        dispatch(kind: "file_close", payload: ["tab_id": tabID])
    }

    func updateDraft(_ contents: String) {
        dispatch(kind: "file_draft", payload: ["contents_utf8": contents])
    }

    func saveFile(_ contents: String) {
        guard let editor = snapshot?.editor,
              let tabID = editor.activeTabID,
              let path = editor.path
        else { return }
        dispatch(kind: "file_save", payload: [
            "tab_id": tabID,
            "path": path,
            "contents_utf8": contents,
            "expected_modified_at_unix_ms": editor.openedModifiedAt.map { NSNumber(value: $0) as Any } ?? NSNull(),
        ])
    }

    func scheduleFileSave(_ contents: String) {
        pendingFileSave?.cancel()
        pendingFileSave = Task { @MainActor [weak self] in
            do {
                try await Task.sleep(for: .milliseconds(450))
            } catch {
                return
            }
            guard !Task.isCancelled else { return }
            self?.saveFile(contents)
            self?.pendingFileSave = nil
        }
    }

    func flushPendingFileSave() {
        guard pendingFileSave != nil,
              let contents = snapshot?.editor.contentsUTF8,
              snapshot?.editor.dirty == true
        else { return }
        pendingFileSave?.cancel()
        pendingFileSave = nil
        saveFile(contents)
    }

    func resolveConflict(_ action: String) {
        dispatch(kind: "file_conflict", payload: ["action": action])
    }

    func persistUIState(
        leftSidebarVisible: Bool? = nil,
        rightPanelVisible: Bool? = nil,
        rightPanelSection: RightPanelSection? = nil,
        expandedPaths: [String]? = nil,
        collapsedWorkspaceIDs: [String]? = nil,
        selectedPath: String? = nil,
        selectedPaneID: String? = nil,
        focusedCheckoutID: String? = nil,
        shortcutBindings: [String: String]? = nil,
        accentHex: String? = nil,
        fontSize: Double? = nil
    ) {
        let current = snapshot?.uiState
        let effectivePath = selectedPath ?? current?.selectedPath
        let effectivePaneID = selectedPaneID ?? current?.selectedPaneID
        var payload: [String: Any] = [
            "left_sidebar_visible": leftSidebarVisible ?? current?.leftSidebarVisible ?? true,
            "right_panel_visible": rightPanelVisible ?? current?.rightPanelVisible ?? true,
            "right_panel_section": (rightPanelSection ?? current?.rightPanelSection ?? .explorer)
                .rawValue,
            "expanded_paths": expandedPaths ?? current?.expandedPaths ?? [],
            "collapsed_workspace_ids": collapsedWorkspaceIDs ?? current?.collapsedWorkspaceIDs ?? [],
            "selected_path": effectivePath.map { $0 as Any } ?? NSNull(),
            "selected_pane_id": effectivePaneID.map { $0 as Any } ?? NSNull(),
            "shortcut_bindings": shortcutBindings ?? current?.shortcutBindings ?? [:],
            "accent_hex": accentHex ?? current?.accentHex ?? "#B9FF66",
            "font_size": fontSize ?? current?.fontSize ?? 13,
        ]
        // Registration events own durable workspace/device lists. A generic
        // UI-state save must not replay a stale snapshot and erase the
        // session-derived temporary catalog.
        if let focusedCheckoutID {
            // This is the one explicit local-selection anchor used after a
            // terminal launcher returns. It never asks Herdr to change focus.
            payload["focused_checkout_id"] = focusedCheckoutID
        }
        if selectedPaneID != nil || focusedCheckoutID != nil {
            HideLaunchTrace.mark(
                "core.dispatch.anchor",
                detail: "selected_pane_id=\(effectivePaneID ?? "nil") focused_checkout_id=\(focusedCheckoutID ?? "preserve")"
            )
        }
        dispatch(kind: "ui_state_update", payload: payload)
    }

    /// Selects the changed file whose diff the changes view shows, or clears
    /// the selection when `path` is nil.
    func selectChangedFile(path: String?) {
        dispatch(kind: "changes_select", payload: ["path": path.map { $0 as Any } ?? NSNull()])
    }

    func dispatch(kind: String, payload: [String: Any]) {
        if CoreDispatchRoutingPolicy.blocks(
            kind: kind,
            whenDeviceIsRemote: commandDevice.isRemote
        ) {
            let message = "device.route_blocked: \(commandDevice.label) is selected. \(kind) was not sent to the local Herdr session."
            routingError = message
            bridgeError = message
            HideLaunchTrace.mark(
                "core.dispatch.blocked",
                detail: "kind=\(kind) device_id=\(commandDevice.id)"
            )
            return
        }
        guard let core else {
            bridgeError = "Hide is still starting. Try again when the Herdr status is available."
            return
        }
        if let detail = dispatchTraceDetail(kind: kind, payload: payload) {
            HideLaunchTrace.mark("core.dispatch", detail: detail)
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

    private func dispatchTraceDetail(kind: String, payload: [String: Any]) -> String? {
        switch kind {
        case "focus_checkout":
            return "kind=focus_checkout workspace_id=\(traceValue(payload, key: "workspace_id")) checkout_id=\(traceValue(payload, key: "checkout_id"))"
        case "ui_state_update":
            return "kind=ui_state_update selected_pane_id=\(traceValue(payload, key: "selected_pane_id")) focused_checkout_id=\(traceValue(payload, key: "focused_checkout_id")) workspace_registrations=\(payload["workspace_registrations"] == nil ? "omitted" : "present")"
        default:
            return nil
        }
    }

    private func traceValue(_ payload: [String: Any], key: String) -> String {
        guard let value = payload[key], !(value is NSNull) else { return "nil" }
        return String(describing: value)
    }

    func environmentState(for key: String) -> String? {
        snapshot?.status.environment.first(where: { $0.key == key })?.state
    }

    private func refreshSnapshot() {
        guard let core else { return }
        let owned = herdr_core_snapshot(core, haveRevision, lastTerminalSequence)
        defer { herdr_core_free_bytes(owned) }
        guard let pointer = owned.ptr, owned.len > 0 else {
            bridgeError = "herdr_core_snapshot returned empty bytes"
            return
        }
        do {
            let data = Data(bytes: pointer, count: owned.len)
            let decoded = try JSONDecoder().decode(CoreSnapshotDelta.self, from: data)
            if decoded.revision != lastLoggedSnapshotRevision {
                lastLoggedSnapshotRevision = decoded.revision
                HideLaunchTrace.mark(
                    "core.snapshot.refresh",
                    detail: "revision=\(decoded.revision) rest=\(decoded.rest != nil) editor=\(decoded.editor != nil) terminal_sequence=\(decoded.terminalSequence)"
                )
            }
            if decoded.chunksDropped {
                HideLaunchTrace.mark(
                    "terminal.chunks_dropped",
                    detail: "cursor \(lastTerminalSequence) behind ring"
                )
            }
            if let rest = decoded.rest {
                // The protocol stamps every section ahead of a fresh cursor,
                // so a missing editor here is a contract violation, not a
                // state to default over.
                guard let editor = decoded.editor ?? snapshot?.editor else {
                    bridgeError = "delta.protocol: first response carried no editor"
                    return
                }
                apply(composed: CoreSnapshot(
                    schemaVersion: decoded.schemaVersion,
                    navigator: rest.navigator,
                    zoomed: rest.zoomed,
                    paneLayout: rest.paneLayout,
                    terminal: rest.terminal,
                    editor: editor,
                    changes: decoded.changes ?? snapshot?.changes ?? .empty,
                    uiState: rest.uiState,
                    status: rest.status,
                    pet: rest.pet
                ))
            } else if decoded.editor != nil || decoded.changes != nil {
                guard let current = snapshot else {
                    bridgeError = "delta.protocol: a section arrived before the first full snapshot"
                    return
                }
                snapshot = current.replacing(
                    editor: decoded.editor,
                    changes: decoded.changes
                )
            }
            haveRevision = decoded.revision
            for chunk in decoded.chunks
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

    /// Replaces the composed snapshot and runs the side effects that watch
    /// it. Only rest-section changes reach here; chunk-only deltas never
    /// touch the published snapshot.
    private func apply(composed decoded: CoreSnapshot) {
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
        let focusedCheckout = decoded.navigator.focusedCheckoutID.flatMap { checkoutID in
            decoded.navigator.workspaces
                .lazy
                .flatMap(\.checkouts)
                .first(where: { $0.id == checkoutID })
        }
        let layoutBelongs = decoded.paneLayout.flatMap { layout in
            focusedCheckout.map { checkout in
                TerminalLayoutPolicy.belongs(layout: layout, to: checkout)
            }
        } ?? false
        let projection = [
            "focused_workspace_id=\(decoded.navigator.focusedWorkspaceID ?? "nil")",
            "focused_checkout_id=\(decoded.navigator.focusedCheckoutID ?? "nil")",
            "checkout_workspace_id=\(focusedCheckout?.workspaceID ?? "nil")",
            "layout_workspace_id=\(decoded.paneLayout?.workspaceID ?? "nil")",
            "layout_tab_id=\(decoded.paneLayout?.tabID ?? "nil")",
            "layout_pane_ids=\(decoded.paneLayout?.root.paneIDs.joined(separator: ",") ?? "nil")",
            "terminal_pane_id=\(decoded.terminal.paneID ?? "nil")",
            "layout_belongs=\(layoutBelongs)"
        ].joined(separator: " ")
        if projection != lastLoggedProjection {
            lastLoggedProjection = projection
            HideLaunchTrace.mark("core.snapshot.projection", detail: projection)
        }
        let previousFocusedPaneID = snapshot?.paneLayout?.focusedPaneID
            ?? snapshot?.terminal.paneID
        snapshot = decoded
        bridgeError = routingError
            ?? decoded.status.lastError.map { "\($0.kind): \($0.message)" }
            ?? startupDiagnostic
        restorePaneSelectionIfNeeded(decoded)
        let authoritativeFocusedPaneID = decoded.paneLayout?.focusedPaneID
            ?? decoded.terminal.paneID
        if authoritativeFocusedPaneID != previousFocusedPaneID,
           let authoritativeFocusedPaneID {
            DispatchQueue.main.async { [weak self] in
                self?.terminalRegistrations[authoritativeFocusedPaneID]?.focus()
            }
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
        let paneID = "fixture-working"
        let workspaceID = "fixture-workspace"
        let tabID = "fixture-tab"
        let agents: [[String: Any]] = [
            Self.fixtureAgent("error", "×", "00", "1755000007000", "Build failed", "12s", "Core", "codex", "status_error_new"),
            Self.fixtureAgent("question", "?", "01", "1755000006000", "Choose persistence scope", "4m", "UI", "claude", "status_question_new"),
            Self.fixtureAgent("approval", "!", "02", "1755000005000", "Approve local save", "8m", "Explorer", "codex", "status_approval_new"),
            Self.fixtureAgent("done", "●", "04", "1755000004000", "Sidebar contract complete", "2h", "Agents", "claude", "status_done_new"),
            Self.fixtureAgent("working", "●", "05", "1755000003000", "Connecting Rust bytes", "18s", "Terminal", "codex", "status_working"),
            Self.fixtureAgent("idle", "○", "10", "1755000002000", "Reviewed fixture", "3d", "Verify", "claude", "status_idle"),
            Self.fixtureAgent("unknown", "~", "10", "1755000001000", "Awaiting lifecycle token", "9m", "Other", "unknown", "status_unknown"),
        ]
        dispatch(kind: "create_workspace", payload: [
            "path": workspaceRoot.path,
            "label": "hide rebrand",
            "initialize_git": false,
        ])
        dispatch(kind: "session_snapshot", payload: [
            "focused_pane_id": paneID,
            "agents": agents,
            "workspaces": [["workspace_id": workspaceID, "label": "hide rebrand"]],
            "tabs": [["tab_id": tabID, "workspace_id": workspaceID, "label": "Round 3"]],
            "panes": [["pane_id": paneID, "cwd": workspaceRoot.path]],
            "layouts": [[
                "workspace_id": workspaceID,
                "tab_id": tabID,
                "zoomed": false,
                "area": ["x": 0, "y": 0, "width": 120, "height": 60],
                "focused_pane_id": paneID,
                "panes": [[
                    "pane_id": paneID,
                    "rect": ["x": 0, "y": 0, "width": 120, "height": 60],
                ]],
                "splits": [],
            ]],
        ])
        let banner = "\u{001B}[1;36mherdr-core ↔ SwiftTerm\u{001B}[0m\r\nLocal byte bridge ready. IME V9 remains blocked.\r\n\r\n"
        dispatch(kind: "terminal_output", payload: [
            "pane_id": paneID,
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

    /// The release bundle identifier. A build carrying any other identifier is
    /// a per-worktree instance (see `macos/scripts/build_dev_app.sh`).
    nonisolated static let releaseBundleIdentifier = "me.grab.hide"

    /// Where this instance keeps its persisted UI state.
    ///
    /// Two builds may run at once - one per worktree - and they must not
    /// overwrite each other's selected pane, expanded folders, panel
    /// visibility, and pet position. The bundle identifier is what already
    /// separates those instances to the system, so it separates their state
    /// too. The release identifier keeps the original path so an upgrade does
    /// not lose the state the user already has.
    nonisolated static func defaultStatePath(
        bundleIdentifier: String? = Bundle.main.bundleIdentifier
    ) -> String {
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)
            .first ?? URL(fileURLWithPath: NSHomeDirectory()).appendingPathComponent("Library/Application Support")
        let root = base.appendingPathComponent("hide")
        guard let identifier = bundleIdentifier, identifier != releaseBundleIdentifier else {
            return root.appendingPathComponent("state.json").path
        }
        return root
            .appendingPathComponent("instances")
            .appendingPathComponent(identifier)
            .appendingPathComponent("state.json")
            .path
    }

    /// Compatibility entry point for existing callers. Runtime selection
    /// uses the live-socket/install/bundle chain; child tools use the
    /// login-shell PATH separately.
    static func resolveHerdrBinaryPath() -> String? {
        HerdrRuntimeResolver.resolve()?.path
    }
}
