import Foundation
import Testing

@testable import HerdrMacOS

@Test func sidebarContentAlternatesBetweenProjectsAndAgents() {
    #expect(SidebarContent.projects.alternate == .agents)
    #expect(SidebarContent.agents.alternate == .projects)
}

private func agent(id: String, paneID: String, state: String) -> SidebarAgent {
    SidebarAgent(
        id: id,
        paneID: paneID,
        workspaceLabel: "hide",
        agentKind: "codex",
        state: state,
        symbol: "●",
        summary: "Summary for \(id)",
        elapsed: "2m",
        sortRank: "01",
        activity: "working",
        ambient: nil
    )
}

private func checkout(id: String, paneIDs: [String]) throws -> CoreCheckoutSnapshot {
    let panes = paneIDs
        .map { paneID in
            #"{"id":"\#(paneID)","label":"\#(paneID)","cwd":"/tmp/hide","state":"idle"}"#
        }
        .joined(separator: ",")
    let json = """
        {"id":"\(id)","workspace_id":"w1","label":"main","path":"/tmp/hide","branch":"main",\
        "is_worktree":false,"exists":true,"temporary":false,\
        "tabs":[{"id":"w1:t1","workspace_id":"w1","checkout_id":"\(id)","label":"1",\
        "empty":false,"panes":[\(panes)]}]}
        """
    return try JSONDecoder().decode(CoreCheckoutSnapshot.self, from: Data(json.utf8))
}

@Test func onlyBlockedAgentsAreListedAsNeedingTheUser() {
    let agents = [
        agent(id: "a", paneID: "w1:p1", state: "working"),
        agent(id: "b", paneID: "w1:p2", state: "idle"),
        agent(id: "c", paneID: "w1:p3", state: "question"),
        agent(id: "d", paneID: "w1:p4", state: "approval"),
        agent(id: "e", paneID: "w1:p5", state: "error"),
        agent(id: "f", paneID: "w1:p6", state: "unseen_completion"),
        agent(id: "g", paneID: "w1:p7", state: "blocked"),
    ]

    // A working or idle agent is visible in its space and is asking the user
    // for nothing, so it never reaches the attention list.
    #expect(SidebarGrouping.needingAttention(agents).map(\.id) == ["c", "d", "e", "f", "g"])
    #expect(SidebarGrouping.requiresCloseConfirmation("working"))
    #expect(SidebarGrouping.requiresCloseConfirmation("blocked"))
    #expect(!SidebarGrouping.requiresCloseConfirmation("idle"))
}

@Test func nothingBlockedLeavesTheAttentionListEmpty() {
    let agents = [
        agent(id: "a", paneID: "w1:p1", state: "working"),
        agent(id: "b", paneID: "w1:p2", state: "idle"),
    ]

    #expect(SidebarGrouping.needingAttention(agents).isEmpty)
}

@Test func agentsAreGroupedByThePanesTheirCheckoutOwns() throws {
    let agents = [
        agent(id: "a", paneID: "w1:p1", state: "working"),
        agent(id: "b", paneID: "w1:p9", state: "working"),
    ]
    let mine = try checkout(id: "checkout-1", paneIDs: ["w1:p1"])

    #expect(SidebarGrouping.agents(agents, in: mine).map(\.id) == ["a"])
}
