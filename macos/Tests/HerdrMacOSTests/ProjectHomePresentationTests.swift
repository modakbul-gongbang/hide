import AppKit
import Foundation
import Testing
@testable import HerdrMacOS

/// The mission board as a caller observes it: given these checkouts and these
/// core agents, these lanes in this order, with these stages, cards and
/// nesting. Nothing here renders; the values are what the view draws.
@Suite("Project Home board")
struct ProjectHomePresentationTests {
    private static let root = "/fixtures/hide"

    private func checkout(
        _ name: String,
        panes: [String] = [],
        primary: Bool = false,
        exists: Bool = true,
        dirty: Bool = false,
        changed: Int = 0,
        base: String? = "main",
        ahead: Int = 0,
        behind: Int = 0,
        pullRequest: CorePullRequest? = nil,
        github: CoreGithubStatus = .empty,
        summary: CoreCheckoutAgentSummary = CoreCheckoutAgentSummary()
    ) -> CoreCheckoutSnapshot {
        var checkout = CoreCheckoutSnapshot(
            id: "checkout-\(name)",
            workspaceID: "w1",
            label: name,
            path: primary ? Self.root : "\(Self.root).worktrees/\(name)",
            branch: name,
            isWorktree: !primary,
            exists: exists,
            temporary: false,
            hasPanes: !panes.isEmpty,
            dirty: dirty,
            changedFileCount: changed,
            baseBranch: base,
            ahead: ahead,
            behind: behind,
            pullRequest: pullRequest,
            tabs: panes.isEmpty ? [] : [CoreTabSnapshot(
                id: "tab-\(name)", workspaceID: "w1", checkoutID: "checkout-\(name)", label: name, empty: false,
                panes: panes.map { CorePaneSnapshot(id: $0, cwd: Self.root, statusLabel: "Idle", activityAt: nil) }
            )]
        )
        checkout.github = github
        checkout.agentSummary = summary
        return checkout
    }

    private func agent(
        _ pane: String,
        _ label: String,
        group: String = "working",
        symbol: String = "●",
        status: String = "Working",
        demand: String = "none",
        activity: String = "working",
        emphasized: Bool = false,
        detail: String? = nil,
        wordVisible: Bool = true,
        recency: String = "1"
    ) -> SidebarAgent {
        SidebarAgent(
            id: pane, paneID: pane, workspaceLabel: "hide", agentKind: "claude",
            demand: demand, activity: activity, group: group, symbol: symbol,
            emphasized: emphasized, statusLabel: status, identityLabel: label,
            detail: detail, statusWordVisible: wordVisible, elapsed: "3m", lastActivity: recency
        )
    }

    private func pullRequest(_ number: Int, badge: CorePullRequestBadge = .open, draft: Bool = false,
                             checks: CorePullRequestChecks? = .passing, title: String? = "Mission board") -> CorePullRequest {
        CorePullRequest(
            title: title, checks: checks, number: number, headBranch: "feature", baseBranch: "main",
            url: "https://example.invalid/pr/\(number)", badge: badge, review: nil, isDraft: draft,
            mergedAtUnixMS: nil, updatedAtUnixMS: nil
        )
    }

    private func board(_ checkouts: [CoreCheckoutSnapshot], _ agents: [SidebarAgent],
                       connected: Bool = true, facts: [String: ProjectHomeWorktreeFact] = [:]) -> ProjectHomeBoard {
        ProjectHomeBoard.build(ProjectHomeInput(
            projectLabel: "hide", projectPath: Self.root, checkouts: checkouts, agents: agents,
            connected: connected, worktreeFacts: facts
        ))
    }

    // MARK: Lane order

    @Test func lanesFollowTheOverviewAttentionRankAndTheirPathBreaksTies() {
        let quiet = checkout("quiet")
        let working = checkout("working", panes: ["p-work"], summary: CoreCheckoutAgentSummary(working: 1))
        let done = checkout("done", panes: ["p-done"], summary: CoreCheckoutAgentSummary(done: 1))
        let asks = checkout("asks", panes: ["p-ask"], summary: CoreCheckoutAgentSummary(needsYou: 1))
        let main = checkout("main", panes: ["p-main"], primary: true, summary: CoreCheckoutAgentSummary(seen: 1))
        let seenB = checkout("b-seen", panes: ["p-b"], summary: CoreCheckoutAgentSummary(seen: 1))
        let result = board([quiet, working, done, asks, main, seenB], [
            agent("p-ask", "질문 있는 에이전트", group: "needs_you", symbol: "?", status: "Question", demand: "question", activity: "stopped", emphasized: true),
            agent("p-done", "완료", group: "done", symbol: "✓", status: "Done", activity: "stopped", emphasized: true),
            agent("p-work", "작업 중", group: "working"),
            agent("p-main", "main idle", group: "seen", symbol: "○", status: "Idle", activity: "stopped"),
            agent("p-b", "b idle", group: "seen", symbol: "○", status: "Idle", activity: "stopped"),
        ])
        #expect(result.lanes.map(\.label) == ["asks", "done", "working", "main", "b-seen", "quiet"])
        #expect(result.lanes.map(\.rank) == [0, 1, 2, 3, 3, 4])
        #expect(result.lanes[3].isPrimary)
        #expect(result.counts == ProjectHomeCounts(needsYou: 1, done: 1, working: 1, seen: 2))
    }

    // MARK: Track stages

    @Test func aFilledTrackReadsChangesCommitsPullRequestAndChecks() {
        let request = pullRequest(42, checks: .passing)
        let lane = board([checkout("feature", dirty: true, changed: 3, ahead: 2, behind: 1, pullRequest: request)], []).lanes[0]
        #expect(lane.track.map(\.kind) == ProjectHomeStageKind.allCases)
        #expect(lane.track.map(\.label) == ["3 changed", "↑2 ↓1", "#42 Open", "Passing", "Merged"])
        #expect(lane.track.map(\.isHollow) == [false, false, false, false, true])
        #expect(lane.track[0].tooltip == "3 uncommitted changes")
        #expect(lane.track[1].tooltip == "2 ahead of main, 1 behind")
        #expect(lane.track[2].tooltip == "PR #42 · Mission board · Open")
        #expect(lane.track[4].tooltip == "Merged: Merge state not read yet")
    }

    @Test func aStageTheDataCannotFillIsHollowWithItsReason() {
        let failed = CoreGithubStatus(available: false, loading: false, stale: false, lastSuccessAtUnixMS: nil,
                                      unavailableReason: "gh is not signed in")
        let lane = board([checkout("feature", base: nil, github: failed)], []).lanes[0]
        #expect(lane.track[0].label == "clean")
        #expect(!lane.track[0].isHollow)
        #expect(lane.track[1].state == .hollow(reason: "No base branch to compare against"))
        #expect(lane.track[2].state == .hollow(reason: "gh is not signed in"))

        let loading = CoreGithubStatus(available: false, loading: true, stale: false, lastSuccessAtUnixMS: nil, unavailableReason: nil)
        let pending = board([checkout("feature", github: loading)], []).lanes[0]
        #expect(pending.track[2].state == .hollow(reason: "Looking up the pull request…"))

        let unanswered = board([checkout("feature")], []).lanes[0]
        #expect(unanswered.track[2].state == .hollow(reason: "GitHub has not answered yet"))

        let answered = CoreGithubStatus(available: true, loading: false, stale: false, lastSuccessAtUnixMS: 1, unavailableReason: nil)
        let none = board([checkout("feature", github: answered)], []).lanes[0]
        #expect(none.track[2].state == .hollow(reason: "No pull request for this branch"))
    }

    @Test func withoutAPullRequestTheTrackStopsAtOneHollowPRChip() {
        // CI and Merged derive from the pull request; twelve lanes of three
        // hollow chips said nothing, so a lane without a PR stops at `PR`.
        let none = board([checkout("feature", ahead: 2)], []).lanes[0]
        #expect(none.track.map(\.kind) == [.changes, .commits, .pullRequest])
        #expect(none.track.map(\.isHollow) == [false, false, true])
        // A branch the base already contains is read from ancestry, not
        // GitHub, so that Merged is still drawn, filled.
        let contained = board([checkout("feature")], [], facts: [
            "\(Self.root).worktrees/feature": ProjectHomeWorktreeFact(merged: true),
        ]).lanes[0]
        #expect(contained.track.map(\.kind) == [.changes, .commits, .pullRequest, .merged])
        #expect(!contained.track[3].isHollow)
        let notYet = board([checkout("feature")], [], facts: [
            "\(Self.root).worktrees/feature": ProjectHomeWorktreeFact(merged: false),
        ]).lanes[0]
        #expect(notYet.track.map(\.kind) == [.changes, .commits, .pullRequest])
        let withRequest = board([checkout("feature", pullRequest: pullRequest(4))], [], facts: [
            "\(Self.root).worktrees/feature": ProjectHomeWorktreeFact(merged: false),
        ]).lanes[0]
        #expect(withRequest.track.map(\.kind) == ProjectHomeStageKind.allCases)
        #expect(withRequest.track[4].state == .hollow(reason: "Not merged into main"))
    }

    @Test func checksFollowTheStatusModelMappingAndAnUnknownResultIsHollow() {
        func ci(_ checks: CorePullRequestChecks?) -> ProjectHomeTrackStage {
            board([checkout("feature", pullRequest: pullRequest(7, checks: checks))], []).lanes[0].track[3]
        }
        #expect(ci(.failed).label == "Failing" && !ci(.failed).isHollow)
        #expect(ci(.pending).label == "Running")
        #expect(ci(CorePullRequestChecks.none).label == "No checks" && !ci(CorePullRequestChecks.none).isHollow)
        #expect(ci(.unknown).state == .hollow(reason: "GitHub reported no check result for PR #7"))
        #expect(ci(nil).isHollow)
    }

    @Test func mergedComesFromTheMergedPullRequestOrTheWorktreeAncestry() {
        let merged = board([checkout("feature", pullRequest: pullRequest(9, badge: .merged))], []).lanes[0].track[4]
        #expect(merged.label == "merged" && !merged.isHollow)
        let byAncestry = board([checkout("feature")], [], facts: [
            "\(Self.root).worktrees/feature": ProjectHomeWorktreeFact(merged: true),
        ]).lanes[0].track[3]
        #expect(byAncestry.tooltip == "Merged into main by ancestry")
        let notYet = board([checkout("feature", pullRequest: pullRequest(9))], [], facts: [
            "\(Self.root).worktrees/feature": ProjectHomeWorktreeFact(merged: false),
        ]).lanes[0].track[4]
        #expect(notYet.state == .hollow(reason: "Not merged into main"))
    }

    @Test func aMissingWorktreeIsHollowThroughoutAndCannotOpen() {
        let lane = board([checkout("gone", exists: false, dirty: true, changed: 2, pullRequest: pullRequest(3))], []).lanes[0]
        #expect(lane.isMissing)
        #expect(!lane.canOpen)
        #expect(lane.track.map(\.kind) == [.changes, .commits, .pullRequest])
        #expect(lane.track.filter(\.isHollow).count == lane.track.count)
        #expect(lane.track[0].state == .hollow(reason: "Worktree folder is missing"))
    }

    // MARK: Cards and nesting

    @Test func aChildNestsUnderItsParentInTheSameLaneInTheCoreOrder() {
        var parent = agent("p-root", "Orchestrator", recency: "9")
        parent.lineageChildPaneIDs = ["p-child-b", "p-child-a"]
        var childA = agent("p-child-a", "Worker A", recency: "5")
        childA.lineageDepth = 1
        childA.delegated = true
        var childB = agent("p-child-b", "Worker B", recency: "7")
        childB.lineageDepth = 1
        childB.delegated = true
        var grandchild = agent("p-grand", "Grandchild", recency: "3")
        grandchild.lineageDepth = 2
        grandchild.delegated = true
        childB.lineageChildPaneIDs = ["p-grand"]
        let other = agent("p-other", "Sibling root", recency: "1")
        let lane = board(
            [checkout("feature", panes: ["p-root", "p-child-a", "p-child-b", "p-grand", "p-other"])],
            [parent, childB, childA, grandchild, other]
        ).lanes[0]
        #expect(lane.cards.map(\.id) == ["p-root", "p-child-b", "p-grand", "p-child-a", "p-other"])
        #expect(lane.cards.map(\.depth) == [0, 1, 2, 1, 0])
        #expect(lane.cards[1].parentPaneID == "p-root")
        #expect(lane.cards[2].parentPaneID == "p-child-b")
        #expect(lane.cards[0].childPaneIDs == ["p-child-b", "p-child-a"])
        #expect(lane.lineage(of: "p-child-b") == ["p-root", "p-child-b", "p-grand"])
        #expect(lane.lineage(of: "p-other") == ["p-other"])
        #expect(lane.cards[4].fromParentCaption == nil)
    }

    @Test func aChildInAnotherCheckoutIsARootThereCaptionedWithItsParent() {
        var parent = agent("p-root", "Orchestrator")
        parent.lineageChildPaneIDs = ["p-child"]
        var child = agent("p-child", "Worker in worktree")
        child.lineageDepth = 1
        child.delegated = true
        let result = board(
            [checkout("main", panes: ["p-root"], primary: true), checkout("wt", panes: ["p-child"])],
            [parent, child]
        )
        let home = result.lanes.first { $0.label == "main" }!
        let away = result.lanes.first { $0.label == "wt" }!
        #expect(home.cards.map(\.id) == ["p-root"])
        #expect(home.cards[0].childPaneIDs.isEmpty)
        #expect(away.cards[0].depth == 0)
        #expect(away.cards[0].fromParentCaption == "↳ from Orchestrator")
        #expect(away.cards[0].foreignParentPaneID == "p-root")
        #expect(home.cards[0].foreignChildPaneIDs == ["p-child"])
        #expect(home.cards[0].toLanesCaption == "↳ to wt")
        #expect(result.family(of: "p-root") == ["p-root", "p-child"])
        #expect(result.family(of: "p-child") == ["p-root", "p-child"])
        #expect(result.lineage(of: "p-child").map(\.label) == ["Orchestrator", "Worker in worktree"])
        #expect(result.lineage(of: "p-child").map(\.laneLabel) == ["main", "wt"])
    }

    @Test func anOrphanAndACycleStayVisibleAsRoots() {
        var orphan = agent("p-orphan", "Orphan")
        orphan.lineageDepth = 1
        orphan.lineageOrphan = true
        var a = agent("p-a", "A")
        a.lineageChildPaneIDs = ["p-b"]
        var b = agent("p-b", "B")
        b.lineageChildPaneIDs = ["p-a"]
        let lane = board([checkout("feature", panes: ["p-orphan", "p-a", "p-b"])], [orphan, a, b]).lanes[0]
        #expect(Set(lane.cards.map(\.id)) == ["p-orphan", "p-a", "p-b"])
        #expect(lane.cards.first { $0.id == "p-orphan" }?.depth == 0)
        #expect(lane.cards.first { $0.id == "p-a" }?.depth == 0)
        #expect(lane.cards.first { $0.id == "p-b" }?.depth == 1)
        #expect(lane.cards.first { $0.id == "p-a" }?.fromParentCaption == nil)
    }

    @Test func aStalledRootCarriesTheCoreSentenceAndAnEscalationEntersNeedsYou() {
        var root = agent("p-root", "Orchestrator", group: "needs_you", symbol: "●", status: "Working", emphasized: true)
        root.lineageChildPaneIDs = ["p-child"]
        root.stallLevel = "hard"
        root.stallNotice = "Worker has waited 15 minutes on a question"
        var child = agent("p-child", "Worker", group: "seen", symbol: "?", status: "Question", demand: "question", activity: "stopped")
        child.lineageDepth = 1
        let result = board([checkout("feature", panes: ["p-root", "p-child"])], [root, child])
        #expect(result.lanes[0].cards[0].stallNotice == "Worker has waited 15 minutes on a question")
        #expect(result.lanes[0].cards[1].stallNotice == nil)
        #expect(result.attention.map(\.id) == ["p-root"])
    }

    // MARK: Attention strip and states

    @Test func theAttentionStripHoldsNeedsYouAndDoneAcrossLanesInTheCoreOrder() {
        let result = board(
            [checkout("a", panes: ["p-1", "p-3"]), checkout("b", panes: ["p-2", "p-4"])],
            [
                agent("p-2", "Question", group: "needs_you", symbol: "?", status: "Question", demand: "question", activity: "stopped", emphasized: true),
                agent("p-1", "Done", group: "done", symbol: "✓", status: "Done", activity: "stopped", emphasized: true),
                agent("p-3", "Working"),
                agent("p-4", "Idle", group: "seen", symbol: "○", status: "Idle", activity: "stopped"),
            ]
        )
        #expect(result.attention.map(\.id) == ["p-2", "p-1"])
        #expect(result.attention.map(\.laneID) == ["checkout-b", "checkout-a"])
    }

    @Test func noCheckoutsIsOneEmptyLaneAndNoAgentsKeepsTheTrack() {
        let empty = board([], [agent("p-elsewhere", "Not in this project")])
        #expect(empty.isEmpty)
        #expect(empty.attention.isEmpty)
        #expect(empty.counts.total == 0)
        #expect(ProjectHomeBoard.noCheckoutsSentence == "No checkouts in this project yet.")

        let quiet = board([checkout("feature", ahead: 1)], [], facts: [
            "\(Self.root).worktrees/feature": ProjectHomeWorktreeFact(agentLine: CoreWorktreeAgentLine(
                uninstrumentedReason: "Hooks are not installed", uninstrumentedLabel: "Uninstrumented"
            )),
        ]).lanes[0]
        #expect(quiet.cards.isEmpty)
        #expect(quiet.track[1].label == "↑1 ↓0")
        #expect(quiet.uninstrumentedReason == "Hooks are not installed")
        #expect(quiet.uninstrumentedLabel == "Uninstrumented")
        #expect(quiet.summary.agentCount == 0)
    }

    @Test func aLaneHeaderCarriesTheSidebarRowsAgentSummary() {
        var summary = CoreCheckoutAgentSummary(needsYou: 1, working: 2)
        summary.representativePaneID = "p-q"
        let lane = board(
            [checkout("feature", panes: ["p-q", "p-w1", "p-w2"], summary: summary)],
            [agent("p-q", "Ask", group: "needs_you", symbol: "?", status: "Question"), agent("p-w1", "One"), agent("p-w2", "Two")]
        ).lanes[0]
        #expect(lane.summary.agentCount == 3)
        #expect(lane.summary.status?.symbol == "?")
        #expect(lane.summary.representativeAgentKind == "claude")
        #expect(lane.summary.detailTooltip.hasPrefix("Needs You: 1 · Working: 2"))
    }

    @Test func aDisconnectedBoardKeepsItsCardsAndCountsAndMarksEveryStatusDisconnected() {
        let agents = [
            agent("p-1", "Question", group: "needs_you", symbol: "?", status: "Question", demand: "question", activity: "stopped", emphasized: true),
            agent("p-2", "Working"),
        ]
        let checkouts = [checkout("feature", panes: ["p-1", "p-2"], summary: CoreCheckoutAgentSummary(needsYou: 1, working: 1))]
        let live = board(checkouts, agents)
        let stale = board(checkouts, agents, connected: false)
        #expect(!stale.connected)
        #expect(stale.counts == live.counts)
        #expect(stale.lanes[0].cards.map(\.id) == live.lanes[0].cards.map(\.id))
        #expect(stale.lanes[0].cards.map(\.status.symbol) == ["⊘", "⊘"])
        #expect(stale.lanes[0].cards.map(\.status.label) == ["Disconnected", "Disconnected"])
        #expect(live.lanes[0].cards[0].status.symbol == "?")
    }

    @Test func aNonGitProjectDrawsEveryStageHollow() {
        let folder = ProjectHomeBoard.build(ProjectHomeInput(
            projectLabel: "notes", projectPath: "/fixtures/notes", isGit: false,
            checkouts: [checkout("notes", primary: true)], agents: []
        ))
        #expect(folder.lanes[0].kindIcon == "folder")
        #expect(folder.lanes[0].track.map(\.state) == Array(repeating: .hollow(reason: "Not a Git repository"), count: 3))
    }

    @Test func aParentFannedOutAcrossLanesNamesTwoLanesAndCountsTheRest() {
        var parent = agent("p-root", "Orchestrator")
        parent.lineageChildPaneIDs = ["c-1", "c-2", "c-3", "c-4"]
        var children: [SidebarAgent] = []
        for pane in ["c-1", "c-2", "c-3", "c-4"] {
            var child = agent(pane, "Worker \(pane)")
            child.lineageDepth = 1
            children.append(child)
        }
        let result = board([
            checkout("main", panes: ["p-root"], primary: true),
            checkout("alpha", panes: ["c-1", "c-2"]),
            checkout("beta", panes: ["c-3"]),
            checkout("gamma", panes: ["c-4"]),
        ], [parent] + children)
        let root = result.card("p-root")!
        #expect(root.foreignChildLaneLabels == ["alpha", "alpha", "beta", "gamma"])
        #expect(root.toLanesCaption == "↳ to alpha, beta +1")
        #expect(result.family(of: "p-root") == ["p-root", "c-1", "c-2", "c-3", "c-4"])
        #expect(result.family(of: "c-3") == ["p-root", "c-3"])
        #expect(result.card("c-1")!.toLanesCaption == nil)
    }

    // MARK: Lane order and strip fold

    @Test func lanesKeepTheirSlotsWhileOpenAndNewOnesAppendUntilSorted() {
        let quiet = checkout("quiet", panes: ["p-q"])
        let busy = checkout("busy", panes: ["p-b"], summary: CoreCheckoutAgentSummary(needsYou: 1))
        let first = board([quiet, busy], [agent("p-q", "Quiet"), agent("p-b", "Busy")])
        let opened = ProjectHomeLaneOrder.settle(nil, projectPath: Self.root, rankedLanes: first.lanes)
        #expect(opened.laneIDs == ["checkout-busy", "checkout-quiet"])
        #expect(opened.isRanked(against: first.lanes))

        // Quiet enters Needs You, busy leaves it, and a third checkout
        // appears: the slots hold, the new lane goes last.
        let quietNow = checkout("quiet", panes: ["p-q"], summary: CoreCheckoutAgentSummary(needsYou: 2))
        let busyNow = checkout("busy", panes: ["p-b"], summary: CoreCheckoutAgentSummary(working: 1))
        let extra = checkout("extra", panes: ["p-e"], summary: CoreCheckoutAgentSummary(needsYou: 3))
        let later = board([quietNow, busyNow, extra], [agent("p-q", "Quiet"), agent("p-b", "Busy"), agent("p-e", "Extra")])
        #expect(later.lanes.map(\.id) == ["checkout-extra", "checkout-quiet", "checkout-busy"])
        let held = ProjectHomeLaneOrder.settle(opened, projectPath: Self.root, rankedLanes: later.lanes)
        #expect(held.laneIDs == ["checkout-busy", "checkout-quiet", "checkout-extra"])
        #expect(held.apply(to: later.lanes).map(\.id) == held.laneIDs)
        #expect(!held.isRanked(against: later.lanes))

        // A filter narrows the lanes but not their order.
        #expect(held.apply(to: later.filtered(query: "Extra").lanes).map(\.id) == ["checkout-extra"])
        #expect(held.apply(to: later.filtered(query: "quiet").lanes + later.filtered(query: "busy").lanes).map(\.id)
                == ["checkout-busy", "checkout-quiet"])

        // Quiet entered Needs You while its slot was frozen and extra
        // arrived already in it, so both carry the mark; busy was there at
        // the rank and does not. A sort clears them.
        #expect(opened.needsYouAtRank == ["checkout-busy"])
        #expect(held.enteredNeedsYou(in: later.lanes) == Set(["checkout-quiet", "checkout-extra"]))
        let sorted = ProjectHomeLaneOrder.settle(nil, projectPath: Self.root, rankedLanes: later.lanes)
        #expect(sorted.enteredNeedsYou(in: later.lanes).isEmpty)
        // A lane that leaves Needs You again drops the mark on its own.
        #expect(held.enteredNeedsYou(in: first.lanes).isEmpty)

        // A gone lane drops; Sort or another project re-ranks.
        let fewer = ProjectHomeLaneOrder.settle(held, projectPath: Self.root, rankedLanes: first.lanes)
        #expect(fewer.laneIDs == ["checkout-busy", "checkout-quiet"])
        #expect(ProjectHomeLaneOrder.settle(nil, projectPath: Self.root, rankedLanes: later.lanes).laneIDs == later.lanes.map(\.id))
        #expect(ProjectHomeLaneOrder.settle(held, projectPath: "/elsewhere", rankedLanes: later.lanes).laneIDs == later.lanes.map(\.id))
    }

    @Test func theStripShowsTwoRowsAndKeepsOneSlotForTheMoreChip() {
        #expect(ProjectHomeAttentionFold.perRow(width: 660, cardWidth: 200, spacing: 8) == 3)
        #expect(ProjectHomeAttentionFold.perRow(width: 615, cardWidth: 200, spacing: 8) == 2)
        #expect(ProjectHomeAttentionFold.perRow(width: 100, cardWidth: 200, spacing: 8) == 1)
        #expect(ProjectHomeAttentionFold.visibleCount(total: 6, perRow: 3, expanded: false) == 6)
        #expect(ProjectHomeAttentionFold.visibleCount(total: 7, perRow: 3, expanded: false) == 5)
        #expect(ProjectHomeAttentionFold.visibleCount(total: 12, perRow: 3, expanded: false) == 5)
        #expect(ProjectHomeAttentionFold.visibleCount(total: 12, perRow: 3, expanded: true) == 12)
        #expect(ProjectHomeAttentionFold.visibleCount(total: 3, perRow: 1, expanded: false) == 1)
        #expect(ProjectHomeAttentionFold.moreLabel(hidden: 7) == "+7 more")
        #expect(ProjectHomeAttentionFold.fewerLabel == "Show fewer")
    }

    // MARK: Search

    @Test func aQueryKeepsMatchingCardsWithTheirAncestorsAndWholeLanesItNames() {
        var parent = agent("p-root", "Orchestrator")
        parent.lineageChildPaneIDs = ["p-child"]
        var child = agent("p-child", "마이그레이션 작성", detail: "Writing the migration")
        child.lineageDepth = 1
        let result = board(
            [checkout("feature", panes: ["p-root", "p-child"]), checkout("docs", panes: ["p-docs"])],
            [parent, child, agent("p-docs", "Docs writer", group: "done", symbol: "✓", status: "Done", activity: "stopped", emphasized: true)]
        )
        let byChild = result.filtered(query: "마이그레이션")
        #expect(byChild.lanes.map(\.label) == ["feature"])
        #expect(byChild.lanes[0].cards.map(\.id) == ["p-root", "p-child"])
        #expect(byChild.attention.isEmpty)

        let byLane = result.filtered(query: "docs")
        #expect(byLane.lanes.map(\.label) == ["docs"])
        #expect(byLane.lanes[0].cards.map(\.id) == ["p-docs"])
        #expect(byLane.attention.map(\.id) == ["p-docs"])

        let nothing = result.filtered(query: "zzz")
        #expect(nothing.isEmpty)
        #expect(nothing.counts == result.counts)
        #expect(result.filtered(query: "  ") == result)
    }

    // MARK: Overlay contract

    @Test func escapeClosesTheOverlayOnlyWhenNoSheetIsAboveIt() {
        let escape = NSEvent.keyEvent(
            with: .keyDown, location: .zero, modifierFlags: [], timestamp: 0, windowNumber: 0, context: nil,
            characters: "\u{1B}", charactersIgnoringModifiers: "\u{1B}", isARepeat: false, keyCode: 53
        )!
        #expect(ProjectHomeShortcutPolicy.shouldClose(escape, visible: true, sheetPresented: false))
        #expect(!ProjectHomeShortcutPolicy.shouldClose(escape, visible: true, sheetPresented: true))
        #expect(!ProjectHomeShortcutPolicy.shouldClose(escape, visible: false, sheetPresented: false))
        #expect(ShellMenuCommand.projectHome.shortcut.canonical == "command+shift+h")
        #expect(ShellMenuCommand.projectHome.title == "Project Home")
        #expect(ShellMenuCommand.allCases.filter { $0.shortcut == ShellMenuCommand.projectHome.shortcut } == [.projectHome])
        #expect(PaneCommand.allCases.filter { $0.defaultShortcut == ShellMenuCommand.projectHome.shortcut }.isEmpty)
    }

    @Test func theMemoRebuildsOnlyWhenTheKeyMoves() {
        let memo = ProjectHomeMemo()
        var builds = 0
        let input = ProjectHomeInput(projectLabel: "hide", projectPath: Self.root, checkouts: [], agents: [])
        let key = ProjectHomeMemo.Key(revision: 4, workspaceID: "w1", connected: true)
        _ = memo.board(for: key) { builds += 1; return ProjectHomeBoard.build(input) }
        _ = memo.board(for: key) { builds += 1; return ProjectHomeBoard.build(input) }
        #expect(builds == 1)
        _ = memo.board(for: ProjectHomeMemo.Key(revision: 5, workspaceID: "w1", connected: true)) {
            builds += 1; return ProjectHomeBoard.build(input)
        }
        #expect(builds == 2)
    }
}

@Suite("Project Home wrap layout")
struct ProjectHomeWrapLayoutTests {
    @Test func groupsWrapAtTheWidthAndARowIsAsTallAsItsTallestGroup() {
        let sizes = [CGSize(width: 100, height: 40), CGSize(width: 100, height: 70), CGSize(width: 100, height: 40)]
        let arranged = ProjectHomeWrapLayout.arrange(sizes: sizes, width: 230, horizontalSpacing: 10, verticalSpacing: 6)
        #expect(arranged.frames.map(\.origin) == [CGPoint(x: 0, y: 0), CGPoint(x: 110, y: 0), CGPoint(x: 0, y: 76)])
        #expect(arranged.size == CGSize(width: 210, height: 116))
    }

    @Test func aGroupWiderThanTheLaneTakesItsOwnRowAndAnUnboundedWidthIsOneRow() {
        let sizes = [CGSize(width: 300, height: 20), CGSize(width: 50, height: 20)]
        let narrow = ProjectHomeWrapLayout.arrange(sizes: sizes, width: 200, horizontalSpacing: 8, verticalSpacing: 8)
        #expect(narrow.frames.map(\.origin.y) == [0, 28])
        #expect(narrow.frames[0].width == 200)
        #expect(narrow.size.width == 200)
        let wide = ProjectHomeWrapLayout.arrange(sizes: sizes, width: .infinity, horizontalSpacing: 8, verticalSpacing: 8)
        #expect(wide.frames.map(\.origin) == [CGPoint(x: 0, y: 0), CGPoint(x: 308, y: 0)])
        #expect(ProjectHomeWrapLayout.arrange(sizes: [], width: 100, horizontalSpacing: 8, verticalSpacing: 8).size == .zero)
    }
}
