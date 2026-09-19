import Foundation

/// What the Overview says, decided here rather than in the view body.
///
/// The rules this file holds are the ones the PRD writes down: one glyph
/// language for every value (a number, `…` while it is being read, `?` when
/// it could not be, and nothing at all when it does not apply), which row a
/// stat cell sits on and when it is drawn, the fixed order of the worktree
/// groups, and what the search keeps. They are pure functions so a test can
/// state the answer from outside rather than from a screenshot (PRD D-04,
/// D-05, D-07, D-08, D-13, engineering rule 12).
enum OverviewPresentation {
    /// How a value is drawn: a measured value in the panel's own colour, a
    /// pending one dimmed, and one that could not be read in warning.
    enum Tone: Equatable {
        case normal
        case muted
        case warning
    }

    /// One cell of the stat strip: a value, the word beside it, and the
    /// glyph that leads it when the cell is about pull requests.
    struct StatCell: Equatable {
        let id: String
        let glyph: String?
        let value: String
        let label: String
        let tone: Tone
        let tooltip: String
    }

    /// The strip's first row is always drawn for a Git project; the second
    /// row holds only cells that have something to say, and is absent when
    /// neither does. A folder project has the disk cell alone (PRD D-07).
    struct StatStrip: Equatable {
        let first: [StatCell]
        let second: [StatCell]
    }

    static func subtitle(workspaceCount: Int, inactiveCount: Int) -> String {
        let workspaces = workspaceCount == 1 ? "1 workspace" : "\(workspaceCount) workspaces"
        return inactiveCount > 0 ? "Project · \(workspaces) · \(inactiveCount) inactive" : "Project · \(workspaces)"
    }

    static func statStrip(
        isGit: Bool,
        project: CoreProjectWorktrees?,
        github: CoreGithubStatus,
        diskMeasuring: Bool,
        now: Date = Date()
    ) -> StatStrip {
        let disk = diskCell(total: project?.diskTotalBytes, confirmed: project?.diskConfirmedBytes,
                            failure: project?.diskUnavailableReason, measuring: diskMeasuring)
        guard isGit else { return StatStrip(first: [disk], second: []) }
        let first = [disk, pullRequestsCell(status: github, requests: project?.pullRequests, now: now)]
        let second = [
            behindCell(base: project?.baseBranch, worktrees: project?.worktrees ?? [], now: now),
            cleanupCell(worktrees: project?.worktrees ?? []),
        ].compactMap { $0 }
        return StatStrip(first: first, second: second)
    }

    /// `25 GB on disk`, or `… GB` while the walk runs and `? GB` when a
    /// component could not be read. A confirmed subtotal beside a failed
    /// component is still `?`: the number the cell would show is not the
    /// project's size, and the popover has the subtotal (PRD B4, D-08).
    static func diskCell(total: UInt64?, confirmed: UInt64?, failure: String?, measuring: Bool) -> StatCell {
        let tooltip = "Allocated on disk, shared Git data counted once"
        if let total {
            return StatCell(id: "disk", glyph: nil, value: CheckoutCardPresentation.formattedBytes(Double(total)),
                            label: "on disk", tone: .normal, tooltip: tooltip)
        }
        if measuring || (failure == nil && confirmed == nil) {
            return StatCell(id: "disk", glyph: nil, value: "… GB", label: "on disk", tone: .muted,
                            tooltip: "Measuring allocated disk…")
        }
        let reason = failure ?? "Some folders could not be measured"
        let subtotal = confirmed.map { "; confirmed subtotal \(CheckoutCardPresentation.formattedBytes(Double($0)))" } ?? ""
        return StatCell(id: "disk", glyph: nil, value: "? GB", label: "on disk", tone: .warning,
                        tooltip: "Disk unavailable: \(reason)\(subtotal)")
    }

    /// `⑂ 2 open PRs`, `⑂ …` while `gh` answers, `⑂ ?` when it cannot. A
    /// stale answer keeps its number; the tooltip and the popover say `as of`
    /// (PRD B5, D-09). Zero is a measured value and is drawn (PRD D-07).
    static func pullRequestsCell(status: CoreGithubStatus, requests: [CorePullRequest]?, now: Date = Date()) -> StatCell {
        let glyph = "⑂"
        if status.loading, requests == nil || !status.available {
            return StatCell(id: "pull-requests", glyph: glyph, value: "…", label: "open PRs", tone: .muted,
                            tooltip: "Updating GitHub status…")
        }
        if let requests, status.available || status.stale {
            let open = requests.filter { !$0.badge.isSettled }.count
            let window = "One PR per branch, from the most recent pull requests"
            let stale = CheckoutCardPresentation.staleNotice(status, now: now).map { " · \($0)" } ?? ""
            return StatCell(id: "pull-requests", glyph: glyph, value: "\(open)",
                            label: open == 1 ? "open PR" : "open PRs", tone: .normal, tooltip: window + stale)
        }
        let reason: String
        if status.failureCategory == "authentication" || status.failureCategory == "not logged in" {
            reason = "Sign in required"
        } else {
            reason = status.unavailableReason ?? "GitHub status not loaded"
        }
        return StatCell(id: "pull-requests", glyph: glyph, value: "?", label: "open PRs", tone: .warning, tooltip: reason)
    }

    /// `main ↓3 behind origin`, only when the base branch is checked out in
    /// a worktree of this project and that worktree has an upstream. No
    /// upstream is not zero: the cell is absent. An upstream whose count
    /// could not be read is `main ↓?`. A base level with its upstream is
    /// also absent, because the row exists for something the operator would
    /// act on (PRD D-07, D-09, design rule 13).
    static func behindCell(base: String?, worktrees: [CoreGitWorktree], now: Date = Date()) -> StatCell? {
        guard let base, let row = worktrees.first(where: { $0.branch == base && !$0.missing }) else { return nil }
        let fetched = row.lastFetchAtUnixMS.map { "since the last fetch \(CheckoutCardPresentation.relativeAge(fromUnixMS: $0, now: now))" }
            ?? "since the last fetch"
        switch row.upstreamState {
        case "no_upstream", "gone":
            return nil
        case "pushed", "unpushed":
            guard let behind = row.behindUpstream else {
                return StatCell(id: "behind", glyph: nil, value: "\(base) ↓?", label: "behind origin", tone: .warning,
                                tooltip: "The upstream of \(base) could not be compared \(fetched)")
            }
            guard behind > 0 else { return nil }
            return StatCell(id: "behind", glyph: nil, value: "\(base) ↓\(behind)", label: "behind origin", tone: .warning,
                            tooltip: "\(base) is \(behind) \(behind == 1 ? "commit" : "commits") behind its upstream \(fetched); Hide does not fetch")
        default:
            return StatCell(id: "behind", glyph: nil, value: "\(base) ↓?", label: "behind origin", tone: .warning,
                            tooltip: row.unavailableReason.map { "The upstream of \(base) could not be read: \($0)" }
                                ?? "The upstream of \(base) could not be read")
        }
    }

    /// `2 merged to clean up`: linked worktrees whose branch is an ancestor
    /// of the base. The cleanup sheet decides what may actually go; this is
    /// the reason to open it, and at zero there is no reason (PRD B3).
    static func cleanupCell(worktrees: [CoreGitWorktree]) -> StatCell? {
        let merged = worktrees.filter { !$0.isMain && !$0.missing && $0.merged == true }.count
        guard merged > 0 else { return nil }
        return StatCell(id: "cleanup", glyph: nil, value: "\(merged)", label: "merged to clean up", tone: .warning,
                        tooltip: "Review merged worktrees that can be removed")
    }

    // MARK: - Group header

    enum ChipKind: Equatable {
        case files
        case aheadBehind
        case disk
        case pullRequest
    }

    /// One chip of the group header's detail line. `text` is the whole of
    /// what the chip draws; the pull request chip draws its number after the
    /// bundled icon the view supplies.
    struct HeaderChip: Equatable {
        let kind: ChipKind
        let text: String
        let tone: Tone
        let tooltip: String
    }

    /// `12 files · ↑3 ↓1 · 2.4 GB · #107`, each part only when it applies:
    /// no changes reads `Clean`, a zero side of `↑↓` is left out, a plain
    /// folder has only its size, and a worktree with no pull request has no
    /// chip. Size follows the glyph language: `…` while measuring, `? GB`
    /// when unreadable (PRD B7, D-04, D-08).
    static func headerChips(checkout: CoreCheckoutSnapshot, isGit: Bool, now: Date = Date()) -> [HeaderChip] {
        var chips: [HeaderChip] = []
        let worktree = checkout.worktree
        if isGit {
            if let worktree, worktree.missing {
                chips.append(HeaderChip(kind: .files, text: "missing", tone: .warning,
                                        tooltip: "Git lists this worktree but its folder is not on disk"))
            } else if let worktree, worktree.unavailableReason != nil {
                chips.append(HeaderChip(kind: .files, text: "? files", tone: .warning,
                                        tooltip: "Git status unavailable: \(worktree.unavailableReason ?? "")"))
            } else if worktree == nil {
                chips.append(HeaderChip(kind: .files, text: "… files", tone: .muted, tooltip: "Reading Git status…"))
            } else {
                let count = checkout.changedFileCount
                chips.append(HeaderChip(
                    kind: .files,
                    text: count == 0 ? "Clean" : "\(count) \(count == 1 ? "file" : "files")",
                    tone: .normal,
                    tooltip: count == 0 ? "No uncommitted changes · Open in History"
                        : "\(count) uncommitted \(count == 1 ? "change" : "changes") · Open in History"
                ))
            }
            if let worktree, !worktree.missing, worktree.baseBranch != nil, worktree.ahead > 0 || worktree.behind > 0 {
                var parts: [String] = []
                if worktree.ahead > 0 { parts.append("↑\(worktree.ahead)") }
                if worktree.behind > 0 { parts.append("↓\(worktree.behind)") }
                let base = worktree.baseBranch ?? ""
                chips.append(HeaderChip(
                    kind: .aheadBehind,
                    text: parts.joined(separator: " "),
                    tone: worktree.behind > 0 ? .warning : .normal,
                    tooltip: "\(worktree.ahead) ahead, \(worktree.behind) behind \(base)"
                ))
            }
        }
        if let worktree {
            if let bytes = worktree.disk.totalBytes {
                chips.append(HeaderChip(kind: .disk, text: CheckoutCardPresentation.formattedBytes(bytes), tone: .normal,
                                        tooltip: "Allocated on disk for this worktree, shared Git data excluded"))
            } else if let reason = worktree.disk.unavailableReason {
                chips.append(HeaderChip(kind: .disk, text: "? GB", tone: .warning, tooltip: "Disk unavailable: \(reason)"))
            } else {
                chips.append(HeaderChip(kind: .disk, text: "…", tone: .muted, tooltip: "Measuring allocated disk…"))
            }
        }
        if isGit, let pullRequest = checkout.pullRequest {
            chips.append(HeaderChip(
                kind: .pullRequest,
                text: "#\(pullRequest.number)",
                tone: .normal,
                tooltip: "#\(pullRequest.number) · \(CheckoutCardPresentation.pullRequestState(pullRequest).lowercased())"
                    + (pullRequest.title.map { " · \($0)" } ?? "")
            ))
        }
        return chips
    }

    /// What VoiceOver reads for a group header: the branch, then each chip
    /// in words, so `#107` becomes `pull request 107 open` (PRD B21).
    static func headerAccessibilityLabel(checkout: CoreCheckoutSnapshot, chips: [HeaderChip]) -> String {
        var parts = [checkout.label]
        for chip in chips {
            switch chip.kind {
            case .files:
                parts.append(chip.text == "Clean" ? "clean" : chip.text.replacingOccurrences(of: "files", with: "changed files"))
            case .aheadBehind:
                let worktree = checkout.worktree
                if let ahead = worktree?.ahead, ahead > 0 { parts.append("\(ahead) ahead") }
                if let behind = worktree?.behind, behind > 0 { parts.append("\(behind) behind") }
            case .disk:
                parts.append(chip.text == "…" ? "size measuring" : chip.text)
            case .pullRequest:
                if let pullRequest = checkout.pullRequest {
                    parts.append("pull request \(pullRequest.number) \(CheckoutCardPresentation.pullRequestState(pullRequest).lowercased())")
                }
            }
        }
        return parts.joined(separator: ", ")
    }

    // MARK: - Order

    /// The list's fixed order: the primary checkout, then the linked
    /// worktrees oldest first by the time they were added, with a worktree
    /// whose time is unknown after the dated ones and ties by name; the
    /// inactive fold's members come out separately, in the same order. No
    /// agent state and no search moves a group (PRD B6, D-05).
    static func orderedGroups(
        checkouts: [CoreCheckoutSnapshot],
        workspacePath: String,
        inactiveIDs: [String]
    ) -> (active: [CoreCheckoutSnapshot], inactive: [CoreCheckoutSnapshot]) {
        let inactive = Set(inactiveIDs)
        let ordered = checkouts.sorted { left, right in
            let leftPrimary = isPrimary(left, workspacePath: workspacePath)
            let rightPrimary = isPrimary(right, workspacePath: workspacePath)
            if leftPrimary != rightPrimary { return leftPrimary }
            switch (left.worktree?.createdAtUnixMS, right.worktree?.createdAtUnixMS) {
            case let (l?, r?) where l != r: return l < r
            case (.some, .none): return true
            case (.none, .some): return false
            default: return left.label.localizedStandardCompare(right.label) == .orderedAscending
            }
        }
        return (ordered.filter { !inactive.contains($0.id) }, ordered.filter { inactive.contains($0.id) })
    }

    static func isPrimary(_ checkout: CoreCheckoutSnapshot, workspacePath: String) -> Bool {
        checkout.worktree?.isMain == true || (!checkout.isWorktree && checkout.path == workspacePath)
    }

    // MARK: - Rows

    /// One agent row inside a group: the agent, how deep under its parent
    /// it sits, and, for a child whose parent works in another worktree,
    /// the caption naming where it came from (PRD B9, B10).
    struct AgentEntry: Identifiable, Equatable {
        let agent: SidebarAgent
        let depth: Int
        let origin: String?
        var id: String { agent.paneID }
    }

    /// The rows of one group: the checkout's agents in pane order, each
    /// child indented under a parent that is in the same group. A child
    /// whose parent is elsewhere is a root of this group with an origin
    /// caption when the parent is still an agent Hide can name; an agent
    /// with no known parent is a root and nothing more (PRD D-05, B10).
    static func rows(
        agents: [SidebarAgent],
        in checkout: CoreCheckoutSnapshot,
        checkouts: [CoreCheckoutSnapshot]
    ) -> [AgentEntry] {
        let paneOrder = checkout.tabs.flatMap(\.panes).map(\.id)
        let here = SidebarGrouping.agents(agents, in: checkout)
        let byPane = Dictionary(uniqueKeysWithValues: here.map { ($0.paneID, $0) })
        let position = Dictionary(uniqueKeysWithValues: paneOrder.enumerated().map { ($1, $0) })
        let ordered = here.sorted { (position[$0.paneID] ?? .max) < (position[$1.paneID] ?? .max) }
        let claimed = Set(here.flatMap { $0.lineageChildPaneIDs.filter { byPane[$0] != nil } })
        let roots = ordered.filter { !claimed.contains($0.paneID) }
        let allByPane = Dictionary(uniqueKeysWithValues: agents.map { ($0.paneID, $0) })
        let checkoutByPane = Dictionary(uniqueKeysWithValues: checkouts.flatMap { checkout in
            checkout.tabs.flatMap(\.panes).map { ($0.id, checkout) }
        })

        var entries: [AgentEntry] = []
        var visited = Set<String>()
        var stack = roots.reversed().map { ($0, 0) }
        while let (agent, depth) = stack.popLast() {
            guard visited.insert(agent.paneID).inserted else { continue }
            var origin: String?
            if depth == 0, let parentPane = agent.spawnOriginPaneID, let parent = allByPane[parentPane] {
                let parentCheckout = checkoutByPane[parentPane]
                origin = "from \(parent.identityLabel)" + (parentCheckout.map { " · \($0.label)" } ?? "")
            }
            entries.append(AgentEntry(agent: agent, depth: depth, origin: origin))
            let children = agent.lineageChildPaneIDs.compactMap { byPane[$0] }
                .sorted { (position[$0.paneID] ?? .max) < (position[$1.paneID] ?? .max) }
            stack.append(contentsOf: children.reversed().map { ($0, depth + 1) })
        }
        return entries
    }

    /// The row's second text: the sentence the core chose for its state, or
    /// the rolling task title when the state chose none.
    static func rowTask(_ agent: SidebarAgent) -> String? {
        agent.detail ?? agent.task
    }

    // MARK: - Search

    /// Whether a group survives the query: its branch matches, or one of its
    /// rows does by name or task. A matching group keeps only its matching
    /// rows plus their ancestors, in the order they already had (PRD B15).
    static func filter(
        entries: [AgentEntry],
        checkout: CoreCheckoutSnapshot,
        query: String
    ) -> [AgentEntry]? {
        let normalized = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !normalized.isEmpty else { return entries }
        if checkout.label.localizedCaseInsensitiveContains(normalized)
            || checkout.branch?.localizedCaseInsensitiveContains(normalized) == true {
            return entries
        }
        let matching = Set(entries.filter { entry in
            [entry.agent.identityLabel, entry.agent.task, entry.agent.detail]
                .compactMap { $0 }
                .contains { $0.localizedCaseInsensitiveContains(normalized) }
        }.map(\.id))
        guard !matching.isEmpty else { return nil }
        var included = matching
        for (index, entry) in entries.enumerated() where matching.contains(entry.id) {
            var remaining = entry.depth
            for ancestor in entries[..<index].reversed() where remaining > 0 {
                if ancestor.depth == remaining - 1 {
                    included.insert(ancestor.id)
                    remaining -= 1
                }
            }
        }
        return entries.filter { included.contains($0.id) }
    }

    // MARK: - Menus

    static let newAgentHere = "New agent here"
    static let startAgent = "Start agent…"
    static let openInHistory = "Open in History"
    static let openPane = "Open pane"
    static let revealInSidebar = "Reveal in sidebar"
    static let copyPaneID = "Copy pane id"
    static let closePane = "Close pane…"
    static let noMatch = "No matching agents or workspaces"
    static let disconnected = "Live task status unavailable; showing the last known agents"

    /// The provider choices of `New agent here ▸`, in the order the New
    /// worktree sheet's `Start with` picker lists them (PRD D-11).
    static let providerChoices: [(title: String, provider: String)] =
        [("Terminal only", "terminal")] + AgentProvider.allCases.map { ($0.rawValue.capitalized, $0.rawValue) }

    static func openPullRequestTitle(_ pullRequest: CorePullRequest) -> String {
        "Open pull request #\(pullRequest.number)"
    }

    /// The header menu's items in order, with `nil` for a separator, so a
    /// test can state the list without building a menu (PRD B12).
    static func headerMenuItems(hasPullRequest: Bool, isWorktree: Bool, hasBranch: Bool) -> [String?] {
        var items: [String?] = [newAgentHere, WorktreeMenuPolicy.newWorktree]
        if hasBranch { items.append(WorktreeMenuPolicy.setBaseBranch) }
        items.append(nil)
        if hasPullRequest { items.append("Open pull request") }
        items.append(openInHistory)
        items.append(nil)
        items.append(WorktreeMenuPolicy.copyPath)
        items.append(WorktreeMenuPolicy.openIn)
        if isWorktree {
            items.append(nil)
            items.append("Delete worktree…")
        }
        return items
    }

    static let agentMenuItems: [String?] = [openPane, revealInSidebar, nil, copyPaneID, nil, closePane]
}
