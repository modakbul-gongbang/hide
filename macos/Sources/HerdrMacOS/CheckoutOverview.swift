import SwiftUI

/// Project inspection stays separate from the terminal's active checkout.
struct CheckoutOverview: View {
    @EnvironmentObject private var model: ShellModel
    @State private var mode = "Tasks"
    @State private var query = ""
    @State private var searchSelection = HideSearchSelection()
    @State private var showDisk = false
    @State private var showGitHub = false
    @State private var showCleanup = false
    @State private var listScrollID: String?
    @State private var inspectedAgentPaneID: String?

    private var project: CoreProjectWorktrees? { model.core.snapshot?.gitWorktrees }
    private var workspace: CoreWorkspaceSnapshot? { model.focusedWorkspace }
    private var selected: CoreCheckoutSnapshot? {
        workspace?.checkouts.first { $0.path == model.card.inspectedCheckoutPath } ?? model.focusedCheckout
    }
    private var rows: [CoreCheckoutSnapshot] {
        let entries = workspace?.checkouts ?? []
        return entries.filter { row in
            query.isEmpty || ([row.label, row.path] + model.agents(in: row).map(\.identityLabel))
                .contains { $0.localizedCaseInsensitiveContains(query) }
        }.sorted { left, right in
            rank(left) == rank(right) ? left.path < right.path : rank(left) < rank(right)
        }
    }
    private var allTaskRows: [ProjectTaskRow] {
        ProjectTaskForestPresentation.rows(
            agents: model.agents,
            checkouts: workspace?.checkouts ?? []
        )
    }
    private var taskRows: [ProjectTaskRow] {
        ProjectTaskForestPresentation.rows(
            agents: model.agents,
            checkouts: workspace?.checkouts ?? [],
            query: query
        )
    }
    private var inspectedAgent: SidebarAgent? {
        let paneID = inspectedAgentPaneID ?? model.focusedPaneID
        return allTaskRows.first { $0.agent.paneID == paneID }?.agent
    }
    private var inspectedAgentCheckout: CoreCheckoutSnapshot? {
        guard let paneID = inspectedAgent?.paneID else { return nil }
        return workspace?.checkouts.first { checkout in
            checkout.tabs.flatMap(\.panes).contains { $0.id == paneID }
        }
    }
    private func rank(_ row: CoreCheckoutSnapshot) -> Int {
        let summary = row.agentSummary
        if summary.needsYou > 0 { return 0 }
        if summary.done > 0 { return 1 }
        if summary.working > 0 { return 2 }
        return summary.total > 0 ? 3 : 4
    }
    private func inspect(_ row: CoreCheckoutSnapshot) {
        model.core.dispatch(kind: "overview_select", payload: ["checkout_path": row.path])
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
                HStack(alignment: .top, spacing: HideTheme.spacingSM) {
                    VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
                        Text(workspace.label).hideFont(size: HideTheme.Typography.headline, weight: .semibold)
                            .lineLimit(1)
                        Text("Project · \(workspace.checkouts.count) workspaces")
                            .hideFont(size: HideTheme.Typography.subhead).foregroundStyle(HideTheme.secondary)
                    }
                    Spacer(minLength: HideTheme.spacingXS)
                    HideChoiceGroup(label: "Project view", values: ["Tasks", "Git"], selection: $mode,
                                    title: { $0 }).fixedSize()
                    .accessibilityIdentifier("overview-view-mode")
                }.padding(HideTheme.spacingMD)
                summary
                Divider()
                ScrollViewReader { proxy in
                    HStack(spacing: HideTheme.spacingSM) {
                        Button {
                            if let row = allTaskRows.first(where: { $0.agent.group == "needs_you" }) {
                                inspectedAgentPaneID = row.agent.paneID
                                mode = "Tasks"
                                query = ""
                                listScrollID = row.id
                            }
                        } label: {
                            Text("? Needs You · \(workspace.checkouts.reduce(0) { $0 + $1.agentSummary.needsYou })")
                                .foregroundStyle(workspace.checkouts.contains { $0.agentSummary.needsYou > 0 } ? HideTheme.warning : HideTheme.secondary)
                        }.buttonStyle(HideInteractiveButtonStyle())
                        Spacer(minLength: HideTheme.spacingXS)
                        Text(mode == "Git" ? "Git ancestry" : "Project task forest").foregroundStyle(HideTheme.muted)
                        HideIconButton(systemImage: "scope", help: "Locate shown pane", variant: .toolbar) {
                            mode = "Tasks"; query = ""
                            if let paneID = model.focusedPaneID {
                                inspectedAgentPaneID = paneID
                                listScrollID = paneID
                            }
                        }
                    }.hideFont(size: HideTheme.Typography.subhead).padding(.horizontal, HideTheme.spacingMD).padding(.vertical, HideTheme.spacingSM)
                    if mode == "Tasks" {
                        HideSearchField(placeholder: "Find task or workspace…", text: $query,
                                        selection: $searchSelection, resultIDs: taskRows.map(\.id), activate: {
                            if let row = searchSelection.entry(in: taskRows) {
                                inspectedAgentPaneID = row.agent.paneID
                            }
                        }, dismiss: { query = "" })
                            .padding(.horizontal, HideTheme.spacingMD)
                            .accessibilityIdentifier("overview-search")
                        if !model.agentsConnected {
                            Label("Live task status unavailable; showing the last known task forest", systemImage: "bolt.slash")
                                .hideFont(size: HideTheme.Typography.subhead)
                                .foregroundStyle(HideTheme.warning)
                                .padding(.horizontal, HideTheme.spacingMD)
                                .padding(.top, HideTheme.spacingXS)
                        }
                        ScrollView {
                            LazyVStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
                                if taskRows.isEmpty {
                                    Text(query.isEmpty ? "No live tasks in this project" : "No matching tasks")
                                        .foregroundStyle(HideTheme.secondary)
                                        .padding(HideTheme.spacingLG)
                                    if !query.isEmpty {
                                        Button("Clear search") { query = "" }.padding(.horizontal, HideTheme.spacingLG)
                                    }
                                }
                                ForEach(taskRows) { row in
                                    taskRow(row).id(row.id)
                                }
                            }.scrollTargetLayout()
                        }.scrollPosition(id: $listScrollID).onChange(of: searchSelection.selectedID) { _, id in
                            if let id { proxy.scrollTo(id, anchor: .center) }
                        }
                    } else if workspace.isGit {
                        OverviewGitTree(history: project?.history, checkouts: workspace.checkouts, selectedID: selected?.id) { row in
                            workspaceRow(row)
                        }
                    } else {
                        HideEmptyState("No Git history", systemImage: "folder", description: Text("This project is a folder. Use Tasks to inspect its work."))
                    }
                }
                Divider()
                if mode == "Tasks" {
                    taskInspector
                } else {
                    inspector
                }
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
            .onChange(of: workspace.id) { _, _ in
                query = ""
                inspectedAgentPaneID = nil
            }
        } else {
            HideEmptyState("No workspace", systemImage: "folder",
                           description: Text("Choose a workspace to see its context."))
        }
    }

    private var summary: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingMD) {
            Button { showGitHub.toggle() } label: {
                summaryRow("GitHub", value: githubLabel)
            }.buttonStyle(HideInteractiveButtonStyle()).popover(isPresented: $showGitHub) { githubDetails }
            Button { showDisk.toggle() } label: {
                summaryRow("Allocated on disk", value: OverviewPresentation.diskLabel(total: project?.diskTotalBytes,
                    confirmed: project?.diskConfirmedBytes, failure: project?.diskUnavailableReason, isGit: workspace?.isGit == true))
            }.buttonStyle(HideInteractiveButtonStyle()).popover(isPresented: $showDisk) { diskDetails }
            HStack {
                Button("Clean up merged worktrees…") {
                    showCleanup = true
                    model.core.dispatch(kind: "cleanup_review", payload: [:])
                }.buttonStyle(HideInteractiveButtonStyle()).disabled(project == nil)
                Spacer(minLength: HideTheme.spacingXS)
                HideIconButton(systemImage: HideTheme.GitIcon.refresh, help: "Refresh Overview", variant: .toolbar) {
                    model.refreshCheckoutCard()
                }
            }.hideFont(size: HideTheme.Typography.subhead)
        }.padding(.horizontal, HideTheme.spacingMD).padding(.bottom, HideTheme.spacingSM)
    }

    private func summaryRow(_ label: String, value: String) -> some View {
        HStack(spacing: HideTheme.spacingSM) {
            Text(label).foregroundStyle(HideTheme.secondary)
            Spacer(minLength: HideTheme.spacingXS)
            Text(value).fontWeight(.medium).multilineTextAlignment(.trailing).fixedSize(horizontal: false, vertical: true)
            Image(systemName: "chevron.right").foregroundStyle(HideTheme.muted)
        }.hideFont(size: HideTheme.Typography.title).contentShape(Rectangle())
    }

    private var githubLabel: String {
        OverviewPresentation.githubLabel(status: model.card.github,
                                         requests: project?.pullRequests, isGit: workspace?.isGit == true)
    }

    private var githubDetails: some View {
        ScrollView { VStack(alignment: .leading, spacing: HideTheme.spacingMD) {
            Text("GitHub").hideFont(size: HideTheme.Typography.title, weight: .semibold)
            if model.card.github.loading {
                ProgressView("Updating GitHub status…").controlSize(.small)
            }
            if let reason = model.card.github.unavailableReason {
                Text(reason).foregroundStyle(HideTheme.warning).fixedSize(horizontal: false, vertical: true)
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
            Button("Refresh") { if let workspace { model.requestGithubStatus(workspace, refresh: true) } }
        }.hideFont(size: HideTheme.Typography.title).padding(HideTheme.spacingLG)
            .frame(width: HideTheme.Layout.pullRequestPopoverWidth) }
            .frame(maxHeight: HideTheme.searchSheetSize.height)
            .foregroundStyle(HideTheme.primary).background(HideTheme.panel)
            .hideOverlayHost().preferredColorScheme(.dark)
    }

    private var diskDetails: some View {
        ScrollView { VStack(alignment: .leading, spacing: HideTheme.spacingMD) {
            Text("Allocated on disk").hideFont(size: HideTheme.Typography.title, weight: .semibold)
            if model.card.diskMeasuring {
                ProgressView("Measuring allocated disk…").controlSize(.small)
            }
            Text("Shared Git data is counted once. APFS clones may share storage; this is not guaranteed reclaimable space.")
                .foregroundStyle(HideTheme.secondary).fixedSize(horizontal: false, vertical: true)
            if project?.diskTotalBytes == nil, let known = project?.diskConfirmedBytes {
                Text("Confirmed subtotal: \(CheckoutCardPresentation.formattedBytes(Double(known))). Missing measurements below are excluded.")
                    .foregroundStyle(HideTheme.warning).fixedSize(horizontal: false, vertical: true)
            }
            ForEach(project?.worktrees ?? []) { row in
                VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
                    summaryRow(row.isMain ? "Main checkout" : row.label,
                               value: row.disk.totalBytes.map(CheckoutCardPresentation.formattedBytes) ?? "Unavailable")
                    if let reason = row.disk.unavailableReason { Text(reason).foregroundStyle(HideTheme.warning) }
                }
            }
            summaryRow("Shared Git data", value: project?.sharedGitDisk?.totalBytes.map(CheckoutCardPresentation.formattedBytes) ?? "Unavailable")
            if let reason = project?.sharedGitDisk?.unavailableReason { Text(reason).foregroundStyle(HideTheme.warning) }
            Button("Measure again") { model.refreshCheckoutCard() }
        }.hideFont(size: HideTheme.Typography.title).padding(HideTheme.spacingLG)
            .frame(width: HideTheme.Layout.pullRequestPopoverWidth) }
            .frame(maxHeight: HideTheme.searchSheetSize.height)
            .foregroundStyle(HideTheme.primary).background(HideTheme.panel)
            .hideOverlayHost().preferredColorScheme(.dark)
    }

    private func taskRow(_ row: ProjectTaskRow) -> some View {
        let state = AgentStatusPresentation(agent: row.agent, connected: model.agentsConnected)
        let selected = inspectedAgent?.paneID == row.agent.paneID
        let shown = model.focusedPaneID == row.agent.paneID
        return HStack(spacing: HideTheme.spacingXS) {
            Button {
                inspectedAgentPaneID = row.agent.paneID
            } label: {
                HStack(spacing: HideTheme.spacingXS) {
                    Image(systemName: row.hasChildren ? "chevron.down" : "circle.fill")
                        .hideFont(
                            size: row.hasChildren
                                ? HideTheme.Typography.micro
                                : HideTheme.spacingXS
                        )
                        .foregroundStyle(HideTheme.muted)
                        .frame(width: HideTheme.lineageChevronWidth)
                    AgentStatusMark(symbol: state.symbol, color: state.color)
                    AgentBadge(
                        agentKind: row.agent.agentKind,
                        stateColor: state.color,
                        size: HideTheme.compactAgentBadgeSize
                    )
                    Text(row.agent.identityLabel)
                        .hideFont(size: HideTheme.Typography.caption, weight: .medium)
                        .foregroundStyle(row.agent.delegated ? HideTheme.secondary : HideTheme.primary)
                        .lineLimit(1)
                        .truncationMode(.tail)
                    Spacer(minLength: HideTheme.spacingXS)
                    if row.depth == 0 || row.agent.lineageWorktreeBadge != nil {
                        Text(row.checkoutLabel)
                            .hideFont(size: HideTheme.Typography.micro)
                            .foregroundStyle(HideTheme.muted)
                            .lineLimit(1)
                    }
                }
                .padding(.leading, HideTheme.lineageInset(depth: row.depth))
                .padding(.vertical, HideTheme.spacingXS)
                .contentShape(Rectangle())
            }
            .buttonStyle(HideInteractiveButtonStyle())
            .accessibilityLabel("Inspect \(row.agent.identityLabel), \(state.label), \(row.checkoutLabel)")

            HideIconButton(
                systemImage: shown ? "rectangle.inset.filled" : "arrow.up.right.square",
                help: shown ? "This pane is shown" : "Open this agent pane",
                accessibilityLabel: shown ? "Agent pane is shown" : "Open \(row.agent.identityLabel)",
                variant: .toolbar
            ) {
                model.selectAgent(paneID: row.agent.paneID)
            }
            .disabled(shown)
        }
        .padding(.horizontal, HideTheme.spacingSM)
        .frame(minHeight: HideTheme.Layout.paneHeaderHeight)
        .background(
            selected ? HideTheme.elevated : Color.clear,
            in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
        )
        .overlay(alignment: .leading) {
            if row.depth > 0 {
                Rectangle()
                    .fill(HideTheme.divider)
                    .frame(width: HideTheme.Layout.hairlineWidth)
                    .padding(
                        .leading,
                        HideTheme.lineageInset(depth: row.depth)
                            + HideTheme.spacingSM - HideTheme.Layout.hairlineWidth
                    )
            }
        }
        .hideTooltip("\(row.agent.identityLabel) · \(row.checkoutLabel) · \(state.label)")
        .accessibilityIdentifier("overview-task-\(row.agent.paneID)")
    }

    private var taskInspector: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingMD) {
            HStack {
                Text("Inspected task").foregroundStyle(HideTheme.secondary)
                Spacer(minLength: HideTheme.spacingXS)
                if let focused = model.focusedPaneID,
                   focused != inspectedAgent?.paneID
                {
                    Button("Back to shown") {
                        inspectedAgentPaneID = focused
                        listScrollID = focused
                    }
                    .buttonStyle(HideTextButtonStyle(appearance: .quiet))
                } else {
                    Text("Shown").foregroundStyle(HideTheme.muted)
                }
            }
            .hideFont(size: HideTheme.Typography.subhead)

            if let agent = inspectedAgent {
                let state = AgentStatusPresentation(agent: agent, connected: model.agentsConnected)
                HStack(spacing: HideTheme.spacingSM) {
                    AgentBadge(
                        agentKind: agent.agentKind,
                        stateColor: state.color,
                        size: HideTheme.compactAgentBadgeSize
                    )
                    VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
                        Text(agent.identityLabel)
                            .hideFont(size: HideTheme.Typography.title, weight: .semibold)
                            .lineLimit(2)
                        Text("\(state.symbol) \(state.label) · \(inspectedAgentCheckout?.label ?? agent.workspaceLabel)")
                            .foregroundStyle(state.color)
                    }
                    Spacer(minLength: HideTheme.spacingXS)
                    Button(agent.paneID == model.focusedPaneID ? "Viewing" : "Open") {
                        model.selectAgent(paneID: agent.paneID)
                    }
                    .disabled(agent.paneID == model.focusedPaneID)
                    .accessibilityIdentifier("overview-open-agent")
                }
                if agent.lineageOrphan {
                    Label(
                        agent.lineageHint ?? "Parent unavailable; shown as operator-owned work",
                        systemImage: "questionmark.circle"
                    )
                    .foregroundStyle(HideTheme.warning)
                }
                if !model.agentsConnected {
                    Label("Live status unavailable while Herdr reconnects", systemImage: "bolt.slash")
                        .foregroundStyle(HideTheme.warning)
                }
                if let checkout = inspectedAgentCheckout {
                    VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
                        Text("Workspace details")
                            .foregroundStyle(HideTheme.primary)
                        Text(checkout.path)
                            .textSelection(.enabled)
                            .fixedSize(horizontal: false, vertical: true)
                        HStack {
                            Button("Reveal") { model.revealCheckout(checkout) }
                            Button("Copy path") { model.copyCheckoutPath(checkout) }
                            Button("View changes") {
                                model.core.dispatch(
                                    kind: "overview_changes",
                                    payload: ["checkout_path": checkout.path]
                                )
                            }
                            .disabled(checkout.id != model.focusedCheckout?.id)
                        }
                    }
                    .foregroundStyle(HideTheme.secondary)
                }
            } else {
                Text(model.agentsConnected
                    ? "Select a task to inspect it without moving terminal focus."
                    : "Task inspection is unavailable while Herdr reconnects.")
                    .foregroundStyle(HideTheme.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .hideFont(size: HideTheme.Typography.title)
        .padding(HideTheme.spacingMD)
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityIdentifier("overview-task-inspector")
    }

    private func workspaceRow(_ checkout: CoreCheckoutSnapshot) -> some View {
        Button { inspect(checkout) } label: {
            VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
                HStack {
                    Text(checkout.label).hideFont(size: HideTheme.Typography.title, weight: .medium)
                        .lineLimit(1).truncationMode(.middle)
                    Spacer(minLength: HideTheme.spacingXS)
                    if checkout.id == model.focusedCheckout?.id { Image(systemName: "arrow.up.right").foregroundStyle(HideTheme.secondary) }
                }
                if let agent = representative(checkout) {
                    HStack(spacing: HideTheme.spacingXS) {
                        Text(agent.identityLabel).lineLimit(1)
                        Spacer(minLength: HideTheme.spacingXS)
                        status(agent)
                    }
                } else { Text("No agent").foregroundStyle(HideTheme.muted) }
                Text(checkout.pullRequest.map { "\(changesLabel(checkout)) · PR #\($0.number)" } ?? changesLabel(checkout))
                    .foregroundStyle(HideTheme.secondary).lineLimit(1)
            }.hideFont(size: HideTheme.Typography.subhead)
                .padding(HideTheme.spacingSM).frame(maxWidth: .infinity, alignment: .leading)
                .background(selected?.id == checkout.id ? HideTheme.elevated : HideTheme.panel,
                            in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall))
        }.buttonStyle(HideInteractiveButtonStyle()).hideTooltip(checkout.label)
            .accessibilityLabel("Inspect \(checkout.label), \(changesLabel(checkout))")
            .accessibilityAddTraits(selected?.id == checkout.id ? .isSelected : [])
            .accessibilityIdentifier("overview-workspace-\(checkout.id)")
    }

    private func representative(_ checkout: CoreCheckoutSnapshot) -> SidebarAgent? {
        model.agents(in: checkout).first { $0.paneID == checkout.agentSummary.representativePaneID }
    }
    private func status(_ agent: SidebarAgent) -> some View {
        let state = AgentStatusPresentation(agent: agent, connected: model.agentsConnected)
        return Text("\(state.symbol) \(state.label)").foregroundStyle(state.color)
    }
    private func changesLabel(_ checkout: CoreCheckoutSnapshot) -> String {
        guard let row = checkout.worktree else { return "Git context unavailable" }
        if row.missing || row.unavailableReason != nil { return "Changes unavailable" }
        return row.changedFileCount == 0 ? "Clean" : "\(row.changedFileCount) changed \(row.changedFileCount == 1 ? "file" : "files")"
    }

    private var inspector: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingMD) {
            HStack {
                Text("Selected workspace").foregroundStyle(HideTheme.secondary)
                Spacer(minLength: HideTheme.spacingXS)
                Button(selected?.id == model.focusedCheckout?.id ? "Viewing" : "Back to viewing") {
                    if let checkout = model.focusedCheckout { inspect(checkout) }
                }.buttonStyle(HideTextButtonStyle(appearance: .quiet))
            }.hideFont(size: HideTheme.Typography.subhead)
            if let selected {
                VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
                    Text(selected.label).hideFont(size: HideTheme.Typography.title, weight: .semibold)
                        .lineLimit(2).truncationMode(.middle)
                    Text(URL(fileURLWithPath: selected.path).lastPathComponent)
                        .foregroundStyle(HideTheme.muted).lineLimit(1)
                }
                if let agent = representative(selected) {
                    HStack(alignment: .center, spacing: HideTheme.spacingSM) {
                        VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
                            Text(agent.identityLabel).lineLimit(2)
                            status(agent)
                        }
                        Spacer(minLength: HideTheme.spacingXS)
                        Button(selected.id == model.focusedCheckout?.id ? "Return to agent" : "Open agent") {
                            model.selectAgent(paneID: agent.paneID)
                        }.controlSize(.large).fixedSize()
                            .accessibilityIdentifier("overview-open-agent")
                    }
                } else {
                    Button("Open workspace") {
                        model.selectCheckout(selected)
                    }.disabled(selected.worktree == nil || !selected.exists)
                }
                HStack {
                    Text(changesLabel(selected)).foregroundStyle(HideTheme.secondary)
                    Spacer(minLength: HideTheme.spacingXS)
                    Button("View changes") {
                        model.core.dispatch(kind: "overview_changes", payload: ["checkout_path": selected.path])
                    }.buttonStyle(HideTextButtonStyle(appearance: .quiet)).disabled(selected.worktree == nil || selected.id != model.focusedCheckout?.id)
                        .hideTooltip("Open this workspace first to view its changes")
                }
                if let pr = selected.pullRequest {
                    HStack {
                        Text("PR #\(pr.number) · \(CheckoutCardPresentation.pullRequestState(pr))").foregroundStyle(HideTheme.secondary)
                        Spacer(minLength: HideTheme.spacingXS)
                        Button("Open PR") { model.openPullRequest(pr) }.buttonStyle(HideTextButtonStyle(appearance: .quiet))
                    }
                }
                DisclosureGroup("Latest commit & details") {
                    VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
                        if let row = selected.worktree {
                            Text(row.lastCommitSubject ?? "No commit available")
                            if let sha = row.headSHA { Text(String(sha.prefix(12))).textSelection(.enabled) }
                            Text(row.baseBranch.map { "Base \($0) · ↑\(row.ahead) ↓\(row.behind)" } ?? "Base unavailable")
                        }
                        Text(selected.path).textSelection(.enabled).fixedSize(horizontal: false, vertical: true)
                        HStack {
                            Button("Reveal") { model.revealCheckout(selected) }
                            Button("Copy path") { model.copyCheckoutPath(selected) }
                        }
                    }.foregroundStyle(HideTheme.secondary)
                }.disclosureGroupStyle(HideDisclosureStyle())
            }
        }.hideFont(size: HideTheme.Typography.title).padding(HideTheme.spacingMD)
            .frame(maxWidth: .infinity, alignment: .leading)
            .accessibilityIdentifier("overview-inspector")
    }
}
