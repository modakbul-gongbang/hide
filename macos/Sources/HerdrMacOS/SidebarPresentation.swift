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
