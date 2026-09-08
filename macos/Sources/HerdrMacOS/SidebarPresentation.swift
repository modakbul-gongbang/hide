import Foundation

enum SidebarCheckoutActivity: Equatable {
    case missing
    case error
    case needsAttention
    case working
    case idle
    case empty
}

struct SidebarCheckoutPresentation: Equatable {
    let paneCount: Int
    let agentCount: Int
    let isPrimary: Bool
    let activity: SidebarCheckoutActivity
    let isDetached: Bool
    let detailTooltip: String

    var activityLabel: String? {
        if agentCount > 0 {
            return Self.countLabel(agentCount, singular: "agent")
        }
        if paneCount > 0 {
            return Self.countLabel(paneCount, singular: "pane")
        }
        return nil
    }

    init(
        workspace: CoreWorkspaceSnapshot,
        checkout: CoreCheckoutSnapshot,
        agents: [SidebarAgent]
    ) {
        let checkoutAgents = SidebarGrouping.agents(agents, in: checkout)
        paneCount = checkout.tabs.reduce(0) { $0 + $1.panes.count }
        agentCount = checkoutAgents.count
        isPrimary = !checkout.isWorktree && checkout.path == workspace.path
        isDetached = checkout.worktree.map { $0.branch == nil } ?? false
        if isDetached {
            let commit = checkout.worktree?.headSHA.map { " at \($0)" } ?? ""
            detailTooltip = "Detached HEAD\(commit)\n\(checkout.path)"
        } else {
            detailTooltip = checkout.branch.map { "\($0)\n\(checkout.path)" } ?? checkout.path
        }

        if !checkout.exists {
            activity = .missing
        } else if checkoutAgents.contains(where: { $0.demand == "error" }) {
            activity = .error
        } else if checkoutAgents.contains(where: { AgentGroup(agent: $0) == .needsYou }) {
            activity = .needsAttention
        } else if checkoutAgents.contains(where: { AgentGroup(agent: $0) == .working }) {
            activity = .working
        } else if paneCount > 0 {
            activity = .idle
        } else {
            activity = .empty
        }
    }

    private static func countLabel(_ count: Int, singular: String) -> String {
        count == 1 ? "1 \(singular)" : "\(count) \(singular)s"
    }
}

struct SidebarWorkspacePresentation: Equatable {
    let checkoutCount: Int
    let paneCount: Int
    let agentCount: Int

    var activityLabel: String {
        if agentCount > 0 {
            return agentCount == 1 ? "1 agent" : "\(agentCount) agents"
        }
        return checkoutCount == 1 ? "1 workspace" : "\(checkoutCount) workspaces"
    }

    init(workspace: CoreWorkspaceSnapshot, agents: [SidebarAgent]) {
        let paneIDs = Set(
            workspace.checkouts
                .flatMap(\.tabs)
                .flatMap(\.panes)
                .map(\.id)
        )
        checkoutCount = workspace.checkouts.count
        paneCount = paneIDs.count
        agentCount = agents.filter { paneIDs.contains($0.paneID) }.count
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
/// position in the list the visible view shows, so ⌃1 always reaches the first
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
        collapsedCheckoutIDs: Set<String> = []
    ) -> [SidebarAgent] {
        switch content {
        case .agents:
            return agents
        case .projects:
            let raised = SidebarGrouping.raised(agents).flatMap(\.agents)
            let raisedIDs = Set(raised.map(\.id))
            let visible = raised + visibleCheckoutIDs.filter { !collapsedCheckoutIDs.contains($0) }.flatMap {
                SidebarGrouping.tree(agents, checkoutID: $0, excluding: raisedIDs)
            }
            var numberedPanes = Set<String>()
            return visible.filter { numberedPanes.insert($0.paneID).inserted }
        }
    }

    /// Only the first nine agents get a number: ⌃0 is not a tenth slot, it is
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
