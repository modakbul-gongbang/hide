import Foundation

/// Composed view state the shell renders. Assembled from delta responses:
/// rest sections replace wholesale when their revision moves, the editor and
/// the changes view each ride their own revision, and terminal chunks stream
/// past this value to the terminal views, so chunk-only updates leave it
/// untouched.
struct CoreSnapshot {
    // Local projection cursor: content-only edits/find updates do not rebuild MRU.
    var navigationRevision: UInt64 = 0
    let schemaVersion: UInt32
    let navigator: CoreNavigatorSnapshot
    let zoomed: String?
    /// Every tab's layout in the local session, keyed by the tab id each one
    /// carries. The canvas draws the entry for the visible tab, so switching
    /// tabs is a lookup here instead of a wait for Herdr to send one.
    let paneLayouts: [CorePaneLayoutSnapshot]
    let terminal: CoreTerminalSnapshot
    let editor: CoreEditorSnapshot
    let changes: CoreChangesSnapshot
    let card: CoreCheckoutCard
    let find: CorePaneFindSnapshot
    let uiState: CoreUIStateSnapshot
    let status: CoreStatusSnapshot
    let pet: CorePetSnapshot
    let recentClosed: CoreRecentClosedSnapshot
    var gitWorktrees: CoreProjectWorktrees? = nil
    var gitWorktreesLoading: Bool = false
    var gitWorktreesRemote: Bool = false
    var worktreeRemoval: CoreWorktreeRemoval? = nil
    var taskOperation: CoreTaskOperation? = nil
    var explorerOperation: CoreExplorerOperation? = nil

    /// Rebuilds the snapshot with only the independently revisioned sections
    /// that arrived, so a delta carrying one of them leaves the rest alone.
    func replacing(
        editor: CoreEditorSnapshot?,
        changes: CoreChangesSnapshot?,
        find: CorePaneFindSnapshot
    ) -> CoreSnapshot {
        CoreSnapshot(
            navigationRevision: navigationRevision &+ ((editor.map { $0.tabs != self.editor.tabs || $0.activeTabID != self.editor.activeTabID } ?? false) ? 1 : 0),
            schemaVersion: schemaVersion,
            navigator: navigator,
            zoomed: zoomed,
            paneLayouts: paneLayouts,
            terminal: terminal,
            editor: editor ?? self.editor,
            changes: changes ?? self.changes,
            card: card,
            find: find,
            uiState: uiState,
            status: status,
            pet: pet,
            recentClosed: recentClosed,
            gitWorktrees: gitWorktrees, gitWorktreesLoading: gitWorktreesLoading,
            gitWorktreesRemote: gitWorktreesRemote, worktreeRemoval: worktreeRemoval,
            taskOperation: taskOperation, explorerOperation: explorerOperation
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
    /// Find state rides top-level because it changes on every keystroke while a
    /// search is open; in `rest` each keystroke would resend every other
    /// section with it.
    let find: CorePaneFindSnapshot
    let terminalSequence: UInt64
    let chunks: [CoreTerminalChunk]
    let chunksDropped: Bool

    enum CodingKeys: String, CodingKey {
        case schemaVersion = "schema_version"
        case revision
        case rest
        case editor
        case changes
        case find
        case terminalSequence = "terminal_sequence"
        case chunks
        case chunksDropped = "chunks_dropped"
    }
}

extension CoreSnapshot {
    /// The pane the keyboard belongs to, decided once for everyone who reads
    /// a snapshot.
    ///
    /// The core owns the focused pane, so its own field is the whole answer:
    /// a click has to move the ring on its own frame rather than on Herdr's
    /// confirming event. Callers used to reach for a layout's focused pane
    /// when this was nil, and the order drifted between them - the recent
    /// navigation list read the layout first and so pointed at the pane Herdr
    /// last confirmed instead of the pane just clicked. That stand-in never
    /// worked in any case: the layout it read was found by looking this same
    /// field up in each layout's pane list, so it was nil in exactly the case
    /// the fallback existed for. Reading a layout here is not the way to fill
    /// the gap; `ShellModel.focusedPaneLayout` resolves one by the focused
    /// tab, which does not defeat itself.
    var focusedPaneID: String? {
        terminal.paneID
    }
}

struct CoreRestSnapshot: Decodable {
    let navigator: CoreNavigatorSnapshot
    let card: CoreCheckoutCard
    let zoomed: String?
    let paneLayouts: [CorePaneLayoutSnapshot]
    let terminal: CoreTerminalSnapshot
    let uiState: CoreUIStateSnapshot
    let status: CoreStatusSnapshot
    let pet: CorePetSnapshot
    let recentClosed: CoreRecentClosedSnapshot
    var gitWorktrees: CoreProjectWorktrees? = nil
    var gitWorktreesLoading: Bool = false
    var gitWorktreesRemote: Bool = false
    var worktreeRemoval: CoreWorktreeRemoval? = nil
    var taskOperation: CoreTaskOperation? = nil
    var explorerOperation: CoreExplorerOperation? = nil

    enum CodingKeys: String, CodingKey {
        case navigator
        case card
        case zoomed
        case paneLayouts = "pane_layouts"
        case terminal
        case uiState = "ui_state"
        case status
        case pet
        case recentClosed = "recent_closed"
        case gitWorktrees = "git_worktrees"
        case gitWorktreesLoading = "git_worktrees_loading"
        case gitWorktreesRemote = "git_worktrees_remote"
        case worktreeRemoval = "worktree_removal"
        case taskOperation = "task_operation"
        case explorerOperation = "explorer_operation"
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        navigator = try container.decode(CoreNavigatorSnapshot.self, forKey: .navigator)
        card = try container.decodeIfPresent(CoreCheckoutCard.self, forKey: .card) ?? .empty
        zoomed = try container.decodeIfPresent(String.self, forKey: .zoomed)
        paneLayouts = try container.decode([CorePaneLayoutSnapshot].self, forKey: .paneLayouts)
        terminal = try container.decode(CoreTerminalSnapshot.self, forKey: .terminal)
        uiState = try container.decode(CoreUIStateSnapshot.self, forKey: .uiState)
        status = try container.decode(CoreStatusSnapshot.self, forKey: .status)
        pet = try container.decode(CorePetSnapshot.self, forKey: .pet)
        recentClosed = try container.decodeIfPresent(CoreRecentClosedSnapshot.self, forKey: .recentClosed) ?? .empty
        gitWorktrees = try container.decodeIfPresent(CoreProjectWorktrees.self, forKey: .gitWorktrees)
        gitWorktreesLoading = try container.decodeIfPresent(Bool.self, forKey: .gitWorktreesLoading) ?? false
        gitWorktreesRemote = try container.decodeIfPresent(Bool.self, forKey: .gitWorktreesRemote) ?? false
        worktreeRemoval = try container.decodeIfPresent(CoreWorktreeRemoval.self, forKey: .worktreeRemoval)
        taskOperation = try container.decodeIfPresent(CoreTaskOperation.self, forKey: .taskOperation)
        explorerOperation = try container.decodeIfPresent(CoreExplorerOperation.self, forKey: .explorerOperation)
    }
}

struct CoreRecentClosedSnapshot: Decodable, Equatable {
    let count: Int
    let topLabel: String?
    let restoring: Bool
    let notices: [CoreRecentClosedNotice]
    let pending: [CoreRecentClosedPending]
    let canReopen: Bool
    let reopenBlockedReason: String?

    static let empty = CoreRecentClosedSnapshot(
        count: 0,
        topLabel: nil,
        restoring: false,
        notices: [],
        pending: [],
        canReopen: false,
        reopenBlockedReason: nil
    )

    init(
        count: Int,
        topLabel: String?,
        restoring: Bool,
        notices: [CoreRecentClosedNotice],
        pending: [CoreRecentClosedPending],
        canReopen: Bool,
        reopenBlockedReason: String?
    ) {
        self.count = count
        self.topLabel = topLabel
        self.restoring = restoring
        self.notices = notices
        self.pending = pending
        self.canReopen = canReopen
        self.reopenBlockedReason = reopenBlockedReason
    }

    enum CodingKeys: String, CodingKey {
        case count
        case topLabel = "top_label"
        case restoring
        case notices
        case pending
        case canReopen = "can_reopen"
        case reopenBlockedReason = "reopen_blocked_reason"
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        count = try container.decodeIfPresent(Int.self, forKey: .count) ?? 0
        topLabel = try container.decodeIfPresent(String.self, forKey: .topLabel)
        restoring = try container.decodeIfPresent(Bool.self, forKey: .restoring) ?? false
        notices = try container.decodeIfPresent([CoreRecentClosedNotice].self, forKey: .notices) ?? []
        pending = try container.decodeIfPresent([CoreRecentClosedPending].self, forKey: .pending) ?? []
        canReopen = try container.decodeIfPresent(Bool.self, forKey: .canReopen) ?? (count > 0 && !restoring && pending.isEmpty)
        reopenBlockedReason = try container.decodeIfPresent(String.self, forKey: .reopenBlockedReason)
    }
}

struct CoreRecentClosedPending: Decodable, Equatable {
    let key: String
    let targetID: String
    let label: String
    let phase: String
    let checking: Bool
    let message: String?
    let retryable: Bool

    enum CodingKeys: String, CodingKey {
        case key
        case targetID = "target_id"
        case label
        case phase
        case checking
        case message
        case retryable
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        key = try container.decode(String.self, forKey: .key)
        targetID = try container.decode(String.self, forKey: .targetID)
        label = try container.decode(String.self, forKey: .label)
        phase = try container.decode(String.self, forKey: .phase)
        checking = try container.decodeIfPresent(Bool.self, forKey: .checking) ?? false
        message = try container.decodeIfPresent(String.self, forKey: .message)
        retryable = try container.decodeIfPresent(Bool.self, forKey: .retryable) ?? false
    }
}

struct CoreRecentClosedNotice: Decodable, Equatable {
    let paneID: String?
    let message: String

    enum CodingKeys: String, CodingKey {
        case paneID = "pane_id"
        case message
    }
}

/// What the core's search of a pane's whole scrollback found.
///
/// The terminal view can only see the rows it is drawing, so the counter it
/// shows comes from here rather than from its own buffer.
struct CorePaneFindSnapshot: Decodable, Equatable {
    let paneID: String?
    let term: String
    let index: Int
    let total: Int
    let truncated: Bool
    let unavailableReason: String?

    static let empty = CorePaneFindSnapshot(
        paneID: nil,
        term: "",
        index: 0,
        total: 0,
        truncated: false,
        unavailableReason: nil
    )

    enum CodingKeys: String, CodingKey {
        case paneID = "pane_id"
        case term
        case index
        case total
        case truncated
        case unavailableReason = "unavailable_reason"
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

/// The four groups the sidebar draws, counted by the core. `needsYou` is the
/// pet's act-now number and `done` is what finished unseen, so a badge can
/// never disagree with the section it stands for.
struct CorePetBadges: Decodable, Equatable {
    let needsYou: Int
    let done: Int
    let working: Int
    let seen: Int
    let disconnected: Int
    let subagentsActive: UInt32

    enum CodingKeys: String, CodingKey {
        case needsYou = "needs_you"
        case done
        case working
        case seen
        case disconnected
        case subagentsActive = "subagents_active"
    }

    static let none = CorePetBadges(
        needsYou: 0,
        done: 0,
        working: 0,
        seen: 0,
        disconnected: 0,
        subagentsActive: 0
    )
}

struct CorePetOrigin: Decodable, Equatable {
    let x: Double
    let y: Double

    var point: CGPoint { CGPoint(x: x, y: y) }
}

struct CoreUIStateSnapshot: Decodable {
    let leftSidebarVisible: Bool
    let rightPanelVisible: Bool
    let rightPanelSection: RightPanelSection
    let expandedPaths: [String]
    let collapsedWorkspaceIDs: [String]
    let collapsedCheckoutIDs: [String]
    let expandedInactiveCheckoutProjectPaths: [String]
    let expandedInactiveProjectDeviceIDs: [String]
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
    let conversationPaneIDs: [String]
    /// The file editor's own zoom. The editor is one surface rather than one
    /// per document, and it is not a pane, so it carries a scale of its own
    /// instead of a row in the pane-keyed map.
    let editorTextScale: Double

    var expandedAgentPaneIDs: [String] = []
    var projectBaseBranches: [String: String] = [:]

    enum CodingKeys: String, CodingKey {
        case expandedAgentPaneIDs = "expanded_agent_pane_ids"
        case projectBaseBranches = "project_base_branches"
        case leftSidebarVisible = "left_sidebar_visible"
        case rightPanelVisible = "right_panel_visible"
        case rightPanelSection = "right_panel_section"
        case expandedPaths = "expanded_paths"
        case collapsedWorkspaceIDs = "collapsed_workspace_ids"
        case collapsedCheckoutIDs = "collapsed_checkout_ids"
        case expandedInactiveCheckoutProjectPaths = "expanded_inactive_checkout_project_paths"
        case expandedInactiveProjectDeviceIDs = "expanded_inactive_project_device_ids"
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
        case editorTextScale = "editor_text_scale"
        case conversationPaneIDs = "conversation_pane_ids"
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        expandedAgentPaneIDs = try container.decodeIfPresent([String].self, forKey: .expandedAgentPaneIDs) ?? []
        projectBaseBranches = try container.decodeIfPresent([String: String].self, forKey: .projectBaseBranches) ?? [:]
        leftSidebarVisible = try container.decodeIfPresent(Bool.self, forKey: .leftSidebarVisible) ?? true
        rightPanelVisible = try container.decodeIfPresent(Bool.self, forKey: .rightPanelVisible) ?? true
        rightPanelSection = try container.decodeIfPresent(
            RightPanelSection.self,
            forKey: .rightPanelSection
        ) ?? .overview
        expandedPaths = try container.decode([String].self, forKey: .expandedPaths)
        collapsedWorkspaceIDs = try container.decodeIfPresent(
            [String].self,
            forKey: .collapsedWorkspaceIDs
        ) ?? []
        collapsedCheckoutIDs = try container.decodeIfPresent(
            [String].self,
            forKey: .collapsedCheckoutIDs
        ) ?? []
        expandedInactiveCheckoutProjectPaths = try container.decodeIfPresent(
            [String].self,
            forKey: .expandedInactiveCheckoutProjectPaths
        ) ?? []
        expandedInactiveProjectDeviceIDs = try container.decodeIfPresent(
            [String].self,
            forKey: .expandedInactiveProjectDeviceIDs
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
        editorTextScale = try container.decodeIfPresent(Double.self, forKey: .editorTextScale) ?? 1
        conversationPaneIDs = try container.decodeIfPresent([String].self, forKey: .conversationPaneIDs) ?? []
    }
}

struct CoreWorkspaceRegistration: Decodable, Identifiable {
    let id: String
    let label: String
    let path: String
    let deviceID: String
    let pinned: Bool

    enum CodingKeys: String, CodingKey {
        case id
        case label
        case path
        case deviceID = "device_id"
        case pinned
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(String.self, forKey: .id)
        label = try container.decode(String.self, forKey: .label)
        path = try container.decode(String.self, forKey: .path)
        deviceID = try container.decode(String.self, forKey: .deviceID)
        pinned = try container.decodeIfPresent(Bool.self, forKey: .pinned) ?? false
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

/// The right panel's three sections. The core owns which one is showing, so the
/// choice survives hiding and reopening the panel.
enum RightPanelSection: String, Decodable, CaseIterable, Identifiable {
    case overview
    case explorer
    case changes

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        let value = try container.decode(String.self)
        // A saved retired tab opens the existing project Overview.
        if value == "git" { self = .overview; return }
        guard let section = Self(rawValue: value) else {
            throw DecodingError.dataCorruptedError(in: container, debugDescription: "Unknown right panel section: \(value)")
        }
        self = section
    }

    var id: String { rawValue }

    var title: String {
        switch self {
        case .overview: "Overview"
        case .explorer: "Explorer"
        // The section keeps its `changes` identity for the saved state; the
        // panel calls it History (right-panel-overview D-16, PR 2 fills it).
        case .changes: "History"
        }
    }

    var systemImage: String {
        switch self {
        case .overview: "info.circle"
        case .explorer: "doc.text.magnifyingglass"
        case .changes: "arrow.triangle.branch"
        }
    }
}
