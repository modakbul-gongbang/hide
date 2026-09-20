import Foundation

enum SidebarProjectRow: Identifiable {
    /// A section header drawn as a list row, so a project row keeps one
    /// identity whether it sits under `Pinned` or under the activity list
    /// and the list animates a pin as a move rather than a removal and an
    /// insertion that leaves the list scrolled past the new header.
    case header(title: String, count: Int)
    case workspace(CoreWorkspaceSnapshot, level: SidebarHierarchyLevel)
    case inactiveProjects(CoreInactiveProjectGroupSnapshot, [CoreWorkspaceSnapshot])

    var id: String {
        switch self {
        case .header(let title, _):
            "header:\(title)"
        case .workspace(let workspace, _):
            "workspace:\(workspace.id)"
        case .inactiveProjects(let group, _):
            "inactive-projects:\(group.deviceID)"
        }
    }
}

/// The project tree's semantic indentation ladder. Each level advances on
/// the same spacing rhythm, while the selected-row surface begins one small
/// inset before its content so the background keeps the child relationship.
enum SidebarHierarchyLevel: Equatable {
    case root
    case child
    case grandchild
    case greatGrandchild

    var childLevel: SidebarHierarchyLevel {
        switch self {
        case .root: .child
        case .child: .grandchild
        case .grandchild, .greatGrandchild: .greatGrandchild
        }
    }

    var contentLeadingInset: CGFloat {
        switch self {
        case .root: HideTheme.spacingMD
        case .child: HideTheme.spacingXL
        case .grandchild: HideTheme.sidebarHierarchyGrandchildInset
        case .greatGrandchild: HideTheme.sidebarHierarchyGreatGrandchildInset
        }
    }

    var selectionLeadingInset: CGFloat {
        contentLeadingInset - HideTheme.spacingSM
    }
}

/// The Projects list as the sidebar draws it: the pinned rows under their own
/// section, then the activity rows with the device folds. Both halves keep
/// the core's order; the split only reads the flag the core set (D-02).
struct SidebarProjectSections {
    static let pinnedTitle = "Pinned"
    static let recentTitle = "Projects · Recent activity"

    let pinned: [CoreWorkspaceSnapshot]
    /// Every project the `Pinned` section does not show, folded or not: the
    /// `Projects · Recent activity` count.
    let recent: [CoreWorkspaceSnapshot]
    /// The list in drawing order: the `Pinned` header and its rows only
    /// while a project is pinned, then the activity header, its rows and
    /// the device folds.
    let rows: [SidebarProjectRow]

    init(_ workspaces: [CoreWorkspaceSnapshot], groups: [CoreInactiveProjectGroupSnapshot]) {
        pinned = workspaces.filter(\.pinned)
        recent = workspaces.filter { !$0.pinned }
        var rows: [SidebarProjectRow] = []
        if !pinned.isEmpty {
            rows.append(.header(title: Self.pinnedTitle, count: pinned.count))
            rows.append(contentsOf: pinned.map { .workspace($0, level: .root) })
        }
        rows.append(.header(title: Self.recentTitle, count: recent.count))
        rows.append(contentsOf: SidebarInactiveProjection.projectRows(recent, groups: groups))
        self.rows = rows
    }
}

/// The `Remove project…` confirmation (D-10). The counts are the core's; the
/// copy names them so the button says what confirming does.
struct WorkspaceRemovalPrompt: Equatable {
    let title: String
    let message: String
    let confirmLabel: String

    init(label: String, removal: CoreWorkspaceRemovalGateSnapshot) {
        title = "Remove \(label) from Hide?"
        if removal.paneCount == 0 {
            message = "Hide will remove only its registration. The folder, repository, worktrees, and running processes stay untouched."
            confirmLabel = "Remove registration"
        } else {
            let panes = Self.count(removal.paneCount, "pane")
            let agents = removal.runningAgentCount > 0
                ? " (\(Self.count(removal.runningAgentCount, "running agent")))"
                : ""
            message = "Closes \(panes)\(agents). The folder, repository, and worktrees stay on disk."
            confirmLabel = "Close \(panes) and remove"
        }
    }

    private static func count(_ value: Int, _ noun: String) -> String {
        "\(value) \(noun)\(value == 1 ? "" : "s")"
    }
}

/// Maps core-owned inactive group IDs back to the authoritative rows. This is
/// presentation only: no merge, age, or exception rule is repeated here.
enum SidebarInactiveProjection {
    static func projectRows(
        _ workspaces: [CoreWorkspaceSnapshot],
        groups: [CoreInactiveProjectGroupSnapshot]
    ) -> [SidebarProjectRow] {
        let byID = Dictionary(uniqueKeysWithValues: workspaces.map { ($0.id, $0) })
        let groupsByDevice = Dictionary(uniqueKeysWithValues: groups.map { ($0.deviceID, $0) })
        var deviceIDs: [String] = []
        var seenDevices = Set<String>()
        for workspace in workspaces where seenDevices.insert(workspace.deviceID).inserted {
            deviceIDs.append(workspace.deviceID)
        }

        return deviceIDs.flatMap { deviceID -> [SidebarProjectRow] in
            let group = groupsByDevice[deviceID]
            let inactiveIDs = Set(group?.projectIDs ?? [])
            var rows = workspaces
                .filter { $0.deviceID == deviceID && !inactiveIDs.contains($0.id) }
                .map { SidebarProjectRow.workspace($0, level: .root) }
            guard let group else { return rows }
            let inactive = group.projectIDs.compactMap { byID[$0] }
            guard !inactive.isEmpty else { return rows }
            rows.append(.inactiveProjects(group, inactive))
            if group.expanded {
                rows.append(contentsOf: inactive.map { SidebarProjectRow.workspace($0, level: .child) })
            }
            return rows
        }
    }

    static func activeCheckouts(in workspace: CoreWorkspaceSnapshot) -> [CoreCheckoutSnapshot] {
        let inactive = Set(workspace.inactiveCheckouts.checkoutIDs)
        return workspace.checkouts.filter { !inactive.contains($0.id) }
    }

    static func inactiveCheckouts(in workspace: CoreWorkspaceSnapshot) -> [CoreCheckoutSnapshot] {
        let byID = Dictionary(uniqueKeysWithValues: workspace.checkouts.map { ($0.id, $0) })
        return workspace.inactiveCheckouts.checkoutIDs.compactMap { byID[$0] }
    }
}

struct SidebarCheckoutPresentation: Equatable {
    let agentCount: Int
    let isPrimary: Bool
    let status: AgentStatusPresentation?
    let representativeAgentKind: String?
    let isDetached: Bool
    let secondLine: String?
    let lastCommitAge: String?
    let kindStage: String
    let kindSystemImage: String
    let showsPullRequestGlyph: Bool
    let kindMuted: Bool
    let kindDanger: Bool
    let rowDimmed: Bool
    let detailTooltip: String
    let accessibilityLabel: String

    func showsSecondLine(expanded: Bool) -> Bool {
        !expanded && canShowSecondLine && (agentCount > 0 || secondLine != nil)
    }

    private let canShowSecondLine: Bool

    init(
        workspace: CoreWorkspaceSnapshot,
        checkout: CoreCheckoutSnapshot,
        agents: [SidebarAgent],
        connected: Bool = true,
        now: Date = Date()
    ) {
        let summary = checkout.agentSummary
        agentCount = summary.total
        isPrimary = !checkout.isWorktree && checkout.path == workspace.path
        isDetached = checkout.worktree.map { $0.branch == nil } ?? false
        secondLine = checkout.purpose?.text
        let gitLoading = workspace.isGit && checkout.worktree == nil
        canShowSecondLine = !gitLoading
        lastCommitAge = checkout.exists && !gitLoading
            ? RelativeActivityToken.token(
                unixMS: checkout.worktree?.lastCommitUnixSeconds.map { UInt64(max(0, $0 * 1000)) },
                now: now
            )
            : nil
        let request = checkout.pullRequest
        showsPullRequestGlyph = request != nil && checkout.github.unavailableReason == nil
        if showsPullRequestGlyph, let request {
            kindStage = OverviewPresentation.pullRequestLabel(request)
            kindSystemImage = HideTheme.gitPullRequestIcon
        } else if !workspace.isGit {
            kindStage = "Folder"
            kindSystemImage = "folder"
        } else if isPrimary {
            kindStage = "Primary checkout"
            kindSystemImage = "house"
        } else if isDetached {
            kindStage = "Detached commit"
            kindSystemImage = "point.3.connected.trianglepath.dotted"
        } else {
            kindStage = "Branch"
            kindSystemImage = "arrow.triangle.branch"
        }
        kindMuted = checkout.github.stale || request?.isDraft == true
        kindDanger = !checkout.exists
        rowDimmed = request?.badge.isSettled == true
        let pathDetail: String
        if isDetached {
            let commit = checkout.worktree?.headSHA.map { " at \($0)" } ?? ""
            pathDetail = "Detached HEAD\(commit)\n\(checkout.path)"
        } else {
            pathDetail = checkout.branch.map { "\($0)\n\(checkout.path)" } ?? checkout.path
        }
        if let representative = agents.first(where: { $0.paneID == summary.representativePaneID }) {
            status = AgentStatusPresentation(agent: representative, connected: connected)
            representativeAgentKind = representative.agentKind
        } else {
            status = nil
            representativeAgentKind = nil
        }
        var tooltipLines: [String] = []
        if let request, showsPullRequestGlyph {
            var first = "#\(request.number) · \(OverviewPresentation.pullRequestLabel(request))"
            if let title = request.title, !title.isEmpty { first += " · \(title)" }
            if checkout.github.stale,
               let last = checkout.github.lastSuccessAtUnixMS {
                let age = RelativeActivityToken.token(unixMS: UInt64(max(0, last)), now: now) ?? "now"
                first += " · Last known \(age)"
            }
            tooltipLines.append(first)
        } else if let reason = checkout.github.unavailableReason {
            tooltipLines.append(reason)
        }
        if summary.total == 0 {
            tooltipLines.append(pathDetail)
        } else if !connected {
            tooltipLines.append("Disconnected · agent activity unavailable")
            tooltipLines.append(pathDetail)
        } else {
            let counts = [("Needs You", summary.needsYou), ("Done", summary.done),
                          ("Working", summary.working), ("Seen", summary.seen)]
                .filter { $0.1 > 0 }.map { "\($0.0): \($0.1)" }.joined(separator: " · ")
            let unknown = summary.unknown > 0 ? " (\(summary.unknown) Unknown)" : ""
            tooltipLines.append("\(counts)\(unknown)")
            tooltipLines.append(pathDetail)
        }
        if let created = checkout.worktree?.createdAtUnixMS {
            tooltipLines.append("Created \(CheckoutCardPresentation.relativeAge(fromUnixMS: created, now: now))")
        }
        detailTooltip = tooltipLines.joined(separator: "\n")
        accessibilityLabel = [checkout.label, kindStage, lastCommitAge, secondLine]
            .compactMap { $0 }
            .joined(separator: ", ")
    }
}

/// How long ago something happened, written the way this app already writes
/// elapsed time: one token of digits and a unit. Herdr's own `elapsed` token
/// is `<digits><s|m|h|d>` and the agent rows draw it unchanged, so a project
/// row saying the same thing reads as the same kind of fact.
///
/// The first minute is "now" rather than a second counter: a project row is
/// read at a glance, and a number that changes every second there is motion
/// without information. A timestamp ahead of this machine's clock is also
/// "now", because the alternative is a negative age.
enum RelativeActivityToken {
    static func token(unixMS: UInt64?, now: Date) -> String? {
        guard let unixMS else { return nil }
        let seconds = now.timeIntervalSince1970 - Double(unixMS) / 1000
        guard seconds >= 60 else { return "now" }
        let minutes = Int(seconds / 60)
        if minutes < 60 { return "\(minutes)m" }
        let hours = minutes / 60
        if hours < 24 { return "\(hours)h" }
        return "\(hours / 24)d"
    }
}

struct SidebarWorkspacePresentation: Equatable {
    let checkoutCount: Int
    let paneCount: Int
    let agentCount: Int
    /// The relative time since this project's newest activity, absent when the
    /// core reported none. The core owns which activity that is; the row only
    /// says how long ago it was.
    let lastActivity: String?

    /// What the row already said, plus the recency the order was decided by.
    /// The count stays first because it names the project's own contents; the
    /// time is the reason this project sits where it does.
    var activityLabel: String {
        let counts: String
        if agentCount > 0 {
            counts = agentCount == 1 ? "1 agent" : "\(agentCount) agents"
        } else {
            counts = checkoutCount == 1 ? "1 workspace" : "\(checkoutCount) workspaces"
        }
        guard let lastActivity else { return counts }
        return "\(counts) · \(lastActivity)"
    }

    init(workspace: CoreWorkspaceSnapshot, agents: [SidebarAgent], now: Date = Date()) {
        let paneIDs = Set(
            workspace.checkouts
                .flatMap(\.tabs)
                .flatMap(\.panes)
                .map(\.id)
        )
        checkoutCount = workspace.checkouts.count
        paneCount = paneIDs.count
        agentCount = agents.filter { paneIDs.contains($0.paneID) }.count
        lastActivity = RelativeActivityToken.token(unixMS: workspace.lastActivityUnixMS, now: now)
    }
}

extension SidebarAgent {
    /// Where the agent runs, as the project tree names it: the project and,
    /// when the core has placed the pane, its checkout. The Agents view and
    /// the Projects view then call one agent's home by the same words.
    var contextLabel: String {
        guard let checkoutQualifier else { return workspaceLabel }
        return "\(workspaceLabel) › \(checkoutQualifier)"
    }

    /// The checkout half of `contextLabel`, present only when it names
    /// something the project name does not already say. The sidebar row draws
    /// it as its own small line so the project name stays the row's title.
    var checkoutQualifier: String? {
        guard let checkoutLabel, !checkoutLabel.isEmpty, checkoutLabel != workspaceLabel else {
            return nil
        }
        return checkoutLabel
    }
}

/// Direct-select numbering for the sidebar. The number is the agent's
/// position in the list the visible view shows, so ⌥1 always reaches the first
/// row the user can see.
///
/// The Agents view numbers the runtime's whole agent projection. The Projects
/// view numbers the Needs You and Done rows it raises to the top first, then
/// the expanded project lineage rows, which is exactly the order those
/// rows appear in.
enum AgentShortcutNumbering {
    static func candidates(
        for content: SidebarContent,
        agents: [SidebarAgent],
        visibleCheckoutIDs: [String],
        collapsedCheckoutIDs: Set<String> = [],
        ownedPaneIDsByCheckout: [String: Set<String>] = [:]
    ) -> [SidebarAgent] {
        switch content {
        case .agents:
            return agents
        case .projects:
            let raised = SidebarGrouping.raised(agents).flatMap(\.agents)
            let visible = raised + visibleCheckoutIDs.filter { !collapsedCheckoutIDs.contains($0) }.flatMap {
                SidebarGrouping.tree(agents, checkoutID: $0, ownedPaneIDs: ownedPaneIDsByCheckout[$0] ?? [])
            }
            var numberedPanes = Set<String>()
            return visible.filter { numberedPanes.insert($0.paneID).inserted }
        }
    }

    /// Only the first nine agents get a number: ⌥0 is not a tenth slot, it is
    /// a different key, and a two-digit chord is not a shortcut anyone reaches
    /// for without looking.
    static let capacity = 9

    static func number(ofPaneID paneID: String, in agents: [SidebarAgent]) -> Int? {
        guard let index = agents.firstIndex(where: { $0.paneID == paneID }),
              index < capacity
        else { return nil }
        return index + 1
    }

    static func agent(atNumber number: Int, in agents: [SidebarAgent]) -> SidebarAgent? {
        guard number >= 1, number <= capacity, number <= agents.count else { return nil }
        return agents[number - 1]
    }
}
