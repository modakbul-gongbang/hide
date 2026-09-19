import Foundation
import SwiftUI
import Testing
@testable import HerdrMacOS

/// Project Home's fixtures: a project of checkouts with panes, and agents
/// placed in them by pane id. Built with the memberwise inits the other
/// presentation tests use, so a case states only what it exercises.
enum ProjectHomeFixture {
    static func checkout(
        _ id: String, branch: String, panes: [String], isWorktree: Bool = true,
        exists: Bool = true, ahead: Int = 0, behind: Int = 0, changed: Int = 0, dirty: Bool = false,
        pullRequest: CorePullRequest? = nil
    ) -> CoreCheckoutSnapshot {
        CoreCheckoutSnapshot(
            id: id, workspaceID: "ws", label: branch, path: "/fixture/\(id)", branch: branch,
            isWorktree: isWorktree, exists: exists, temporary: false, hasPanes: !panes.isEmpty,
            dirty: dirty, changedFileCount: changed, ahead: ahead, behind: behind, pullRequest: pullRequest,
            tabs: panes.isEmpty ? [] : [CoreTabSnapshot(
                id: "tab-\(id)", workspaceID: "ws", checkoutID: id, label: "Tab", empty: false,
                panes: panes.map { CorePaneSnapshot(id: $0, cwd: "/fixture/\(id)", statusLabel: "Idle", activityAt: nil) }
            )]
        )
    }

    static func workspace(_ checkouts: [CoreCheckoutSnapshot], name: String = "hide") -> CoreWorkspaceSnapshot {
        CoreWorkspaceSnapshot(
            id: "ws", label: name, path: "/fixture/main", remoteTargetID: nil, expanded: true,
            deviceID: "local", repoName: name, isGit: true, defaultBranch: "main",
            registered: true, temporary: false, checkouts: checkouts
        )
    }

    static func agent(
        _ paneID: String, identity: String, group: String = "seen", symbol: String = "○",
        demand: String = "none", activity: String = "stopped", emphasized: Bool = false,
        statusLabel: String = "Idle", detail: String? = nil, children: [String] = [], depth: Int = 0,
        delegated: Bool = false, stallLevel: String = "", stallNotice: String? = nil,
        lastActivity: String = "00000000000000000001", elapsed: String = "3m"
    ) -> SidebarAgent {
        var agent = SidebarAgent(
            id: paneID, paneID: paneID, workspaceLabel: "hide", agentKind: "claude",
            demand: demand, activity: activity, unread: group == "needs_you" || group == "done",
            group: group, symbol: symbol, emphasized: emphasized, statusLabel: statusLabel,
            identityLabel: identity, detail: detail, statusWordVisible: detail == nil,
            elapsed: elapsed, lastActivity: lastActivity
        )
        agent.lineageChildPaneIDs = children
        agent.lineageDepth = depth
        agent.delegated = delegated
        agent.stallLevel = stallLevel
        agent.stallNotice = stallNotice
        return agent
    }

    static func pullRequest(_ number: Int, badge: CorePullRequestBadge = .open, draft: Bool = false, checks: CorePullRequestChecks? = .passing) -> CorePullRequest {
        CorePullRequest(
            title: "PR \(number)", checks: checks, number: number, headBranch: "topic", baseBranch: "main",
            url: "https://example.invalid/pull/\(number)", badge: badge, review: nil, isDraft: draft,
            mergedAtUnixMS: nil, updatedAtUnixMS: nil
        )
    }

    /// Four checkouts, nine agents, one delegation chain two deep: the
    /// screenshot fixture's shape.
    static func fourCheckouts() -> (CoreWorkspaceSnapshot, [SidebarAgent]) {
        let workspace = workspace([
            checkout("main", branch: "main", panes: ["p-main-1"], isWorktree: false, pullRequest: nil),
            checkout("home", branch: "prd/home-graph", panes: ["p-home-1", "p-home-2", "p-home-3"], ahead: 4, changed: 2, dirty: true, pullRequest: pullRequest(110, draft: true, checks: .pending)),
            checkout("board", branch: "prd/home-board", panes: ["p-board-1", "p-board-2"], ahead: 2, pullRequest: pullRequest(111)),
            checkout("labels", branch: "feature/labels", panes: ["p-labels-1", "p-labels-2", "p-labels-3"], behind: 3),
        ])
        let agents = [
            agent("p-main-1", identity: "Release notes", group: "done", symbol: "✓", emphasized: true, statusLabel: "Done", detail: "Wrote the changelog", lastActivity: "1789000000000"),
            agent("p-home-1", identity: "Constellation layout", group: "needs_you", symbol: "?", demand: "question", emphasized: true, statusLabel: "Question", detail: "Which ring radius?", children: ["p-home-2", "p-home-3"], lastActivity: "1789000100000"),
            agent("p-home-2", identity: "Label collision tests", group: "working", symbol: "●", activity: "working", statusLabel: "Working", detail: "Running the suite", depth: 1, delegated: true, lastActivity: "1789000200000"),
            agent("p-home-3", identity: "Docs subsection", group: "seen", symbol: "○", depth: 1, delegated: true, lastActivity: "1789000300000"),
            agent("p-board-1", identity: "Lane board", group: "working", symbol: "●", activity: "working", statusLabel: "Working", detail: "Drawing lanes", lastActivity: "1789000400000"),
            agent("p-board-2", identity: "Board tests", group: "seen", symbol: "~", activity: "unknown", statusLabel: "Unknown", lastActivity: "1789000500000"),
            agent("p-labels-1", identity: "한국어 레이블 정리", group: "needs_you", symbol: "!", demand: "approval", emphasized: true, statusLabel: "Approval", detail: "Approve the rename", lastActivity: "1789000600000"),
            agent("p-labels-2", identity: "Plugin watcher", group: "done", symbol: "✓", emphasized: true, statusLabel: "Done", lastActivity: "1789000700000"),
            agent("p-labels-3", identity: "Orphaned child", group: "seen", symbol: "○", depth: 1, delegated: false, lastActivity: "1789000800000"),
        ]
        return (workspace, agents)
    }

    /// Twelve checkouts and thirty agents, some with long Korean names and
    /// a delegated child on every checkout with three or more, for the
    /// label-separation guarantee.
    static func twelveCheckouts() -> (CoreWorkspaceSnapshot, [SidebarAgent]) {
        var checkouts: [CoreCheckoutSnapshot] = []
        var agents: [SidebarAgent] = []
        let names = [
            "Release notes", "Constellation layout", "긴 한국어 작업 이름을 가진 에이전트", "Plugin watcher",
            "A very long identifier that will be truncated by the label width", "Board", "Lane board tests",
            "사이드바 정리", "Docs", "Hooks", "Usage popover", "Explorer decorations",
        ]
        var pane = 0
        for index in 0..<12 {
            let count = [1, 2, 3, 4, 2, 3, 1, 4, 2, 3, 2, 3][index]
            var panes: [String] = []
            for slot in 0..<count {
                let id = "p\(pane)"
                pane += 1
                panes.append(id)
                let group = ["seen", "working", "needs_you", "done"][(index + slot) % 4]
                let symbol = ["○", "●", "?", "✓"][(index + slot) % 4]
                // The third pane is delegated by the first.
                let children = slot == 0 && count >= 3 ? ["p\(pane + 1)"] : []
                agents.append(agent(id, identity: names[(index + slot) % names.count], group: group, symbol: symbol,
                                    demand: group == "needs_you" ? "question" : "none",
                                    activity: group == "working" ? "working" : "stopped",
                                    emphasized: group != "seen", children: children, depth: slot == 2 ? 1 : 0,
                                    delegated: slot == 2, lastActivity: "17890000\(String(format: "%05d", pane))"))
            }
            checkouts.append(checkout("c\(index)", branch: index == 0 ? "main" : "feature/branch-\(index)", panes: panes, isWorktree: index != 0,
                                      pullRequest: index % 3 == 0 ? pullRequest(100 + index) : nil))
        }
        return (workspace(checkouts), agents)
    }
}

@Suite("Project Home presentation")
struct ProjectHomePresentationTests {
    @Test func aMissingSnapshotIsLoadingAndNothingElse() {
        let home = ProjectHomePresentation.build(workspace: nil, agents: [], connected: true)
        #expect(home.shape == .loading)
        #expect(home.nodes.isEmpty)
        #expect(home.rail.isEmpty)
    }

    @Test func aProjectWithoutCheckoutsIsTheProjectNodeAndTheEmptySentence() {
        let home = ProjectHomePresentation.build(workspace: ProjectHomeFixture.workspace([]), agents: [], connected: true)
        #expect(home.shape == .noCheckouts)
        #expect(home.nodes.map(\.kind) == [.project])
        #expect(home.emptyMessage?.contains("No checkouts") == true)
        #expect(home.counts == .zero)
    }

    @Test func checkoutsWithoutAgentsAreHubsAroundTheProjectWithTheEmptySentence() {
        let workspace = ProjectHomeFixture.workspace([
            ProjectHomeFixture.checkout("main", branch: "main", panes: [], isWorktree: false),
            ProjectHomeFixture.checkout("b", branch: "feature/b", panes: ["p1"]),
        ])
        let home = ProjectHomePresentation.build(workspace: workspace, agents: [], connected: true)
        #expect(home.shape == .noAgents)
        #expect(home.nodes.map(\.kind) == [.project, .checkout, .checkout])
        #expect(home.edges.allSatisfy { $0.kind == .membership })
        #expect(home.emptyMessage?.contains("Nobody is working") == true)
    }

    @Test func thePrimaryCheckoutLeadsAndTheRestFollowBranchName() {
        let workspace = ProjectHomeFixture.workspace([
            ProjectHomeFixture.checkout("z", branch: "zeta", panes: []),
            ProjectHomeFixture.checkout("main", branch: "main", panes: [], isWorktree: false),
            ProjectHomeFixture.checkout("a", branch: "alpha", panes: []),
        ])
        let ordered = ProjectHomePresentation.orderedCheckouts(workspace).map(\.id)
        #expect(ordered == ["main", "a", "z"])
        let home = ProjectHomePresentation.build(workspace: workspace, agents: [], connected: true)
        #expect(home.nodes.filter { $0.kind == .checkout }.map(\.layout.rank) == [0, 1, 2])
    }

    @Test func agentsHangOffTheirCheckoutAndChildrenOffTheirParentWithDashedEdges() {
        let (workspace, agents) = ProjectHomeFixture.fourCheckouts()
        let home = ProjectHomePresentation.build(workspace: workspace, agents: agents, connected: true)
        #expect(home.shape == .populated)
        #expect(home.nodes.filter { $0.kind == .agent }.count == 9)
        #expect(home.nodes.filter { $0.kind == .checkout }.count == 4)
        #expect(home.nodes.filter { $0.kind == .pullRequest }.count == 2)

        let parent = ProjectHomeModel.agentNodeID("p-home-1")
        let child = ProjectHomeModel.agentNodeID("p-home-2")
        let delegation = home.edges.first { $0.from == parent && $0.to == child }
        #expect(delegation?.kind == .delegation)
        #expect(delegation?.dashed == true)
        #expect(home.node(child)?.layout.parentID == parent)
        #expect(home.node(child)?.layout.kind == .child)
        #expect(home.node(child)?.delegated == true)
        // A delegated child is smaller than an operator-owned agent of the same recency.
        #expect(home.node(child)!.radius < home.node(parent)!.radius)

        let root = home.node(ProjectHomeModel.agentNodeID("p-board-1"))
        #expect(root?.layout.parentID == ProjectHomeModel.checkoutNodeID("board"))
        #expect(home.edges.first { $0.to == root?.id }?.kind == .agent)
    }

    @Test func anOrphanRootStaysOnItsCheckoutAndStaysBright() {
        let (workspace, agents) = ProjectHomeFixture.fourCheckouts()
        let home = ProjectHomePresentation.build(workspace: workspace, agents: agents, connected: true)
        let orphan = home.node(ProjectHomeModel.agentNodeID("p-labels-3"))
        #expect(orphan?.layout.parentID == ProjectHomeModel.checkoutNodeID("labels"))
        #expect(orphan?.layout.kind == .agent)
        #expect(orphan?.delegated == false)
    }

    @Test func aChildWhoseParentIsInAnotherProjectIsARootHere() {
        let workspace = ProjectHomeFixture.workspace([
            ProjectHomeFixture.checkout("main", branch: "main", panes: ["p1"], isWorktree: false),
        ])
        let child = ProjectHomeFixture.agent("p1", identity: "Child", depth: 1, delegated: true)
        let foreignParent = ProjectHomeFixture.agent("elsewhere", identity: "Parent", children: ["p1"])
        let home = ProjectHomePresentation.build(workspace: workspace, agents: [foreignParent, child], connected: true)
        #expect(home.nodes.filter { $0.kind == .agent }.map(\.paneID) == ["p1"])
        #expect(home.node(ProjectHomeModel.agentNodeID("p1"))?.layout.parentID == ProjectHomeModel.checkoutNodeID("main"))
    }

    @Test func statusColourAndMarkAreTheCoresAndTheStallHaloRidesTheRoot() {
        let (workspace, agents) = ProjectHomeFixture.fourCheckouts()
        var stalled = agents
        stalled[1].stallLevel = "hard"
        stalled[1].stallNotice = "Label collision tests has waited 15m on a question"
        let home = ProjectHomePresentation.build(workspace: workspace, agents: stalled, connected: true)
        let question = home.node(ProjectHomeModel.agentNodeID("p-home-1"))!
        #expect(question.symbol == "?")
        #expect(question.color == HideTheme.warning)
        #expect(question.stallLevel == "hard")
        #expect(question.card?.stallNotice?.contains("waited 15m") == true)
        let working = home.node(ProjectHomeModel.agentNodeID("p-board-1"))!
        #expect(working.symbol == "●")
        #expect(working.color == HideTheme.agentWorking)
        let done = home.node(ProjectHomeModel.agentNodeID("p-main-1"))!
        #expect(done.symbol == "✓")
        #expect(done.color == HideTheme.success)
        let unknown = home.node(ProjectHomeModel.agentNodeID("p-board-2"))!
        #expect(unknown.symbol == "~")
        #expect(unknown.card?.statusLabel == "Unknown")
    }

    @Test func disconnectedKeepsTheMapDimmedWithTheDisconnectedMark() {
        let (workspace, agents) = ProjectHomeFixture.fourCheckouts()
        let home = ProjectHomePresentation.build(workspace: workspace, agents: agents, connected: false)
        #expect(home.stale)
        #expect(home.shape == .populated)
        #expect(home.nodes.filter { $0.kind == .agent }.allSatisfy { $0.symbol == "⊘" })
        #expect(home.rail.allSatisfy { $0.statusLabel == "Disconnected" })
        // The topology is the connected one, so the layout does not move.
        let live = ProjectHomePresentation.build(workspace: workspace, agents: agents, connected: true)
        #expect(home.topology.key == live.topology.key)
    }

    @Test func aMissingWorktreeIsHollowWithABadgeAndCannotBeOpened() {
        let workspace = ProjectHomeFixture.workspace([
            ProjectHomeFixture.checkout("main", branch: "main", panes: [], isWorktree: false),
            ProjectHomeFixture.checkout("gone", branch: "feature/gone", panes: [], exists: false),
        ])
        let home = ProjectHomePresentation.build(workspace: workspace, agents: [], connected: true)
        let gone = home.node(ProjectHomeModel.checkoutNodeID("gone"))!
        #expect(gone.missing)
        #expect(!gone.opensOnActivate)
        #expect(gone.accessibilityLabel.contains("missing"))
        // The fixture checkouts carry no worktree projection, so neither opens.
        #expect(!home.node(ProjectHomeModel.checkoutNodeID("main"))!.opensOnActivate)
    }

    @Test func thePullRequestIsTheCheckoutsOutwardEdgeInItsStateColour() {
        let (workspace, agents) = ProjectHomeFixture.fourCheckouts()
        let home = ProjectHomePresentation.build(workspace: workspace, agents: agents, connected: true)
        let draft = home.node(ProjectHomeModel.pullRequestNodeID("home"))!
        #expect(draft.label == "#110")
        #expect(draft.color == HideTheme.PullRequest.draft)
        #expect(draft.fullLabel == "#110 Draft · Running")
        #expect(draft.layout.parentID == ProjectHomeModel.checkoutNodeID("home"))
        let edge = home.edges.first { $0.to == draft.id }!
        #expect(edge.kind == .pullRequest)
        #expect(edge.color == HideTheme.PullRequest.draft)
        let open = home.node(ProjectHomeModel.pullRequestNodeID("board"))!
        #expect(open.color == HideTheme.PullRequest.open)
        #expect(home.node(ProjectHomeModel.pullRequestNodeID("main")) == nil)
    }

    @Test func checkoutDetailSaysAheadBehindAndChangesOnlyWhenNonZero() {
        let (workspace, _) = ProjectHomeFixture.fourCheckouts()
        let home = ProjectHomePresentation.build(workspace: workspace, agents: [], connected: true)
        #expect(home.node(ProjectHomeModel.checkoutNodeID("home"))?.detail == "↑4 · 2 changes")
        #expect(home.node(ProjectHomeModel.checkoutNodeID("labels"))?.detail == "↓3")
        #expect(home.node(ProjectHomeModel.checkoutNodeID("main"))?.detail == nil)
        let dirty = ProjectHomeFixture.checkout("d", branch: "d", panes: [], changed: 1, dirty: true)
        #expect(ProjectHomePresentation.checkoutDetail(dirty) == "1 change")
    }

    @Test func headerCountsAndTheAttentionRailFollowTheCoresGroups() {
        let (workspace, agents) = ProjectHomeFixture.fourCheckouts()
        let home = ProjectHomePresentation.build(workspace: workspace, agents: agents, connected: true)
        #expect(home.counts == ProjectHomeCounts(needsYou: 2, done: 2, working: 2, seen: 3))
        #expect(home.rail.map(\.paneID) == ["p-main-1", "p-home-1", "p-labels-1", "p-labels-2"])
        #expect(home.rail.map(\.group) == [.done, .needsYou, .needsYou, .done])
        let approval = home.rail.first { $0.paneID == "p-labels-1" }!
        #expect(approval.title == "한국어 레이블 정리")
        #expect(approval.detail == "Approve the rename")
        #expect(approval.checkoutLabel == "feature/labels")
        #expect(approval.statusColor == HideTheme.warning)
    }

    @Test func agentsOutsideTheFocusedProjectNeverReachTheMapOrTheCounts() {
        let (workspace, agents) = ProjectHomeFixture.fourCheckouts()
        let stranger = ProjectHomeFixture.agent("elsewhere", identity: "Other project", group: "needs_you", symbol: "?", demand: "question")
        let home = ProjectHomePresentation.build(workspace: workspace, agents: agents + [stranger], connected: true)
        #expect(home.node(paneID: "elsewhere") == nil)
        #expect(home.counts.needsYou == 2)
        #expect(!home.rail.contains { $0.paneID == "elsewhere" })
    }

    @Test func nodeSizeFollowsRecencyOfLastActivity() {
        let now = Date(timeIntervalSince1970: 1_789_003_600)
        let radii = HideTheme.Home.agentNodeRadii
        #expect(ProjectHomePresentation.radius(for: ProjectHomeFixture.agent("a", identity: "a", lastActivity: "1789003000000"), now: now) == radii[2])
        #expect(ProjectHomePresentation.radius(for: ProjectHomeFixture.agent("a", identity: "a", lastActivity: "1788950000000"), now: now) == radii[1])
        #expect(ProjectHomePresentation.radius(for: ProjectHomeFixture.agent("a", identity: "a", lastActivity: "1780000000000"), now: now) == radii[0])
        #expect(ProjectHomePresentation.radius(for: ProjectHomeFixture.agent("a", identity: "a", lastActivity: "00000000000000000009"), now: now) == radii[1])
    }

    @Test func checkoutSizeGrowsWithAgentCountUpToTheCap() {
        let (workspace, agents) = ProjectHomeFixture.fourCheckouts()
        let home = ProjectHomePresentation.build(workspace: workspace, agents: agents, connected: true)
        let one = home.node(ProjectHomeModel.checkoutNodeID("main"))!.radius
        let three = home.node(ProjectHomeModel.checkoutNodeID("home"))!.radius
        #expect(three > one)
        #expect(three <= HideTheme.Home.checkoutNodeRadiusMax)
    }

    @Test func labelsTruncateByEstimatedWidthAndKeepTheFullTextForTheCard() {
        let long = "A very long identifier that will be truncated by the label width"
        let cut = ProjectHomePresentation.truncated(long)
        #expect(cut.hasSuffix("…"))
        #expect(cut.count < long.count)
        #expect(ProjectHomePresentation.labelWidth(long) == HideTheme.Home.labelMaxWidth)
        let korean = "긴 한국어 작업 이름을 가진 에이전트"
        let koreanCut = ProjectHomePresentation.truncated(korean)
        #expect(koreanCut.count < korean.count)
        #expect(ProjectHomePresentation.truncated("Docs") == "Docs")
        let (workspace, agents) = ProjectHomeFixture.twelveCheckouts()
        let home = ProjectHomePresentation.build(workspace: workspace, agents: agents, connected: true)
        let node = home.nodes.first { $0.fullLabel == long }!
        #expect(node.label == cut)
        #expect(node.card?.title == long)
        #expect(node.accessibilityLabel.hasPrefix(long))
    }

    @Test func initialFocusIsTheFocusedPanesAgentThenTheCheckoutThenNothing() {
        let (workspace, agents) = ProjectHomeFixture.fourCheckouts()
        let home = ProjectHomePresentation.build(workspace: workspace, agents: agents, connected: true)
        #expect(ProjectHomePresentation.initialFocus(home, focusedPaneID: "p-board-1", focusedCheckoutID: "home") == ProjectHomeModel.agentNodeID("p-board-1"))
        #expect(ProjectHomePresentation.initialFocus(home, focusedPaneID: "not-here", focusedCheckoutID: "home") == ProjectHomeModel.checkoutNodeID("home"))
        #expect(ProjectHomePresentation.initialFocus(home, focusedPaneID: nil, focusedCheckoutID: "unknown") == nil)
    }

    @Test func theLocalGraphIsDepthTwoAndHoverNarrowsToNeighbours() {
        let (workspace, agents) = ProjectHomeFixture.fourCheckouts()
        let home = ProjectHomePresentation.build(workspace: workspace, agents: agents, connected: true)
        let child = ProjectHomeModel.agentNodeID("p-home-2")
        let local = ProjectHomePresentation.emphasized(home, focus: child, hover: nil)!
        #expect(local.contains(child))
        #expect(local.contains(ProjectHomeModel.agentNodeID("p-home-1")))
        #expect(local.contains(ProjectHomeModel.checkoutNodeID("home")))
        #expect(local.contains(ProjectHomeModel.agentNodeID("p-home-3")))
        #expect(!local.contains(ProjectHomeModel.projectNodeID))
        #expect(!local.contains(ProjectHomeModel.checkoutNodeID("board")))
        let hovered = ProjectHomePresentation.emphasized(home, focus: child, hover: ProjectHomeModel.checkoutNodeID("board"))!
        #expect(hovered.contains(ProjectHomeModel.agentNodeID("p-board-1")))
        #expect(hovered.contains(ProjectHomeModel.projectNodeID))
        #expect(!hovered.contains(child))
        #expect(ProjectHomePresentation.emphasized(home, focus: nil, hover: nil) == nil)
        #expect(ProjectHomePresentation.emphasized(home, focus: "gone", hover: nil) == nil)
    }

    @Test func homeTakesTheIdleEmptyStateAndTheOverlayNeverCoversARemoteContext() {
        #expect(ProjectHomeEntryPolicy.drawsHome(startState: .idle, hasCheckout: true, projectionNotice: nil))
        #expect(!ProjectHomeEntryPolicy.drawsHome(startState: .starting, hasCheckout: true, projectionNotice: nil))
        #expect(!ProjectHomeEntryPolicy.drawsHome(startState: .failed("x"), hasCheckout: true, projectionNotice: nil))
        #expect(!ProjectHomeEntryPolicy.drawsHome(startState: .idle, hasCheckout: false, projectionNotice: nil))
        #expect(!ProjectHomeEntryPolicy.drawsHome(startState: .idle, hasCheckout: true, projectionNotice: "pane.projection_unavailable"))
        #expect(ProjectHomeEntryPolicy.drawsOverlay(visible: true, remote: false, hasCheckout: true))
        #expect(!ProjectHomeEntryPolicy.drawsOverlay(visible: true, remote: true, hasCheckout: true))
        #expect(!ProjectHomeEntryPolicy.drawsOverlay(visible: false, remote: false, hasCheckout: true))
    }

    @Test func projectHomeIsInTheViewMenuOnCommandShiftH() {
        #expect(ShellMenuCommand.projectHome.title == "Project Home")
        #expect(ShellMenuCommand.projectHome.shortcut.canonical == "command+shift+h")
        #expect(ShellMenuCommand.projectHome.displayShortcut == "⇧⌘H")
        #expect(ShellMenuCommand.projectHome.scope == .applicationMenu)
    }

    @MainActor
    @Test func theOverlayIsSessionLocalAndClosesOnAnyActionThatOpensAPane() async throws {
        let root = FileManager.default.temporaryDirectory
            .appendingPathComponent("hide-project-home-\(UUID().uuidString)", isDirectory: true)
        try FileManager.default.createDirectory(at: root, withIntermediateDirectories: true)
        let stateURL = FileManager.default.temporaryDirectory
            .appendingPathComponent("hide-project-home-state-\(UUID().uuidString).json")
        defer {
            try? FileManager.default.removeItem(at: root)
            try? FileManager.default.removeItem(at: stateURL)
        }
        let bridge = CoreBridge(arguments: [
            "HerdrMacOS",
            "--verification-ui-fixture",
            "--workspace-root", root.path,
            "--state-path", stateURL.path,
        ])
        try await Task.sleep(for: .milliseconds(100))
        let model = ShellModel(core: bridge)
        #expect(!model.projectHomeVisible)
        model.toggleProjectHome()
        #expect(model.projectHomeVisible)
        model.addTab()
        #expect(!model.projectHomeVisible)
        model.toggleProjectHome()
        // An agent that is gone opens nothing, so Home stays where it was.
        model.selectAgent(paneID: "absent")
        #expect(model.projectHomeVisible)
        #expect(model.interactionNotice?.contains("absent") == true)
    }
}
