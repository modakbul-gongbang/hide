import Testing

@testable import HerdrMacOS

private func presentationPane(id: String) -> CorePaneSnapshot {
    CorePaneSnapshot(
        id: id,
        cwd: "/tmp/hide",
        state: "idle",
        summary: nil,
        activityAt: nil
    )
}

private func presentationCheckout(
    id: String,
    path: String,
    isWorktree: Bool = false,
    exists: Bool = true,
    paneIDs: [String] = []
) -> CoreCheckoutSnapshot {
    CoreCheckoutSnapshot(
        id: id,
        workspaceID: "workspace-1",
        label: id,
        path: path,
        branch: id,
        isWorktree: isWorktree,
        exists: exists,
        temporary: false,
        tabs: paneIDs.isEmpty ? [] : [
            CoreTabSnapshot(
                id: "tab-\(id)",
                workspaceID: "workspace-1",
                checkoutID: id,
                label: "1",
                empty: false,
                panes: paneIDs.map(presentationPane)
            ),
        ]
    )
}

private func presentationWorkspace(
    checkouts: [CoreCheckoutSnapshot]
) -> CoreWorkspaceSnapshot {
    CoreWorkspaceSnapshot(
        id: "workspace-1",
        label: "hide",
        path: "/tmp/hide",
        remoteTargetID: nil,
        expanded: true,
        deviceID: "local",
        repoName: "hide",
        isGit: true,
        defaultBranch: "main",
        registered: true,
        temporary: false,
        checkouts: checkouts
    )
}

private func presentationAgent(
    id: String,
    paneID: String,
    state: String
) -> SidebarAgent {
    SidebarAgent(
        id: id,
        paneID: paneID,
        workspaceLabel: "hide",
        agentKind: "codex",
        state: state,
        symbol: "●",
        summary: "Agent \(id)",
        elapsed: "1m",
        sortRank: "01",
        activity: state,
        ambient: nil
    )
}

@Test func checkoutSummaryPrefersAgentCountOverPaneCount() {
    let checkout = presentationCheckout(
        id: "main",
        path: "/tmp/hide",
        paneIDs: ["pane-1", "pane-2"]
    )
    let workspace = presentationWorkspace(checkouts: [checkout])
    let agents = [presentationAgent(id: "agent-1", paneID: "pane-1", state: "working")]

    let presentation = SidebarCheckoutPresentation(
        workspace: workspace,
        checkout: checkout,
        agents: agents
    )

    #expect(presentation.activityLabel == "1 agent")
    #expect(presentation.paneCount == 2)
    #expect(presentation.activity == .working)
    #expect(presentation.isPrimary)
}

@Test func checkoutSummaryFallsBackToPanesAndKeepsMissingExplicit() {
    let checkout = presentationCheckout(
        id: "missing",
        path: "/tmp/hide.worktrees/missing",
        isWorktree: true,
        exists: false,
        paneIDs: ["pane-1", "pane-2"]
    )
    let presentation = SidebarCheckoutPresentation(
        workspace: presentationWorkspace(checkouts: [checkout]),
        checkout: checkout,
        agents: []
    )

    #expect(presentation.activityLabel == "2 panes")
    #expect(presentation.activity == .missing)
    #expect(!presentation.isPrimary)
}

@Test func workspaceSummaryCountsOnlyAgentsAttachedToItsPanes() {
    let root = presentationCheckout(id: "main", path: "/tmp/hide", paneIDs: ["pane-1"])
    let worktree = presentationCheckout(
        id: "feature",
        path: "/tmp/hide.worktrees/feature",
        isWorktree: true,
        paneIDs: ["pane-2"]
    )
    let workspace = presentationWorkspace(checkouts: [root, worktree])
    let agents = [
        presentationAgent(id: "inside", paneID: "pane-2", state: "idle"),
        presentationAgent(id: "outside", paneID: "pane-9", state: "working"),
    ]

    let presentation = SidebarWorkspacePresentation(workspace: workspace, agents: agents)

    #expect(presentation.checkoutCount == 2)
    #expect(presentation.paneCount == 2)
    #expect(presentation.agentCount == 1)
    #expect(presentation.activityLabel == "1 agent")
}

@Test func agentShortcutNumbersFollowListOrderAndStopAtNine() {
    let agents = (1...11).map {
        presentationAgent(id: "agent-\($0)", paneID: "pane-\($0)", state: "idle")
    }

    #expect(AgentShortcutNumbering.number(ofPaneID: "pane-1", in: agents) == 1)
    #expect(AgentShortcutNumbering.number(ofPaneID: "pane-9", in: agents) == 9)
    #expect(AgentShortcutNumbering.number(ofPaneID: "pane-10", in: agents) == nil)
    #expect(AgentShortcutNumbering.number(ofPaneID: "pane-absent", in: agents) == nil)
}

@Test func agentLookupByNumberRejectsSlotsPastTheList() {
    let agents = (1...3).map {
        presentationAgent(id: "agent-\($0)", paneID: "pane-\($0)", state: "idle")
    }

    #expect(AgentShortcutNumbering.agent(atNumber: 2, in: agents)?.paneID == "pane-2")
    #expect(AgentShortcutNumbering.agent(atNumber: 4, in: agents) == nil)
    #expect(AgentShortcutNumbering.agent(atNumber: 0, in: agents) == nil)
}

@Test func shortcutCandidatesFollowTheVisibleSidebarView() {
    let agents = (1...4).map {
        presentationAgent(id: "agent-\($0)", paneID: "pane-\($0)", state: "idle")
    }
    let checkout = presentationCheckout(id: "main", path: "/tmp/hide", paneIDs: ["pane-3", "pane-4"])

    let agentsView = AgentShortcutNumbering.candidates(
        for: .agents, agents: agents, focusedCheckout: checkout
    )
    let projectsView = AgentShortcutNumbering.candidates(
        for: .projects, agents: agents, focusedCheckout: checkout
    )
    let noCheckout = AgentShortcutNumbering.candidates(
        for: .projects, agents: agents, focusedCheckout: nil
    )

    #expect(agentsView.map(\.paneID) == ["pane-1", "pane-2", "pane-3", "pane-4"])
    // ⌘1 in the Projects view is the checkout's first agent, not the global first.
    #expect(projectsView.map(\.paneID) == ["pane-3", "pane-4"])
    #expect(AgentShortcutNumbering.number(ofPaneID: "pane-3", in: projectsView) == 1)
    #expect(AgentShortcutNumbering.number(ofPaneID: "pane-1", in: projectsView) == nil)
    #expect(noCheckout.isEmpty)
}
