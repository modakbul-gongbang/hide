import Foundation
import Testing

@testable import HerdrMacOS

@Test func hideSearchFiltersAgentsAndCheckoutsWithoutTerminalEntries() throws {
    let agent = SidebarAgent(
        id: "agent-1",
        paneID: "pane-1",
        workspaceLabel: "hide",
        agentKind: "codex",
        state: "working",
        symbol: "●",
        summary: "Build the release",
        elapsed: "2m",
        sortRank: "01",
        activity: "building",
        ambient: nil
    )
    let workspace = try JSONDecoder().decode(
        CoreWorkspaceSnapshot.self,
        from: Data(
            #"{"id":"workspace-1","label":"hide","path":"/tmp/hide","device_id":"local","repo_name":"hide","is_git":true,"registered":true,"temporary":false,"checkouts":[{"id":"checkout-1","workspace_id":"workspace-1","label":"main","path":"/tmp/hide","branch":"main","is_worktree":false,"exists":true,"temporary":false,"tabs":[]}]}"#.utf8
        )
    )
    let checkout = try #require(workspace.checkouts.first)
    let entries = [
        HideSearchEntry(
            id: "agent-agent-1",
            title: agent.summary,
            subtitle: "Agent · \(agent.workspaceLabel)",
            kind: .agent(agent)
        ),
        HideSearchEntry(
            id: "checkout-checkout-1",
            title: "\(workspace.repoName) / \(checkout.label)",
            subtitle: checkout.path,
            kind: .checkout(workspace, checkout)
        ),
    ]

    #expect(HideSearchEntry.filtered(entries, query: "repo").isEmpty)
    #expect(HideSearchEntry.filtered(entries, query: "release").map(\.id) == ["agent-agent-1"])
    #expect(HideSearchEntry.filtered(entries, query: " / main").map(\.id) == ["checkout-checkout-1"])
    #expect(HideSearchEntry.filtered(entries, query: "  ").map(\.id) == ["agent-agent-1", "checkout-checkout-1"])
    #expect(entries[0].route == .agent(paneID: "pane-1"))
    #expect(entries[1].route == .checkout(workspaceID: "workspace-1", checkoutID: "checkout-1"))
}
