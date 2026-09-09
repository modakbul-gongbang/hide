import AppKit
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
    var gitWorktrees: CoreProjectWorktrees? = nil
    var gitWorktreesLoading: Bool = false
    var gitWorktreesRemote: Bool = false
    var worktreeRemoval: CoreWorktreeRemoval? = nil
    var taskOperation: CoreTaskOperation? = nil

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
            gitWorktrees: gitWorktrees, gitWorktreesLoading: gitWorktreesLoading,
            gitWorktreesRemote: gitWorktreesRemote, worktreeRemoval: worktreeRemoval,
            taskOperation: taskOperation
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
    var gitWorktrees: CoreProjectWorktrees? = nil
    var gitWorktreesLoading: Bool = false
    var gitWorktreesRemote: Bool = false
    var worktreeRemoval: CoreWorktreeRemoval? = nil
    var taskOperation: CoreTaskOperation? = nil

    enum CodingKeys: String, CodingKey {
        case navigator
        case card
        case zoomed
        case paneLayouts = "pane_layouts"
        case terminal
        case uiState = "ui_state"
        case status
        case pet
        case gitWorktrees = "git_worktrees"
        case gitWorktreesLoading = "git_worktrees_loading"
        case gitWorktreesRemote = "git_worktrees_remote"
        case worktreeRemoval = "worktree_removal"
        case taskOperation = "task_operation"
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
        gitWorktrees = try container.decodeIfPresent(CoreProjectWorktrees.self, forKey: .gitWorktrees)
        gitWorktreesLoading = try container.decodeIfPresent(Bool.self, forKey: .gitWorktreesLoading) ?? false
        gitWorktreesRemote = try container.decodeIfPresent(Bool.self, forKey: .gitWorktreesRemote) ?? false
        worktreeRemoval = try container.decodeIfPresent(CoreWorktreeRemoval.self, forKey: .worktreeRemoval)
        taskOperation = try container.decodeIfPresent(CoreTaskOperation.self, forKey: .taskOperation)
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
    let backgroundRunning: UInt32
    let backgroundFailed: UInt32

    enum CodingKeys: String, CodingKey {
        case needsYou = "needs_you"
        case done
        case working
        case seen
        case disconnected
        case subagentsActive = "subagents_active"
        case backgroundRunning = "background_running"
        case backgroundFailed = "background_failed"
    }

    static let none = CorePetBadges(
        needsYou: 0,
        done: 0,
        working: 0,
        seen: 0,
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
    /// The one space that is not a project.
    let scratch: CoreScratchSnapshot

    enum CodingKeys: String, CodingKey {
        case scratch
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
        scratch = try container.decodeIfPresent(CoreScratchSnapshot.self, forKey: .scratch)
            ?? CoreScratchSnapshot.empty
    }
}

/// The Scratch node as the sidebar draws it: one folder, and the tabs in it.
struct CoreScratchSnapshot: Decodable {
    let id: String
    let label: String
    let path: String
    let expanded: Bool
    let sessionWorkspaceIDs: [String]
    let tabs: [CoreScratchTabSnapshot]

    static let empty = CoreScratchSnapshot(
        id: "scratch",
        label: "Scratch",
        path: "",
        expanded: false,
        sessionWorkspaceIDs: [],
        tabs: []
    )

    enum CodingKeys: String, CodingKey {
        case id
        case label
        case path
        case expanded
        case sessionWorkspaceIDs = "session_workspace_ids"
        case tabs
    }

    init(
        id: String,
        label: String,
        path: String,
        expanded: Bool,
        sessionWorkspaceIDs: [String],
        tabs: [CoreScratchTabSnapshot]
    ) {
        self.id = id
        self.label = label
        self.path = path
        self.expanded = expanded
        self.sessionWorkspaceIDs = sessionWorkspaceIDs
        self.tabs = tabs
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decodeIfPresent(String.self, forKey: .id) ?? "scratch"
        label = try container.decodeIfPresent(String.self, forKey: .label) ?? "Scratch"
        path = try container.decodeIfPresent(String.self, forKey: .path) ?? ""
        expanded = try container.decodeIfPresent(Bool.self, forKey: .expanded) ?? false
        sessionWorkspaceIDs = try container.decodeIfPresent(
            [String].self,
            forKey: .sessionWorkspaceIDs
        ) ?? []
        tabs = try container.decodeIfPresent([CoreScratchTabSnapshot].self, forKey: .tabs) ?? []
    }
}

/// One Scratch row.
struct CoreScratchTabSnapshot: Decodable, Identifiable {
    let id: String
    let label: String
    /// The chat's title, when the composer wrote one onto its pane.
    let title: String?
    let panes: [CorePaneSnapshot]

    /// What the row says: the title when there is one, the tab label when
    /// there is not. Derived once here so no view has to decide it again.
    var displayName: String { title ?? label }
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
    let branches: [String]
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
        case branches
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
        branches: [String] = [],
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
        self.branches = branches
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
        branches = try container.decodeIfPresent([String].self, forKey: .branches) ?? []
        registered = try container.decodeIfPresent(Bool.self, forKey: .registered) ?? true
        temporary = try container.decodeIfPresent(Bool.self, forKey: .temporary) ?? false
        checkouts = try container.decodeIfPresent([CoreCheckoutSnapshot].self, forKey: .checkouts) ?? []
    }
}

struct CoreCheckoutAgentSummary: Decodable, Equatable {
    var representativePaneID: String? = nil
    var needsYou = 0
    var done = 0
    var working = 0
    var seen = 0
    var unknown = 0
    var total: Int { needsYou + done + working + seen }

    enum CodingKeys: String, CodingKey {
        case representativePaneID = "representative_pane_id"
        case needsYou = "needs_you"
        case done, working, seen, unknown
    }
}

struct CoreCheckoutSnapshot: Decodable, Identifiable {
    var github: CoreGithubStatus = .empty
    var agentSummary = CoreCheckoutAgentSummary()
    let id: String
    let workspaceID: String
    let label: String
    let path: String
    let branch: String?
    var worktree: CoreGitWorktree? = nil
    let isWorktree: Bool
    let exists: Bool
    let temporary: Bool
    /// Whether Herdr has a pane here. A worktree is a row because git lists
    /// it, so this is what tells the ones with no terminal apart.
    let hasPanes: Bool
    let dirty: Bool
    let changedFileCount: Int
    let baseBranch: String?
    let ahead: Int
    let behind: Int
    let addedLines: Int
    let removedLines: Int
    let unpushed: CoreUnpushed?
    let pullRequest: CorePullRequest?
    let tabs: [CoreTabSnapshot]
    /// The one ordered tab strip the core owns for this checkout. The shell
    /// draws it in this order and never composes an order of its own.
    let strip: [CoreStripTabSnapshot]
    /// The tab Herdr reports as active here. `nil` means no tab is active in
    /// this checkout, which the shell shows as such; it never promotes the
    /// first tab in its place.
    let activeTabID: String?
    /// The label the next Herdr tab created here should carry. The core
    /// decides it, next to the code that formats every other tab's label, so
    /// the shell never reads a number back out of a label it was given to draw.
    let nextTabLabel: String

    enum CodingKeys: String, CodingKey {
        case github
        case agentSummary = "agent_summary"
        case id
        case workspaceID = "workspace_id"
        case label
        case path
        case branch
        case worktree
        case isWorktree = "is_worktree"
        case exists
        case temporary
        case hasPanes = "has_panes"
        case dirty
        case changedFileCount = "changed_file_count"
        case baseBranch = "base_branch"
        case ahead
        case behind
        case addedLines = "added_lines"
        case removedLines = "removed_lines"
        case unpushed
        case pullRequest = "pull_request"
        case tabs
        case strip
        case activeTabID = "active_tab_id"
        case nextTabLabel = "next_tab_label"
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
        hasPanes: Bool = false,
        dirty: Bool = false,
        changedFileCount: Int = 0,
        baseBranch: String? = nil,
        ahead: Int = 0,
        behind: Int = 0,
        addedLines: Int = 0,
        removedLines: Int = 0,
        unpushed: CoreUnpushed? = nil,
        pullRequest: CorePullRequest? = nil,
        tabs: [CoreTabSnapshot],
        strip: [CoreStripTabSnapshot] = [],
        activeTabID: String? = nil,
        nextTabLabel: String = "Tab 1"
    ) {
        self.id = id
        self.workspaceID = workspaceID
        self.label = label
        self.path = path
        self.branch = branch
        self.isWorktree = isWorktree
        self.exists = exists
        self.temporary = temporary
        self.hasPanes = hasPanes
        self.dirty = dirty
        self.changedFileCount = changedFileCount
        self.baseBranch = baseBranch
        self.ahead = ahead
        self.behind = behind
        self.addedLines = addedLines
        self.removedLines = removedLines
        self.unpushed = unpushed
        self.pullRequest = pullRequest
        self.tabs = tabs
        self.strip = strip
        self.activeTabID = activeTabID
        self.nextTabLabel = nextTabLabel
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        github = try container.decodeIfPresent(CoreGithubStatus.self, forKey: .github) ?? .empty
        agentSummary = try container.decode(CoreCheckoutAgentSummary.self, forKey: .agentSummary)
        id = try container.decode(String.self, forKey: .id)
        workspaceID = try container.decode(String.self, forKey: .workspaceID)
        label = try container.decode(String.self, forKey: .label)
        path = try container.decode(String.self, forKey: .path)
        branch = try container.decodeIfPresent(String.self, forKey: .branch)
        worktree = try container.decodeIfPresent(CoreGitWorktree.self, forKey: .worktree)
        isWorktree = try container.decodeIfPresent(Bool.self, forKey: .isWorktree) ?? false
        exists = try container.decodeIfPresent(Bool.self, forKey: .exists) ?? true
        temporary = try container.decodeIfPresent(Bool.self, forKey: .temporary) ?? false
        hasPanes = try container.decodeIfPresent(Bool.self, forKey: .hasPanes) ?? false
        dirty = try container.decodeIfPresent(Bool.self, forKey: .dirty) ?? false
        changedFileCount = try container.decodeIfPresent(Int.self, forKey: .changedFileCount) ?? 0
        baseBranch = try container.decodeIfPresent(String.self, forKey: .baseBranch)
        ahead = try container.decodeIfPresent(Int.self, forKey: .ahead) ?? 0
        behind = try container.decodeIfPresent(Int.self, forKey: .behind) ?? 0
        addedLines = try container.decodeIfPresent(Int.self, forKey: .addedLines) ?? 0
        removedLines = try container.decodeIfPresent(Int.self, forKey: .removedLines) ?? 0
        unpushed = try container.decodeIfPresent(CoreUnpushed.self, forKey: .unpushed)
        pullRequest = try container.decodeIfPresent(CorePullRequest.self, forKey: .pullRequest)
        tabs = try container.decodeIfPresent([CoreTabSnapshot].self, forKey: .tabs) ?? []
        strip = try container.decode([CoreStripTabSnapshot].self, forKey: .strip)
        activeTabID = try container.decodeIfPresent(String.self, forKey: .activeTabID)
        nextTabLabel = try container.decode(String.self, forKey: .nextTabLabel)
    }
}

/// One entry in the core's tab strip. It names what to draw and what it stands
/// for; the panes, the dirty mark, and the active mark come from the snapshot
/// the entry points at.
struct CoreStripTabSnapshot: Decodable, Identifiable, Equatable {
    enum Kind: String, Decodable {
        case herdr
        case file
        case diff
    }

    let id: String
    let kind: Kind
    let sourceID: String
    let label: String

    enum CodingKeys: String, CodingKey {
        case id
        case kind
        case sourceID = "source_id"
        case label
    }

    init(id: String, kind: Kind, sourceID: String, label: String) {
        self.id = id
        self.kind = kind
        self.sourceID = sourceID
        self.label = label
    }
}

/// How far a branch is from the remote it tracks. Absent when it tracks none,
/// because "nothing to push" and "nowhere to push to" are different facts.
struct CoreUnpushed: Decodable, Equatable {
    let remote: String
    let count: Int
}

/// The five values a branch's pull request reduces to. The core owns the
/// mapping from `gh`'s state, review decision, and draft flag; nothing here
/// re-derives it.
enum CorePullRequestBadge: String, Decodable, Equatable {
    case merged
    case closed
    case review
    case open

    /// Settled pull requests dim their checkout row; deletion uses its own core gate.
    var isSettled: Bool { self == .merged || self == .closed }
}

enum CoreReviewDecision: String, Decodable, Equatable {
    case reviewRequired = "review_required"
    case changesRequested = "changes_requested"
    case approved
}

enum CorePullRequestChecks: String, Decodable, Equatable {
    case unknown, none, pending, failed, passing
}

struct CorePullRequest: Decodable, Equatable {
    var title: String? = nil
    var checks: CorePullRequestChecks? = nil
    let number: Int
    let headBranch: String
    let baseBranch: String
    let url: String
    let badge: CorePullRequestBadge
    /// Present only for a `review` badge, and only to pick its colour.
    let review: CoreReviewDecision?
    let isDraft: Bool
    let mergedAtUnixMS: Double?
    let updatedAtUnixMS: Double?

    enum CodingKeys: String, CodingKey {
        case title, checks
        case number
        case headBranch = "head_branch"
        case baseBranch = "base_branch"
        case url
        case badge
        case review
        case isDraft = "is_draft"
        case mergedAtUnixMS = "merged_at_unix_ms"
        case updatedAtUnixMS = "updated_at_unix_ms"
    }
}

/// How a repository's `gh` lookup is doing, independent of what it found.
struct CoreGithubStatus: Decodable, Equatable {
    let available: Bool
    let loading: Bool
    let stale: Bool
    let lastSuccessAtUnixMS: Double?
    /// The card's one allowed sentence: gh missing, logged out, or the exact
    /// failure.
    let unavailableReason: String?

    var failureCategory: String? = nil

    static let empty = CoreGithubStatus(
        available: false,
        loading: false,
        stale: false,
        lastSuccessAtUnixMS: nil,
        unavailableReason: nil
    )

    enum CodingKeys: String, CodingKey {
        case failureCategory = "failure_category"
        case available
        case loading
        case stale
        case lastSuccessAtUnixMS = "last_success_at_unix_ms"
        case unavailableReason = "unavailable_reason"
    }
}

struct CoreDiskUsage: Decodable, Equatable {
    let path: String?
    let totalBytes: Double?
    let largestChildName: String?
    let largestChildBytes: Double?
    let unavailableReason: String?

    static let empty = CoreDiskUsage(
        path: nil,
        totalBytes: nil,
        largestChildName: nil,
        largestChildBytes: nil,
        unavailableReason: nil
    )

    enum CodingKeys: String, CodingKey {
        case path
        case totalBytes = "total_bytes"
        case largestChildName = "largest_child_name"
        case largestChildBytes = "largest_child_bytes"
        case unavailableReason = "unavailable_reason"
    }
}

/// What the summary card needs that a checkout row does not already carry.
struct CoreCheckoutCard: Decodable, Equatable {
    var inspectedCheckoutPath: String? = nil
    let checkoutID: String?
    let github: CoreGithubStatus
    let disk: CoreDiskUsage
    let diskMeasuring: Bool
    let deletionGate: CoreWorktreeDeletionGate?
    let panes: [CoreCheckoutPaneContext]

    static let empty = CoreCheckoutCard(
        checkoutID: nil,
        github: .empty,
        disk: .empty,
        diskMeasuring: false,
        deletionGate: nil
    )

    enum CodingKeys: String, CodingKey {
        case inspectedCheckoutPath = "inspected_checkout_path"
        case checkoutID = "checkout_id"
        case github
        case disk
        case diskMeasuring = "disk_measuring"
        case deletionGate = "deletion_gate"
        case panes
    }

    init(
        checkoutID: String?,
        github: CoreGithubStatus,
        disk: CoreDiskUsage,
        diskMeasuring: Bool,
        deletionGate: CoreWorktreeDeletionGate?,
        panes: [CoreCheckoutPaneContext] = []
    ) {
        self.checkoutID = checkoutID
        self.github = github
        self.disk = disk
        self.diskMeasuring = diskMeasuring
        self.deletionGate = deletionGate
        self.panes = panes
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        inspectedCheckoutPath = try container.decodeIfPresent(String.self, forKey: .inspectedCheckoutPath)
        checkoutID = try container.decodeIfPresent(String.self, forKey: .checkoutID)
        github = try container.decodeIfPresent(CoreGithubStatus.self, forKey: .github) ?? .empty
        disk = try container.decodeIfPresent(CoreDiskUsage.self, forKey: .disk) ?? .empty
        diskMeasuring = try container.decodeIfPresent(Bool.self, forKey: .diskMeasuring) ?? false
        deletionGate = try container.decodeIfPresent(CoreWorktreeDeletionGate.self, forKey: .deletionGate)
        panes = try container.decodeIfPresent([CoreCheckoutPaneContext].self, forKey: .panes) ?? []
    }
}

struct CoreCheckoutPaneContext: Decodable, Equatable, Identifiable {
    let paneID: String
    let title: String
    let status: String
    let sessionID: String?
    let parentPaneID: String?
    var id: String { paneID }

    enum CodingKeys: String, CodingKey {
        case paneID = "pane_id"
        case title, status
        case sessionID = "session_id"
        case parentPaneID = "parent_pane_id"
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
    let content: CorePaneContent
    /// The three names a pane can be shown by. The core ships all three and
    /// `PaneHeaderPresentation` picks; see its ladder for the order.
    let herdrLabel: String?
    let terminalTitle: String?
    let workspaceLabel: String?
    let cwd: String
    /// The one short human word for the agent in this pane. The core derives
    /// it; no view builds a label out of a state name.
    let statusLabel: String
    /// Whether closing this pane needs confirmation first, as the core derived
    /// it for the same agent the sidebar row shows.
    let requiresCloseConfirmation: Bool
    let summary: String?
    let activityAt: UInt64?
    let fork: CorePaneFork
    /// Ports listened on from at or below this pane's working directory.
    let ports: [UInt16]

    enum CodingKeys: String, CodingKey {
        case id
        case content
        case fork
        case ports
        case herdrLabel = "herdr_label"
        case terminalTitle = "terminal_title"
        case workspaceLabel = "workspace_label"
        case cwd
        case statusLabel = "status_label"
        case requiresCloseConfirmation = "requires_close_confirmation"
        case summary
        case activityAt = "activity_at_unix_ms"
    }

    init(
        id: String,
        content: CorePaneContent = .terminal,
        herdrLabel: String? = nil,
        terminalTitle: String? = nil,
        workspaceLabel: String? = nil,
        cwd: String,
        statusLabel: String,
        requiresCloseConfirmation: Bool = false,
        summary: String?,
        activityAt: UInt64?,
        fork: CorePaneFork = CorePaneFork(),
        ports: [UInt16] = []
    ) {
        self.id = id
        self.content = content
        self.herdrLabel = herdrLabel
        self.terminalTitle = terminalTitle
        self.workspaceLabel = workspaceLabel
        self.cwd = cwd
        self.statusLabel = statusLabel
        self.requiresCloseConfirmation = requiresCloseConfirmation
        self.summary = summary
        self.activityAt = activityAt
        self.fork = fork
        self.ports = ports
    }

    /// Terminal is the default content when no native content is specified.
    /// `fork` and `ports` are optional capabilities. Every other
    /// field describes the pane itself, and a pane missing one of those is a
    /// snapshot worth rejecting.
    init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(String.self, forKey: .id)
        content = try container.decodeIfPresent(CorePaneContent.self, forKey: .content) ?? .terminal
        herdrLabel = try container.decodeIfPresent(String.self, forKey: .herdrLabel)
        terminalTitle = try container.decodeIfPresent(String.self, forKey: .terminalTitle)
        workspaceLabel = try container.decodeIfPresent(String.self, forKey: .workspaceLabel)
        cwd = try container.decode(String.self, forKey: .cwd)
        statusLabel = try container.decode(String.self, forKey: .statusLabel)
        requiresCloseConfirmation = try container.decode(
            Bool.self, forKey: .requiresCloseConfirmation
        )
        summary = try container.decodeIfPresent(String.self, forKey: .summary)
        activityAt = try container.decodeIfPresent(UInt64.self, forKey: .activityAt)
        fork = try container.decodeIfPresent(CorePaneFork.self, forKey: .fork) ?? CorePaneFork()
        ports = try container.decodeIfPresent([UInt16].self, forKey: .ports) ?? []
    }
}

/// What the core says about forking one pane.
///
/// `available` is the core's answer to whether this pane runs an agent whose
/// own fork command could take its recorded session, so the header renders the
/// control rather than deciding the question a second time.
struct CorePaneFork: Decodable, Equatable {
    let available: Bool
    let forkedFromPaneID: String?

    enum CodingKeys: String, CodingKey {
        case available
        case forkedFromPaneID = "forked_from_pane_id"
    }

    init(available: Bool = false, forkedFromPaneID: String? = nil) {
        self.available = available
        self.forkedFromPaneID = forkedFromPaneID
    }
}

struct SidebarAgent: Decodable, Equatable, Identifiable {
    let id: String
    let paneID: String
    let workspaceLabel: String
    /// The checkout the agent's pane is in, once the core has placed it in a
    /// project. Absent for a pane the navigator does not hold.
    var checkoutLabel: String? = nil
    let agentKind: String
    /// What the agent needs from the operator: `question`, `approval`,
    /// `error`, or `none`.
    let demand: String
    /// Whether the agent is running: `working`, `stopped`, or `unknown`.
    let activity: String
    /// Whether this pane has changed since the operator last focused it. Hide
    /// owns this per pane; Herdr's seen is tab-scoped and is never used here.
    let unread: Bool
    /// Herdr reports an approval prompt on this pane right now.
    let blocked: Bool
    /// Which of the four sidebar groups this row belongs to.
    let group: String
    let symbol: String
    /// Whether the row is drawn bright rather than subdued.
    let emphasized: Bool
    /// The one short human word this row shows.
    let statusLabel: String
    /// Whether closing this pane needs confirmation first.
    let requiresCloseConfirmation: Bool
    let summary: String
    let elapsed: String
    let lastActivity: String
    let ambient: CoreAmbientSignal?

    var lineageDepth: Int = 0
    var lineageChildPaneIDs: [String] = []
    var lineageRootCheckoutID: String? = nil
    var lineageWorktreeBadge: String? = nil
    var lineageOrphan: Bool = false
    var lineageHint: String? = nil
    var raisedHint: String? = nil
    var lineageCollapsed: Bool = false

    /// Fixtures and tests build a row directly. Every derived value defaults
    /// to the quiet reading, so a fixture states only what it is exercising.
    init(
        id: String,
        paneID: String,
        workspaceLabel: String,
        checkoutLabel: String? = nil,
        agentKind: String,
        demand: String = "none",
        activity: String = "unknown",
        unread: Bool = false,
        blocked: Bool = false,
        group: String = "seen",
        symbol: String,
        emphasized: Bool = false,
        statusLabel: String = "Idle",
        requiresCloseConfirmation: Bool = false,
        summary: String,
        elapsed: String,
        lastActivity: String,
        ambient: CoreAmbientSignal?
    ) {
        self.id = id
        self.paneID = paneID
        self.workspaceLabel = workspaceLabel
        self.checkoutLabel = checkoutLabel
        self.agentKind = agentKind
        self.demand = demand
        self.activity = activity
        self.unread = unread
        self.blocked = blocked
        self.group = group
        self.symbol = symbol
        self.emphasized = emphasized
        self.statusLabel = statusLabel
        self.requiresCloseConfirmation = requiresCloseConfirmation
        self.summary = summary
        self.elapsed = elapsed
        self.lastActivity = lastActivity
        self.ambient = ambient
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(String.self, forKey: .id)
        paneID = try container.decode(String.self, forKey: .paneID)
        workspaceLabel = try container.decode(String.self, forKey: .workspaceLabel)
        checkoutLabel = try container.decodeIfPresent(String.self, forKey: .checkoutLabel)
        agentKind = try container.decode(String.self, forKey: .agentKind)
        demand = try container.decode(String.self, forKey: .demand)
        activity = try container.decode(String.self, forKey: .activity)
        unread = try container.decode(Bool.self, forKey: .unread)
        blocked = try container.decode(Bool.self, forKey: .blocked)
        group = try container.decode(String.self, forKey: .group)
        symbol = try container.decode(String.self, forKey: .symbol)
        emphasized = try container.decode(Bool.self, forKey: .emphasized)
        statusLabel = try container.decode(String.self, forKey: .statusLabel)
        requiresCloseConfirmation = try container.decode(Bool.self, forKey: .requiresCloseConfirmation)
        summary = try container.decode(String.self, forKey: .summary)
        elapsed = try container.decode(String.self, forKey: .elapsed)
        lastActivity = try container.decode(String.self, forKey: .lastActivity)
        ambient = try container.decodeIfPresent(CoreAmbientSignal.self, forKey: .ambient)
        lineageDepth = try container.decodeIfPresent(Int.self, forKey: .lineageDepth) ?? 0
        lineageChildPaneIDs = try container.decodeIfPresent([String].self, forKey: .lineageChildPaneIDs) ?? []
        lineageRootCheckoutID = try container.decodeIfPresent(String.self, forKey: .lineageRootCheckoutID)
        lineageWorktreeBadge = try container.decodeIfPresent(String.self, forKey: .lineageWorktreeBadge)
        lineageOrphan = try container.decodeIfPresent(Bool.self, forKey: .lineageOrphan) ?? false
        lineageHint = try container.decodeIfPresent(String.self, forKey: .lineageHint)
        raisedHint = try container.decodeIfPresent(String.self, forKey: .raisedHint)
        lineageCollapsed = try container.decodeIfPresent(Bool.self, forKey: .lineageCollapsed) ?? false
    }

    enum CodingKeys: String, CodingKey {
        case id
        case paneID = "pane_id"
        case workspaceLabel = "workspace_label"
        case checkoutLabel = "checkout_label"
        case agentKind = "agent_kind"
        case demand
        case activity
        case unread
        case blocked
        case group
        case symbol
        case emphasized
        case statusLabel = "status_label"
        case requiresCloseConfirmation = "requires_close_confirmation"
        case summary
        case elapsed
        case lastActivity = "last_activity"
        case ambient
        case lineageDepth = "lineage_depth"
        case lineageChildPaneIDs = "lineage_child_pane_ids"
        case lineageRootCheckoutID = "lineage_root_checkout_id"
        case lineageWorktreeBadge = "lineage_worktree_badge"
        case lineageOrphan = "lineage_orphan"
        case lineageHint = "lineage_hint"
        case raisedHint = "raised_hint"
        case lineageCollapsed = "lineage_collapsed"
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
    var attachments: [CoreAttachmentShelf]? = nil

    enum CodingKeys: String, CodingKey {
        case paneID = "pane_id"
        case closed
        case exitCode = "exit_code"
        case panes
        case attachments
    }
}

struct CoreTerminalChunk: Decodable {
    let paneID: String
    let sequence: UInt64
    let bytesBase64: String
    let frame: CoreTerminalFrame?
    let inputSent: CoreTerminalInputSent?

    enum CodingKeys: String, CodingKey {
        case frame
        case inputSent = "input_sent"
        case paneID = "pane_id"
        case sequence
        case bytesBase64 = "bytes_base64"
    }
}

struct CoreTerminalFrame: Decodable {
    let width: Int
    let height: Int
    let full: Bool
}

struct CoreTerminalInputSent: Decodable {
    let id: UInt64
    let milliseconds: Double
    let outcome: String
}

struct TerminalDelivery {
    let bytes: [UInt8]
    let frame: CoreTerminalFrame?
    let inputSent: CoreTerminalInputSent?
}

struct CoreTerminalPaneSnapshot: Decodable, Identifiable {
    var id: String { paneID }
    let paneID: String
    let closed: Bool
    let exitCode: Int32?
    let transportState: String
    let transportMessage: String?
    let transportGeneration: UInt64
    let transportLastAttemptAtUnixMS: UInt64?
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
        case transportLastAttemptAtUnixMS = "transport_last_attempt_at_unix_ms"
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
        transportLastAttemptAtUnixMS = try container.decodeIfPresent(UInt64.self, forKey: .transportLastAttemptAtUnixMS)
        transportExitCategory = try container.decodeIfPresent(String.self, forKey: .transportExitCategory)
        transportRetryDecision = try container.decodeIfPresent(
            String.self,
            forKey: .transportRetryDecision
        ) ?? "none"
    }
}

struct CoreEditorSnapshot: Decodable {
    let tabs: [CoreEditorTabSnapshot]
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

enum CoreEditorTabKind: String, Decodable, Equatable {
    case file
    case diff
}

struct CoreEditorTabSnapshot: Decodable, Identifiable, Equatable {
    let id: String
    let workspaceID: String
    let checkoutID: String
    let path: String
    let label: String
    let kind: CoreEditorTabKind
    let diffCommitted: Bool?
    var markdownPreview: Bool? = nil
    var wrap: Bool? = nil
    let dirty: Bool

    enum CodingKeys: String, CodingKey {
        case id
        case workspaceID = "workspace_id"
        case checkoutID = "checkout_id"
        case path
        case label
        case kind
        case markdownPreview = "markdown_preview"
        case wrap
        case diffCommitted = "diff_committed"
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
    let collapsedCheckoutIDs: [String]
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
    /// The file editor's own zoom. The editor is one surface rather than one
    /// per document, and it is not a pane, so it carries a scale of its own
    /// instead of a row in the pane-keyed map.
    let editorTextScale: Double
    /// The composer's defaults for the next chat, and whether the Scratch
    /// section is open. Written by those surfaces only.
    let lastAgentKind: String
    let lastAgentBypass: Bool
    let scratchExpanded: Bool

    var collapsedAgentPaneIDs: [String] = []
    var projectBaseBranches: [String: String] = [:]

    enum CodingKeys: String, CodingKey {
        case lastAgentKind = "last_agent_kind"
        case lastAgentBypass = "last_agent_bypass"
        case scratchExpanded = "scratch_expanded"
        case collapsedAgentPaneIDs = "collapsed_agent_pane_ids"
        case projectBaseBranches = "project_base_branches"
        case leftSidebarVisible = "left_sidebar_visible"
        case rightPanelVisible = "right_panel_visible"
        case rightPanelSection = "right_panel_section"
        case expandedPaths = "expanded_paths"
        case collapsedWorkspaceIDs = "collapsed_workspace_ids"
        case collapsedCheckoutIDs = "collapsed_checkout_ids"
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
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        collapsedAgentPaneIDs = try container.decodeIfPresent([String].self, forKey: .collapsedAgentPaneIDs) ?? []
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
        selectedPath = try container.decodeIfPresent(String.self, forKey: .selectedPath)
        selectedPaneID = try container.decodeIfPresent(String.self, forKey: .selectedPaneID)
        lastAgentKind = try container.decodeIfPresent(String.self, forKey: .lastAgentKind)
            ?? AgentProvider.claude.rawValue
        lastAgentBypass = try container.decodeIfPresent(Bool.self, forKey: .lastAgentBypass) ?? false
        scratchExpanded = try container.decodeIfPresent(Bool.self, forKey: .scratchExpanded) ?? false
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

/// The right panel's four sections. The core owns which one is showing, so the
/// choice survives hiding and reopening the panel.
enum RightPanelSection: String, Decodable, CaseIterable, Identifiable {
    case overview
    case explorer
    case changes
    case git

    var id: String { rawValue }

    var title: String {
        switch self {
        case .overview: "Overview"
        case .explorer: "Explorer"
        case .changes: "Changes"
        case .git: "Git"
        }
    }

    var systemImage: String {
        switch self {
        case .overview: "info.circle"
        case .explorer: "doc.text.magnifyingglass"
        case .changes: "arrow.triangle.branch"
        case .git: HideTheme.gitSectionIcon
        }
    }
}

/// One checkout's Git working-tree state as the core read it.
struct CoreChangesSnapshot: Decodable {
    let rootPath: String?
    /// The working tree's own changes.
    let entries: [CoreChangedFile]
    /// What commits on this branch changed since `baseBranch`.
    let committed: [CoreChangedFile]
    /// What the committed group is measured against. `nil` means there is no
    /// comparison to make, so that group is not shown at all.
    let baseBranch: String?
    let selectedPath: String?
    /// Which group the selection is in. The same path can appear in both and
    /// its two diffs differ, so the group is part of the selection.
    let selectedCommitted: Bool
    let diff: CoreChangedFileDiff?
    /// Why there is nothing to list. An empty list with no reason means the
    /// checkout genuinely has no changes.
    let unavailableReason: String?

    enum CodingKeys: String, CodingKey {
        case rootPath = "root_path"
        case entries
        case committed
        case baseBranch = "base_branch"
        case selectedPath = "selected_path"
        case selectedCommitted = "selected_committed"
        case diff
        case unavailableReason = "unavailable_reason"
    }

    static let empty = CoreChangesSnapshot(
        rootPath: nil,
        entries: [],
        committed: [],
        baseBranch: nil,
        selectedPath: nil,
        selectedCommitted: false,
        diff: nil,
        unavailableReason: nil
    )

    init(
        rootPath: String?,
        entries: [CoreChangedFile],
        committed: [CoreChangedFile] = [],
        baseBranch: String? = nil,
        selectedPath: String?,
        selectedCommitted: Bool = false,
        diff: CoreChangedFileDiff?,
        unavailableReason: String?
    ) {
        self.rootPath = rootPath
        self.entries = entries
        self.committed = committed
        self.baseBranch = baseBranch
        self.selectedPath = selectedPath
        self.selectedCommitted = selectedCommitted
        self.diff = diff
        self.unavailableReason = unavailableReason
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        rootPath = try container.decodeIfPresent(String.self, forKey: .rootPath)
        entries = try container.decodeIfPresent([CoreChangedFile].self, forKey: .entries) ?? []
        committed = try container.decodeIfPresent([CoreChangedFile].self, forKey: .committed) ?? []
        baseBranch = try container.decodeIfPresent(String.self, forKey: .baseBranch)
        selectedPath = try container.decodeIfPresent(String.self, forKey: .selectedPath)
        selectedCommitted = try container.decodeIfPresent(Bool.self, forKey: .selectedCommitted) ?? false
        diff = try container.decodeIfPresent(CoreChangedFileDiff.self, forKey: .diff)
        unavailableReason = try container.decodeIfPresent(String.self, forKey: .unavailableReason)
    }
}

struct CoreChangedFile: Decodable, Identifiable, Equatable {
    let path: String
    let relativePath: String
    let status: CoreChangedFileStatus
    /// Absent for a file git cannot count - an untracked one has no index side
    /// and a binary one has no lines - so the row shows no numbers rather than
    /// a zero that would read as "changed nothing".
    let addedLines: Int?
    let removedLines: Int?

    var id: String { path }

    /// Row identity per group. A file edited again after being committed is in
    /// both groups under one path, and the list must show it twice.
    var uncommittedRowID: String { "uncommitted:" + path }
    var committedRowID: String { "committed:" + path }

    /// The directory the row shows in the dimmer half, empty at the root.
    var directory: String {
        let components = relativePath.split(separator: "/").dropLast()
        return components.joined(separator: "/")
    }

    /// The file name the row shows in the brighter half.
    var name: String {
        String(relativePath.split(separator: "/").last ?? "")
    }

    enum CodingKeys: String, CodingKey {
        case path
        case relativePath = "relative_path"
        case status
        case addedLines = "added_lines"
        case removedLines = "removed_lines"
    }

    init(
        path: String,
        relativePath: String,
        status: CoreChangedFileStatus,
        addedLines: Int? = nil,
        removedLines: Int? = nil
    ) {
        self.path = path
        self.relativePath = relativePath
        self.status = status
        self.addedLines = addedLines
        self.removedLines = removedLines
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
    /// Why this diff is not the whole story: it was cut for size, or git
    /// would not produce it. Either way the reader is told rather than shown a
    /// short diff that looks complete.
    let notice: String?

    enum CodingKeys: String, CodingKey {
        case path
        case text
        case notice
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
        "reorder_tab",
        "create_tab",
        "create_pane",
        "toggle_zoom",
        "close_pane",
        "fork_pane",
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
    #if DEBUG
    private let verificationSnapshotPath: String?
    private let verificationSnapshotOutput: String?
    // Assigned once during initialization; Dispatch source cancellation is thread-safe.
    nonisolated(unsafe) private var verificationSnapshotWatcher: (any DispatchSourceFileSystemObject)?
    #endif
    /// Why this launch cannot proceed, when it cannot. Nil once the runtime
    /// resolved and the server started, whatever the connection does after.
    @Published private(set) var startupDiagnostic: String?
    private var runtimeInitializationStarted = false
    /// The runtime resolution started at init, awaited once before the core
    /// is created. Held so the two are the same piece of work rather than two
    /// resolutions racing.
    private var runtimePreparation: Task<RuntimeStartupPreparation, Never>?
    private var lastLoggedHerdrState: String?
    private var lastLoggedErrorKind: String?
    private var lastLoggedProjection: String?
    private var lastLoggedSnapshotRevision: UInt64?
    private var lastTerminalSequence: UInt64 = 0
    private var haveRevision: UInt64 = 0
    private var pendingTerminalBytes = PendingTerminalBuffer<TerminalDelivery>()
    private var terminalRegistrations: [String: TerminalRegistration] = [:]
    private var restoredPaneSelection = false
    private lazy var attachmentFiles = ImageAttachmentFiles()
    private var pendingFileSave: Task<Void, Never>?
    private var pendingFileSavePayload: [String: Any]?
    private var commandDevice = CommandDevice.local
    private var routingError: String?
    private let remoteTargets: [[String: String]]

    private struct CommandDevice {
        let id: String
        let label: String
        let isRemote: Bool

        static let local = CommandDevice(id: "local", label: "This Mac", isRemote: false)
    }

    private struct TerminalRegistration {
        let id: UUID
        let receive: (TerminalDelivery) -> Void
        let focus: () -> Void
    }

    init(arguments: [String] = CommandLine.arguments) {
        let initStarted = Date()
        HideLaunchTrace.mark("core_bridge.init.begin")
        remoteTargets = Self.remoteTargets(arguments: arguments)
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
        #if DEBUG
        verificationSnapshotPath = resolvedFixtureMode
            ? LaunchArguments.value("--verification-snapshot", in: arguments) : nil
        verificationSnapshotOutput = LaunchArguments.value("--verification-snapshot-output", in: arguments)
        #endif
        statePath = resolvedStatePath
        fixtureMode = resolvedFixtureMode
        runtimeSelection = nil
        if resolvedFixtureMode {
            // The verification fixture never talks to Herdr, so it has no
            // runtime to resolve and its core is ready immediately.
            guard adoptCore(herdrBinaryPath: nil) else {
                HideLaunchTrace.mark(
                    "core_bridge.init.failed",
                    detail: "core_create_null",
                    durationMilliseconds: Int(Date().timeIntervalSince(initStarted) * 1_000)
                )
                return
            }
            #if DEBUG
            seedVerificationFixture()
            if let verificationSnapshotPath {
                let descriptor = open(verificationSnapshotPath, O_EVTONLY)
                precondition(descriptor >= 0, "Cannot observe the verification snapshot")
                let watcher = DispatchSource.makeFileSystemObjectSource(fileDescriptor: descriptor, eventMask: .write, queue: .main)
                watcher.setEventHandler { [weak self] in self?.refreshSnapshot() }
                watcher.setCancelHandler { close(descriptor) }
                verificationSnapshotWatcher = watcher
                watcher.resume()
            }
            #endif
            HideLaunchTrace.mark(
                "core_bridge.init.ready",
                detail: "fixture",
                durationMilliseconds: Int(Date().timeIntervalSince(initStarted) * 1_000)
            )
            return
        }
        startupDiagnostic = HideStartupDiagnostic.initializing
        bridgeError = startupDiagnostic
        // The core is created once, and it needs the resolved Herdr binary to
        // attach a terminal at all, so resolution has to finish first.
        // Starting it here rather than at the first window means it overlaps
        // AppKit's launch instead of following it, while the subprocesses it
        // runs stay off the main thread.
        let bundlePath = Bundle.main.path(
            forResource: "herdr",
            ofType: nil,
            inDirectory: "herdr-runtime"
        )
        runtimePreparation = Task.detached(priority: .userInitiated) {
            RuntimeStartupPreparation(
                selection: HerdrRuntimeResolver.resolve(
                    bundlePath: bundlePath,
                    pin: HerdrRuntimePinLoader.pinned
                ),
                environment: HideRuntimeEnvironment.childEnvironment()
            )
        }
        HideLaunchTrace.mark(
            "core_bridge.init.ready",
            detail: "awaiting_runtime",
            durationMilliseconds: Int(Date().timeIntervalSince(initStarted) * 1_000)
        )
    }

    /// Creates the one core of this launch, once the application has made its
    /// first window visible and the runtime it needs has been resolved.
    /// Finder launches therefore cannot lose their first window to a slow
    /// shell or CLI subprocess.
    func startRuntimeInitialization() {
        guard !fixtureMode, !runtimeInitializationStarted else { return }
        runtimeInitializationStarted = true
        let socketPath = HideRuntimeEnvironment.herdrSocketPath()
        let startedAt = Date()
        HideLaunchTrace.mark("runtime_initialization.begin")
        Task { @MainActor [weak self] in
            guard let preparation = await self?.runtimePreparation?.value else { return }
            guard let self else { return }
            self.runtimePreparation = nil
            let detail = preparation.selection.map {
                "bundled_v\($0.version)"
            } ?? "no_runtime"
            HideLaunchTrace.mark(
                "runtime_initialization.resolved",
                detail: detail,
                durationMilliseconds: Int(Date().timeIntervalSince(startedAt) * 1_000)
            )

            let socketExists = FileManager.default.fileExists(atPath: socketPath)
            self.runtimeSelection = preparation.selection
            guard self.adoptCore(herdrBinaryPath: preparation.selection?.path) else {
                self.setStartupDiagnostic(
                    "Hide could not initialize its Herdr connection. Reopen the app to retry."
                )
                HideLaunchTrace.mark("runtime_initialization.failed", detail: "core_create_null")
                return
            }
            HideLaunchTrace.markOnce(
                "core_bridge.ready",
                detail: preparation.selection != nil ? "runtime_bundled" : "socket_only"
            )
            // A missing or unverified bundle is reported even when a server
            // is already running: the core can still read that socket, but
            // nothing hide starts (agents, terminals) has a binary to run.
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
        statePath: String,
        remoteTargets: [[String: String]]
    ) -> OpaquePointer? {
        let options: [String: Any] = [
            "schema_version": coreSchemaVersion,
            "herdr_socket_path": fixtureMode ? NSNull() : HideRuntimeEnvironment.herdrSocketPath() as Any,
            "herdr_bin_path": herdrBinaryPath.map { $0 as Any } ?? NSNull(),
            "remote_targets": remoteTargets,
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

    /// Creates this launch's core and takes ownership of it. A bridge holds
    /// one core for its whole life, so this runs once.
    private func adoptCore(herdrBinaryPath: String?) -> Bool {
        guard let created = Self.createCore(
            herdrBinaryPath: herdrBinaryPath,
            fixtureMode: fixtureMode,
            statePath: statePath,
            remoteTargets: remoteTargets
        ) else {
            bridgeError = "herdr_core_create returned null"
            return false
        }
        core = created
        herdr_core_on_change(
            created,
            coreChangeCallback,
            Unmanaged.passUnretained(self).toOpaque()
        )
        startupDiagnostic = nil
        refreshSnapshot()
        return true
    }

    static func remoteTargets(arguments: [String]) -> [[String: String]] {
        #if DEBUG
        if arguments.contains("--verification-no-remote") {
            return []
        }
        #endif
        return [[
            "id": "mini",
            "label": "Mac mini",
            "ssh_alias": "mini",
            "herdr_socket_path": "/Users/example/.config/herdr/herdr.sock",
        ]]
    }

    private func setStartupDiagnostic(_ message: String?) {
        startupDiagnostic = message
        bridgeError = message
    }

    deinit {
        #if DEBUG
        verificationSnapshotWatcher?.cancel()
        #endif
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
        var payload: [String: Any] = ["pane_id": target, "bytes_base64": Data(bytes).base64EncodedString()]
        if let trace = TerminalLatency.takeInput(paneID: target) { payload["input_trace"] = trace }
        dispatch(kind: "key", payload: payload)
    }

    func clickTerminal(paneID: String, column: Int, row: Int, modifiers: Int) {
        dispatch(kind: "terminal_click", payload: [
            "pane_id": paneID, "column": column, "row": row, "modifiers": modifiers,
        ])
    }

    func reportTerminalViewport(paneID: String, cols: Int, rows: Int, newView: Bool) {
        dispatch(kind: "terminal_viewport", payload: ["pane_id": paneID, "cols": cols, "rows": rows, "new_view": newView])
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
    func scrollTerminal(paneID: String, direction: String, lines: Int, column: Int, row: Int, modifiers: Int) {
        dispatch(kind: "terminal_scroll", payload: [
            "pane_id": paneID,
            "direction": direction,
            "lines": lines,
            "column": column, "row": row, "modifiers": modifiers,
        ])
    }

    /// Why a pane focus is being asked for.
    ///
    /// The core raises a pane's read record only for an operator focus, so a
    /// launch restore reinstating the last session's selection must say so:
    /// the operator has not looked at what changed while the app was closed.
    enum PaneFocusOrigin: String {
        case operatorChoice = "operator"
        case restore
    }

    func focusPane(_ paneID: String, origin: PaneFocusOrigin) {
        dispatch(
            kind: "focus_pane",
            payload: ["pane_id": paneID, "origin": origin.rawValue]
        )
    }

    /// The core owns the ladder and its bounds, so the shell sends a direction
    /// rather than a computed size and reads the result back off the snapshot.
    var paneFind: CorePaneFindSnapshot { snapshot?.find ?? .empty }

    /// Searches a pane's whole scrollback, or steps to the next or previous
    /// match. An empty term clears the search.
    ///
    /// `step` is relative so that typing and stepping share one path: 0
    /// searches and stays put, +1 and -1 move.
    func findInPane(
        paneID: String,
        term: String,
        caseSensitive: Bool = false,
        wholeWord: Bool = false,
        regex: Bool = false,
        step: Int = 0
    ) {
        dispatch(
            kind: "pane_find",
            payload: [
                "pane_id": paneID,
                "term": term,
                "case_sensitive": caseSensitive,
                "whole_word": wholeWord,
                "regex": regex,
                "step": step,
            ]
        )
    }

    func setPaneTextScale(paneID: String, direction: PaneTextScaleDirection) {
        dispatch(
            kind: "pane_text_scale",
            payload: ["pane_id": paneID, "direction": direction.rawValue]
        )
    }

    func setEditorTextScale(direction: PaneTextScaleDirection) {
        dispatch(kind: "editor_text_scale", payload: ["direction": direction.rawValue])
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
        if let paneID = snapshot?.paneLayouts.first(where: { $0.tabID == tabID })?.focusedPaneID {
            TerminalLatency.end(.tabToDraw, paneID: paneID, outcome: "superseded")
            TerminalLatency.begin(.tabToDraw, paneID: paneID)
        }
        dispatch(kind: "focus_tab", payload: [
            "workspace_id": workspaceID,
            "checkout_id": checkoutID,
            "tab_id": tabID,
        ])
    }

    /// Asks the core to put one strip entry at another place in the strip.
    ///
    /// `tabID` is the strip entry's id, which spans both kinds, and `toIndex`
    /// is where it ends up in the resulting strip. The core decides whether
    /// that needs anything from Herdr; the shell only reports the drop.
    func reorderTab(workspaceID: String, checkoutID: String, tabID: String, toIndex: Int) {
        dispatch(kind: "reorder_tab", payload: [
            "workspace_id": workspaceID,
            "checkout_id": checkoutID,
            "tab_id": tabID,
            "to_index": toIndex,
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

    /// Starts one chat and reports what happened.
    ///
    /// The four Herdr calls run on a detached task, never on the main actor
    /// and never under the runtime lock: the readiness wait alone is up to
    /// thirty seconds, and holding either through it would freeze the window.
    /// The completion is what unlocks the composer, so the sheet stays locked
    /// for exactly as long as the work takes.
    func startChat(
        destination: CheckoutChatDestination,
        provider: AgentProvider,
        message: String,
        bypassWarnings: Bool,
        completion: @escaping @MainActor (ChatLaunchResult) -> Void
    ) {
        guard let runtimeSelection else {
            bridgeError = HideStartupDiagnostic.runtimeUnavailable
            completion(
                ChatLaunchResult(
                    succeeded: false,
                    failedStep: .createTab,
                    message: HideStartupDiagnostic.runtimeUnavailable,
                    paneID: nil
                )
            )
            return
        }
        let herdrPath = runtimeSelection.path
        HideLaunchTrace.mark(
            "chat.launch.requested",
            detail: "kind=\(provider.rawValue) workspace_id=\(destination.workspaceID ?? "new")"
        )
        Task { @MainActor [weak self] in
            let result = await Task.detached {
                HerdrChatLauncher.start(
                    herdrPath: herdrPath,
                    destination: destination,
                    provider: provider,
                    message: message,
                    bypassWarnings: bypassWarnings
                )
            }.value
            guard let self else { return }
            HideLaunchTrace.mark(
                result.succeeded ? "chat.launch.ready" : "chat.launch.failed",
                detail: result.failedStep.map { "step=\($0.rawValue)" } ?? "ok"
            )
            if let paneID = result.paneID {
                // The CLI-created root pane anchors the selection until
                // session sync publishes its authoritative layout. The tab
                // stays visible even when a later step failed, because it is
                // a shell prompt the operator can still use.
                self.persistUIState(
                    selectedPaneID: paneID,
                    focusedCheckoutID: destination.id
                )
            }
            completion(result)
        }
    }

    func startAgentInCreatedPane(
        paneID: String,
        path: String,
        provider: AgentProvider,
        message: String?,
        bypassWarnings: Bool,
        completion: @escaping @MainActor (ChatLaunchResult) -> Void
    ) {
        guard let runtimeSelection else {
            completion(ChatLaunchResult(
                succeeded: false,
                failedStep: .startAgent,
                message: HideStartupDiagnostic.runtimeUnavailable,
                paneID: paneID
            ))
            return
        }
        let herdrPath = runtimeSelection.path
        Task { @MainActor in
            let result = await Task.detached {
                HerdrChatLauncher.startInPane(
                    paneID: paneID,
                    path: path,
                    provider: provider,
                    message: message,
                    bypassWarnings: bypassWarnings,
                    agentIsInstalled: AgentCLIAvailability.isUsable(provider.rawValue),
                    run: { arguments in
                        HerdrChatLauncher.run(herdrPath: herdrPath, arguments: arguments)
                    }
                )
            }.value
            completion(result)
        }
    }

    @discardableResult
    func registerTerminal(
        paneID: String,
        receive: @escaping (TerminalDelivery) -> Void,
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

    func forkPane(_ paneID: String) {
        dispatch(kind: "fork_pane", payload: ["pane_id": paneID])
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

    /// One clicked path, with everything it changes on screen decided by the
    /// core in a single event.
    ///
    /// This is deliberately not a sequence of `focusCheckout`, `persistUIState`
    /// and `openFile`: dispatch is fire-and-forget, so those would arrive as
    /// separate frames and a refusal partway would leave the screen half
    /// moved. The shell's part is the filesystem question - what the path is,
    /// and whose checkout it belongs to - which cannot run under the runtime
    /// mutex.
    func revealPath(_ url: URL, workspaceID: String, checkoutID: String, isDirectory: Bool) {
        dispatch(kind: "reveal_path", payload: [
            "path": url.path,
            "workspace_id": workspaceID,
            "checkout_id": checkoutID,
            "is_directory": isDirectory,
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

    func pasteImages(_ board: NSPasteboard, paneID: String) -> Bool {
        attachmentFiles.paste(board, paneID: paneID, bridge: self)
    }

    func stageImages(_ urls: [URL], paneID: String) {
        attachmentFiles.stage(urls, paneID: paneID, bridge: self)
    }

    func attachmentAction(_ action: String, paneID: String, id: String, name: String? = nil,
                          path: String? = nil, error: String? = nil, followingBottom: Bool? = nil, active: Bool? = nil) {
        var payload: [String: Any] = ["action": action, "pane_id": paneID, "id": id]
        if let name { payload["name"] = name }
        if let path { payload["path"] = path }
        if let error { payload["error"] = error }
        if let followingBottom { payload["following_bottom"] = followingBottom }
        if let active { payload["active"] = active }
        dispatch(kind: "attachment", payload: payload)
    }

    func setFileView(tabID: String, preview: Bool, wrap: Bool) {
        dispatch(kind: "file_view", payload: ["tab_id": tabID, "markdown_preview": preview, "wrap": wrap])
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
        guard let editor = snapshot?.editor, let tabID = editor.activeTabID, let path = editor.path else { return }
        if let previous = pendingFileSavePayload?["tab_id"] as? String, previous != tabID {
            flushPendingFileSave()
        }
        pendingFileSave?.cancel()
        pendingFileSavePayload = [
            "tab_id": tabID, "path": path, "contents_utf8": contents,
            "expected_modified_at_unix_ms": editor.openedModifiedAt.map { NSNumber(value: $0) as Any } ?? NSNull(),
        ]
        pendingFileSave = Task { @MainActor [weak self] in
            do { try await Task.sleep(for: .milliseconds(450)) } catch { return }
            guard !Task.isCancelled else { return }
            self?.flushPendingFileSave()
        }
    }

    func flushPendingFileSave() {
        guard let payload = pendingFileSavePayload else { return }
        pendingFileSave?.cancel()
        pendingFileSave = nil
        pendingFileSavePayload = nil
        dispatch(kind: "file_save", payload: payload)
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
        collapsedCheckoutIDs: [String]? = nil,
        selectedPath: String? = nil,
        selectedPaneID: String? = nil,
        focusedCheckoutID: String? = nil,
        shortcutBindings: [String: String]? = nil,
        accentHex: String? = nil,
        fontSize: Double? = nil,
        lastAgentKind: String? = nil,
        lastAgentBypass: Bool? = nil,
        scratchExpanded: Bool? = nil
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
        if let collapsedCheckoutIDs {
            payload["collapsed_checkout_ids"] = collapsedCheckoutIDs
        }
        if let focusedCheckoutID {
            // This is the one explicit local-selection anchor used after a
            // terminal launcher returns. It never asks Herdr to change focus.
            payload["focused_checkout_id"] = focusedCheckoutID
        }
        // The composer's memory and the Scratch section's state are written
        // only by the surfaces that own them. Absent means unchanged, so a
        // navigator save cannot reset the composer and a submission cannot
        // close the Scratch section.
        if let lastAgentKind {
            payload["last_agent_kind"] = lastAgentKind
        }
        if let lastAgentBypass {
            payload["last_agent_bypass"] = lastAgentBypass
        }
        if let scratchExpanded {
            payload["scratch_expanded"] = scratchExpanded
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
    func selectChangedFile(path: String?, committed: Bool = false) {
        dispatch(
            kind: "changes_select",
            payload: [
                "path": path.map { $0 as Any } ?? NSNull(),
                "committed": committed,
            ]
        )
    }

    /// The card's refresh button: the pull-request lookup, the worktree
    /// counts, and the size are all read again.
    func refreshCheckoutCard() {
        dispatch(kind: "card_refresh", payload: [:])
    }

    /// Measures the selected checkout again, for the confirmation that states
    /// the size it is about to delete.
    func measureCheckoutDisk() {
        dispatch(kind: "card_measure_disk", payload: [:])
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
        case "focus_tab", "focus_pane":
            // The two view-state dispatches. Hide decides both of them itself,
            // so the interval from this mark to the projection that follows is
            // the whole of what the operator waits for, with no Herdr round
            // trip in it. Measuring it any other way costs the synthetic input
            // harness, which on this machine is over a hundred milliseconds
            // before a keystroke even reaches the app.
            return "kind=\(kind) tab_id=\(traceValue(payload, key: "tab_id")) pane_id=\(traceValue(payload, key: "pane_id")) origin=\(traceValue(payload, key: "origin"))"
        case "terminal_resize":
            // When a view first reports its size is when a pane whose size is
            // not yet known can attach, so the launch trace has to be able to
            // see it. A view reports only when its cell count changes, so this
            // is not a per-frame line.
            return "kind=terminal_resize pane_id=\(traceValue(payload, key: "pane_id")) rows=\(traceValue(payload, key: "rows")) cols=\(traceValue(payload, key: "cols"))"
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
        var requestedRevision = haveRevision
        #if DEBUG
        // Recording requests the existing full wire format. Replaying is only
        // available through the isolated UI fixture, never the live client.
        if verificationSnapshotOutput != nil { requestedRevision = 0 }
        #endif
        let owned = herdr_core_snapshot(core, requestedRevision, lastTerminalSequence)
        defer { herdr_core_free_bytes(owned) }
        guard let pointer = owned.ptr, owned.len > 0 else {
            bridgeError = "herdr_core_snapshot returned empty bytes"
            return
        }
        do {
            var data = Data(bytes: pointer, count: owned.len)
            #if DEBUG
            if let verificationSnapshotPath {
                data = try Data(contentsOf: URL(fileURLWithPath: verificationSnapshotPath))
            }
            if let verificationSnapshotOutput {
                try data.write(to: URL(fileURLWithPath: verificationSnapshotOutput), options: .atomic)
            }
            #endif
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
                    paneLayouts: rest.paneLayouts,
                    terminal: rest.terminal,
                    editor: editor,
                    changes: decoded.changes ?? snapshot?.changes ?? .empty,
                    card: rest.card,
                    find: decoded.find,
                    uiState: rest.uiState,
                    status: rest.status,
                    pet: rest.pet,
                    gitWorktrees: rest.gitWorktrees, gitWorktreesLoading: rest.gitWorktreesLoading,
                    gitWorktreesRemote: rest.gitWorktreesRemote, worktreeRemoval: rest.worktreeRemoval,
                    taskOperation: rest.taskOperation
                ))
            } else if decoded.editor != nil
                || decoded.changes != nil
                || decoded.find != snapshot?.find
            {
                guard let current = snapshot else {
                    bridgeError = "delta.protocol: a section arrived before the first full snapshot"
                    return
                }
                // Find state arrives on every response, so it is compared
                // rather than applied: republishing on each one would rebuild
                // the view for every terminal chunk, which is the invariant
                // chunk-only deltas exist to protect.
                snapshot = current.replacing(
                    editor: decoded.editor,
                    changes: decoded.changes,
                    find: decoded.find
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
                let dropped = pendingTerminalBytes.append(TerminalDelivery(bytes: [UInt8](data), frame: chunk.frame, inputSent: chunk.inputSent), for: chunk.paneID)
                if dropped > 0 {
                    HideLaunchTrace.mark(
                        "terminal.buffer.dropped",
                        detail: "pane_\(chunk.paneID)_chunks_\(dropped)"
                    )
                }
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
        var decoded = decoded
        decoded.navigationRevision = (snapshot?.navigationRevision ?? 0) &+ 1
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
        // The layout the canvas draws is the visible tab's, so that is what
        // the projection line names. A layout no longer needs to be tested
        // against the focused checkout: it is found through that checkout's
        // own active tab or not at all.
        let visibleLayout = focusedCheckout?.activeTabID.flatMap { tabID in
            decoded.paneLayouts.first(where: { $0.tabID == tabID })
        }
        let projection = [
            "focused_workspace_id=\(decoded.navigator.focusedWorkspaceID ?? "nil")",
            "focused_checkout_id=\(decoded.navigator.focusedCheckoutID ?? "nil")",
            "checkout_workspace_id=\(focusedCheckout?.workspaceID ?? "nil")",
            "layout_workspace_id=\(visibleLayout?.workspaceID ?? "nil")",
            "layout_tab_id=\(visibleLayout?.tabID ?? "nil")",
            "layout_pane_ids=\(visibleLayout?.root.paneIDs.joined(separator: ",") ?? "nil")",
            "terminal_pane_id=\(decoded.terminal.paneID ?? "nil")",
            "layout_count=\(decoded.paneLayouts.count)"
        ].joined(separator: " ")
        if projection != lastLoggedProjection {
            lastLoggedProjection = projection
            HideLaunchTrace.mark("core.snapshot.projection", detail: projection)
        }
        // A pane whose session the core released, or that has left the
        // session, redraws from Herdr's own full frame when it is next shown.
        // Its held bytes are frames nobody will draw, so they go now rather
        // than being carried for the life of the process.
        //
        // Which panes still exist is read from Herdr's layouts, not from the
        // transport projection. The projection is emptied while a selection is
        // in progress - `clear_terminal_projection` in the core does exactly
        // that, and deliberately leaves the layouts alone - so a tick taken in
        // that moment lists no pane at all. Dropping every held frame on that
        // is what left a pane blank after a zoom or a switch to a tab of
        // several panes: the full frame was thrown away while its view was
        // still being built, and the next small delta drew on an empty grid.
        // An empty layout set is no answer either, so nothing is dropped then.
        if let keep = PendingTerminalRetention.keep(
            layouts: decoded.paneLayouts,
            transportPanes: decoded.terminal.panes
        ) {
            for paneID in pendingTerminalBytes.retain(paneIDs: keep) {
                HideLaunchTrace.mark("terminal.buffer.cleared", detail: "pane_\(paneID)")
            }
        }
        let previousFocusedPaneID = snapshot?.focusedPaneID
        snapshot = decoded
        attachmentFiles.reconcile(decoded.terminal.attachments ?? [], bridge: self)
        bridgeError = routingError
            ?? decoded.status.lastError.map { "\($0.kind): \($0.message)" }
            ?? startupDiagnostic
        restorePaneSelectionIfNeeded(decoded)
        let authoritativeFocusedPaneID = decoded.focusedPaneID
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
        focusPane(persisted, origin: .restore)
    }

    private func drainPendingTerminalBytes(for paneID: String) {
        guard let registration = terminalRegistrations[paneID],
              let pending = pendingTerminalBytes.take(paneID)
        else { return }
        for delivery in pending {
            let bytes = delivery.bytes
            registration.receive(delivery)
            // The launch is over when a terminal first has a frame to draw.
            // The core writes `ESC c` immediately before a matching full
            // frame to reset the parser, so that
            // chunk marks the request, not the frame. Bytes held for a pane
            // with no view yet are not the moment either, which is why this
            // is here and not where the snapshot is decoded.
            if bytes != Self.terminalGridReset {
                HideLaunchTrace.markOnce("first_terminal_frame", detail: "pane_\(paneID)")
            }
        }
    }

    /// The grid reset the core writes when it requests a terminal session.
    private static let terminalGridReset: [UInt8] = [0x1b, 0x63]

    #if DEBUG
    private func seedVerificationFixture() {
        let paneID = "fixture-working"
        let workspaceID = "fixture-workspace"
        let tabID = "fixture-tab"
        let agents: [[String: Any]] = [
            Self.fixtureAgent("error", "×", "1755000007000", "Build failed", "12s", "Core", "codex", "status_error_new"),
            Self.fixtureAgent("question", "?", "1755000006000", "Choose persistence scope", "4m", "UI", "claude", "status_question_new"),
            Self.fixtureAgent("approval", "!", "1755000005000", "Approve local save", "8m", "Explorer", "codex", "status_approval_new"),
            Self.fixtureAgent("done", "●", "1755000004000", "Sidebar contract complete", "2h", "Agents", "claude", "status_done_new"),
            Self.fixtureAgent("working", "●", "1755000003000", "Connecting Rust bytes", "18s", "Terminal", "codex", "status_working"),
            Self.fixtureAgent("idle", "○", "1755000002000", "Reviewed fixture", "3d", "Verify", "claude", "status_idle"),
            Self.fixtureAgent("unknown", "~", "1755000001000", "Awaiting lifecycle token", "9m", "Other", "unknown", "status_unknown"),
        ]
        dispatch(kind: "create_workspace", payload: [
            "path": workspaceRoot.path,
            "label": "hide rebrand",
            "initialize_git": false,
        ])
        dispatch(kind: "session_snapshot", payload: [
            "focused_pane_id": paneID,
            "agents": agents,
            "workspaces": [[
                "workspace_id": workspaceID,
                "label": "hide rebrand",
                "active_tab_id": tabID,
            ]],
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
                "activity": activity,
                "summary": summary,
                "elapsed": elapsed,
            ],
        ]
    }
    #endif

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

}
