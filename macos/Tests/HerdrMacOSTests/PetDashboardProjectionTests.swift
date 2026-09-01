import Testing

@testable import HerdrMacOS

@Suite("Pet dashboard projection")
struct PetDashboardProjectionTests {
    @Test func mixedSnapshotStatesGroupByPaneAndExposeOnlyProvidedAmbient() {
        let ambient = CoreAmbientSignal(
            subagentsActive: 2,
            backgroundRunning: 1,
            backgroundFailed: 0
        )
        let agents = [
            agent("p1", state: "working", ambient: ambient),
            agent("p2", state: "unseen_completion"),
            agent("p3", state: "idle"),
            agent("p4", state: "error"),
            agent("p5", state: "question"),
            agent("p6", state: "blocked"),
        ]
        let projection = PetDashboardProjector.project(
            agents: agents,
            workspaces: [workspace("w1", label: "Workspace A", paneIDs: ["p1", "p2", "p3", "p4"])],
            connection: "connected",
            connectionMessage: nil
        )

        #expect(projection.counts == PetDashboardCounts(
            total: 6,
            working: 1,
            done: 1,
            idle: 3,
            error: 1,
            disconnected: 0
        ))
        #expect(projection.groups.map(\.id) == ["w1", "unassigned-agents"])
        #expect(projection.groups[0].agents.map(\.paneID) == ["p1", "p2", "p3", "p4"])
        #expect(projection.groups[1].agents.map(\.paneID) == ["p5", "p6"])
        #expect(projection.groups[0].agents[0].ambient == ambient)
        #expect(projection.groups[0].agents[1].ambient == nil)
        #expect(projection.groups[0].agents[1].status == "unseen_completion")
        #expect(projection.groups[0].agents[1].unseen)
        #expect(projection.groups[1].agents[0].status == "question")
        #expect(projection.groups[0].agents[3].unseen)
        #expect(projection.groups[1].agents[0].unseen)
        #expect(projection.groups[1].agents[1].status == "blocked")
        #expect(projection.groups[1].agents[1].unseen)

        let rowFields = Set(Mirror(reflecting: projection.groups[0].agents[0]).children.compactMap(\.label))
        #expect(rowFields == [
            "id", "paneID", "agentKind", "status", "summary", "elapsed",
            "unseen", "connection", "ambient",
        ])
    }

    @Test func disconnectedRowsHideStaleAmbientInsteadOfInventingZeroes() {
        let projection = PetDashboardProjector.project(
            agents: [
                agent(
                    "p1",
                    state: "working",
                    ambient: CoreAmbientSignal(
                        subagentsActive: 3,
                        backgroundRunning: 2,
                        backgroundFailed: 1
                    )
                ),
                agent("p2", state: "disconnected"),
            ],
            workspaces: [workspace("w1", label: "Workspace A", paneIDs: ["p1", "p2"])],
            connection: "unavailable",
            connectionMessage: "Herdr server is not answering"
        )

        #expect(projection.counts == PetDashboardCounts(
            total: 2,
            working: 0,
            done: 0,
            idle: 0,
            error: 0,
            disconnected: 2
        ))
        #expect(projection.groups[0].agents.allSatisfy { $0.status == "disconnected" })
        #expect(projection.groups[0].agents.allSatisfy { $0.connection == "disconnected" })
        #expect(projection.groups[0].agents.allSatisfy { $0.ambient == nil })
        #expect(projection.connectionMessage == "Herdr server is not answering")
    }

    @Test func duplicatePaneProjectionIsIdempotentAndEmptySnapshotStaysEmpty() {
        let duplicate = PetDashboardProjector.project(
            agents: [agent("p1", state: "working")],
            workspaces: [
                workspace("w1", label: "First", paneIDs: ["p1", "p1"]),
                workspace("w2", label: "Second", paneIDs: ["p1"]),
            ],
            connection: "connected",
            connectionMessage: nil
        )
        #expect(duplicate.counts.total == 1)
        #expect(duplicate.groups.map(\.id) == ["w1"])
        #expect(duplicate.groups[0].agents.map(\.paneID) == ["p1"])

        let empty = PetDashboardProjector.project(
            agents: [],
            workspaces: [],
            connection: "connected",
            connectionMessage: nil
        )
        #expect(empty.counts == PetDashboardCounts(
            total: 0,
            working: 0,
            done: 0,
            idle: 0,
            error: 0,
            disconnected: 0
        ))
        #expect(empty.groups.isEmpty)
    }

    private func agent(
        _ paneID: String,
        state: String,
        ambient: CoreAmbientSignal? = nil
    ) -> SidebarAgent {
        SidebarAgent(
            id: "agent-\(paneID)",
            paneID: paneID,
            workspaceLabel: "Workspace A",
            agentKind: paneID == "p2" ? "claude" : "codex",
            state: state,
            symbol: "-",
            summary: "Agent \(paneID)",
            elapsed: "2m",
            sortRank: paneID,
            activity: state,
            ambient: ambient
        )
    }

    private func workspace(
        _ id: String,
        label: String,
        paneIDs: [String]
    ) -> CoreWorkspaceSnapshot {
        let panes = paneIDs.map { paneID in
            CorePaneSnapshot(
                id: paneID,
                label: paneID,
                cwd: "/tmp/\(id)",
                state: "active",
                summary: nil,
                activityAt: nil
            )
        }
        let tab = CoreTabSnapshot(
            id: "\(id)-tab",
            workspaceID: id,
            checkoutID: "\(id)-checkout",
            label: "1",
            empty: panes.isEmpty,
            panes: panes
        )
        let checkout = CoreCheckoutSnapshot(
            id: "\(id)-checkout",
            workspaceID: id,
            label: "main",
            path: "/tmp/\(id)",
            branch: "main",
            isWorktree: false,
            exists: true,
            temporary: false,
            tabs: [tab]
        )
        return CoreWorkspaceSnapshot(
            id: id,
            label: label,
            path: "/tmp/\(id)",
            remoteTargetID: nil,
            expanded: true,
            deviceID: "local",
            repoName: label,
            isGit: true,
            defaultBranch: "main",
            registered: true,
            temporary: false,
            checkouts: [checkout]
        )
    }
}
