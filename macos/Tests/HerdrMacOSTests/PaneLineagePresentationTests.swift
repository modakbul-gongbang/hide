import Foundation
import Testing

@testable import HerdrMacOS

private func chip(_ paneID: String, _ label: String) -> CoreAgentChip {
    CoreAgentChip(paneID: paneID, label: label, detail: "running tests")
}

/// PRD B6, D-20: overflow folds into the same `+N` the Workspace summary
/// chip uses, and the number it shows is honest.
@Test func chipsBeyondTheRowFoldIntoAnHonestOverflowCount() {
    let five = (1...5).map { chip("w1:p\($0)", "Child \($0)") }

    let fits = PaneLineagePresentation.chipRow(five, limit: 5)
    #expect(fits.visible.count == 5)
    #expect(fits.overflow == 0, "nothing is folded when everything fits")

    // One slot pays for the `+N` itself, so three chips are drawn and the
    // count names the two that were not.
    let folded = PaneLineagePresentation.chipRow(five, limit: 4)
    #expect(folded.visible.map(\.label) == ["Child 1", "Child 2", "Child 3"])
    #expect(folded.overflow == 2)
    #expect(folded.visible.count + folded.overflow == five.count)

    let none = PaneLineagePresentation.chipRow(five, limit: 0)
    #expect(none.visible.isEmpty)
    #expect(none.overflow == 5, "a row with no room still says how many there are")
}

@Test func theChipLimitFollowsTheWidthTheRowWasGiven() {
    #expect(PaneLineagePresentation.chipLimit(width: 0) == 1, "a row always offers one slot")
    let slot = HideTheme.Layout.paneChildChipMaxWidth + HideTheme.spacingXS
    #expect(PaneLineagePresentation.chipLimit(width: slot * 3) == 3)
    #expect(PaneLineagePresentation.chipLimit(width: slot * 3 - 1) == 2)
}

/// PRD B24, B32, D-53: unknown is drawn as unknown, never as a zero.
@Test func theSubagentBadgeSaysUnknownRatherThanZero() {
    #expect(PaneLineagePresentation.subagentBadge(CoreSubagentCounts()) == nil)

    let known = try! #require(
        PaneLineagePresentation.subagentBadge(CoreSubagentCounts(working: 2, done: 4))
    )
    #expect(known.text == "2/4")
    #expect(known.accessibility.contains("2 working"))
    #expect(known.accessibility.contains("4 done"))

    let partial = try! #require(
        PaneLineagePresentation.subagentBadge(CoreSubagentCounts(working: nil, done: 7))
    )
    #expect(partial.text == "-/7", "a dash, because zero would claim it works alone")
    #expect(partial.accessibility.contains("an unknown number working"))

    let zero = try! #require(
        PaneLineagePresentation.subagentBadge(CoreSubagentCounts(working: 0, done: 0))
    )
    #expect(zero.text == "0/0", "a confirmed zero is still a real answer")
}

/// PRD B22, B23, D-30: three different states, three different screens.
@Test func theChildRowExistsOnlyWhenThereIsSomethingToSay() {
    #expect(
        !PaneLineagePresentation.showsChildRow(nil),
        "a pane with no agent draws neither chips nor a mark"
    )
    #expect(
        !PaneLineagePresentation.showsChildRow(CorePaneChildren(instrumented: true)),
        "an instrumented session that spawned nothing is a confirmed answer, and a quiet one"
    )
    #expect(
        PaneLineagePresentation.showsChildRow(
            CorePaneChildren(instrumented: true, chips: [chip("w1:p2", "Implementor")])
        )
    )
    #expect(
        PaneLineagePresentation.showsChildRow(
            CorePaneChildren(
                instrumented: true,
                subagents: CoreSubagentCounts(working: 1, done: 0)
            )
        ),
        "in-process subagents are worth a row even with no pane children"
    )
    #expect(
        PaneLineagePresentation.showsChildRow(
            CorePaneChildren(
                instrumented: false,
                uninstrumentedReason: "This runtime's Hide hook is not installed.",
                uninstrumentedLabel: "Children unknown: the hook is not installed",
                uninstrumentedCode: "hooks_not_installed"
            )
        ),
        "not knowing is a thing to say, and it says it here"
    )
}

private func row(
    paneID: String,
    delegated: Bool,
    stallNotice: String? = nil
) -> SidebarAgent {
    var agent = SidebarAgent(
        id: paneID,
        paneID: paneID,
        workspaceLabel: "hide",
        agentKind: "claude",
        demand: "none",
        activity: "working",
        group: delegated ? "seen" : "working",
        symbol: "\u{25cf}",
        emphasized: !delegated,
        statusLabel: "Working",
        summary: "running tests",
        elapsed: "2m",
        lastActivity: "1",
        ambient: nil
    )
    agent.delegated = delegated
    agent.stallNotice = stallNotice
    return agent
}

/// PRD B11, B21, B17, B18, D-36, D-60: the row carries ownership, the stall
/// notice and the uninstrumented mark, and every one of them has words.
@Test func theSidebarRowCarriesOwnershipStallAndInstrumentation() {
    let mine = AgentRowPresentation(
        agent: row(paneID: "w1:p1", delegated: false),
        density: .compact,
        connected: true,
        children: CorePaneChildren(instrumented: true)
    )
    #expect(!mine.delegated)
    #expect(mine.stallNotice == nil)
    #expect(mine.uninstrumentedReason == nil, "an instrumented pane has no mark")

    let theirs = AgentRowPresentation(
        agent: row(
            paneID: "w1:p2",
            delegated: true,
            stallNotice: "Implementor has been waiting 16 minutes on an approval"
        ),
        density: .compact,
        connected: true,
        children: CorePaneChildren(
            instrumented: false,
            uninstrumentedReason: "This runtime's Hide hook is not installed.",
            uninstrumentedLabel: "Children unknown: the hook is not installed",
            uninstrumentedCode: "hooks_not_installed"
        )
    )
    #expect(theirs.delegated)
    #expect(theirs.stallNotice?.contains("16 minutes") == true)
    #expect(theirs.uninstrumentedReason?.contains("not installed") == true)
    // The symbol never carries the meaning by itself.
    #expect(theirs.uninstrumentedLabel?.isEmpty == false)

    // A pane with no agent has no children snapshot, and no mark follows.
    let bare = AgentRowPresentation(
        agent: row(paneID: "w1:p3", delegated: false),
        density: .compact,
        connected: true
    )
    #expect(bare.uninstrumentedReason == nil)
    #expect(bare.uninstrumentedLabel == nil)
}
