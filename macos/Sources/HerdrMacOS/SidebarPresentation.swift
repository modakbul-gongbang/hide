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

        if !checkout.exists {
            activity = .missing
        } else if checkoutAgents.contains(where: { $0.state == "error" }) {
            activity = .error
        } else if checkoutAgents.contains(where: {
            SidebarGrouping.attentionStates.contains($0.state)
        }) {
            activity = .needsAttention
        } else if checkoutAgents.contains(where: { $0.state == "working" }) {
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
        return checkoutCount == 1 ? "1 checkout" : "\(checkoutCount) checkouts"
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

/// Direct-select numbering for the sidebar agent list. The number is a
/// property of the agent's position in the runtime's own agent projection, not
/// of whichever sidebar view is showing, so the Projects view and the Agents
/// view label the same agent identically.
enum AgentShortcutNumbering {
    /// Only the first nine agents get a number: ⌘0 is not a tenth slot, it is
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
