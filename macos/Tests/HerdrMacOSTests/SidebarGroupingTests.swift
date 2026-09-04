import Foundation
import Testing

@testable import HerdrMacOS

@Test func sidebarContentAlternatesBetweenProjectsAndAgents() {
    #expect(SidebarContent.projects.alternate == .agents)
    #expect(SidebarContent.agents.alternate == .projects)
}

private func agent(id: String, paneID: String, group: String) -> SidebarAgent {
    SidebarAgent(
        id: id,
        paneID: paneID,
        workspaceLabel: "hide",
        agentKind: "codex",
        demand: group == "needs_you" ? "question" : "none",
        activity: group == "working" ? "working" : "stopped",
        unread: group != "seen",
        group: group,
        symbol: "\u{25cf}",
        summary: "Summary for \(id)",
        elapsed: "2m",
        lastActivity: "0000000000001",
        ambient: nil
    )
}

private func checkout(id: String, paneIDs: [String]) throws -> CoreCheckoutSnapshot {
    let panes = paneIDs
        .map { paneID in
            #"{"id":"\#(paneID)","label":"\#(paneID)","cwd":"/tmp/hide","status_label":"Idle","requires_close_confirmation":false}"#
        }
        .joined(separator: ",")
    let json = """
        {"id":"\(id)","workspace_id":"w1","label":"main","path":"/tmp/hide","branch":"main",\
        "is_worktree":false,"exists":true,"temporary":false,\
        "strip":[{"id":"herdr:w1:t1","kind":"herdr","source_id":"w1:t1","label":"Tab 1"}],"next_tab_label":"Tab 2",\
        "tabs":[{"id":"w1:t1","workspace_id":"w1","checkout_id":"\(id)","label":"1",\
        "empty":false,"panes":[\(panes)]}]}
        """
    return try JSONDecoder().decode(CoreCheckoutSnapshot.self, from: Data(json.utf8))
}

@Test func onlyAgentsInNeedsYouAreListedAsNeedingTheUser() {
    let agents = [
        agent(id: "a", paneID: "w1:p1", group: "working"),
        agent(id: "b", paneID: "w1:p2", group: "seen"),
        agent(id: "c", paneID: "w1:p3", group: "needs_you"),
        agent(id: "d", paneID: "w1:p4", group: "done"),
        agent(id: "e", paneID: "w1:p5", group: "needs_you"),
    ]

    // Membership is the core's group and nothing else. A working, read, or
    // merely finished agent is visible in its space and is asking the user for
    // nothing, so it never reaches the attention list.
    #expect(SidebarGrouping.agents(agents, in: .needsYou).map(\.id) == ["c", "e"])
}

@Test func nothingBlockedLeavesTheAttentionListEmpty() {
    let agents = [
        agent(id: "a", paneID: "w1:p1", group: "working"),
        agent(id: "b", paneID: "w1:p2", group: "seen"),
    ]

    #expect(SidebarGrouping.agents(agents, in: .needsYou).isEmpty)
}

/// The Agents view draws one section per non-empty group, in group order, and
/// keeps the order the core put the rows in inside each one.
@Test func sectionsFollowGroupOrderAndSkipEmptyGroups() {
    let agents = [
        agent(id: "c", paneID: "w1:p3", group: "needs_you"),
        agent(id: "e", paneID: "w1:p5", group: "needs_you"),
        agent(id: "a", paneID: "w1:p1", group: "working"),
        agent(id: "b", paneID: "w1:p2", group: "seen"),
    ]

    #expect(
        SidebarGrouping.sections(agents).map(\.group) == [.needsYou, .working, .seen]
    )
    #expect(SidebarGrouping.sections(agents)[0].agents.map(\.id) == ["c", "e"])
}

/// The Projects view raises Needs You and Done above the tree, and the tree
/// below must not repeat those rows.
@Test func raisedSectionsAreNeedsYouThenDone() {
    let agents = [
        agent(id: "d", paneID: "w1:p4", group: "done"),
        agent(id: "c", paneID: "w1:p3", group: "needs_you"),
        agent(id: "a", paneID: "w1:p1", group: "working"),
    ]

    #expect(SidebarGrouping.raised(agents).map(\.group) == [.needsYou, .done])
    #expect(SidebarGrouping.raised(agents).flatMap(\.agents).map(\.id) == ["c", "d"])
}

@Test func agentsAreGroupedByThePanesTheirCheckoutOwns() throws {
    let agents = [
        agent(id: "a", paneID: "w1:p1", group: "working"),
        agent(id: "b", paneID: "w1:p9", group: "working"),
    ]
    let mine = try checkout(id: "checkout-1", paneIDs: ["w1:p1"])

    #expect(SidebarGrouping.agents(agents, in: mine).map(\.id) == ["a"])
}
