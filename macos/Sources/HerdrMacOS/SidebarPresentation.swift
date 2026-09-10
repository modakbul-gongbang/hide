import Foundation

struct SidebarCheckoutPresentation: Equatable {
    let agentCount: Int
    let isPrimary: Bool
    let status: AgentStatusPresentation?
    let representativeAgentKind: String?
    let isDetached: Bool
    let detailTooltip: String

    init(
        workspace: CoreWorkspaceSnapshot,
        checkout: CoreCheckoutSnapshot,
        agents: [SidebarAgent],
        connected: Bool = true
    ) {
        let summary = checkout.agentSummary
        agentCount = summary.total
        isPrimary = !checkout.isWorktree && checkout.path == workspace.path
        isDetached = checkout.worktree.map { $0.branch == nil } ?? false
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
        if summary.total == 0 {
            detailTooltip = pathDetail
        } else if !connected {
            detailTooltip = "Disconnected · agent activity unavailable\n\(pathDetail)"
        } else {
            let counts = [("Needs You", summary.needsYou), ("Done", summary.done),
                          ("Working", summary.working), ("Seen", summary.seen)]
                .filter { $0.1 > 0 }.map { "\($0.0): \($0.1)" }.joined(separator: " · ")
            let unknown = summary.unknown > 0 ? " (\(summary.unknown) Unknown)" : ""
            detailTooltip = "\(counts)\(unknown)\n\(pathDetail)"
        }
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
