import SwiftUI

/// Project inspection stays separate from the terminal's active checkout.
struct CheckoutOverview: View {
    @EnvironmentObject private var model: ShellModel
    @State private var mode = "Tree"
    @State private var query = ""
    @State private var searchSelection = HideSearchSelection()
    @State private var showDisk = false
    @State private var showGitHub = false
    @State private var showCleanup = false
    @State private var listScrollID: String?

    private var project: CoreProjectWorktrees? { model.core.snapshot?.gitWorktrees }
    private var workspace: CoreWorkspaceSnapshot? { model.focusedWorkspace }
    private var selected: CoreCheckoutSnapshot? {
        workspace?.checkouts.first { $0.path == model.card.inspectedCheckoutPath } ?? model.focusedCheckout
    }
    private var rows: [CoreCheckoutSnapshot] {
        let entries = workspace?.checkouts ?? []
        return entries.filter { row in
            query.isEmpty || ([row.label, row.path] + model.agents(in: row).map(\.summary))
                .contains { $0.localizedCaseInsensitiveContains(query) }
        }.sorted { left, right in
            rank(left) == rank(right) ? left.path < right.path : rank(left) < rank(right)
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
            ContentUnavailableView("Overview is local only", systemImage: "externaldrive",
                                   description: Text("Git context is read on this Mac."))
        } else if let workspace {
            VStack(alignment: .leading, spacing: HideTheme.spacingNone) {
                HStack(alignment: .top, spacing: HideTheme.spacingSM) {
                    VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
                        Text(workspace.label).hideFont(size: HideTheme.Typography.title, weight: .semibold)
                            .lineLimit(1)
                        Text("Project · \(workspace.checkouts.count) workspaces")
                            .hideFont(size: HideTheme.Typography.caption).foregroundStyle(HideTheme.secondary)
                    }
                    Spacer(minLength: HideTheme.spacingXS)
                    Picker("Project view", selection: $mode) {
                        Text("Tree").tag("Tree")
                        Text("List").tag("List")
                    }.pickerStyle(.segmented).labelsHidden().fixedSize()
                    .accessibilityIdentifier("overview-view-mode")
                }.padding(HideTheme.spacingMD)
                summary
                Divider()
                ScrollViewReader { proxy in
                    HStack(spacing: HideTheme.spacingSM) {
                        Button {
                            if let row = workspace.checkouts.first(where: { $0.agentSummary.needsYou > 0 }) {
                                inspect(row); mode = "List"; query = ""; listScrollID = row.id
                            }
                        } label: {
                            Text("Needs You · \(workspace.checkouts.reduce(0) { $0 + $1.agentSummary.needsYou })")
                        }.buttonStyle(.plain)
                        Spacer(minLength: HideTheme.spacingXS)
                        Text(mode == "Tree" ? "Git history" : "Attention first").foregroundStyle(HideTheme.muted)
                        HideIconButton(systemImage: "scope", help: "Locate selected workspace", variant: .toolbar) {
                            mode = "List"; query = ""
                            if let selected { listScrollID = selected.id }
                        }
                    }.hideFont(size: HideTheme.Typography.caption).padding(.horizontal, HideTheme.spacingMD)
                    if mode == "List" {
                        TextField("Find workspace or agent…", text: $query)
                            .textFieldStyle(.plain).hideFont(size: HideTheme.Typography.body)
                            .padding(HideTheme.spacingSM).background(HideTheme.elevated)
                            .padding(.horizontal, HideTheme.spacingMD)
                            .hideSearchKeyboard(selection: $searchSelection, resultIDs: rows.map(\.id), activate: {
                                if let row = searchSelection.entry(in: rows) { inspect(row) }
                            }, dismiss: { query = "" })
                            .accessibilityIdentifier("overview-search")
                        ScrollView {
                            LazyVStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
                                if rows.isEmpty {
                                    Text("No matching workspaces").foregroundStyle(HideTheme.secondary)
                                        .padding(HideTheme.spacingLG)
                                    Button("Clear search") { query = "" }.padding(.horizontal, HideTheme.spacingLG)
                                }
                                ForEach(Array(rows.enumerated()), id: \.element.id) { index, row in
                                    if index == 0 || rank(rows[index - 1]) != rank(row) {
                                        Text(["Needs You", "Done", "Working", "Seen", "No agent"][rank(row)])
                                            .hideFont(size: HideTheme.Typography.caption, weight: .semibold)
                                            .foregroundStyle(HideTheme.secondary).padding(HideTheme.spacingSM)
                                    }
                                    workspaceRow(row).id(row.id)
                                }
                            }.scrollTargetLayout()
                        }.scrollPosition(id: $listScrollID).onChange(of: searchSelection.selectedID) { _, id in
                            if let id { proxy.scrollTo(id, anchor: .center) }
                        }
                    } else if workspace.isGit {
                        OverviewGitTree(history: project?.history, checkouts: workspace.checkouts) { row in
                            workspaceRow(row)
                        }
                    } else {
                        ContentUnavailableView("No Git history", systemImage: "folder", description: Text("This project is a folder. Use List to inspect its workspaces."))
                    }
                }
                Divider()
                inspector
            }
            .foregroundStyle(HideTheme.primary)
            .accessibilityIdentifier("checkout-overview")
            .sheet(isPresented: $showCleanup) {
                MergedWorktreeCleanup(review: project?.cleanup) {
                    model.core.dispatch(kind: "cleanup_dismiss", payload: [:]); showCleanup = false
                }
                .environmentObject(model)
            }
            .onChange(of: workspace.id) { _, _ in query = "" }
        } else {
            ContentUnavailableView("No workspace", systemImage: "folder",
                                   description: Text("Choose a workspace to see its context."))
        }
    }

    private var summary: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingSM) {
            Button { showGitHub.toggle() } label: {
                summaryRow("GitHub", value: githubLabel)
            }.buttonStyle(.plain).popover(isPresented: $showGitHub) { githubDetails }
            Button { showDisk.toggle() } label: {
                summaryRow("Allocated on disk", value: OverviewPresentation.diskLabel(total: project?.diskTotalBytes,
                    confirmed: project?.diskConfirmedBytes, failure: project?.diskUnavailableReason, isGit: workspace?.isGit == true))
            }.buttonStyle(.plain).popover(isPresented: $showDisk) { diskDetails }
            HStack {
                Button("Clean up merged worktrees…") {
                    showCleanup = true
                    model.core.dispatch(kind: "cleanup_review", payload: [:])
                }.buttonStyle(.plain).disabled(project == nil)
                Spacer(minLength: HideTheme.spacingXS)
                HideIconButton(systemImage: HideTheme.GitIcon.refresh, help: "Refresh Overview", variant: .toolbar) {
                    model.refreshCheckoutCard()
                }
            }.hideFont(size: HideTheme.Typography.caption)
        }.padding(.horizontal, HideTheme.spacingMD).padding(.bottom, HideTheme.spacingSM)
    }

    private func summaryRow(_ label: String, value: String) -> some View {
        HStack(spacing: HideTheme.spacingSM) {
            Text(label).foregroundStyle(HideTheme.secondary)
            Spacer(minLength: HideTheme.spacingXS)
            Text(value).lineLimit(1)
            Image(systemName: "chevron.right").foregroundStyle(HideTheme.muted)
        }.hideFont(size: HideTheme.Typography.body).contentShape(Rectangle())
    }

    private var githubLabel: String {
        OverviewPresentation.githubLabel(status: model.card.github,
                                         requests: project?.pullRequests, isGit: workspace?.isGit == true)
    }

    private var githubDetails: some View {
        ScrollView { VStack(alignment: .leading, spacing: HideTheme.spacingMD) {
            Text("GitHub").hideFont(size: HideTheme.Typography.title, weight: .semibold)
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
                }.buttonStyle(.plain)
            }
            Button("Refresh") { if let workspace { model.requestGithubStatus(workspace, refresh: true) } }
        }.hideFont(size: HideTheme.Typography.body).padding(HideTheme.spacingLG)
            .frame(width: HideTheme.Layout.pullRequestPopoverWidth) }.frame(maxHeight: HideTheme.searchSheetSize.height).hideOverlayHost()
    }

    private var diskDetails: some View {
        ScrollView { VStack(alignment: .leading, spacing: HideTheme.spacingMD) {
            Text("Allocated on disk").hideFont(size: HideTheme.Typography.title, weight: .semibold)
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
        }.hideFont(size: HideTheme.Typography.body).padding(HideTheme.spacingLG)
            .frame(width: HideTheme.Layout.pullRequestPopoverWidth) }.frame(maxHeight: HideTheme.searchSheetSize.height).hideOverlayHost()
    }

    private func workspaceRow(_ checkout: CoreCheckoutSnapshot) -> some View {
        Button { inspect(checkout) } label: {
            VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
                HStack {
                    Text(checkout.label).hideFont(size: HideTheme.Typography.body, weight: .medium)
                        .lineLimit(1).truncationMode(.middle)
                    Spacer(minLength: HideTheme.spacingXS)
                    if checkout.id == model.focusedCheckout?.id { Image(systemName: "arrow.up.right") }
                }
                if let agent = representative(checkout) {
                    HStack(spacing: HideTheme.spacingXS) {
                        Text(agent.summary).lineLimit(1)
                        Spacer(minLength: HideTheme.spacingXS)
                        status(agent)
                    }
                } else { Text("No agent").foregroundStyle(HideTheme.muted) }
                Text(changesLabel(checkout)).foregroundStyle(HideTheme.secondary)
            }.hideFont(size: HideTheme.Typography.caption)
                .padding(HideTheme.spacingSM).frame(maxWidth: .infinity, alignment: .leading)
                .background(selected?.id == checkout.id || (mode == "List" && searchSelection.selectedID == checkout.id) ? HideTheme.elevated : HideTheme.panel,
                            in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall))
        }.buttonStyle(.plain).hideTooltip(checkout.label)
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
        return row.changedFileCount == 0 ? "Clean" : "\(row.changedFileCount) changed files"
    }

    private var inspector: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingSM) {
            HStack {
                Text("Selected workspace").foregroundStyle(HideTheme.secondary)
                Spacer(minLength: HideTheme.spacingXS)
                Button(selected?.id == model.focusedCheckout?.id ? "Viewing" : "Back to viewing") {
                    if let checkout = model.focusedCheckout { inspect(checkout) }
                }.buttonStyle(.plain)
            }.hideFont(size: HideTheme.Typography.caption)
            if let selected {
                Text(selected.label).hideFont(size: HideTheme.Typography.subhead, weight: .semibold)
                    .lineLimit(2).truncationMode(.middle)
                Text(URL(fileURLWithPath: selected.path).lastPathComponent)
                    .foregroundStyle(HideTheme.muted).lineLimit(1)
                if let agent = representative(selected) {
                    HStack {
                        Text(agent.summary).lineLimit(1)
                        Spacer(minLength: HideTheme.spacingXS)
                        status(agent)
                    }
                    Button(selected.id == model.focusedCheckout?.id ? "Return to agent" : "Open agent") {
                        model.selectAgent(paneID: agent.paneID)
                    }.accessibilityIdentifier("overview-open-agent")
                } else {
                    HStack {
                        Button("Open workspace") {
                            model.selectCheckout(selected)
                        }.disabled(selected.worktree == nil || !selected.exists)
                        Button("Start agent…") { model.openComposer(checkoutID: selected.id) }
                    }
                }
                HStack {
                    Text(changesLabel(selected)).foregroundStyle(HideTheme.secondary)
                    Spacer(minLength: HideTheme.spacingXS)
                    Button("View changes") {
                        model.core.dispatch(kind: "overview_changes", payload: ["checkout_path": selected.path])
                    }.buttonStyle(.plain).disabled(selected.worktree == nil || selected.id != model.focusedCheckout?.id)
                        .hideTooltip("Open this workspace first to view its changes")
                }
                if let pr = selected.pullRequest {
                    HStack {
                        Text("PR #\(pr.number)").foregroundStyle(HideTheme.secondary)
                        Spacer(minLength: HideTheme.spacingXS)
                        Button("Open PR") { model.openPullRequest(pr) }.buttonStyle(.plain)
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
                }
            }
        }.hideFont(size: HideTheme.Typography.body).padding(HideTheme.spacingMD)
            .frame(maxWidth: .infinity, alignment: .leading)
            .accessibilityIdentifier("overview-inspector")
    }
}
