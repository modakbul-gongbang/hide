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
            agent("p1", group: "working", ambient: ambient),
            agent("p2", group: "done"),
            agent("p3", group: "seen"),
            agent("p4", group: "needs_you", demand: "error"),
            agent("p5", group: "needs_you", demand: "question"),
            agent("p6", group: "needs_you", demand: "approval"),
        ]
        let projection = PetDashboardProjector.project(
            agents: agents,
            workspaces: [workspace("w1", label: "Workspace A", paneIDs: ["p1", "p2", "p3", "p4"])],
            connection: "connected",
            connectionMessage: nil
        )

        // R6, AC9: the tiles count the sidebar's four groups, so Needs You
        // holds the error alongside the question and the approval.
        #expect(projection.counts == PetDashboardCounts(
            total: 6,
            needsYou: 3,
            done: 1,
            working: 1,
            seen: 1,
            disconnected: 0
        ))
        #expect(projection.groups.map(\.id) == ["w1", "unassigned-agents"])
        #expect(projection.groups[0].agents.map(\.paneID) == ["p1", "p2", "p3", "p4"])
        #expect(projection.groups[1].agents.map(\.paneID) == ["p5", "p6"])
        #expect(projection.groups[0].agents[0].ambient == ambient)
        #expect(projection.groups[0].agents[1].ambient == nil)
        #expect(projection.groups[0].agents[1].group == .done)
        #expect(projection.groups[0].agents[1].statusLabel == "Done")
        #expect(projection.groups[1].agents[0].group == .needsYou)
        #expect(projection.groups[1].agents[1].demand == "approval")

        let rowFields = Set(Mirror(reflecting: projection.groups[0].agents[0]).children.compactMap(\.label))
        #expect(rowFields == [
            "id", "paneID", "agentKind", "group", "demand", "activity",
            "emphasized", "symbol", "statusLabel", "summary", "elapsed",
            "connection", "ambient",
        ])
    }

    @Test func disconnectedRowsHideStaleAmbientInsteadOfInventingZeroes() {
        let projection = PetDashboardProjector.project(
            agents: [
                agent(
                    "p1",
                    group: "working",
                    ambient: CoreAmbientSignal(
                        subagentsActive: 3,
                        backgroundRunning: 2,
                        backgroundFailed: 1
                    )
                ),
                agent("p2", group: "seen"),
            ],
            workspaces: [workspace("w1", label: "Workspace A", paneIDs: ["p1", "p2"])],
            connection: "unavailable",
            connectionMessage: "Herdr server is not answering"
        )

        // AC9: a server that stopped answering counts nothing as waiting.
        #expect(projection.counts == PetDashboardCounts(
            total: 2,
            needsYou: 0,
            done: 0,
            working: 0,
            seen: 0,
            disconnected: 2
        ))
        #expect(projection.groups[0].agents.allSatisfy { $0.statusLabel == "Disconnected" })
        #expect(projection.groups[0].agents.allSatisfy { $0.connection == "disconnected" })
        #expect(projection.groups[0].agents.allSatisfy { $0.ambient == nil })
        #expect(projection.connectionMessage == "Herdr server is not answering")
    }

    @Test func duplicatePaneProjectionIsIdempotentAndEmptySnapshotStaysEmpty() {
        let duplicate = PetDashboardProjector.project(
            agents: [agent("p1", group: "working")],
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
            needsYou: 0,
            done: 0,
            working: 0,
            seen: 0,
            disconnected: 0
        ))
        #expect(empty.groups.isEmpty)
    }

    /// R6, SC5. The dashboard tiles and the sidebar sections read the same
    /// group off the same rows, so a count on one surface cannot disagree with
    /// the section on the other. This is the class of bug the dashboard's own
    /// buckets used to cause.
    @Test func dashboardTilesMatchTheSidebarSections() {
        let agents = [
            agent("p1", group: "working"),
            agent("p2", group: "done"),
            agent("p3", group: "done"),
            agent("p4", group: "seen"),
            agent("p5", group: "needs_you", demand: "error"),
            agent("p6", group: "needs_you", demand: "question"),
        ]
        let projection = PetDashboardProjector.project(
            agents: agents,
            workspaces: [
                workspace(
                    "w1",
                    label: "Workspace A",
                    paneIDs: ["p1", "p2", "p3", "p4", "p5", "p6"]
                )
            ],
            connection: "connected",
            connectionMessage: nil
        )
        let sidebar = { (group: AgentGroup) in SidebarGrouping.agents(agents, in: group).count }

        #expect(projection.counts.needsYou == sidebar(.needsYou))
        #expect(projection.counts.done == sidebar(.done))
        #expect(projection.counts.working == sidebar(.working))
        #expect(projection.counts.seen == sidebar(.seen))
        #expect(projection.counts.total == agents.count)
    }

    private func agent(
        _ paneID: String,
        group: String,
        demand: String = "none",
        ambient: CoreAmbientSignal? = nil
    ) -> SidebarAgent {
        SidebarAgent(
            id: "agent-\(paneID)",
            paneID: paneID,
            workspaceLabel: "Workspace A",
            agentKind: paneID == "p2" ? "claude" : "codex",
            demand: demand,
            activity: group == "working" ? "working" : "stopped",
            unread: group == "needs_you" || group == "done",
            group: group,
            symbol: "-",
            emphasized: group == "needs_you" || group == "done",
            statusLabel: group == "done" ? "Done" : "Idle",
            summary: "Agent \(paneID)",
            elapsed: "2m",
            lastActivity: "0000000000001",
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
                cwd: "/tmp/\(id)",
                statusLabel: "Idle",
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
