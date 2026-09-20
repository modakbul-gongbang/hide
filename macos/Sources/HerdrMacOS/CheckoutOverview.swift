import SwiftUI

/// The right panel's Overview: the current project's worktrees as one list,
/// each group a worktree with its Git facts on the header and the agents
/// working in it as rows, under a strip of derived project facts.
///
/// Every row click moves the terminal to that pane; the header opens the
/// workspace; the `N files` chip opens the checkout on History. Nothing here
/// inspects without moving, and nothing scrolls, folds, searches or refreshes
/// changes pane focus or read state (right-panel-overview D-01, D-02, B20).
struct CheckoutOverview: View {
    @EnvironmentObject private var model: ShellModel
    @State private var query = ""
    @State private var searchSelection = HideSearchSelection()
    @State private var showDisk = false
    @State private var showGitHub = false
    @State private var showCleanup = false

    private var project: CoreProjectWorktrees? { model.core.snapshot?.gitWorktrees }
    private var workspace: CoreWorkspaceSnapshot? { model.focusedWorkspace }

    /// One worktree group as the list draws it: the checkout, its rows after
    /// the search, and whether it sits behind the `Inactive N` fold.
    private struct WorktreeGroup: Identifiable {
        let checkout: CoreCheckoutSnapshot
        let entries: [OverviewPresentation.AgentEntry]
        let inactive: Bool
        var id: String { checkout.id }
    }

    private var ordered: (active: [CoreCheckoutSnapshot], inactive: [CoreCheckoutSnapshot]) {
        guard let workspace else { return ([], []) }
        return OverviewPresentation.orderedGroups(
            checkouts: workspace.checkouts,
            workspacePath: workspace.path,
            inactiveIDs: workspace.inactiveCheckouts.checkoutIDs
        )
    }

    private func entries(for checkout: CoreCheckoutSnapshot) -> [OverviewPresentation.AgentEntry] {
        OverviewPresentation.rows(agents: model.agents, in: checkout, checkouts: workspace?.checkouts ?? [])
    }

    /// The groups the list shows for the current query: every active group
    /// when there is none, otherwise only the groups the query keeps, each
    /// reduced to its matching rows. Inactive checkouts join only when their
    /// own branch matches, as a header line.
    private var groups: [WorktreeGroup] {
        let (active, inactive) = ordered
        let searching = !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        var groups: [WorktreeGroup] = active.compactMap { checkout in
            let rows = entries(for: checkout)
            guard let kept = OverviewPresentation.filter(entries: rows, checkout: checkout, query: query) else { return nil }
            return WorktreeGroup(checkout: checkout, entries: kept, inactive: false)
        }
        if searching {
            groups += inactive.compactMap { checkout in
                OverviewPresentation.filter(entries: [], checkout: checkout, query: query)
                    .map { _ in WorktreeGroup(checkout: checkout, entries: [], inactive: true) }
            }
        }
        return groups
    }

    /// The inactive checkouts under the `Inactive N` fold, when it is open
    /// and no search is narrowing the list; a search lists a matching
    /// inactive checkout among the groups instead (PRD D-06, B15).
    private var unfoldedInactive: [CoreCheckoutSnapshot] {
        guard query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty,
              workspace?.inactiveCheckouts.expanded == true else { return [] }
        return ordered.inactive
    }

    private var searchResultIDs: [String] {
        groups.flatMap { group in group.entries.map(\.id) }
    }

    var body: some View {
        if model.isRemoteContext {
            HideEmptyState("Overview is local only", systemImage: "externaldrive",
                           description: Text("Git context is read on this Mac."))
        } else if model.core.snapshot == nil {
            HideEmptyState("Loading Overview", systemImage: "clock",
                           description: Text("Waiting for the first runtime snapshot."))
        } else if let workspace {
            VStack(alignment: .leading, spacing: HideTheme.spacingNone) {
                projectBlock(workspace)
                Divider()
                HideSearchField(placeholder: "Find agent or workspace…", text: $query,
                                selection: $searchSelection, resultIDs: searchResultIDs, activate: {
                    if let paneID = searchSelection.selectedID { model.selectAgent(paneID: paneID) }
                }, dismiss: { query = "" })
                    .padding(.horizontal, HideTheme.spacingMD)
                    .padding(.top, HideTheme.spacingSM)
                    .accessibilityIdentifier("overview-search")
                if !model.agentsConnected {
                    Label(OverviewPresentation.disconnected, systemImage: "bolt.slash")
                        .hideFont(size: HideTheme.Typography.subhead)
                        .foregroundStyle(HideTheme.warning)
                        .padding(.horizontal, HideTheme.spacingMD)
                        .padding(.top, HideTheme.spacingXS)
                        .accessibilityIdentifier("overview-disconnected")
                }
                list(workspace)
            }
            .foregroundStyle(HideTheme.primary)
            .buttonStyle(HideTextButtonStyle(density: .regular))
            .accessibilityIdentifier("checkout-overview")
            .sheet(isPresented: $showCleanup) {
                MergedWorktreeCleanup(review: project?.cleanup) {
                    model.core.dispatch(kind: "cleanup_dismiss", payload: [:]); showCleanup = false
                }
                .environmentObject(model)
            }
            .onChange(of: workspace.id) { _, _ in query = "" }
        } else {
            HideEmptyState("No workspace", systemImage: "folder",
                           description: Text("Choose a workspace to see its context."))
        }
    }

    // MARK: - Project block

    private func projectBlock(_ workspace: CoreWorkspaceSnapshot) -> some View {
        let strip = OverviewPresentation.statStrip(
            isGit: workspace.isGit,
            project: workspace.isGit ? project : nil,
            folderDisk: model.card.disk,
            github: model.card.github,
            diskMeasuring: model.card.diskMeasuring
        )
        return VStack(alignment: .leading, spacing: HideTheme.spacingSM) {
            HStack(alignment: .top, spacing: HideTheme.spacingSM) {
                VStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
                    Text(workspace.label).hideFont(size: HideTheme.Typography.headline, weight: .semibold)
                        .lineLimit(1)
                    Text(OverviewPresentation.subtitle(
                        workspaceCount: workspace.checkouts.count,
                        inactiveCount: workspace.inactiveCheckouts.checkoutIDs.count
                    ))
                    .hideFont(size: HideTheme.Typography.subhead).foregroundStyle(HideTheme.secondary)
                }
                Spacer(minLength: HideTheme.spacingXS)
                HideIconButton(systemImage: HideTheme.GitIcon.refresh, help: "Refresh Overview", variant: .toolbar) {
                    model.refreshCheckoutCard()
                }
            }
            VStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
                HStack(spacing: HideTheme.spacingMD) {
                    ForEach(strip.first, id: \.id) { cell in statCell(cell, workspace: workspace) }
                }
                if !strip.second.isEmpty {
                    HStack(spacing: HideTheme.spacingMD) {
                        ForEach(strip.second, id: \.id) { cell in statCell(cell, workspace: workspace) }
                    }
                }
            }
            .padding(.horizontal, HideTheme.spacingSM)
            .accessibilityIdentifier("overview-stat-strip")
        }
        .padding(.horizontal, HideTheme.spacingMD).padding(.top, HideTheme.spacingMD).padding(.bottom, HideTheme.spacingSM)
    }

    /// One stat cell. The disk and pull-request cells open their popovers,
    /// cleanup opens the review sheet, and `behind` is read only: Hide does
    /// not fetch, so there is nothing for a click to do (PRD D-07).
    @ViewBuilder
    private func statCell(_ cell: OverviewPresentation.StatCell, workspace: CoreWorkspaceSnapshot) -> some View {
        let label = statLabel(cell)
        switch cell.id {
        case "disk":
            Button { showDisk.toggle() } label: { label }
                .buttonStyle(HideInteractiveButtonStyle())
                .popover(isPresented: $showDisk) { diskDetails(workspace) }
                .hideTooltip(cell.tooltip)
                .accessibilityLabel("\(cell.value) \(cell.label)")
                .accessibilityIdentifier("overview-stat-disk")
        case "pull-requests":
            Button { showGitHub.toggle() } label: { label }
                .buttonStyle(HideInteractiveButtonStyle())
                .popover(isPresented: $showGitHub) { githubDetails(workspace) }
                .hideTooltip(cell.tooltip)
                .accessibilityLabel("\(cell.value) \(cell.label)")
                .accessibilityIdentifier("overview-stat-pull-requests")
        case "cleanup":
            Button {
                showCleanup = true
                model.core.dispatch(kind: "cleanup_review", payload: [:])
            } label: { label }
                .buttonStyle(HideInteractiveButtonStyle())
                .hideTooltip(cell.tooltip)
                .accessibilityLabel("\(cell.value) \(cell.label)")
                .accessibilityIdentifier("overview-stat-cleanup")
        default:
            label
                .hideTooltip(cell.tooltip)
                .accessibilityElement(children: .ignore)
                .accessibilityLabel("\(cell.value) \(cell.label)")
                .accessibilityIdentifier("overview-stat-\(cell.id)")
        }
    }

    private func statLabel(_ cell: OverviewPresentation.StatCell) -> some View {
        HStack(spacing: HideTheme.spacingXS) {
            if let glyph = cell.glyph {
                Text(glyph).hideFont(size: HideTheme.Typography.caption, weight: .semibold)
                    .foregroundStyle(tone(cell.tone))
            }
            Text(cell.value).hideFont(size: HideTheme.Typography.subhead, weight: .semibold, design: .monospaced)
                .foregroundStyle(tone(cell.tone))
            Text(cell.label).hideFont(size: HideTheme.Typography.caption)
                .foregroundStyle(HideTheme.secondary)
        }
        .padding(.vertical, HideTheme.spacingXXS).padding(.horizontal, HideTheme.spacingXS)
        .contentShape(Rectangle())
    }

    private func tone(_ tone: OverviewPresentation.Tone) -> Color {
        switch tone {
        case .normal: HideTheme.primary
        case .muted: HideTheme.muted
        case .warning: HideTheme.warning
        }
    }

    private func githubDetails(_ workspace: CoreWorkspaceSnapshot) -> some View {
        ScrollView { VStack(alignment: .leading, spacing: HideTheme.spacingMD) {
            Text("GitHub").hideFont(size: HideTheme.Typography.title, weight: .semibold)
            if model.card.github.loading {
                ProgressView("Updating GitHub status…").controlSize(.small)
            }
            if let reason = model.card.github.unavailableReason {
                Text(reason).foregroundStyle(HideTheme.warning).fixedSize(horizontal: false, vertical: true)
            }
            if let stale = CheckoutCardPresentation.staleNotice(model.card.github) {
                Text(stale).foregroundStyle(HideTheme.secondary)
            }
            Text("One PR per branch, from the latest \(project?.pullRequestWindow ?? "") PRs.")
                .foregroundStyle(HideTheme.secondary).fixedSize(horizontal: false, vertical: true)
            if project?.github?.available == true && project?.pullRequests?.isEmpty == true {
                Text("No recent pull requests")
            }
            ForEach(project?.pullRequests ?? [], id: \.number) { pr in
                Button { model.openPullRequest(pr) } label: {
                    VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
                        Text(pr.title ?? "PR #\(pr.number)").lineLimit(2)
                        Text("#\(pr.number) · \(pr.headBranch)").foregroundStyle(HideTheme.secondary)
                    }
                }.buttonStyle(HideTextButtonStyle(appearance: .quiet))
            }
            Button("Refresh") { model.requestGithubStatus(workspace, refresh: true) }
        }.hideFont(size: HideTheme.Typography.title).padding(HideTheme.spacingLG)
            .frame(width: HideTheme.Layout.pullRequestPopoverWidth) }
            .frame(maxHeight: HideTheme.searchSheetSize.height)
            .buttonStyle(HideTextButtonStyle(density: .regular))
            .foregroundStyle(HideTheme.primary).background(HideTheme.panel)
            .hideOverlayHost().preferredColorScheme(.dark)
    }

    private func diskDetails(_ workspace: CoreWorkspaceSnapshot) -> some View {
        ScrollView { VStack(alignment: .leading, spacing: HideTheme.spacingMD) {
            Text("Allocated on disk").hideFont(size: HideTheme.Typography.title, weight: .semibold)
            if model.card.diskMeasuring {
                ProgressView("Measuring allocated disk…").controlSize(.small)
            }
            Text("Shared Git data is counted once. APFS clones may share storage; this is not guaranteed reclaimable space.")
                .foregroundStyle(HideTheme.secondary).fixedSize(horizontal: false, vertical: true)
            if workspace.isGit {
                if project?.diskTotalBytes == nil, let known = project?.diskConfirmedBytes {
                    Text("Confirmed subtotal: \(CheckoutCardPresentation.formattedBytes(Double(known))). Missing measurements below are excluded.")
                        .foregroundStyle(HideTheme.warning).fixedSize(horizontal: false, vertical: true)
                }
                ForEach(project?.worktrees ?? []) { row in
                    VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
                        detailRow(row.isMain ? "Main checkout" : row.label,
                                  value: row.disk.totalBytes.map(CheckoutCardPresentation.formattedBytes) ?? "Unavailable")
                        if let reason = row.disk.unavailableReason { Text(reason).foregroundStyle(HideTheme.warning) }
                    }
                }
                detailRow("Shared Git data", value: project?.sharedGitDisk?.totalBytes.map(CheckoutCardPresentation.formattedBytes) ?? "Unavailable")
                if let reason = project?.sharedGitDisk?.unavailableReason { Text(reason).foregroundStyle(HideTheme.warning) }
            } else {
                detailRow(workspace.label, value: model.card.disk.totalBytes.map(CheckoutCardPresentation.formattedBytes) ?? "Unavailable")
                if let reason = model.card.disk.unavailableReason { Text(reason).foregroundStyle(HideTheme.warning) }
            }
            Button("Measure again") { model.refreshCheckoutCard() }
        }.hideFont(size: HideTheme.Typography.title).padding(HideTheme.spacingLG)
            .frame(width: HideTheme.Layout.pullRequestPopoverWidth) }
            .frame(maxHeight: HideTheme.searchSheetSize.height)
            .buttonStyle(HideTextButtonStyle(density: .regular))
            .foregroundStyle(HideTheme.primary).background(HideTheme.panel)
            .hideOverlayHost().preferredColorScheme(.dark)
    }

    private func detailRow(_ label: String, value: String) -> some View {
        HStack(spacing: HideTheme.spacingSM) {
            Text(label).foregroundStyle(HideTheme.secondary)
            Spacer(minLength: HideTheme.spacingXS)
            Text(value).fontWeight(.medium).multilineTextAlignment(.trailing).fixedSize(horizontal: false, vertical: true)
        }.hideFont(size: HideTheme.Typography.title).contentShape(Rectangle())
    }

    // MARK: - List

    private func list(_ workspace: CoreWorkspaceSnapshot) -> some View {
        let groups = groups
        let searching = !query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        let (_, inactive) = ordered
        return ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: HideTheme.spacingNone) {
                    if groups.isEmpty {
                        if searching {
                            VStack(alignment: .leading, spacing: HideTheme.spacingSM) {
                                Text(OverviewPresentation.noMatch).foregroundStyle(HideTheme.secondary)
                                Button("Clear search") { query = "" }
                            }
                            .hideFont(size: HideTheme.Typography.subhead)
                            .padding(HideTheme.spacingLG)
                            .accessibilityIdentifier("overview-no-match")
                        } else {
                            Text("No workspace").foregroundStyle(HideTheme.secondary)
                                .hideFont(size: HideTheme.Typography.subhead)
                                .padding(HideTheme.spacingLG)
                        }
                    }
                    ForEach(groups) { group in
                        groupHeader(group, workspace: workspace)
                        if !group.inactive, model.isCheckoutExpanded(group.checkout) {
                            if group.entries.isEmpty, !searching {
                                emptyGroupRow(group.checkout)
                            }
                            ForEach(group.entries) { entry in
                                agentRow(entry).id(entry.id)
                                if let origin = entry.origin { originCaption(origin, depth: entry.depth) }
                            }
                        }
                    }
                    if !searching, !inactive.isEmpty {
                        inactiveFold(workspace, count: inactive.count)
                        ForEach(unfoldedInactive) { checkout in
                            groupHeader(WorktreeGroup(checkout: checkout, entries: [], inactive: true), workspace: workspace)
                        }
                    }
                }.scrollTargetLayout()
            }
            .onChange(of: searchSelection.selectedID) { _, id in
                if let id { proxy.scrollTo(id, anchor: .center) }
            }
        }
    }

    /// The group header: the branch on the left, opening the workspace, and
    /// the Git facts on the right. The chevron folds the group's rows and
    /// shares the sidebar's collapsed set; an inactive checkout is one line.
    private func groupHeader(_ group: WorktreeGroup, workspace: CoreWorkspaceSnapshot) -> some View {
        let checkout = group.checkout
        let chips = OverviewPresentation.headerChips(
            checkout: checkout, isGit: workspace.isGit,
            folderDisk: model.focusedCheckout?.id == checkout.id ? model.card.disk : nil
        )
        let expanded = !group.inactive && model.isCheckoutExpanded(checkout)
        let focused = model.focusedCheckout?.id == checkout.id
        return HStack(spacing: HideTheme.spacingXS) {
            if group.inactive {
                Image(systemName: "chevron.right")
                    .hideFont(size: HideTheme.Typography.micro, weight: .bold)
                    .foregroundStyle(HideTheme.muted)
                    .frame(width: HideTheme.lineageChevronWidth)
                    .accessibilityHidden(true)
            } else {
                HideIconButton(
                    systemImage: expanded ? "chevron.down" : "chevron.right",
                    help: expanded ? "Fold \(checkout.label)" : "Unfold \(checkout.label)",
                    variant: .toolbar
                ) {
                    model.toggleCheckoutExpansion(checkout)
                }
                .frame(width: HideTheme.lineageChevronWidth)
            }
            Button { model.selectCheckout(checkout) } label: {
                HStack(spacing: HideTheme.spacingXS) {
                    if workspace.isGit {
                        Image(systemName: "arrow.triangle.branch")
                            .hideFont(size: HideTheme.Typography.caption, weight: .semibold)
                            .foregroundStyle(HideTheme.secondary)
                            .frame(width: HideTheme.checkoutIconWidth)
                    }
                    Text(checkout.label)
                        .hideFont(size: HideTheme.Typography.body, weight: .semibold)
                        .foregroundStyle(focused ? HideTheme.primary : HideTheme.secondary)
                        .lineLimit(1)
                        .truncationMode(.head)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(HideInteractiveButtonStyle())
            .hideTooltip(checkout.path)
            .accessibilityLabel(OverviewPresentation.headerAccessibilityLabel(checkout: checkout, chips: chips))
            .accessibilityHint("Open this workspace")
            .accessibilityIdentifier("overview-group-\(checkout.id)")
            Spacer(minLength: HideTheme.spacingSM)
            HStack(spacing: HideTheme.spacingXS) {
                ForEach(Array(chips.enumerated()), id: \.offset) { index, chip in
                    if index > 0 {
                        Text("·").hideFont(size: HideTheme.Typography.micro, design: .monospaced)
                            .foregroundStyle(HideTheme.muted).accessibilityHidden(true)
                    }
                    headerChip(chip, checkout: checkout)
                }
            }
            .fixedSize()
        }
        .padding(.horizontal, HideTheme.spacingMD)
        .padding(.top, HideTheme.spacingMD).padding(.bottom, HideTheme.spacingSM)
        .contextMenu { headerMenu(checkout, workspace: workspace) }
    }

    @ViewBuilder
    private func headerChip(_ chip: OverviewPresentation.HeaderChip, checkout: CoreCheckoutSnapshot) -> some View {
        let color: Color = switch chip.tone {
        case .normal: HideTheme.muted
        case .muted: HideTheme.muted
        case .warning: HideTheme.warning
        }
        switch chip.kind {
        case .files where chip.tone == .normal:
            Button { model.openHistory(for: checkout) } label: {
                Text(chip.text).hideFont(size: HideTheme.Typography.micro, design: .monospaced)
                    .foregroundStyle(color)
                    .padding(.horizontal, HideTheme.spacingXXS)
                    .contentShape(Rectangle())
            }
            .buttonStyle(HideInteractiveButtonStyle())
            .hideTooltip(OverviewPresentation.openInHistory)
            .accessibilityLabel(chip.tooltip)
            .accessibilityIdentifier("overview-files-\(checkout.id)")
        case .pullRequest:
            if let pullRequest = checkout.pullRequest {
                Button { model.openPullRequest(pullRequest) } label: {
                    HStack(spacing: HideTheme.spacingXXS) {
                        CheckoutCardPresentation.pullRequestIcon(pullRequest)
                            .resizable().frame(width: HideTheme.Typography.caption, height: HideTheme.Typography.caption)
                        Text(chip.text).hideFont(size: HideTheme.Typography.micro, weight: .semibold, design: .monospaced)
                    }
                    .foregroundStyle(CheckoutCardPresentation.pullRequestColor(pullRequest))
                    .padding(.horizontal, HideTheme.spacingXXS)
                    .contentShape(Rectangle())
                }
                .buttonStyle(HideInteractiveButtonStyle())
                .hideTooltip(chip.tooltip)
                .accessibilityLabel("pull request \(pullRequest.number) \(CheckoutCardPresentation.pullRequestState(pullRequest).lowercased())")
                .accessibilityIdentifier("overview-pull-request-\(checkout.id)")
            }
        default:
            Text(chip.text).hideFont(size: HideTheme.Typography.micro, design: .monospaced)
                .foregroundStyle(color)
                .hideTooltip(chip.tooltip)
                .accessibilityLabel(chip.tooltip)
        }
    }

    @ViewBuilder
    private func headerMenu(_ checkout: CoreCheckoutSnapshot, workspace: CoreWorkspaceSnapshot) -> some View {
        providerMenu(OverviewPresentation.newAgentHere, systemImage: "plus.circle", checkout: checkout)
        Button(WorktreeMenuPolicy.newWorktree, systemImage: "plus") { model.requestNewWorktree(workspace) }
        if let branch = checkout.branch {
            Button(WorktreeMenuPolicy.setBaseBranch, systemImage: "arrow.triangle.branch") { model.setBaseBranch(checkout, in: workspace) }
                .disabled(branch == model.baseBranch(for: workspace))
        }
        Divider()
        if let pullRequest = checkout.pullRequest {
            Button(OverviewPresentation.openPullRequestTitle(pullRequest), systemImage: "arrow.up.right.square") {
                model.openPullRequest(pullRequest)
            }
        }
        Button(OverviewPresentation.openInHistory, systemImage: "clock.arrow.circlepath") { model.openHistory(for: checkout) }
        Divider()
        Button(WorktreeMenuPolicy.copyPath, systemImage: "doc.on.doc") { model.copyCheckoutPath(checkout) }
        Menu(WorktreeMenuPolicy.openIn, systemImage: "arrow.up.forward.app") {
            Button("Finder") { model.revealCheckout(checkout) }
            Button("Default editor") { model.openCheckoutInDefaultEditor(checkout) }
        }
        if checkout.isWorktree {
            Divider()
            Button(checkout.worktree?.deletionGate.buttonLabel ?? "Delete worktree…", role: .destructive) {
                model.requestDeleteWorktree(checkout)
            }
            .disabled(checkout.worktree?.deletionGate.blockedReason != nil || checkout.worktree == nil)
            if let reason = checkout.worktree?.deletionGate.blockedReason { Text(reason) }
        }
    }

    /// `New agent here ▸ Terminal only / Claude / Codex`, the New worktree
    /// sheet's `Start with` choices as a submenu (PRD D-11).
    private func providerMenu(_ title: String, systemImage: String, checkout: CoreCheckoutSnapshot) -> some View {
        Menu(title, systemImage: systemImage) {
            ForEach(OverviewPresentation.providerChoices, id: \.provider) { choice in
                Button(choice.title) { model.startAgent(in: checkout, provider: choice.provider) }
            }
        }
    }

    /// A group with nobody in it says so once and offers the way to change
    /// that (design rule 9, PRD B18).
    private func emptyGroupRow(_ checkout: CoreCheckoutSnapshot) -> some View {
        HStack(spacing: HideTheme.spacingXS) {
            Color.clear.frame(width: HideTheme.lineageChevronWidth + HideTheme.agentMarkWidth)
            Text("No agent").foregroundStyle(HideTheme.muted)
            Text("·").foregroundStyle(HideTheme.muted)
            Menu {
                ForEach(OverviewPresentation.providerChoices, id: \.provider) { choice in
                    Button(choice.title) { model.startAgent(in: checkout, provider: choice.provider) }
                }
            } label: {
                Text(OverviewPresentation.startAgent).foregroundStyle(HideTheme.secondary)
            }
            .menuStyle(.borderlessButton).menuIndicator(.hidden).fixedSize()
            .accessibilityLabel("Start agent in \(checkout.label)")
            .accessibilityIdentifier("overview-start-agent-\(checkout.id)")
            Spacer(minLength: HideTheme.spacingXS)
        }
        .hideFont(size: HideTheme.Typography.caption)
        .padding(.horizontal, HideTheme.spacingMD)
        .frame(minHeight: HideTheme.Layout.paneHeaderHeight)
        .accessibilityIdentifier("overview-empty-\(checkout.id)")
    }

    /// One agent row: mark, badge, name, task, and the trailing `↗` that
    /// says what a click does. The whole row is the button (PRD B9).
    private func agentRow(_ entry: OverviewPresentation.AgentEntry) -> some View {
        let agent = entry.agent
        let state = AgentStatusPresentation(agent: agent, connected: model.agentsConnected)
        let shown = model.focusedPaneID == agent.paneID
        let task = OverviewPresentation.rowTask(agent)
        return Button { model.selectAgent(paneID: agent.paneID) } label: {
            HStack(spacing: HideTheme.spacingXS) {
                Group {
                    if entry.depth > 0 {
                        Text("↳").hideFont(size: HideTheme.Typography.micro).foregroundStyle(HideTheme.muted)
                    } else {
                        Color.clear
                    }
                }
                .frame(width: HideTheme.lineageChevronWidth)
                AgentStatusMark(symbol: state.symbol, color: state.color)
                AgentBadge(agentKind: agent.agentKind, stateColor: state.color, size: HideTheme.compactAgentBadgeSize)
                Text(agent.identityLabel)
                    .hideFont(size: HideTheme.Typography.caption, weight: .medium)
                    .foregroundStyle(agent.delegated ? HideTheme.secondary : HideTheme.primary)
                    .lineLimit(1)
                    .layoutPriority(1)
                if let task {
                    Text(task)
                        .hideFont(size: HideTheme.Typography.caption)
                        .foregroundStyle(HideTheme.secondary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                }
                Spacer(minLength: HideTheme.spacingXS)
                Image(systemName: "arrow.up.right")
                    .hideFont(size: HideTheme.Typography.caption, weight: .semibold)
                    .foregroundStyle(HideTheme.muted)
                    .frame(width: HideTheme.lineageChevronWidth)
            }
            .padding(.leading, HideTheme.spacingSM + HideTheme.lineageInset(depth: entry.depth))
            .padding(.trailing, HideTheme.spacingSM)
            .frame(minHeight: HideTheme.Layout.paneHeaderHeight)
            .contentShape(Rectangle())
        }
        .buttonStyle(HideInteractiveButtonStyle())
        .padding(.horizontal, HideTheme.spacingXS)
        .background(shown ? HideTheme.elevated : Color.clear, in: RoundedRectangle(cornerRadius: HideTheme.radiusExtraSmall))
        .hideTooltip([agent.identityLabel, state.label, task].compactMap { $0 }.joined(separator: " · "))
        .accessibilityLabel([agent.identityLabel, state.label, agent.checkoutLabel].compactMap { $0 }.joined(separator: ", "))
        .accessibilityValue(shown ? "Shown" : "")
        .accessibilityIdentifier("overview-agent-\(agent.paneID)")
        .contextMenu { agentMenu(agent) }
    }

    /// The caption under a child whose parent works in another worktree
    /// (PRD B10).
    private func originCaption(_ origin: String, depth: Int) -> some View {
        HStack(spacing: HideTheme.spacingXS) {
            Text("↳").foregroundStyle(HideTheme.muted)
            Text(origin).foregroundStyle(HideTheme.muted).lineLimit(1)
        }
        .hideFont(size: HideTheme.Typography.caption)
        .padding(.leading, HideTheme.spacingMD + HideTheme.lineageChevronWidth + HideTheme.lineageInset(depth: depth))
        .padding(.bottom, HideTheme.spacingXS)
        .accessibilityLabel(origin)
    }

    @ViewBuilder
    private func agentMenu(_ agent: SidebarAgent) -> some View {
        Button(OverviewPresentation.openPane, systemImage: "arrow.up.right.square") { model.selectAgent(paneID: agent.paneID) }
        Button(OverviewPresentation.revealInSidebar, systemImage: "sidebar.left") { model.revealAgentInSidebar(agent) }
        Divider()
        Button(OverviewPresentation.copyPaneID, systemImage: "doc.on.doc") { model.copyPaneID(agent.paneID) }
        Divider()
        Button(OverviewPresentation.closePane, systemImage: "xmark", role: .destructive) { model.closePaneFromHeader(agent.paneID) }
    }

    /// `› Inactive N`, sharing the sidebar's fold state (PRD B19, D-06).
    private func inactiveFold(_ workspace: CoreWorkspaceSnapshot, count: Int) -> some View {
        let expanded = workspace.inactiveCheckouts.expanded
        return Button { model.toggleInactiveCheckouts(in: workspace) } label: {
            HStack(spacing: HideTheme.spacingXS) {
                Image(systemName: expanded ? "chevron.down" : "chevron.right")
                    .hideFont(size: HideTheme.Typography.micro, weight: .bold)
                    .foregroundStyle(HideTheme.muted)
                    .frame(width: HideTheme.lineageChevronWidth)
                Text("Inactive \(count)")
                    .hideFont(size: HideTheme.Typography.body)
                    .foregroundStyle(HideTheme.muted)
                Spacer(minLength: HideTheme.spacingXS)
            }
            .padding(.horizontal, HideTheme.spacingMD)
            .padding(.vertical, HideTheme.spacingSM)
            .contentShape(Rectangle())
        }
        .buttonStyle(HideInteractiveButtonStyle())
        .accessibilityLabel("Inactive, \(count) \(count == 1 ? "workspace" : "workspaces"), \(expanded ? "expanded" : "collapsed")")
        .accessibilityValue(expanded ? "Expanded" : "Collapsed")
        .accessibilityIdentifier("overview-inactive-\(workspace.id)")
    }
}
