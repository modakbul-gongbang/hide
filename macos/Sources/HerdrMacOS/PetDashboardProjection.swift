import Foundation

struct PetDashboardCounts: Equatable {
    let total: Int
    let working: Int
    let done: Int
    let idle: Int
    let error: Int
    let disconnected: Int
}

struct PetDashboardRow: Identifiable, Equatable {
    let id: String
    let paneID: String
    let agentKind: String
    /// The core's group for this row: `needs_you`, `done`, `working`, `seen`,
    /// or `disconnected` when the server stopped answering.
    let group: String
    /// What the agent needs from the operator, for the row's mark and color.
    let demand: String
    /// Whether the agent is running.
    let activity: String
    /// Whether the row is drawn bright rather than subdued.
    let emphasized: Bool
    let symbol: String
    let statusLabel: String
    let summary: String
    let elapsed: String
    /// Whether the server that owns this row is still answering. A row from a
    /// server that stopped is drawn in the warning hue and says so in its
    /// status word; there is no second unread flag, because the bright mark
    /// and the Done word already say that.
    let connection: String
    let ambient: CoreAmbientSignal?
}

struct PetDashboardGroup: Identifiable, Equatable {
    let id: String
    let label: String
    let agents: [PetDashboardRow]
}

struct PetDashboardProjection: Equatable {
    let counts: PetDashboardCounts
    let groups: [PetDashboardGroup]
    let connection: String
    let connectionMessage: String?
}

enum PetDashboardProjector {
    static func project(
        agents: [SidebarAgent],
        workspaces: [CoreWorkspaceSnapshot],
        connection: String,
        connectionMessage: String?
    ) -> PetDashboardProjection {
        let connected = connection == "connected"
        var rowsByPaneID: [String: PetDashboardRow] = [:]
        for agent in agents {
            let agentConnected = connected
            rowsByPaneID[agent.paneID] = PetDashboardRow(
                id: agent.paneID,
                paneID: agent.paneID,
                agentKind: agent.agentKind,
                group: agentConnected ? agent.group : "disconnected",
                demand: agentConnected ? agent.demand : "none",
                activity: agentConnected ? agent.activity : "unknown",
                emphasized: agentConnected && agent.emphasized,
                symbol: agent.symbol,
                statusLabel: agentConnected ? agent.statusLabel : "Disconnected",
                summary: agent.summary,
                elapsed: agent.elapsed,
                connection: agentConnected ? "connected" : "disconnected",
                ambient: agentConnected ? agent.ambient : nil
            )
        }

        var assignedPaneIDs = Set<String>()
        var groups = workspaces.compactMap { workspace -> PetDashboardGroup? in
            let paneIDs = workspace.checkouts
                .flatMap { $0.tabs }
                .flatMap { $0.panes }
                .map { $0.id }
            let rows: [PetDashboardRow] = paneIDs.compactMap { paneID in
                guard !assignedPaneIDs.contains(paneID),
                      let row = rowsByPaneID[paneID]
                else { return nil }
                assignedPaneIDs.insert(paneID)
                return row
            }
            guard !rows.isEmpty else { return nil }
            return PetDashboardGroup(id: workspace.id, label: workspace.label, agents: rows)
        }

        let unassigned = agents.compactMap { agent -> PetDashboardRow? in
            guard !assignedPaneIDs.contains(agent.paneID) else { return nil }
            return rowsByPaneID[agent.paneID]
        }
        if !unassigned.isEmpty {
            groups.append(
                PetDashboardGroup(
                    id: "unassigned-agents",
                    label: "Other agents",
                    agents: unassigned
                )
            )
        }

        let rows = groups.flatMap { $0.agents }
        let counts = PetDashboardCounts(
            total: rows.count,
            working: rows.count { $0.group == "working" },
            done: rows.count { $0.group == "done" },
            idle: rows.count { $0.group == "seen" },
            error: rows.count { $0.group == "needs_you" && $0.demand == "error" },
            disconnected: rows.count { $0.group == "disconnected" }
        )
        return PetDashboardProjection(
            counts: counts,
            groups: groups,
            connection: connection,
            connectionMessage: connectionMessage
        )
    }

}
