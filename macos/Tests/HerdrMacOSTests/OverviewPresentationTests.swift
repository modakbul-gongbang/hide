import Foundation
import Testing
@testable import HerdrMacOS

/// A worktree row as the core sends it, with the fields a test varies.
private func worktree(
    path: String,
    branch: String? = "main",
    isMain: Bool = false,
    missing: Bool = false,
    changed: Int = 0,
    ahead: Int = 0,
    behind: Int = 0,
    merged: Bool? = nil,
    upstreamState: String = "pushed",
    behindUpstream: Int? = nil,
    createdAtUnixMS: Int? = nil,
    diskBytes: Int? = 1_073_741_824,
    diskUnavailable: String? = nil,
    unavailableReason: String? = nil
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
        "path": "\(path)", "branch": \(json(branch)), "head_sha": "abc12345",
        "last_commit_subject": "work", "missing": \(missing), "is_main": \(isMain),
        "dirty": \(changed > 0), "changed_file_count": \(changed), "base_branch": "main",
        "ahead": \(ahead), "behind": \(behind), "merged": \(json(merged)), "upstream_state": "\(upstreamState)",
        "unpushed": null, "unavailable_reason": \(json(unavailableReason)),
        "behind_upstream": \(json(behindUpstream)), "created_at_unix_ms": \(json(createdAtUnixMS)),
        "last_fetch_at_unix_ms": null, "measured_at_unix_ms": null,
        "last_commit_unix_seconds": null,
        "pane_count": 0, "running_agent_count": 0,
        "disk": {"path": "\(path)", "total_bytes": \(json(diskBytes)), "unavailable_reason": \(json(diskUnavailable))},
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

private func pullRequest(
    _ number: Int,
    badge: CorePullRequestBadge = .open,
    title: String? = nil,
    review: CoreReviewDecision? = nil,
    isDraft: Bool = false,
    checks: CorePullRequestChecks? = nil
) -> CorePullRequest {
    var request = CorePullRequest(
        number: number,
        headBranch: "feature",
        baseBranch: "main",
        url: "https://example.invalid/pull/\(number)",
        badge: badge,
        review: review,
        isDraft: isDraft,
        mergedAtUnixMS: nil,
        updatedAtUnixMS: nil
    )
    request.title = title
    request.checks = checks
    return request
}

private func checkout(
    _ id: String,
    path: String,
    isWorktree: Bool = true,
    changed: Int = 0,
    pullRequest: CorePullRequest? = nil,
    worktree: CoreGitWorktree? = nil,
    paneIDs: [String] = []
) -> CoreCheckoutSnapshot {
    var checkout = CoreCheckoutSnapshot(
        id: id, workspaceID: "workspace-1", label: id, path: path, branch: id,
        isWorktree: isWorktree, exists: true, temporary: false,
        changedFileCount: changed, pullRequest: pullRequest,
        tabs: paneIDs.isEmpty ? [] : [CoreTabSnapshot(
            id: "tab-\(id)", workspaceID: "workspace-1", checkoutID: id, label: "1", empty: false,
            panes: paneIDs.map { CorePaneSnapshot(id: $0, cwd: path, statusLabel: "Idle", activityAt: nil) }
        )]
    )
    checkout.worktree = worktree
    return checkout
}

private func agent(
    _ id: String,
    title: String,
    detail: String? = nil,
    delegated: Bool = false
) -> SidebarAgent {
    var agent = SidebarAgent(
        id: id, paneID: id, workspaceLabel: "hide", agentKind: "claude", demand: "none", activity: "working",
        unread: false, group: "working", symbol: "\u{25cf}", identityLabel: title,
        detail: detail, elapsed: "1m", lastActivity: "0000000000001"
    )
    agent.delegated = delegated
    return agent
}

private let ready = CoreGithubStatus(available: true, loading: false, stale: false, lastSuccessAtUnixMS: 1, unavailableReason: nil)

@Suite("Project Overview")
struct OverviewPresentationTests {
    @Test func aTitleIsNotRepeatedAndProgressRemainsTheRowDetail() {
        let titleOnly = agent("p1", title: "훅 보고 경로 수정")
        #expect(OverviewPresentation.rowDetail(titleOnly) == nil)
        let progressing = agent(
            "p3",
            title: "훅 보고 경로 수정",
            detail: "회귀 테스트 추가 중"
        )
        #expect(OverviewPresentation.rowDetail(progressing) == "회귀 테스트 추가 중")
    }

    @Test func theSubtitleCountsWorkspacesAndNamesInactiveOnesOnlyWhenThereAreSome() {
        #expect(OverviewPresentation.subtitle(workspaceCount: 1, inactiveCount: 0) == "Project · 1 workspace")
        #expect(OverviewPresentation.subtitle(workspaceCount: 6, inactiveCount: 2) == "Project · 6 workspaces · 2 inactive")
    }

    @Test func theDiskCellFollowsTheGlyphLanguage() {
        #expect(OverviewPresentation.diskCell(total: 26_843_545_600, confirmed: nil, failure: nil, measuring: false).value == "25 GB")
        let measuring = OverviewPresentation.diskCell(total: nil, confirmed: nil, failure: nil, measuring: true)
        #expect(measuring.value == "… GB" && measuring.tone == .muted)
        let failed = OverviewPresentation.diskCell(total: nil, confirmed: 4096, failure: "denied", measuring: false)
        #expect(failed.value == "? GB" && failed.tone == .warning)
        #expect(failed.tooltip.contains("denied") && failed.tooltip.contains("4.0 KB"))
    }

    @Test func thePullRequestCellDrawsZeroAndHidesReasonsInTheTooltip() {
        let zero = OverviewPresentation.pullRequestsCell(status: ready, requests: [pullRequest(1, badge: .merged)])
        #expect(zero.value == "0" && zero.label == "open PRs" && zero.glyph == "⑂")
        let one = OverviewPresentation.pullRequestsCell(status: ready, requests: [pullRequest(1), pullRequest(2, badge: .closed)])
        #expect(one.value == "1" && one.label == "open PR")
        let loading = CoreGithubStatus(available: false, loading: true, stale: false, lastSuccessAtUnixMS: nil, unavailableReason: nil)
        #expect(OverviewPresentation.pullRequestsCell(status: loading, requests: nil).value == "…")
        let signedOut = CoreGithubStatus(available: false, loading: false, stale: false, lastSuccessAtUnixMS: nil,
                                         unavailableReason: "gh: not logged in", failureCategory: "not logged in")
        let failed = OverviewPresentation.pullRequestsCell(status: signedOut, requests: nil)
        #expect(failed.value == "?" && failed.tone == .warning && failed.tooltip == "Sign in required")
    }

    @Test func theSecondRowExistsOnlyForSomethingToActOn() throws {
        let level = try worktree(path: "/p", branch: "main", isMain: true, behindUpstream: 0)
        #expect(OverviewPresentation.behindCell(base: "main", worktrees: [level]) == nil)
        #expect(OverviewPresentation.behindCell(base: "main", worktrees: [try worktree(path: "/p", isMain: true, upstreamState: "no_upstream")]) == nil)
        #expect(OverviewPresentation.behindCell(base: "main", worktrees: [try worktree(path: "/p", isMain: true, upstreamState: "gone", behindUpstream: 3)]) == nil)
        #expect(OverviewPresentation.behindCell(base: "main", worktrees: [try worktree(path: "/p", branch: "other", isMain: true, behindUpstream: 3)]) == nil)
        let behind = try #require(OverviewPresentation.behindCell(base: "main", worktrees: [try worktree(path: "/p", isMain: true, behindUpstream: 3)]))
        #expect(behind.value == "main ↓3" && behind.label == "behind origin" && behind.tone == .warning)
        let unreadable = try #require(OverviewPresentation.behindCell(base: "main", worktrees: [try worktree(path: "/p", isMain: true, behindUpstream: nil)]))
        #expect(unreadable.value == "main ↓?")

        #expect(OverviewPresentation.cleanupCell(worktrees: [level, try worktree(path: "/a", branch: "a", merged: false)]) == nil)
        let cleanup = try #require(OverviewPresentation.cleanupCell(worktrees: [
            try worktree(path: "/a", branch: "a", merged: true),
            try worktree(path: "/b", branch: "b", merged: true),
            try worktree(path: "/gone", branch: "gone", missing: true, merged: true),
        ]))
        #expect(cleanup.value == "2" && cleanup.label == "merged to clean up")

        let strip = OverviewPresentation.statStrip(isGit: true, project: nil, folderDisk: .empty, github: ready, diskMeasuring: true)
        #expect(strip.first.map(\.id) == ["disk", "pull-requests"])
        #expect(strip.second.isEmpty)
        let folderDisk = CoreDiskUsage(path: "/d", totalBytes: 432_013_312, largestChildName: nil, largestChildBytes: nil, unavailableReason: nil)
        let folder = OverviewPresentation.statStrip(isGit: false, project: nil, folderDisk: folderDisk, github: ready, diskMeasuring: false)
        #expect(folder.first.map(\.value) == ["412 MB"] && folder.second.isEmpty)
        #expect(OverviewPresentation.statStrip(isGit: false, project: nil, folderDisk: .empty, github: ready, diskMeasuring: false).first.first?.value == "… GB")
    }

    @Test func theHeaderLineOmitsWhatDoesNotApply() throws {
        let clean = checkout("main", path: "/p", isWorktree: false, worktree: try worktree(path: "/p", isMain: true))
        #expect(OverviewPresentation.headerChips(checkout: clean, isGit: true).map(\.text) == ["Clean", "1.0 GB"])

        let busy = checkout("feature", path: "/f", changed: 12, pullRequest: pullRequest(107),
                            worktree: try worktree(path: "/f", branch: "feature", changed: 12, ahead: 3, behind: 2, diskBytes: 2_576_980_378))
        let chips = OverviewPresentation.headerChips(checkout: busy, isGit: true)
        #expect(chips.map(\.text) == ["#107 Open", "12 files", "↓2 behind main", "2.4 GB"])
        #expect(chips[0].tone == .normal)
        #expect(chips[0].tooltip == "#107 · Open")
        #expect(OverviewPresentation.headerAccessibilityLabel(checkout: busy, chips: chips)
                == "feature, pull request 107 open, 12 changed files, 2 behind, 2.4 GB")

        let aheadOnly = checkout("a", path: "/a", worktree: try worktree(path: "/a", branch: "a", ahead: 1))
        #expect(OverviewPresentation.headerChips(checkout: aheadOnly, isGit: true).map(\.text) == ["Clean", "↑1 ahead", "1.0 GB"])

        let measuring = checkout("m", path: "/m", worktree: try worktree(path: "/m", branch: "m", diskBytes: nil))
        let measuringChips = OverviewPresentation.headerChips(checkout: measuring, isGit: true)
        #expect(measuringChips.last?.text == "…" && measuringChips.last?.tone == .muted)
        let unreadable = checkout("u", path: "/u", worktree: try worktree(path: "/u", branch: "u", diskBytes: nil, diskUnavailable: "denied"))
        #expect(OverviewPresentation.headerChips(checkout: unreadable, isGit: true).last?.text == "? GB")
        let noStatus = checkout("s", path: "/s", worktree: try worktree(path: "/s", branch: "s", unavailableReason: "index locked"))
        #expect(OverviewPresentation.headerChips(checkout: noStatus, isGit: true).first?.text == "? files")

        let reading = checkout("r", path: "/r")
        #expect(OverviewPresentation.headerChips(checkout: reading, isGit: true).map(\.text) == ["… files"])

        let folder = checkout("folder", path: "/d", isWorktree: false)
        let folderDisk = CoreDiskUsage(path: "/d", totalBytes: 432_013_312, largestChildName: nil, largestChildBytes: nil, unavailableReason: nil)
        #expect(OverviewPresentation.headerChips(checkout: folder, isGit: false, folderDisk: folderDisk).map(\.text) == ["412 MB"])
        #expect(OverviewPresentation.headerChips(checkout: folder, isGit: false, folderDisk: .empty).map(\.text) == ["…"])
    }

    @Test func headerBadgesKeepLifecycleChecksAndAncestryInTheApprovedOrder() throws {
        let request = pullRequest(
            118,
            badge: .review,
            title: "Ship checkout purpose",
            review: .changesRequested,
            checks: .failed
        )
        var row = checkout(
            "topic",
            path: "/topic",
            changed: 3,
            pullRequest: request,
            worktree: try worktree(
                path: "/topic",
                branch: "topic",
                changed: 3,
                ahead: 4,
                behind: 64,
                diskBytes: 2_362_232_012
            )
        )
        row.purpose = CoreCheckoutPurpose(text: "체크아웃 행 목적 표시", origin: .token)

        let chips = OverviewPresentation.headerChips(checkout: row, isGit: true)
        #expect(chips.map(\.text) == [
            "#118 Changes requested", "✗ Checks", "3 files", "↓64 behind main", "2.2 GB",
        ])
        #expect(chips.map(\.tone) == [.warning, .danger, .warning, .warning, .normal])
        #expect(OverviewPresentation.headerAccessibilityLabel(checkout: row, chips: chips) ==
            "topic, 체크아웃 행 목적 표시, pull request 118 changes requested, checks failing, 3 changed files, 64 behind, 2.2 GB")

        #expect(OverviewPresentation.pullRequestLabel(pullRequest(1, badge: .merged, isDraft: true)) == "Merged")
        #expect(OverviewPresentation.pullRequestLabel(pullRequest(2, badge: .closed, review: .approved)) == "Closed")
        #expect(OverviewPresentation.pullRequestLabel(pullRequest(3, isDraft: true)) == "Draft")
        #expect(OverviewPresentation.pullRequestLabel(pullRequest(4, badge: .review, review: .approved)) == "Approved")
        #expect(OverviewPresentation.pullRequestLabel(pullRequest(5, badge: .review, review: .reviewRequired)) == "Review required")
        #expect(OverviewPresentation.pullRequestColorRole(pullRequest(6)) == .open)
        #expect(OverviewPresentation.pullRequestColorRole(pullRequest(7, badge: .merged)) == .merged)
        #expect(OverviewPresentation.pullRequestColorRole(pullRequest(8, badge: .closed)) == .closed)
        #expect(OverviewPresentation.pullRequestColorRole(pullRequest(9, isDraft: true)) == .draft)
        #expect(OverviewPresentation.pullRequestColorRole(
            pullRequest(10, badge: .review, review: .changesRequested)
        ) == .warning)
        #expect(OverviewPresentation.pullRequestColorRole(
            pullRequest(11, badge: .review, review: .approved)
        ) == .success)
    }

    @Test func githubPopoverNamesLifecycleAndChecksForEveryPullRequest() {
        let requested = OverviewPresentation.pullRequestPopoverDetail(pullRequest(
            118,
            badge: .review,
            review: .changesRequested,
            checks: .failed
        ))
        #expect(requested.status == "#118 · Changes requested · feature")
        #expect(requested.statusTone == .warning)
        #expect(requested.checks == "Checks failing")
        #expect(requested.checksTone == .danger)

        let absent = OverviewPresentation.pullRequestPopoverDetail(pullRequest(119))
        #expect(absent.status == "#119 · Open · feature")
        #expect(absent.checks == "Checks unavailable")
        #expect(absent.checksTone == .muted)
    }

    @Test func headerBadgesRenderLoadingUnavailableAndMissingAsSmallStates() throws {
        var loading = checkout("loading", path: "/loading")
        loading.github = CoreGithubStatus(
            available: false,
            loading: true,
            stale: false,
            lastSuccessAtUnixMS: nil,
            unavailableReason: nil
        )
        #expect(OverviewPresentation.headerChips(checkout: loading, isGit: true).map(\.text) == ["… PR", "… files"])

        var unavailable = checkout(
            "unavailable",
            path: "/unavailable",
            worktree: try worktree(path: "/unavailable", branch: "unavailable")
        )
        unavailable.github = CoreGithubStatus(
            available: false,
            loading: false,
            stale: false,
            lastSuccessAtUnixMS: nil,
            unavailableReason: "Sign in required"
        )
        #expect(OverviewPresentation.headerChips(checkout: unavailable, isGit: true).map(\.text) == ["? PR", "Clean", "1.0 GB"])

        let missing = checkout(
            "missing",
            path: "/missing",
            worktree: try worktree(path: "/missing", branch: "missing", missing: true)
        )
        #expect(OverviewPresentation.headerChips(checkout: missing, isGit: true).map(\.text) == ["missing", "1.0 GB"])
    }

    @Test func theOrderIsPrimaryThenLinkedByCreationTimeThenInactive() throws {
        let checkouts = [
            checkout("newer", path: "/n", worktree: try worktree(path: "/n", branch: "newer", createdAtUnixMS: 300)),
            checkout("undated", path: "/u"),
            checkout("main", path: "/p", isWorktree: false, worktree: try worktree(path: "/p", isMain: true)),
            checkout("older", path: "/o", worktree: try worktree(path: "/o", branch: "older", createdAtUnixMS: 100)),
            checkout("idle", path: "/i", worktree: try worktree(path: "/i", branch: "idle", createdAtUnixMS: 200)),
        ]
        let (active, inactive) = OverviewPresentation.orderedGroups(checkouts: checkouts, workspacePath: "/p", inactiveIDs: ["idle"])
        #expect(active.map(\.id) == ["main", "older", "newer", "undated"])
        #expect(inactive.map(\.id) == ["idle"])
    }

    @Test func rowsNestChildrenUnderParentsInTheSameGroupAndCaptionOnesFromElsewhere() {
        var parent = agent("pane-parent", title: "PRD orchestration")
        parent.lineageChildPaneIDs = ["pane-child", "pane-far"]
        var child = agent("pane-child", title: "Implement PRD", delegated: true)
        child.lineageDepth = 1
        child.spawnOriginPaneID = "pane-parent"
        var far = agent("pane-far", title: "Review PRD", delegated: true)
        far.lineageDepth = 1
        far.spawnOriginPaneID = "pane-parent"
        let main = checkout("main", path: "/p", isWorktree: false, paneIDs: ["pane-parent", "pane-child"])
        let other = checkout("other", path: "/o", paneIDs: ["pane-far"])
        let agents = [far, child, parent]

        let mainRows = OverviewPresentation.rows(agents: agents, in: main, checkouts: [main, other])
        #expect(mainRows.map(\.id) == ["pane-parent", "pane-child"])
        #expect(mainRows.map(\.depth) == [0, 1])
        #expect(mainRows.map(\.origin) == [nil, nil])

        let otherRows = OverviewPresentation.rows(agents: agents, in: other, checkouts: [main, other])
        #expect(otherRows.map(\.id) == ["pane-far"])
        #expect(otherRows.map(\.depth) == [0])
        #expect(otherRows.first?.origin == "from PRD orchestration · main")
    }

    @Test func searchKeepsMatchingRowsWithTheirGroupAndMatchesBranches() {
        var parent = agent("pane-parent", title: "PRD review")
        parent.lineageChildPaneIDs = ["pane-child"]
        var child = agent("pane-child", title: "구성 검토", delegated: true)
        child.lineageDepth = 1
        let feature = checkout("prd/hide-orchestrator", path: "/f", paneIDs: ["pane-parent", "pane-child"])
        let rows = OverviewPresentation.rows(agents: [parent, child], in: feature, checkouts: [feature])

        #expect(OverviewPresentation.filter(entries: rows, checkout: feature, query: "")?.map(\.id) == ["pane-parent", "pane-child"])
        #expect(OverviewPresentation.filter(entries: rows, checkout: feature, query: "구성")?.map(\.id) == ["pane-parent", "pane-child"])
        #expect(OverviewPresentation.filter(entries: rows, checkout: feature, query: "review")?.map(\.id) == ["pane-parent"])
        #expect(OverviewPresentation.filter(entries: rows, checkout: feature, query: "orchestrator")?.map(\.id) == ["pane-parent", "pane-child"])
        #expect(OverviewPresentation.filter(entries: rows, checkout: feature, query: "nothing") == nil)
        #expect(OverviewPresentation.filter(entries: [], checkout: feature, query: "hide")?.isEmpty == true)
    }

    @Test func menusListTheAgreedItemsInOrder() {
        #expect(OverviewPresentation.headerMenuItems(hasPullRequest: true, isWorktree: true, hasBranch: true) == [
            "New agent here", "New worktree…", "Set as base branch", nil,
            "Open pull request", "Open in History", nil,
            "Copy Path", "Open in", nil, "Delete worktree…",
        ])
        #expect(OverviewPresentation.headerMenuItems(hasPullRequest: false, isWorktree: false, hasBranch: false) == [
            "New agent here", "New worktree…", nil, "Open in History", nil, "Copy Path", "Open in",
        ])
        #expect(OverviewPresentation.agentMenuItems == ["Open pane", "Reveal in sidebar", nil, "Copy pane id", nil, "Close pane…"])
        #expect(OverviewPresentation.providerChoices.map(\.title) == ["Terminal only", "Claude", "Codex"])
        #expect(OverviewPresentation.providerChoices.map(\.provider) == ["terminal", "claude", "codex"])
    }

    @Test func cleanupWireKeepsFailureAndExclusionSeparateFromSize() throws {
        let json = #"{"id":1,"repository_root":"/fixture","phase":"complete","message":null,"rows":[{"path":"/fixture/linked","branch":"작업/한글","head":"abc","exclusion":null,"disk":{"path":"/fixture/linked","total_bytes":null,"largest_child_name":null,"largest_child_bytes":null,"unavailable_reason":"unreadable"},"result":"refused","message":"State changed. Review again."}]}"#
        let review = try JSONDecoder().decode(CoreCleanup.self, from: Data(json.utf8))
        #expect(review.rows[0].disk.totalBytes == nil)
        #expect(review.rows[0].disk.unavailableReason == "unreadable")
        #expect(review.rows[0].result == "refused")
        #expect(review.rows[0].message == "State changed. Review again.")
        #expect(review.rows[0].branch == "작업/한글")
    }
}
