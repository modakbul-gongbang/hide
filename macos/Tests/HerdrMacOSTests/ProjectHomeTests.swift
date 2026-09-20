import AppKit
import Foundation
import Testing
@testable import HerdrMacOS

@Suite("Project Home")
struct ProjectHomeTests {
    private func checkout(_ id: String = "task", changed: Int = 0, ahead: Int = 0,
                          pr: CorePullRequest? = nil, worktree: Bool = true) -> CoreCheckoutSnapshot {
        CoreCheckoutSnapshot(id: id, workspaceID: "project", label: id, path: "/fixture/\(id)", branch: id,
                             isWorktree: worktree, exists: true, temporary: false,
                             changedFileCount: changed, ahead: ahead, pullRequest: pr, tabs: [])
    }
    private func workspace(_ checkouts: [CoreCheckoutSnapshot], git: Bool = true) -> CoreWorkspaceSnapshot {
        CoreWorkspaceSnapshot(id: "project", label: "Project", path: "/fixture", remoteTargetID: nil,
                              expanded: true, deviceID: "local", repoName: "Project", isGit: git,
                              defaultBranch: "main", registered: true, temporary: false, checkouts: checkouts)
    }
    @Test func gitStagePriorityDoesNotDependOnAgentOrProjectStatus() throws {
        let open = try JSONDecoder().decode(CorePullRequest.self, from: Data(#"{"number":7,"head_branch":"task","base_branch":"main","url":"https://github.com/acme/project/pull/7","badge":"open","is_draft":false}"#.utf8))
        let merged = try JSONDecoder().decode(CorePullRequest.self, from: Data(#"{"number":7,"head_branch":"task","base_branch":"main","url":"https://github.com/acme/project/pull/7","badge":"merged","is_draft":false}"#.utf8))
        #expect(ProjectHomeStage.stage(checkout()) == .ready)
        #expect(ProjectHomeStage.stage(checkout(changed: 1)) == .working)
        #expect(ProjectHomeStage.stage(checkout(ahead: 1)) == .working)
        #expect(ProjectHomeStage.stage(checkout(changed: 3, pr: open)) == .review)
        #expect(ProjectHomeStage.stage(checkout(changed: 3, pr: merged)) == .merged)
        #expect(ProjectHomeStage.review.matches(projectStatus: "In Review"))
        #expect(!ProjectHomeStage.review.matches(projectStatus: "Done"))
    }
    @Test func linkedOpenIssuesAreDeduplicatedAndClosedIssuesNeverBecomeBacklog() {
        let open = CoreIssue(reference: .init(repository: "acme/project", number: 7), title: "한국어 작업",
                             url: "https://github.com/acme/project/issues/7", state: "OPEN", projectStatus: nil, updatedAtUnixMS: nil)
        let closed = CoreIssue(reference: .init(repository: "acme/project", number: 8), title: "Closed",
                               url: "https://github.com/acme/project/issues/8", state: "CLOSED", projectStatus: nil, updatedAtUnixMS: nil)
        var linked = checkout()
        linked.issue = CoreIssueLink(issue: open, source: "pane 토큰")
        var project = workspace([linked, checkout("main", worktree: false)])
        project.homeIssues = CoreProjectIssues(repository: "acme/project", issues: [open, closed], overflow: true)
        let board = ProjectHomeBoard.build(workspace: project, agents: [])
        #expect(board.tasks.count == 1)
        #expect(board.tasks[0].title == "한국어 작업")
        #expect(board.adHoc.isEmpty)
        #expect(board.overflow)
        var unlinked = workspace([])
        unlinked.homeIssues = project.homeIssues
        #expect(ProjectHomeBoard.build(workspace: unlinked, agents: []).tasks.map(\.id) == ["issue:acme/project#7"])
    }
    @Test func crossCheckoutDescendantsRemainOneRootRequestAndWarnWithoutMovingStage() {
        func ownedCheckout(_ id: String, pane: String) -> CoreCheckoutSnapshot {
            let row = CorePaneSnapshot(id: pane, cwd: "/fixture/\(id)", statusLabel: "Working", activityAt: nil)
            let tab = CoreTabSnapshot(id: "\(id):tab", workspaceID: "project", checkoutID: id, label: "Tab 1", empty: false, panes: [row])
            return CoreCheckoutSnapshot(id: id, workspaceID: "project", label: id, path: "/fixture/\(id)", branch: id,
                isWorktree: true, exists: true, temporary: false, hasPanes: true, changedFileCount: 1, tabs: [tab])
        }
        var parent = SidebarAgent(id: "parent", paneID: "p", workspaceLabel: "Project", agentKind: "terminal",
            demand: "question", group: "needs_you", symbol: "?", identityLabel: "질문", elapsed: "1m", lastActivity: "")
        parent.lineageChildPaneIDs = ["child"]
        var child = SidebarAgent(id: "child", paneID: "child", workspaceLabel: "Project", agentKind: "terminal",
            group: "working", symbol: "●", identityLabel: "하위 작업", elapsed: "1m", lastActivity: "")
        child.lineageParentPaneID = "p"
        child.lineageDepth = 1
        let board = ProjectHomeBoard.build(workspace: workspace([ownedCheckout("parent-branch", pane: "p"), ownedCheckout("child-branch", pane: "child")]), agents: [child, parent])
        #expect(board.agents.count == 1)
        #expect(board.agents[0].rows.map(\.id) == ["p", "child"])
        #expect(board.agents[0].rows[1].foreignBranch == "child-branch")
        #expect(board.tasks[0].needsYou)
        #expect(board.tasks[0].stage == .working)
    }

    @Test func issueInputRejectsOtherHostsAndExpandsCurrentRepository() {
        for value in ["#42", "acme/project#42", "https://github.com/acme/project/issues/42/"] {
            guard case .success(let token) = ProjectHomeIssueInput.parse(value, repository: "acme/project") else {
                Issue.record("valid issue rejected: \(value)"); continue
            }
            #expect(token == "acme/project#42")
        }
        for value in ["#0", "#42?x", "https://example.com/acme/project/issues/42", "https://github.com/acme/project/pull/42"] {
            guard case .failure = ProjectHomeIssueInput.parse(value, repository: "acme/project") else {
                Issue.record("invalid issue accepted: \(value)"); continue
            }
        }
    }
    @Test func projectionMemoBuildsOncePerSnapshotRevision() {
        let memo = ProjectHomeMemo()
        var builds = 0
        let project = workspace([])
        for _ in 0..<100 {
            _ = memo.board(for: .init(revision: 1, workspaceID: "project", connected: true)) {
                builds += 1
                return .build(workspace: project, agents: [])
            }
        }
        #expect(builds == 1)
        _ = memo.board(for: .init(revision: 2, workspaceID: "project", connected: true)) {
            builds += 1
            return .build(workspace: project, agents: [])
        }
        #expect(builds == 2)
    }
    @Test func escapeClosesOnlyAnOverlayWithoutASheet() throws {
        let event = try #require(NSEvent.keyEvent(with: .keyDown, location: .zero, modifierFlags: [],
            timestamp: 0, windowNumber: 0, context: nil, characters: "\u{1b}", charactersIgnoringModifiers: "\u{1b}", isARepeat: false, keyCode: 53))
        #expect(ProjectHomeShortcutPolicy.shouldClose(event, visible: true, sheetPresented: false))
        #expect(!ProjectHomeShortcutPolicy.shouldClose(event, visible: true, sheetPresented: true))
        #expect(!ProjectHomeShortcutPolicy.shouldClose(event, visible: false, sheetPresented: false))
    }
}
