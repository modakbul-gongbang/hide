import Testing
@testable import HerdrMacOS

/// The delegated-agent boundary as the row draws it: the descendant badge,
/// the leading role column, and the worktree qualifier (PRD B3, B9, B10,
/// B11, B12, D-04, D-07, D-08).
@Suite("Delegated row presentation")
struct DelegatedRowPresentationTests {
    private func agent(
        _ paneID: String,
        depth: Int = 0,
        parent: String? = nil,
        children: [String] = [],
        collapsed: Bool = true,
        counts: DescendantCounts = DescendantCounts(),
        worktree: String? = nil,
        orphan: Bool = false
    ) -> SidebarAgent {
        var row = SidebarAgent(
            id: paneID,
            paneID: paneID,
            workspaceLabel: "hide",
            agentKind: "claude",
            demand: "none",
            activity: "working",
            group: "working",
            symbol: "\u{25cf}",
            statusLabel: "Working",
            identityLabel: "긴 한국어 작업 이름과 English mixed title",
            elapsed: "1h",
            lastActivity: "1"
        )
        row.lineageDepth = depth
        row.lineageParentPaneID = parent
        row.lineageChildPaneIDs = children
        row.lineageCollapsed = collapsed
        row.descendantCounts = counts
        row.lineageWorktreeBadge = worktree
        row.lineageOrphan = orphan
        row.delegated = parent != nil
        return row
    }

    @Test func aFoldedParentWearsTheBadgeAndAnOpenedOneHandsItToItsChildren() {
        let counts = DescendantCounts(question: 1, working: 2)
        let folded = AgentRowPresentation(
            agent: agent("p1", children: ["p2"], collapsed: true, counts: counts),
            density: .compact,
            connected: true
        )
        #expect(folded.descendantBadge == counts)
        #expect(folded.role == .disclosure(collapsed: true, interactive: true))

        let opened = AgentRowPresentation(
            agent: agent("p1", children: ["p2"], collapsed: false, counts: counts),
            density: .compact,
            connected: true
        )
        #expect(opened.descendantBadge == nil, "open children carry their own marks")
        #expect(opened.role == .disclosure(collapsed: false, interactive: true))

        let childless = AgentRowPresentation(
            agent: agent("p3"),
            density: .compact,
            connected: true
        )
        #expect(childless.descendantBadge == nil)
        #expect(childless.role == .blank)
    }

    @Test func aRaisedRowAlwaysWearsTheBadgeAndItsChevronOnlyIndicates() {
        let counts = DescendantCounts(done: 2)
        let raised = AgentRowPresentation(
            agent: agent("p1", children: ["p2"], collapsed: false, counts: counts),
            density: .prominent,
            connected: true
        )
        #expect(raised.descendantBadge == counts, "a raised row never unfolds")
        #expect(raised.role == .disclosure(collapsed: false, interactive: false))
    }

    @Test func aChildAwayFromItsParentSaysWhoseItIsAndUnderItsParentSaysWhereItRuns() {
        // Under its own worktree the tree rebased it to depth zero.
        let visiting = AgentRowPresentation(
            agent: agent("p2", depth: 0, parent: "p1", worktree: "feature"),
            density: .compact,
            connected: true
        )
        #expect(visiting.role == .returnToParent(paneID: "p1"))
        #expect(visiting.qualifier == nil, "the worktree heading above it already says where")

        let nested = AgentRowPresentation(
            agent: agent("p2", depth: 1, parent: "p1", worktree: "feature"),
            density: .compact,
            connected: true
        )
        #expect(nested.role == .blank, "the tree line already says whose it is")
        #expect(nested.qualifier == "feature")
        #expect(nested.qualifierSystemImage == HideTheme.GitIcon.worktree)

        let sameWorktree = AgentRowPresentation(
            agent: agent("p3", depth: 1, parent: "p1"),
            density: .compact,
            connected: true
        )
        #expect(sameWorktree.qualifier == nil)

        // Listed flat, the child keeps the way back and its project context.
        var flat = agent("p2", depth: 1, parent: "p1", worktree: "feature")
        flat.checkoutLabel = "feature"
        let listed = AgentRowPresentation(agent: flat, density: .prominent, connected: true)
        #expect(listed.role == .returnToParent(paneID: "p1"))
        #expect(listed.qualifier == flat.contextLabel)
    }

    @Test func anOrphanHasNoParentToReturnToAndKeepsItsHint() {
        var orphan = agent("p2", depth: 0, orphan: true)
        orphan.lineageHint = "↳ from an agent Hide can't see"
        let presentation = AgentRowPresentation(agent: orphan, density: .compact, connected: true)
        #expect(presentation.role == .blank)
    }

    @Test func theBadgeDrawsOnlyNonZeroStatesWorstFirstWithTheRowMarks() {
        let cells = DescendantBadgeCell.cells(DescendantCounts(error: 1, question: 2, done: 3))
        #expect(cells.map(\.id) == ["error", "question", "done"])
        #expect(cells.map(\.symbol) == ["\u{d7}", "?", "✓"])
        #expect(cells.map(\.count) == [1, 2, 3])
        #expect(DescendantBadgeCell.accessibilityLabel(DescendantCounts(approval: 1, working: 4))
            == "Descendants: 1 approval, 4 working")
        #expect(DescendantBadgeCell.cells(DescendantCounts()).isEmpty)
    }
}
