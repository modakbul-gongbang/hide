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
        "strip":[{"id":"herdr:w1:t1","kind":"herdr","source_id":"w1:t1","label":"Tab 1"}],"agent_summary":{"representative_pane_id":null,"needs_you":0,"done":0,"working":0,"seen":0,"unknown":0},"next_tab_label":"Tab 2",\
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

private func scratchTab(id: String, paneIDs: [String], title: String?) throws
    -> CoreScratchTabSnapshot
{
    let panes = paneIDs
        .map { paneID in
            #"{"id":"\#(paneID)","label":"\#(paneID)","cwd":"/scratch","status_label":"Idle","requires_close_confirmation":false}"#
        }
        .joined(separator: ",")
    let titleField = title.map { "\"\($0)\"" } ?? "null"
    let json = """
        {"id":"\(id)","label":"Tab 1","title":\(titleField),"panes":[\(panes)]}
        """
    return try JSONDecoder().decode(CoreScratchTabSnapshot.self, from: Data(json.utf8))
}

/// AC6: a Scratch agent that Needs You already raised to the top is not drawn
/// a second time under Scratch. Two rows for one agent is the defect; the
/// project tree has always followed this rule and Scratch joins it.
@Test func aRaisedScratchAgentIsNotDrawnAgainUnderScratch() throws {
    let agents = [
        agent(id: "waiting", paneID: "s1:p1", group: "needs_you"),
        agent(id: "working", paneID: "s1:p2", group: "working"),
    ]
    let tabs = [
        try scratchTab(id: "s1:t1", paneIDs: ["s1:p1"], title: "waiting chat"),
        try scratchTab(id: "s1:t2", paneIDs: ["s1:p2"], title: "working chat"),
        try scratchTab(id: "s1:t3", paneIDs: ["s1:p3"], title: nil),
    ]

    let drawn = SidebarGrouping.scratchTabsBelowRaisedSections(tabs: tabs, agents: agents)

    #expect(drawn.map(\.id) == ["s1:t2", "s1:t3"])
}

/// The row's own name: its title when the composer wrote one, the tab label
/// when it did not. A row that showed nothing would be worse than one showing
/// `Tab 1`.
@Test func aScratchRowNamesItselfByTitleThenLabel() throws {
    let titled = try scratchTab(id: "s1:t1", paneIDs: ["s1:p1"], title: "build me a parser")
    let untitled = try scratchTab(id: "s1:t2", paneIDs: ["s1:p2"], title: nil)

    #expect(titled.displayName == "build me a parser")
    #expect(untitled.displayName == "Tab 1")
}

/// One row per line of the tree the connector has to draw, at the depths the
/// visible preorder produces:
///
///     root
///     ├ a
///     │ └ a1
///     └ b
///
private func treeRow(_ id: String, depth: Int) -> SidebarAgent {
    var row = agent(id: id, paneID: "w1:\(id)", group: "working")
    row.lineageDepth = depth
    return row
}

@Test func theTreeGuideCarriesATrunkThroughEveryRowBetweenAParentAndItsLastChild() {
    let guides = SidebarGrouping.lineageGuides([
        treeRow("root", depth: 0),
        treeRow("a", depth: 1),
        treeRow("a1", depth: 2),
        treeRow("b", depth: 1),
    ])

    // `a` has `b` still to come, so its own trunk carries on down.
    #expect(guides[1].isLastChild == false)
    // `a1` sits between `a` and `b`, so level 1's trunk passes straight
    // through it. Without this the line breaks at every nested row, which is
    // the defect this covers.
    #expect(guides[2].continuing == [1])
    #expect(guides[2].isLastChild == true)
    // `b` ends the run: its trunk stops at the elbow and nothing passes it.
    #expect(guides[3].isLastChild == true)
    #expect(guides[3].continuing.isEmpty)
    // A root draws no guide at all.
    #expect(guides[0].continuing.isEmpty)
}

@Test func aDeeperBranchDoesNotReopenARunItsParentAlreadyClosed() {
    let guides = SidebarGrouping.lineageGuides([
        treeRow("root", depth: 0),
        treeRow("a", depth: 1),
        treeRow("a1", depth: 2),
        treeRow("a2", depth: 2),
        treeRow("c", depth: 0),
    ])

    // `a1` is followed by a sibling at its own depth.
    #expect(guides[2].isLastChild == false)
    #expect(guides[3].isLastChild == true)
    // `c` is a second root, so level 1 is closed by the time it is reached
    // and no trunk is drawn through it.
    #expect(guides[4].continuing.isEmpty)
}

@Test func aChildsStatusMarkIsCenteredUnderItsParentsAgentBadge() {
    // The indent is the row's own column step, so the elbow arrives on the
    // mark rather than near it. Measured the way the row lays out: the mark
    // is agentMarkWidth wide, then iconSpacing, then the badge.
    let step = HideTheme.lineageIndent
    let markCenter = HideTheme.agentMarkWidth / 2
    let badgeCenter = HideTheme.agentMarkWidth + HideTheme.spacingXS
        + HideTheme.compactAgentBadgeSize / 2
    #expect(step + markCenter == badgeCenter)

    // And the inset is uniform, so level three lines up with level two the
    // same way level two lines up with level one.
    #expect(HideTheme.lineageInset(depth: 3) - HideTheme.lineageInset(depth: 2) == step)
    #expect(HideTheme.lineageInset(depth: 0) == 0)

    // The trunk for a level lands on that level's own mark center, which is
    // the column its children's marks occupy one step over.
    #expect(
        HideTheme.lineageTrunkX(depth: 1) - HideTheme.lineageTrunkX(depth: 0) == step
    )
}
