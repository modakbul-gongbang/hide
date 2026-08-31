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
    let status: String
    let summary: String
    let elapsed: String
    let unseen: Bool
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
            let agentConnected = connected && agent.state != "disconnected"
            let status = agentConnected ? agent.state : "disconnected"
            rowsByPaneID[agent.paneID] = PetDashboardRow(
                id: agent.paneID,
                paneID: agent.paneID,
                agentKind: agent.agentKind,
                status: status,
                summary: agent.summary,
                elapsed: agent.elapsed,
                unseen: isUnseen(agent.state),
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
            working: rows.count { statusBucket($0.status) == "working" },
            done: rows.count { statusBucket($0.status) == "done" },
            idle: rows.count { statusBucket($0.status) == "idle" },
            error: rows.count { statusBucket($0.status) == "error" },
            disconnected: rows.count { statusBucket($0.status) == "disconnected" }
        )
        return PetDashboardProjection(
            counts: counts,
            groups: groups,
            connection: connection,
            connectionMessage: connectionMessage
        )
    }

    private static func statusBucket(_ state: String) -> String {
        switch state {
        case "working": "working"
        case "done", "unseen_completion": "done"
        case "error": "error"
        case "disconnected": "disconnected"
        default: "idle"
        }
    }

    private static func isUnseen(_ state: String) -> Bool {
        ["question", "approval", "error", "unseen_completion"].contains(state)
    }
}
