import Foundation
import Testing
import SwiftUI

@testable import HerdrMacOS

private func presentationPane(id: String) -> CorePaneSnapshot {
    CorePaneSnapshot(
        id: id,
        cwd: "/tmp/hide",
        statusLabel: "Idle",
        activityAt: nil
    )
}

private func presentationCheckout(
    id: String,
    path: String,
    isWorktree: Bool = false,
    exists: Bool = true,
    purpose: CoreCheckoutPurpose? = nil,
    pullRequest: CorePullRequest? = nil,
    worktree: CoreGitWorktree? = nil,
    github: CoreGithubStatus = .empty,
    paneIDs: [String] = []
) -> CoreCheckoutSnapshot {
    var checkout = CoreCheckoutSnapshot(
        id: id,
        workspaceID: "workspace-1",
        label: id,
        path: path,
        branch: id,
        isWorktree: isWorktree,
        exists: exists,
        temporary: false,
        pullRequest: pullRequest,
        purpose: purpose,
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
    checkout.worktree = worktree
    checkout.github = github
    return checkout
}

private func presentationPullRequest(
    _ number: Int,
    badge: CorePullRequestBadge = .open,
    title: String? = nil,
    review: CoreReviewDecision? = nil,
    draft: Bool = false
) -> CorePullRequest {
    var request = CorePullRequest(
        number: number,
        headBranch: "feature",
        baseBranch: "main",
        url: "https://example.invalid/pull/\(number)",
        badge: badge,
        review: review,
        isDraft: draft,
        mergedAtUnixMS: nil,
        updatedAtUnixMS: nil
    )
    request.title = title
    return request
}

private func presentationWorktree(
    branch: String? = "feature",
    missing: Bool = false,
    createdAtUnixMS: Int? = nil,
    lastCommitUnixSeconds: Int? = nil
) throws -> CoreGitWorktree {
    func json(_ value: Any?) -> String {
        switch value {
        case nil: return "null"
        case let string as String: return "\"\(string)\""
        case let bool as Bool: return bool ? "true" : "false"
        case let int as Int: return "\(int)"
        default: return "null"
        }
    }
    let text = """
    {
        "path": "/tmp/hide.worktrees/feature", "branch": \(json(branch)), "head_sha": "abc12345",
        "last_commit_subject": "work", "missing": \(missing), "is_main": false,
        "dirty": false, "changed_file_count": 0, "base_branch": "main",
        "ahead": 0, "behind": 0, "merged": false, "upstream_state": "pushed",
        "unpushed": null, "unavailable_reason": null, "behind_upstream": 0,
        "created_at_unix_ms": \(json(createdAtUnixMS)), "last_fetch_at_unix_ms": null,
        "measured_at_unix_ms": null, "last_commit_unix_seconds": \(json(lastCommitUnixSeconds)),
        "pane_count": 0, "running_agent_count": 0,
        "disk": {"path": "/tmp/hide.worktrees/feature", "total_bytes": 1024, "unavailable_reason": null},
        "pull_request": null,
        "github": {"available": true, "loading": false, "stale": false,
                   "last_success_at_unix_ms": null, "unavailable_reason": null},
        "deletion_gate": {"blocked_reason": null, "warnings": [], "button_label": "Delete worktree…",
                          "can_delete_branch": false},
        "open_error": null
    }
    """
    return try JSONDecoder().decode(CoreGitWorktree.self, from: Data(text.utf8))
}

private func presentationWorkspace(
    id: String = "workspace-1",
    deviceID: String = "local",
    checkouts: [CoreCheckoutSnapshot],
    lastActivityUnixMS: UInt64? = nil,
    inactiveCheckoutIDs: [String] = [],
    inactiveExpanded: Bool = false
) -> CoreWorkspaceSnapshot {
    CoreWorkspaceSnapshot(
        id: id,
        label: "hide",
        path: "/tmp/hide",
        remoteTargetID: nil,
        expanded: true,
        deviceID: deviceID,
        repoName: "hide",
        isGit: true,
        defaultBranch: "main",
        registered: true,
        temporary: false,
        lastActivityUnixMS: lastActivityUnixMS,
        checkouts: checkouts,
        inactiveCheckouts: CoreInactiveCheckoutGroupSnapshot(
            expanded: inactiveExpanded,
            checkoutIDs: inactiveCheckoutIDs
        )
    )
}

/// B1, B4, B11, B12, B17. The shell maps the core's ordered IDs to the same
/// existing rows, keeping active rows first and revealing inactive rows in the
/// exact order the core supplied.
@Test func inactiveProjectionPreservesRowsAndCoreOrder() throws {
    let main = presentationCheckout(id: "main", path: "/tmp/hide")
    let active = presentationCheckout(id: "active", path: "/tmp/hide-active", isWorktree: true)
    let inactiveOne = presentationCheckout(
        id: "old-one",
        path: "/tmp/hide-old-one",
        isWorktree: true
    )
    let inactiveTwo = presentationCheckout(
        id: "old-two",
        path: "/tmp/hide-old-two",
        isWorktree: true
    )
    let foldedProject = presentationWorkspace(
        checkouts: [main, active, inactiveOne, inactiveTwo],
        inactiveCheckoutIDs: ["old-two", "old-one"],
        inactiveExpanded: true
    )
    let visibleProject = CoreWorkspaceSnapshot(
        id: "workspace-visible",
        label: "visible",
        path: "/tmp/visible",
        remoteTargetID: nil,
        expanded: true,
        deviceID: "local",
        repoName: "visible",
        isGit: true,
        defaultBranch: "main",
        registered: true,
        temporary: false,
        checkouts: []
    )
    let group = try JSONDecoder().decode(
        CoreInactiveProjectGroupSnapshot.self,
        from: Data(
            #"{"device_id":"local","expanded":false,"project_ids":["workspace-1"]}"#.utf8
        )
    )

    let remoteActive = presentationWorkspace(
        id: "remote-active",
        deviceID: "mini",
        checkouts: []
    )
    let remoteInactive = presentationWorkspace(
        id: "remote-inactive",
        deviceID: "mini",
        checkouts: []
    )
    let remoteGroup = try JSONDecoder().decode(
        CoreInactiveProjectGroupSnapshot.self,
        from: Data(
            #"{"device_id":"mini","expanded":true,"project_ids":["remote-inactive"]}"#.utf8
        )
    )
    let rows = SidebarInactiveProjection.projectRows(
        [visibleProject, foldedProject, remoteActive, remoteInactive],
        groups: [group, remoteGroup]
    )
    #expect(rows.map(\.id) == [
        "workspace:workspace-visible",
        "inactive-projects:local",
        "workspace:remote-active",
        "inactive-projects:mini",
        "workspace:remote-inactive",
    ])
    let workspaceLevels = rows.compactMap { row -> (String, SidebarHierarchyLevel)? in
        guard case .workspace(let workspace, let level) = row else { return nil }
        return (workspace.id, level)
    }
    #expect(workspaceLevels.map(\.0) == [
        "workspace-visible",
        "remote-active",
        "remote-inactive",
    ])
    #expect(workspaceLevels.map(\.1) == [.root, .root, .child])
    #expect(
        SidebarInactiveProjection.activeCheckouts(in: foldedProject).map(\.id)
            == ["main", "active"]
    )
    #expect(
        SidebarInactiveProjection.inactiveCheckouts(in: foldedProject).map(\.id)
            == ["old-two", "old-one"]
    )
}

/// Human review asked for a visible hierarchy ladder. The policy keeps every
/// content edge distinct and starts selection behind that edge, so a selected
/// checkout still reads as a child rather than a full-width top-level row.
@Test func sidebarHierarchyKeepsNestedContentAndSelectionDistinct() {
    let levels: [SidebarHierarchyLevel] = [.root, .child, .grandchild, .greatGrandchild]
    let contentInsets = levels.map(\.contentLeadingInset)

    #expect(contentInsets == [
        HideTheme.spacingMD,
        HideTheme.spacingXL,
        HideTheme.sidebarHierarchyGrandchildInset,
        HideTheme.sidebarHierarchyGreatGrandchildInset,
    ])
    #expect(zip(contentInsets, contentInsets.dropFirst()).allSatisfy { $0 < $1 })
    #expect(levels.allSatisfy {
        $0.selectionLeadingInset + HideTheme.spacingSM == $0.contentLeadingInset
    })
    #expect(SidebarHierarchyLevel.root.childLevel == .child)
    #expect(SidebarHierarchyLevel.child.childLevel == .grandchild)
    #expect(SidebarHierarchyLevel.grandchild.childLevel == .greatGrandchild)
}

/// Additive wire fields default to closed and empty, so an older snapshot
/// cannot erase the sidebar or accidentally open a fold.
@Test func inactiveProjectionWireDefaultsClosedAndEmpty() throws {
    let workspace = try JSONDecoder().decode(
        CoreWorkspaceSnapshot.self,
        from: Data(
            #"{"id":"legacy","label":"legacy","path":"/tmp/legacy","checkouts":[]}"#.utf8
        )
    )
    let state = try JSONDecoder().decode(
        CoreUIStateSnapshot.self,
        from: Data(#"{"expanded_paths":[]}"#.utf8)
    )

    #expect(workspace.inactiveCheckouts == .empty)
    #expect(state.expandedInactiveCheckoutProjectPaths.isEmpty)
    #expect(state.expandedInactiveProjectDeviceIDs.isEmpty)
}

/// B2, B3, B4, B6. The `Pinned` section shows the rows the core flagged, in
/// the core's order, and the activity list counts and draws only the rest;
/// a pinned project the core kept out of its device fold never appears
/// there. With no pin the split is the whole list, so the header can go.
@Test func projectSectionsSplitPinnedRowsFromTheActivityList() throws {
    let pinnedRemote = try JSONDecoder().decode(
        CoreWorkspaceSnapshot.self,
        from: Data(
            #"{"id":"remote-pinned","label":"remote","path":"/tmp/remote","device_id":"mini","pinned":true,"checkouts":[]}"#.utf8
        )
    )
    let pinnedLocal = try JSONDecoder().decode(
        CoreWorkspaceSnapshot.self,
        from: Data(
            #"{"id":"local-pinned","label":"pinned","path":"/tmp/pinned","pinned":true,"checkouts":[]}"#.utf8
        )
    )
    let active = presentationWorkspace(id: "active", checkouts: [])
    let folded = presentationWorkspace(id: "folded", checkouts: [])
    let group = try JSONDecoder().decode(
        CoreInactiveProjectGroupSnapshot.self,
        from: Data(#"{"device_id":"local","expanded":false,"project_ids":["folded"]}"#.utf8)
    )

    let sections = SidebarProjectSections(
        [pinnedLocal, active, folded, pinnedRemote],
        groups: [group]
    )

    #expect(sections.pinned.map(\.id) == ["local-pinned", "remote-pinned"])
    #expect(sections.recent.map(\.id) == ["active", "folded"])
    #expect(sections.rows.map(\.id) == [
        "header:Pinned",
        "workspace:local-pinned",
        "workspace:remote-pinned",
        "header:Projects · Recent activity",
        "workspace:active",
        "inactive-projects:local",
    ])
    guard case .header(_, let pinnedCount) = sections.rows[0],
          case .header(_, let recentCount) = sections.rows[3] else {
        Issue.record("the headers are list rows")
        return
    }
    #expect(pinnedCount == 2)
    #expect(recentCount == 2)

    let unpinned = SidebarProjectSections([active, folded], groups: [group])
    #expect(unpinned.pinned.isEmpty)
    #expect(unpinned.recent.count == 2)
    #expect(unpinned.rows.map(\.id) == [
        "header:Projects · Recent activity",
        "workspace:active",
        "inactive-projects:local",
    ])
}

/// B5, B9. A row from a wire without the pin or the removal counts, which is
/// every remote navigation row, reads as unpinned with nothing to close.
@Test func pinAndRemovalWireDefaultsAreUnpinnedAndEmpty() throws {
    let workspace = try JSONDecoder().decode(
        CoreWorkspaceSnapshot.self,
        from: Data(#"{"id":"legacy","label":"legacy","path":"/tmp/legacy","checkouts":[]}"#.utf8)
    )
    let counted = try JSONDecoder().decode(
        CoreWorkspaceSnapshot.self,
        from: Data(
            #"{"id":"busy","label":"busy","path":"/tmp/busy","checkouts":[],"removal":{"pane_count":3,"running_agent_count":2}}"#.utf8
        )
    )
    let registration = try JSONDecoder().decode(
        CoreWorkspaceRegistration.self,
        from: Data(#"{"id":"workspace:a","label":"a","path":"/tmp/a","device_id":"local"}"#.utf8)
    )

    #expect(!workspace.pinned)
    #expect(workspace.removal == .none)
    #expect(counted.removal == CoreWorkspaceRemovalGateSnapshot(paneCount: 3, runningAgentCount: 2))
    #expect(!registration.pinned)
}

/// B11, B13. The confirmation says what confirming closes, from the core's
/// counts: nothing for a project without panes, the pane and agent counts
/// otherwise, and the button names the same outcome.
@Test func removalPromptNamesThePanesAndAgentsItCloses() {
    let none = WorkspaceRemovalPrompt(label: "hide", removal: .none)
    #expect(none.title == "Remove hide from Hide?")
    #expect(none.message.hasPrefix("Hide will remove only its registration."))
    #expect(none.confirmLabel == "Remove registration")

    let busy = WorkspaceRemovalPrompt(
        label: "hide",
        removal: CoreWorkspaceRemovalGateSnapshot(paneCount: 3, runningAgentCount: 2)
    )
    #expect(
        busy.message
            == "Closes 3 panes (2 running agents). The folder, repository, and worktrees stay on disk."
    )
    #expect(busy.confirmLabel == "Close 3 panes and remove")

    let quiet = WorkspaceRemovalPrompt(
        label: "hide",
        removal: CoreWorkspaceRemovalGateSnapshot(paneCount: 1, runningAgentCount: 0)
    )
    #expect(quiet.message == "Closes 1 pane. The folder, repository, and worktrees stay on disk.")
    #expect(quiet.confirmLabel == "Close 1 pane and remove")

    let one = WorkspaceRemovalPrompt(
        label: "hide",
        removal: CoreWorkspaceRemovalGateSnapshot(paneCount: 1, runningAgentCount: 1)
    )
    #expect(one.message.hasPrefix("Closes 1 pane (1 running agent)."))
}

private func presentationAgent(
    id: String,
    paneID: String,
    group: String,
    demand: String = "none",
    identityLabel: String? = nil
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
        identityLabel: identityLabel ?? "Agent \(id)",
        elapsed: "1m",
        lastActivity: "0000000000001"
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

@Test func agentsScopeUsesTheCoresFinalDelegatedAnswer() {
    let own = presentationAgent(id: "own", paneID: "pane-own", group: "working")
    var delegated = presentationAgent(id: "delegated", paneID: "pane-child", group: "working")
    delegated.delegated = true
    let all = [own, delegated]

    #expect(SidebarGrouping.visibleAgents(all, scope: .mine).map(\.id) == ["own"])
    #expect(SidebarGrouping.visibleAgents(all, scope: .all).map(\.id) == ["own", "delegated"])
    #expect(SidebarGrouping.sections(SidebarGrouping.visibleAgents(all, scope: .mine)).flatMap(\.agents).map(\.id) == ["own"])
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

@Test func checkoutKindGlyphStagesCoverPullRequestsBranchesAndSpecialRows() throws {
    let workspace = presentationWorkspace(checkouts: [])
    let open = SidebarCheckoutPresentation(
        workspace: workspace,
        checkout: presentationCheckout(
            id: "open",
            path: "/tmp/open",
            isWorktree: true,
            pullRequest: presentationPullRequest(1),
            worktree: try presentationWorktree()
        ),
        agents: []
    )
    let draft = SidebarCheckoutPresentation(
        workspace: workspace,
        checkout: presentationCheckout(
            id: "draft",
            path: "/tmp/draft",
            isWorktree: true,
            pullRequest: presentationPullRequest(2, draft: true),
            worktree: try presentationWorktree()
        ),
        agents: []
    )
    let merged = SidebarCheckoutPresentation(
        workspace: workspace,
        checkout: presentationCheckout(
            id: "merged",
            path: "/tmp/merged",
            isWorktree: true,
            pullRequest: presentationPullRequest(3, badge: .merged),
            worktree: try presentationWorktree()
        ),
        agents: []
    )
    let closed = SidebarCheckoutPresentation(
        workspace: workspace,
        checkout: presentationCheckout(
            id: "closed",
            path: "/tmp/closed",
            isWorktree: true,
            pullRequest: presentationPullRequest(4, badge: .closed),
            worktree: try presentationWorktree()
        ),
        agents: []
    )
    let branch = SidebarCheckoutPresentation(
        workspace: workspace,
        checkout: presentationCheckout(
            id: "branch",
            path: "/tmp/branch",
            isWorktree: true,
            worktree: try presentationWorktree()
        ),
        agents: []
    )
    let primary = SidebarCheckoutPresentation(
        workspace: workspace,
        checkout: presentationCheckout(id: "main", path: "/tmp/hide"),
        agents: []
    )
    let detached = SidebarCheckoutPresentation(
        workspace: workspace,
        checkout: presentationCheckout(
            id: "detached",
            path: "/tmp/detached",
            isWorktree: true,
            worktree: try presentationWorktree(branch: nil)
        ),
        agents: []
    )
    let folderWorkspace = CoreWorkspaceSnapshot(
        id: "folder",
        label: "folder",
        path: "/tmp/folder",
        remoteTargetID: nil,
        expanded: true,
        deviceID: "local",
        repoName: "folder",
        isGit: false,
        defaultBranch: nil,
        registered: true,
        temporary: false,
        checkouts: []
    )
    let folder = SidebarCheckoutPresentation(
        workspace: folderWorkspace,
        checkout: presentationCheckout(id: "folder", path: "/tmp/folder"),
        agents: []
    )

    #expect([open.kindStage, draft.kindStage, merged.kindStage, closed.kindStage] ==
        ["Open", "Draft", "Merged", "Closed"])
    #expect([open, draft, merged, closed].map(\.showsPullRequestGlyph) == [true, true, true, true])
    #expect(draft.kindMuted)
    #expect(merged.rowDimmed && closed.rowDimmed)
    #expect(branch.kindStage == "Branch" && branch.kindSystemImage == "arrow.triangle.branch")
    #expect(primary.kindStage == "Primary checkout" && primary.kindSystemImage == "house")
    #expect(detached.kindStage == "Detached commit")
    #expect(folder.kindStage == "Folder" && folder.kindSystemImage == "folder")
}

@Test func checkoutSummaryCarriesPurposeCommitAgeTooltipAndAccessibilityInOrder() throws {
    let now = Date(timeIntervalSince1970: 1_700_000_000)
    let github = CoreGithubStatus(
        available: false,
        loading: false,
        stale: true,
        lastSuccessAtUnixMS: 1_699_992_800_000,
        unavailableReason: "gh timed out"
    )
    var checkout = presentationCheckout(
        id: "checkout-row-d",
        path: "/tmp/hide.worktrees/checkout-row-d",
        isWorktree: true,
        purpose: CoreCheckoutPurpose(text: "체크아웃 행 목적 표시", origin: .token),
        pullRequest: presentationPullRequest(118, title: "Checkout row D"),
        worktree: try presentationWorktree(
            createdAtUnixMS: 1_699_913_600_000,
            lastCommitUnixSeconds: 1_699_985_600
        ),
        github: github,
        paneIDs: ["pane-1"]
    )
    checkout.agentSummary = CoreCheckoutAgentSummary(
        representativePaneID: "pane-1",
        working: 1
    )
    let agent = presentationAgent(
        id: "agent-1",
        paneID: "pane-1",
        group: "working",
        identityLabel: "Checkout row D"
    )
    let presentation = SidebarCheckoutPresentation(
        workspace: presentationWorkspace(checkouts: [checkout]),
        checkout: checkout,
        agents: [agent],
        now: now
    )

    #expect(presentation.secondLine == "체크아웃 행 목적 표시")
    #expect(presentation.lastCommitAge == "4h")
    #expect(presentation.showsPullRequestGlyph)
    #expect(presentation.kindMuted)
    #expect(presentation.detailTooltip.hasPrefix("#118 · Open · Checkout row D · Last known 2h\n"))
    #expect(presentation.detailTooltip.contains("gh timed out"))
    #expect(presentation.detailTooltip.contains("Working: 1"))
    #expect(presentation.detailTooltip.hasSuffix("Created 1 day ago"))
    #expect(presentation.accessibilityLabel == "checkout-row-d, Open, 4h, 체크아웃 행 목적 표시")
    #expect(presentation.showsSecondLine(expanded: false))
    #expect(!presentation.showsSecondLine(expanded: true))
}

@Test func missingCheckoutKeepsItsBranchPurposeButGitLoadingStaysOneLine() throws {
    let purpose = CoreCheckoutPurpose(text: "브랜치에 남은 목적", origin: .branchDescription)
    let missing = presentationCheckout(
        id: "missing",
        path: "/tmp/missing",
        isWorktree: true,
        exists: false,
        purpose: purpose,
        worktree: try presentationWorktree(branch: "missing", missing: true)
    )
    let missingPresentation = SidebarCheckoutPresentation(
        workspace: presentationWorkspace(checkouts: [missing]),
        checkout: missing,
        agents: []
    )
    #expect(missingPresentation.secondLine == "브랜치에 남은 목적")
    #expect(missingPresentation.showsSecondLine(expanded: false))

    let loading = presentationCheckout(
        id: "loading",
        path: "/tmp/loading",
        isWorktree: true,
        purpose: purpose
    )
    let loadingPresentation = SidebarCheckoutPresentation(
        workspace: presentationWorkspace(checkouts: [loading]),
        checkout: loading,
        agents: []
    )
    #expect(!loadingPresentation.showsSecondLine(expanded: false))
}

@Test func unavailableGithubFallsBackToBranchGlyphAndKeepsItsReasonInTheTooltip() throws {
    let github = CoreGithubStatus(
        available: false,
        loading: false,
        stale: false,
        lastSuccessAtUnixMS: nil,
        unavailableReason: "Sign in required"
    )
    let checkout = presentationCheckout(
        id: "feature",
        path: "/tmp/feature",
        isWorktree: true,
        pullRequest: presentationPullRequest(118),
        worktree: try presentationWorktree(),
        github: github
    )
    let presentation = SidebarCheckoutPresentation(
        workspace: presentationWorkspace(checkouts: [checkout]),
        checkout: checkout,
        agents: []
    )

    #expect(!presentation.showsPullRequestGlyph)
    #expect(presentation.kindStage == "Branch")
    #expect(presentation.detailTooltip.hasPrefix("Sign in required\n"))
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

/// B4, B5. The row keeps the count it already showed and adds how long ago
/// this project last did anything, in the app's own one-token elapsed form.
@Test func workspaceRowShowsTheRelativeTimeOfTheLastActivity() {
    let now = Date(timeIntervalSince1970: 1_700_000_000)
    let checkout = presentationCheckout(id: "main", path: "/tmp/hide", paneIDs: ["pane-1"])
    let agents = [presentationAgent(id: "inside", paneID: "pane-1", group: "working")]

    let cases: [(TimeInterval, String)] = [
        (0, "now"),
        (59, "now"),
        (60, "1m"),
        (59 * 60, "59m"),
        (60 * 60, "1h"),
        (23 * 3600 + 3599, "23h"),
        (24 * 3600, "1d"),
        (5 * 24 * 3600, "5d"),
    ]
    for (age, expected) in cases {
        let workspace = presentationWorkspace(
            checkouts: [checkout],
            lastActivityUnixMS: UInt64((now.timeIntervalSince1970 - age) * 1000)
        )
        let presentation = SidebarWorkspacePresentation(
            workspace: workspace,
            agents: agents,
            now: now
        )
        #expect(presentation.lastActivity == expected)
        #expect(presentation.activityLabel == "1 agent · \(expected)")
    }
}

/// B4. A project the core reported no activity for shows the label it always
/// showed and no time. An empty time is the honest answer; "now" would claim
/// a recency nothing measured.
@Test func workspaceRowWithoutActivityKeepsItsExistingLabel() {
    let checkout = presentationCheckout(id: "main", path: "/tmp/hide")
    let workspace = presentationWorkspace(checkouts: [checkout])

    let presentation = SidebarWorkspacePresentation(workspace: workspace, agents: [])

    #expect(presentation.lastActivity == nil)
    #expect(presentation.activityLabel == "1 workspace")
}

/// B5. A timestamp ahead of this machine's clock is still "now": a remote
/// device's clock is not this one's, and a negative age is not a time.
@Test func workspaceRowReadsAFutureTimestampAsNow() {
    let now = Date(timeIntervalSince1970: 1_700_000_000)
    let workspace = presentationWorkspace(
        checkouts: [presentationCheckout(id: "main", path: "/tmp/hide")],
        lastActivityUnixMS: UInt64((now.timeIntervalSince1970 + 3600) * 1000)
    )

    let presentation = SidebarWorkspacePresentation(
        workspace: workspace,
        agents: [],
        now: now
    )

    #expect(presentation.lastActivity == "now")
    #expect(presentation.activityLabel == "1 workspace · now")
}

/// B5. The label is recomputed from each snapshot's own timestamp against the
/// current clock, so a project nobody touched still ages as the app stays
/// open rather than freezing at the value it was first drawn with.
@Test func theRelativeTimeAgesWithEachNewSnapshot() {
    let activity = Date(timeIntervalSince1970: 1_700_000_000)
    let workspace = presentationWorkspace(
        checkouts: [presentationCheckout(id: "main", path: "/tmp/hide")],
        lastActivityUnixMS: UInt64(activity.timeIntervalSince1970 * 1000)
    )

    let first = SidebarWorkspacePresentation(
        workspace: workspace,
        agents: [],
        now: activity.addingTimeInterval(120)
    )
    let later = SidebarWorkspacePresentation(
        workspace: workspace,
        agents: [],
        now: activity.addingTimeInterval(7200)
    )

    #expect(first.lastActivity == "2m")
    #expect(later.lastActivity == "2h")
    #expect(first != later)
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
    // ⌥1 in the Projects view is the checkout's first agent, not the global first.
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

/// PRD B1, D-01: the row's title is the core's identity at both densities;
/// a raised row names its home in the qualifier instead of the title.
@Test func sidebarRowTitleIsTheIdentityAtBothDensities() {
    let agent = SidebarAgent(
        id: "transport-child", paneID: "w1:p2", workspaceLabel: "Project", checkoutLabel: "main",
        agentKind: "codex", symbol: "●",
        identityLabel: "긴 한국어 작업명과 English가 함께 있는 원래 사용자 작업 이름",
        elapsed: "0s", lastActivity: ""
    )
    let compact = AgentRowPresentation(agent: agent, density: .compact, connected: true)
    let prominent = AgentRowPresentation(agent: agent, density: .prominent, connected: true)
    #expect(compact.title == "긴 한국어 작업명과 English가 함께 있는 원래 사용자 작업 이름")
    #expect(prominent.title == compact.title)
    #expect(compact.qualifier == nil)
    #expect(prominent.qualifier == "Project › main")
}

/// PRD D-06, D-07: the second line is the core's sentence and word flag,
/// carried through unchanged, and the emphasis decides the sentence colour.
@Test func sidebarRowSecondLineCarriesTheCoresChoice() {
    let working = SidebarAgent(
        id: "w", paneID: "w1:p1", workspaceLabel: "hide", agentKind: "claude",
        activity: "working", group: "working", symbol: "●", statusLabel: "Working",
        identityLabel: "Hook 버그 확인", detail: "hook 보고 경로를 소켓 호출로 교체 중", statusWordVisible: false,
        elapsed: "2m", lastActivity: ""
    )
    let question = SidebarAgent(
        id: "q", paneID: "w1:p2", workspaceLabel: "hide", agentKind: "claude",
        demand: "question", unread: true, group: "needs_you", symbol: "?", emphasized: true,
        statusLabel: "Question",
        identityLabel: "결제 멱등키 PR", detail: "A/B 선택 후 DB 마이그레이션 승인", statusWordVisible: false,
        elapsed: "2m", lastActivity: ""
    )
    let seen = SidebarAgent(
        id: "s", paneID: "w1:p3", workspaceLabel: "hide", agentKind: "codex",
        activity: "stopped", symbol: "○", statusLabel: "Idle",
        identityLabel: "컨텍스트 라벨 표시", detail: nil, statusWordVisible: false,
        elapsed: "1h", lastActivity: ""
    )
    let workingRow = AgentRowPresentation(agent: working, density: .compact, connected: true)
    #expect(workingRow.detail == "hook 보고 경로를 소켓 호출로 교체 중")
    #expect(!workingRow.statusWordVisible)
    #expect(!workingRow.emphasized)
    let questionRow = AgentRowPresentation(agent: question, density: .compact, connected: true)
    #expect(questionRow.detail == "A/B 선택 후 DB 마이그레이션 승인")
    #expect(!questionRow.statusWordVisible)
    #expect(questionRow.emphasized)
    let seenRow = AgentRowPresentation(agent: seen, density: .compact, connected: true)
    #expect(seenRow.detail == nil)
    #expect(!seenRow.statusWordVisible)
}

/// PRD D-08, B9: the header line reads `title · sentence`, with the word only
/// when the core chose no sentence, and a shell operation on the pane takes
/// the slot while it runs. The accessibility label carries the word either way.
@Test func paneHeaderSentenceFollowsTheRowAndYieldsToAShellOperation() {
    let question = SidebarAgent(
        id: "q", paneID: "w1:p2", workspaceLabel: "hide", agentKind: "claude",
        demand: "question", unread: true, group: "needs_you", symbol: "?", emphasized: true,
        statusLabel: "Question",
        identityLabel: "결제 멱등키 PR", detail: "A/B 중 하나를 선택하고 DB 마이그레이션 실행 승인 여부를 지시하세요",
        statusWordVisible: false, elapsed: "2m", lastActivity: ""
    )
    let sentence = PaneHeaderPresentation.sentence(agent: question, activity: "")
    #expect(sentence.word == nil)
    #expect(sentence.text == "A/B 중 하나를 선택하고 DB 마이그레이션 실행 승인 여부를 지시하세요")
    #expect(sentence.emphasized)
    #expect(PaneHeaderPresentation.sentence(agent: question, activity: " · forking…").isEmpty)
    #expect(PaneHeaderPresentation.sentence(agent: nil, activity: "").isEmpty)
    let label = PaneHeaderPresentation.accessibilityLabel(
        kind: "terminal", title: "결제 멱등키 PR", paneID: "w1:p2", agent: question, activity: "", notice: nil
    )
    #expect(label == "Focus terminal pane 결제 멱등키 PR (w1:p2), claude, Question, A/B 중 하나를 선택하고 DB 마이그레이션 실행 승인 여부를 지시하세요")
}
