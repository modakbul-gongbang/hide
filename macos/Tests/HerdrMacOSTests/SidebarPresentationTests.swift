import Testing
import SwiftUI

@testable import HerdrMacOS

private func presentationPane(id: String) -> CorePaneSnapshot {
    CorePaneSnapshot(
        id: id,
        cwd: "/tmp/hide",
        statusLabel: "Idle",
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
    group: String,
    demand: String = "none"
) -> SidebarAgent {
    SidebarAgent(
        id: id,
        paneID: paneID,
        workspaceLabel: "hide",
        agentKind: "codex",
        demand: demand,
        activity: group == "working" ? "working" : "stopped",
        unread: group == "needs_you" || group == "done",
        group: group,
        symbol: "\u{25cf}",
        summary: "Agent \(id)",
        elapsed: "1m",
        lastActivity: "0000000000001",
        ambient: nil
    )
}

@Test func checkoutSummaryPrefersAgentCountOverPaneCount() {
    var checkout = presentationCheckout(
        id: "main",
        path: "/tmp/hide",
        paneIDs: ["pane-1", "pane-2"]
    )
    checkout.agentSummary = CoreCheckoutAgentSummary(representativePaneID: "pane-1", working: 1)
    let workspace = presentationWorkspace(checkouts: [checkout])
    let agents = [presentationAgent(id: "agent-1", paneID: "pane-1", group: "working")]

    let presentation = SidebarCheckoutPresentation(
        workspace: workspace,
        checkout: checkout,
        agents: agents
    )

    #expect(presentation.agentCount == 1)
    #expect(presentation.status?.color == HideTheme.agentWorking)
    #expect(presentation.detailTooltip.hasPrefix("Working: 1"))
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

    #expect(presentation.agentCount == 0)
    #expect(presentation.status == nil)
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
        presentationAgent(id: "inside", paneID: "pane-2", group: "seen"),
        presentationAgent(id: "outside", paneID: "pane-9", group: "working"),
    ]

    let presentation = SidebarWorkspacePresentation(workspace: workspace, agents: agents)

    #expect(presentation.checkoutCount == 2)
    #expect(presentation.paneCount == 2)
    #expect(presentation.agentCount == 1)
    #expect(presentation.activityLabel == "1 agent")
}

@Test func agentShortcutNumbersFollowListOrderAndStopAtNine() {
    let agents = (1...11).map {
        presentationAgent(id: "agent-\($0)", paneID: "pane-\($0)", group: "seen")
    }

    #expect(AgentShortcutNumbering.number(ofPaneID: "pane-1", in: agents) == 1)
    #expect(AgentShortcutNumbering.number(ofPaneID: "pane-9", in: agents) == 9)
    #expect(AgentShortcutNumbering.number(ofPaneID: "pane-10", in: agents) == nil)
    #expect(AgentShortcutNumbering.number(ofPaneID: "pane-absent", in: agents) == nil)
}

@Test func agentLookupByNumberRejectsSlotsPastTheList() {
    let agents = (1...3).map {
        presentationAgent(id: "agent-\($0)", paneID: "pane-\($0)", group: "seen")
    }

    #expect(AgentShortcutNumbering.agent(atNumber: 2, in: agents)?.paneID == "pane-2")
    #expect(AgentShortcutNumbering.agent(atNumber: 4, in: agents) == nil)
    #expect(AgentShortcutNumbering.agent(atNumber: 0, in: agents) == nil)
}

@Test func shortcutCandidatesFollowTheVisibleSidebarView() {
    let agents = (1...4).map { index in
        var row = presentationAgent(id: "agent-\(index)", paneID: "pane-\(index)", group: "seen")
        row.lineageRootCheckoutID = index >= 3 ? "main" : "other"
        return row
    }
    let checkout = presentationCheckout(id: "main", path: "/tmp/hide", paneIDs: ["pane-3", "pane-4"])

    let agentsView = AgentShortcutNumbering.candidates(
        for: .agents, agents: agents, visibleCheckoutIDs: [checkout.id]
    )
    let projectsView = AgentShortcutNumbering.candidates(
        for: .projects, agents: agents, visibleCheckoutIDs: [checkout.id]
    )
    let noCheckout = AgentShortcutNumbering.candidates(
        for: .projects, agents: agents, visibleCheckoutIDs: []
    )

    #expect(agentsView.map(\.paneID) == ["pane-1", "pane-2", "pane-3", "pane-4"])
    // ⌃1 in the Projects view is the checkout's first agent, not the global first.
    #expect(projectsView.map(\.paneID) == ["pane-3", "pane-4"])
    #expect(AgentShortcutNumbering.number(ofPaneID: "pane-3", in: projectsView) == 1)
    #expect(AgentShortcutNumbering.number(ofPaneID: "pane-1", in: projectsView) == nil)
    #expect(noCheckout.isEmpty)
    let collapsed = AgentShortcutNumbering.candidates(
        for: .projects, agents: agents, visibleCheckoutIDs: [checkout.id],
        collapsedCheckoutIDs: [checkout.id]
    )
    #expect(collapsed.isEmpty)
    let agentsWithCollapsedWorkspace = AgentShortcutNumbering.candidates(
        for: .agents, agents: agents, visibleCheckoutIDs: [checkout.id],
        collapsedCheckoutIDs: [checkout.id]
    )
    #expect(agentsWithCollapsedWorkspace.map(\.paneID) == agentsView.map(\.paneID))
}

@Test func agentContextLabelNamesTheProjectAndItsCheckout() {
    var agent = presentationAgent(id: "agent-1", paneID: "pane-1", group: "seen")
    #expect(agent.contextLabel == agent.workspaceLabel)
    agent.checkoutLabel = "main"
    #expect(agent.contextLabel == "\(agent.workspaceLabel) › main")
    agent.checkoutLabel = agent.workspaceLabel
    #expect(agent.contextLabel == agent.workspaceLabel)
}

@Test func agentCheckoutQualifierIsAbsentWhenItRepeatsTheProject() {
    var agent = presentationAgent(id: "agent-1", paneID: "pane-1", group: "seen")
    // The sidebar row draws this on its own small line, so a qualifier that
    // only repeats the row's title has to read as nothing at all.
    #expect(agent.checkoutQualifier == nil)
    agent.checkoutLabel = "main"
    #expect(agent.checkoutQualifier == "main")
    agent.checkoutLabel = agent.workspaceLabel
    #expect(agent.checkoutQualifier == nil)
    agent.checkoutLabel = ""
    #expect(agent.checkoutQualifier == nil)
}

@Test func tabLookupByNumberFollowsTheStripOrderAndStopsAtNine() {
    let tabs = (1...10).map { index in
        ShellTabItem(
            id: "tab-\(index)",
            label: "Tab \(index)",
            dirty: false,
            active: index == 1,
            kind: .editor(
                CoreEditorTabSnapshot(
                    id: "file-\(index)",
                    workspaceID: "w1",
                    checkoutID: "c1",
                    path: "/tmp/file-\(index)",
                    label: "Tab \(index)",
                    kind: .file,
                    diffCommitted: nil,
                    dirty: false
                )
            )
        )
    }

    #expect(TabShortcutNumbering.number(ofTabID: "tab-1", in: tabs) == 1)
    #expect(TabShortcutNumbering.number(ofTabID: "tab-9", in: tabs) == 9)
    #expect(TabShortcutNumbering.number(ofTabID: "tab-10", in: tabs) == nil)
    #expect(TabShortcutNumbering.number(ofTabID: "tab-absent", in: tabs) == nil)
    #expect(TabShortcutNumbering.tab(atNumber: 2, in: tabs)?.id == "tab-2")
    #expect(TabShortcutNumbering.tab(atNumber: 0, in: tabs) == nil)
    #expect(TabShortcutNumbering.tab(atNumber: 11, in: tabs) == nil)
}

@Test func workspaceDisconnectedSuppressesRetainedCountsAndMark() {
    var checkout = presentationCheckout(id: "main", path: "/tmp/hide", paneIDs: ["p1"])
    checkout.agentSummary = CoreCheckoutAgentSummary(representativePaneID: "p1", working: 1)
    let agent = presentationAgent(id: "a1", paneID: "p1", group: "working")
    let presentation = SidebarCheckoutPresentation(workspace: presentationWorkspace(checkouts: [checkout]),
        checkout: checkout, agents: [agent], connected: false)
    #expect(presentation.representativeAgentKind == agent.agentKind)
    #expect(presentation.status?.symbol == "⊘")
    #expect(presentation.status?.label == "Disconnected")
    #expect(presentation.status?.color == HideTheme.secondary)
    #expect(!presentation.detailTooltip.contains("Working: 1"))
    let row = AgentRowPresentation(agent: agent, density: .compact, connected: false)
    #expect(row.symbol == presentation.status?.symbol)
    #expect(row.statusColor == presentation.status?.color)
}

@Test func semanticStatusUsesFixedColorsAndAcknowledgesWithoutResolving() {
    let working = AgentStatusPresentation(demand: "none", activity: "working", emphasized: false,
        symbol: "●", label: "Working", connected: true)
    #expect(working.color == HideTheme.agentWorking)
    let done = AgentStatusPresentation(demand: "none", activity: "stopped", emphasized: true,
        symbol: "✓", label: "Done", connected: true)
    #expect(done.color == HideTheme.success)
    let readError = AgentStatusPresentation(demand: "error", activity: "stopped", emphasized: false,
        symbol: "×", label: "Error", connected: true)
    #expect(readError.symbol == "×")
    #expect(readError.color == HideTheme.danger.opacity(HideTheme.readStatusOpacity))
}
