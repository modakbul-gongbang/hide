import Foundation

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
    let inactiveProjects: [CoreInactiveProjectGroupSnapshot]
    let agents: [SidebarAgent]
    let providerUsage: [CoreProviderUsageSnapshot]

    enum CodingKeys: String, CodingKey {
        case rootPath = "root_path"
        case focusedDeviceID = "focused_device_id"
        case focusedWorkspaceID = "focused_workspace_id"
        case focusedCheckoutID = "focused_checkout_id"
        case devices
        case workspaces
        case inactiveProjects = "inactive_projects"
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
        inactiveProjects = try container.decodeIfPresent(
            [CoreInactiveProjectGroupSnapshot].self,
            forKey: .inactiveProjects
        ) ?? []
        agents = try container.decodeIfPresent([SidebarAgent].self, forKey: .agents) ?? []
        providerUsage = try container.decodeIfPresent(
            [CoreProviderUsageSnapshot].self,
            forKey: .providerUsage
        ) ?? []
    }
}

struct CoreInactiveProjectGroupSnapshot: Decodable, Identifiable {
    var id: String { deviceID }
    let deviceID: String
    let expanded: Bool
    let projectIDs: [String]

    enum CodingKeys: String, CodingKey {
        case deviceID = "device_id"
        case expanded
        case projectIDs = "project_ids"
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
    let lastSuccessAtUnixMilliseconds: UInt64?
    let lastErrorKind: String?
    let buckets: [CoreProviderUsageBucketSnapshot]

    enum CodingKeys: String, CodingKey {
        case provider
        case label
        case windowMinutes = "window_minutes"
        case state
        case usedPercent = "used_percent"
        case resetsAtUnixSeconds = "resets_at_unix_seconds"
        case message
        case lastCheckedAtUnixMilliseconds = "last_checked_at_unix_ms"
        case lastSuccessAtUnixMilliseconds = "last_success_at_unix_ms"
        case lastErrorKind = "last_error_kind"
        case buckets
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        provider = try container.decode(String.self, forKey: .provider)
        label = try container.decode(String.self, forKey: .label)
        windowMinutes = try container.decode(UInt64.self, forKey: .windowMinutes)
        state = try container.decode(String.self, forKey: .state)
        usedPercent = try container.decodeIfPresent(Double.self, forKey: .usedPercent)
        resetsAtUnixSeconds = try container.decodeIfPresent(UInt64.self, forKey: .resetsAtUnixSeconds)
        message = try container.decodeIfPresent(String.self, forKey: .message)
        lastCheckedAtUnixMilliseconds = try container.decodeIfPresent(UInt64.self, forKey: .lastCheckedAtUnixMilliseconds)
        lastSuccessAtUnixMilliseconds = try container.decodeIfPresent(UInt64.self, forKey: .lastSuccessAtUnixMilliseconds)
        lastErrorKind = try container.decodeIfPresent(String.self, forKey: .lastErrorKind)
        buckets = try container.decodeIfPresent([CoreProviderUsageBucketSnapshot].self, forKey: .buckets) ?? []
    }
}

struct CoreProviderUsageBucketSnapshot: Decodable, Identifiable {
    var id: String { "\(label)-\(resetsAtUnixSeconds ?? 0)" }
    let label: String
    let state: String
    let usedPercent: Double?
    let resetsAtUnixSeconds: UInt64?
    let message: String?

    enum CodingKeys: String, CodingKey {
        case label
        case state
        case usedPercent = "used_percent"
        case resetsAtUnixSeconds = "resets_at_unix_seconds"
        case message
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
    /// The core's own ordering key: the newest of this project's commit and
    /// agent activity, in Unix milliseconds. Absent when the project has
    /// neither, which is what lets the row leave its time blank.
    let lastActivityUnixMS: UInt64?
    let checkouts: [CoreCheckoutSnapshot]
    let inactiveCheckouts: CoreInactiveCheckoutGroupSnapshot

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
        case lastActivityUnixMS = "last_activity_unix_ms"
        case checkouts
        case inactiveCheckouts = "inactive_checkouts"
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
        lastActivityUnixMS: UInt64? = nil,
        checkouts: [CoreCheckoutSnapshot],
        inactiveCheckouts: CoreInactiveCheckoutGroupSnapshot = .empty
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
        self.lastActivityUnixMS = lastActivityUnixMS
        self.checkouts = checkouts
        self.inactiveCheckouts = inactiveCheckouts
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
        lastActivityUnixMS = try container.decodeIfPresent(UInt64.self, forKey: .lastActivityUnixMS)
        checkouts = try container.decodeIfPresent([CoreCheckoutSnapshot].self, forKey: .checkouts) ?? []
        inactiveCheckouts = try container.decodeIfPresent(
            CoreInactiveCheckoutGroupSnapshot.self,
            forKey: .inactiveCheckouts
        ) ?? .empty
    }
}

struct CoreInactiveCheckoutGroupSnapshot: Decodable, Equatable {
    let expanded: Bool
    let checkoutIDs: [String]

    static let empty = CoreInactiveCheckoutGroupSnapshot(expanded: false, checkoutIDs: [])

    enum CodingKeys: String, CodingKey {
        case expanded
        case checkoutIDs = "checkout_ids"
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
    /// The one agent this tab holds, when the core found exactly one. The
    /// Recent Panels switcher names the tab by it; a shell-only tab and a tab
    /// with several agents carry none and keep their label.
    let agentIdentity: CoreAgentChip?

    enum CodingKeys: String, CodingKey {
        case id
        case kind
        case sourceID = "source_id"
        case label
        case agentIdentity = "agent_identity"
    }

    init(id: String, kind: Kind, sourceID: String, label: String, agentIdentity: CoreAgentChip? = nil) {
        self.id = id
        self.kind = kind
        self.sourceID = sourceID
        self.label = label
        self.agentIdentity = agentIdentity
    }

    init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(String.self, forKey: .id)
        kind = try container.decode(Kind.self, forKey: .kind)
        sourceID = try container.decode(String.self, forKey: .sourceID)
        label = try container.decode(String.self, forKey: .label)
        agentIdentity = try container.decodeIfPresent(CoreAgentChip.self, forKey: .agentIdentity)
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
    /// A tab holding nothing but delegated children. The core keeps it out of
    /// the strip; nothing in the shell decides that a second time.
    let delegated: Bool

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
        case delegated
    }

    init(
        id: String?,
        workspaceID: String?,
        checkoutID: String?,
        label: String?,
        empty: Bool,
        panes: [CorePaneSnapshot],
        delegated: Bool = false
    ) {
        self.id = id
        self.workspaceID = workspaceID
        self.checkoutID = checkoutID
        self.label = label
        self.empty = empty
        self.panes = panes
        self.delegated = delegated
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decodeIfPresent(String.self, forKey: .id)
        workspaceID = try container.decodeIfPresent(String.self, forKey: .workspaceID)
        checkoutID = try container.decodeIfPresent(String.self, forKey: .checkoutID)
        label = try container.decodeIfPresent(String.self, forKey: .label)
        empty = try container.decodeIfPresent(Bool.self, forKey: .empty) ?? true
        panes = try container.decodeIfPresent([CorePaneSnapshot].self, forKey: .panes) ?? []
        delegated = try container.decodeIfPresent(Bool.self, forKey: .delegated) ?? false
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
    /// Whether activity must be refreshed before the core permits a close.
    let requiresCloseStatusCheck: Bool
    /// The agent's stable name, from the same ladder the sidebar row shows.
    let identityLabel: String?
    let activityAt: UInt64?
    let fork: CorePaneFork
    /// Ports listened on from at or below this pane's working directory.
    let ports: [UInt16]
    /// What this pane's session has spawned, when a session was detected in
    /// it. Absent means no agent here, which is why nothing is drawn.
    let children: CorePaneChildren?
    /// The pane's ancestors, root first, each carrying that layer's siblings.
    let lineagePath: [CoreLineageStep]

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
        case requiresCloseStatusCheck = "requires_close_status_check"
        case identityLabel = "identity_label"
        case activityAt = "activity_at_unix_ms"
        case children
        case lineagePath = "lineage_path"
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
        requiresCloseStatusCheck: Bool = false,
        identityLabel: String? = nil,
        activityAt: UInt64?,
        fork: CorePaneFork = CorePaneFork(),
        ports: [UInt16] = [],
        children: CorePaneChildren? = nil,
        lineagePath: [CoreLineageStep] = []
    ) {
        self.id = id
        self.content = content
        self.herdrLabel = herdrLabel
        self.terminalTitle = terminalTitle
        self.workspaceLabel = workspaceLabel
        self.cwd = cwd
        self.statusLabel = statusLabel
        self.requiresCloseConfirmation = requiresCloseConfirmation
        self.requiresCloseStatusCheck = requiresCloseStatusCheck
        self.identityLabel = identityLabel
        self.activityAt = activityAt
        self.fork = fork
        self.ports = ports
        self.children = children
        self.lineagePath = lineagePath
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
        requiresCloseStatusCheck = try container.decodeIfPresent(
            Bool.self, forKey: .requiresCloseStatusCheck
        ) ?? false
        identityLabel = try container.decodeIfPresent(String.self, forKey: .identityLabel)
        activityAt = try container.decodeIfPresent(UInt64.self, forKey: .activityAt)
        fork = try container.decodeIfPresent(CorePaneFork.self, forKey: .fork) ?? CorePaneFork()
        ports = try container.decodeIfPresent([UInt16].self, forKey: .ports) ?? []
        children = try container.decodeIfPresent(CorePaneChildren.self, forKey: .children)
        lineagePath = try container.decodeIfPresent([CoreLineageStep].self, forKey: .lineagePath) ?? []
    }
}

/// One agent as any line of agents draws it: a pane header chip, a breadcrumb
/// step's sibling, or an Overview worktree row's agent.
struct CoreAgentChip: Decodable, Equatable, Identifiable {
    var id: String { paneID }
    let paneID: String
    let label: String
    /// The row's second line as the core chose it: the sentence, or nothing.
    let detail: String?
    /// Whether the status word is drawn beside that sentence.
    let statusWordVisible: Bool
    let agentKind: String
    let demand: String
    let activity: String
    let emphasized: Bool
    let symbol: String
    let statusLabel: String
    let delegated: Bool

    /// The tooltip and accessibility description: the status word as the
    /// surface resolved it (a disconnected server says so), then the
    /// sentence when there is one.
    func description(status: AgentStatusPresentation) -> String {
        [status.label, detail].compactMap { $0 }.joined(separator: ". ")
    }

    enum CodingKeys: String, CodingKey {
        case paneID = "pane_id"
        case label
        case detail
        case statusWordVisible = "status_word_visible"
        case agentKind = "agent_kind"
        case demand
        case activity
        case emphasized
        case symbol
        case statusLabel = "status_label"
        case delegated
    }

    init(
        paneID: String,
        label: String,
        detail: String? = nil,
        statusWordVisible: Bool = true,
        agentKind: String = "claude",
        demand: String = "none",
        activity: String = "working",
        emphasized: Bool = false,
        symbol: String = "\u{25cf}",
        statusLabel: String = "Working",
        delegated: Bool = true
    ) {
        self.paneID = paneID
        self.label = label
        self.detail = detail
        self.statusWordVisible = statusWordVisible
        self.agentKind = agentKind
        self.demand = demand
        self.activity = activity
        self.emphasized = emphasized
        self.symbol = symbol
        self.statusLabel = statusLabel
        self.delegated = delegated
    }

    init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        paneID = try container.decode(String.self, forKey: .paneID)
        label = try container.decode(String.self, forKey: .label)
        detail = try container.decodeIfPresent(String.self, forKey: .detail)
        statusWordVisible = try container.decodeIfPresent(Bool.self, forKey: .statusWordVisible) ?? true
        agentKind = try container.decode(String.self, forKey: .agentKind)
        demand = try container.decode(String.self, forKey: .demand)
        activity = try container.decode(String.self, forKey: .activity)
        emphasized = try container.decode(Bool.self, forKey: .emphasized)
        symbol = try container.decode(String.self, forKey: .symbol)
        statusLabel = try container.decode(String.self, forKey: .statusLabel)
        delegated = try container.decodeIfPresent(Bool.self, forKey: .delegated) ?? false
    }
}

/// What one pane's session has spawned, and why that is or is not knowable.
struct CorePaneChildren: Decodable, Equatable {
    let instrumented: Bool
    /// The whole explanation, present exactly when `instrumented` is false.
    let uninstrumentedReason: String?
    /// The accessible name for the mark, so the symbol never carries the
    /// meaning by itself.
    let uninstrumentedLabel: String?
    /// The reason's stable name, for a view that keys on the cause rather
    /// than on its sentence.
    let uninstrumentedCode: String?
    /// One chip per pane child, in the lineage's own child order.
    let chips: [CoreAgentChip]
    /// The one child the parent's badge speaks for.
    let representative: CoreAgentChip?
    let subagents: CoreSubagentCounts

    enum CodingKeys: String, CodingKey {
        case instrumented
        case uninstrumentedReason = "uninstrumented_reason"
        case uninstrumentedLabel = "uninstrumented_label"
        case uninstrumentedCode = "uninstrumented_code"
        case chips
        case representative
        case subagents
    }

    init(
        instrumented: Bool = true,
        uninstrumentedReason: String? = nil,
        uninstrumentedLabel: String? = nil,
        uninstrumentedCode: String? = nil,
        chips: [CoreAgentChip] = [],
        representative: CoreAgentChip? = nil,
        subagents: CoreSubagentCounts = CoreSubagentCounts()
    ) {
        self.instrumented = instrumented
        self.uninstrumentedReason = uninstrumentedReason
        self.uninstrumentedLabel = uninstrumentedLabel
        self.uninstrumentedCode = uninstrumentedCode
        self.chips = chips
        self.representative = representative
        self.subagents = subagents
    }

    init(from decoder: any Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        instrumented = try container.decodeIfPresent(Bool.self, forKey: .instrumented) ?? false
        uninstrumentedReason = try container.decodeIfPresent(String.self, forKey: .uninstrumentedReason)
        uninstrumentedLabel = try container.decodeIfPresent(String.self, forKey: .uninstrumentedLabel)
        uninstrumentedCode = try container.decodeIfPresent(String.self, forKey: .uninstrumentedCode)
        chips = try container.decodeIfPresent([CoreAgentChip].self, forKey: .chips) ?? []
        representative = try container.decodeIfPresent(CoreAgentChip.self, forKey: .representative)
        subagents = try container.decodeIfPresent(CoreSubagentCounts.self, forKey: .subagents)
            ?? CoreSubagentCounts()
    }
}

/// The in-process subagents a session reports.
///
/// Each count is separately knowable, and one the adapter cannot observe stays
/// absent. Absent draws as unknown and never as zero, because a zero claims
/// the agent is working alone.
struct CoreSubagentCounts: Decodable, Equatable {
    let working: UInt32?
    let done: UInt32?
    let blocked: UInt32?

    /// Nothing to draw: every count is unknown.
    var isSilent: Bool { working == nil && done == nil && blocked == nil }

    init(working: UInt32? = nil, done: UInt32? = nil, blocked: UInt32? = nil) {
        self.working = working
        self.done = done
        self.blocked = blocked
    }
}

/// One step of a pane's breadcrumb, carrying that layer's siblings.
struct CoreLineageStep: Decodable, Equatable, Identifiable {
    var id: String { paneID }
    let paneID: String
    let label: String
    /// Everything at this layer, including the step itself, for the step's
    /// dropdown. A root has none, so it draws no chevron.
    let siblings: [CoreAgentChip]

    enum CodingKeys: String, CodingKey {
        case paneID = "pane_id"
        case label
        case siblings
    }

    init(paneID: String, label: String, siblings: [CoreAgentChip] = []) {
        self.paneID = paneID
        self.label = label
        self.siblings = siblings
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
    /// Whether the activity status must be refreshed before closing this pane.
    let requiresCloseStatusCheck: Bool
    /// The stable name every surface calls this agent by (PRD D-01).
    let identityLabel: String
    /// The label plugin's rolling task title; the search sheet's subtitle
    /// when the state chose no sentence (PRD D-15).
    let task: String?
    /// The second line the core chose for this row's state, or nothing.
    let detail: String?
    /// Whether the status word is drawn on the second line.
    let statusWordVisible: Bool
    let elapsed: String
    let lastActivity: String

    var lineageDepth: Int = 0
    var lineageChildPaneIDs: [String] = []
    var lineageRootCheckoutID: String? = nil
    var lineageWorktreeBadge: String? = nil
    var lineageOrphan: Bool = false
    var lineageHint: String? = nil
    var raisedHint: String? = nil
    var spawnOriginPaneID: String? = nil
    var lineageCollapsed: Bool = false
    /// Whether this row is somebody else's work. It is derived from the
    /// lineage by the core, and it lifts when a stall hands the child back.
    var delegated: Bool = false
    /// How long a delegated descendant has been waiting: empty, `soft`, or
    /// `hard`. It is set on the lineage root, not on the stalled child.
    var stallLevel: String = ""
    /// The sentence naming that descendant and what it is waiting on.
    var stallNotice: String? = nil

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
        requiresCloseStatusCheck: Bool = false,
        identityLabel: String,
        task: String? = nil,
        detail: String? = nil,
        statusWordVisible: Bool = true,
        elapsed: String,
        lastActivity: String
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
        self.requiresCloseStatusCheck = requiresCloseStatusCheck
        self.identityLabel = identityLabel
        self.task = task
        self.detail = detail
        self.statusWordVisible = statusWordVisible
        self.elapsed = elapsed
        self.lastActivity = lastActivity
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
        requiresCloseStatusCheck = try container.decodeIfPresent(Bool.self, forKey: .requiresCloseStatusCheck) ?? false
        identityLabel = try container.decode(String.self, forKey: .identityLabel)
        task = try container.decodeIfPresent(String.self, forKey: .task)
        detail = try container.decodeIfPresent(String.self, forKey: .detail)
        statusWordVisible = try container.decodeIfPresent(Bool.self, forKey: .statusWordVisible) ?? true
        elapsed = try container.decode(String.self, forKey: .elapsed)
        lastActivity = try container.decode(String.self, forKey: .lastActivity)
        lineageDepth = try container.decodeIfPresent(Int.self, forKey: .lineageDepth) ?? 0
        lineageChildPaneIDs = try container.decodeIfPresent([String].self, forKey: .lineageChildPaneIDs) ?? []
        lineageRootCheckoutID = try container.decodeIfPresent(String.self, forKey: .lineageRootCheckoutID)
        lineageWorktreeBadge = try container.decodeIfPresent(String.self, forKey: .lineageWorktreeBadge)
        lineageOrphan = try container.decodeIfPresent(Bool.self, forKey: .lineageOrphan) ?? false
        lineageHint = try container.decodeIfPresent(String.self, forKey: .lineageHint)
        raisedHint = try container.decodeIfPresent(String.self, forKey: .raisedHint)
        spawnOriginPaneID = try container.decodeIfPresent(String.self, forKey: .spawnOriginPaneID)
        lineageCollapsed = try container.decodeIfPresent(Bool.self, forKey: .lineageCollapsed) ?? false
        delegated = try container.decodeIfPresent(Bool.self, forKey: .delegated) ?? false
        stallLevel = try container.decodeIfPresent(String.self, forKey: .stallLevel) ?? ""
        stallNotice = try container.decodeIfPresent(String.self, forKey: .stallNotice)
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
        case requiresCloseStatusCheck = "requires_close_status_check"
        case identityLabel = "identity_label"
        case task
        case detail
        case statusWordVisible = "status_word_visible"
        case elapsed
        case lastActivity = "last_activity"
        case lineageDepth = "lineage_depth"
        case lineageChildPaneIDs = "lineage_child_pane_ids"
        case lineageRootCheckoutID = "lineage_root_checkout_id"
        case lineageWorktreeBadge = "lineage_worktree_badge"
        case lineageOrphan = "lineage_orphan"
        case lineageHint = "lineage_hint"
        case raisedHint = "raised_hint"
        case spawnOriginPaneID = "spawn_origin_pane_id"
        case lineageCollapsed = "lineage_collapsed"
        case delegated
        case stallLevel = "stall_level"
        case stallNotice = "stall_notice"
    }
}

