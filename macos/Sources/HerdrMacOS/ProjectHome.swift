import SwiftUI

struct ProjectHome: View {
    @EnvironmentObject private var model: ShellModel
    @State private var memo = ProjectHomeMemo()
    @State private var mergedExpanded = false
    @State private var seenExpanded = false

    private var workspace: CoreWorkspaceSnapshot? { model.focusedWorkspace }
    private var board: ProjectHomeBoard? {
        guard let workspace else { return nil }
        return memo.board(for: .init(revision: model.core.snapshot?.navigationRevision ?? 0,
                                     workspaceID: workspace.id, connected: model.agentsConnected)) {
            ProjectHomeBoard.build(workspace: workspace, agents: model.agents)
        }
    }
    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingNone) {
            HStack(spacing: HideTheme.spacingLG) {
                HideChoiceGroup(label: "Project Home view", values: ProjectHomeViewKind.allCases,
                                selection: $model.projectHomeView, title: { $0.rawValue },
                                appearance: .tabs,
                                identifier: { "project-home-view-\($0.rawValue.lowercased())" })
                Spacer()
                HideIconButton(systemImage: "arrow.clockwise", help: "Refresh Project Home") {
                    if let workspace { model.requestGithubStatus(workspace, refresh: true) }
                }
                Button("Start new terminal", action: model.addTab)
                    .buttonStyle(HideTextButtonStyle(appearance: .prominent))
                    .accessibilityIdentifier("project-home-start-terminal")
            }
            .padding(.horizontal, HideTheme.spacingLG)
            .padding(.vertical, HideTheme.spacingSM)
            Divider().overlay(HideTheme.divider)
            if workspace?.checkouts.isEmpty == true {
                Text("No checkouts in this project yet.")
                    .foregroundStyle(HideTheme.muted).padding(HideTheme.spacingLG)
            } else if let board {
                GeometryReader { geometry in
                ScrollView([.horizontal, .vertical]) {
                    VStack(alignment: .leading, spacing: HideTheme.spacingLG) {
                        if model.projectHomeView == .tasks {
                            if !board.adHoc.isEmpty {
                                Text("즉석").hideFont(size: HideTheme.Typography.body, weight: .semibold)
                                    .foregroundStyle(HideTheme.muted)
                                HStack(alignment: .top, spacing: HideTheme.spacingMD) {
                                    ForEach(board.adHoc) { card in
                                        ProjectHomeCardView(card: card).frame(width: HideTheme.Home.columnWidth)
                                    }
                                }
                                Divider().overlay(HideTheme.divider)
                            }
                            if board.isGit {
                                HStack(alignment: .top, spacing: HideTheme.spacingMD) {
                                    ForEach(ProjectHomeStage.allCases, id: \.self) { stage in
                                        column(stage.rawValue, cards: board.tasks.filter { $0.stage == stage },
                                               collapsed: stage == .merged && !mergedExpanded,
                                               overflow: stage == .ready && board.overflow,
                                               toggle: stage == .merged ? { mergedExpanded.toggle() } : nil)
                                    }
                                }
                            }
                        } else {
                            HStack(alignment: .top, spacing: HideTheme.spacingMD) {
                                column("진행 중", cards: board.agents.filter { ["working", "needs_you"].contains($0.root?.group ?? "") })
                                column("내 확인 대기", cards: board.agents.filter { $0.root?.group == "done" })
                                column("끝", cards: board.agents.filter { $0.root?.group == "seen" },
                                       collapsed: !seenExpanded, toggle: { seenExpanded.toggle() }, dimmed: true)
                            }
                        }
                    }
                    .padding(HideTheme.spacingLG)
                    .frame(minWidth: geometry.size.width, minHeight: geometry.size.height, alignment: .topLeading)
                }
                }
            } else {
                HideEmptyState("No project", systemImage: "square.stack.3d.up",
                    description: Text("Register a local folder, then choose a checkout from the sidebar."))
            }
            Spacer(minLength: HideTheme.spacingNone)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(HideTheme.background)
        .onAppear { if let workspace { model.requestGithubStatus(workspace, refresh: true) } }
        .accessibilityIdentifier("project-home")
    }

    private func column(_ title: String, cards: [ProjectHomeCard], collapsed: Bool = false,
                        overflow: Bool = false, toggle: (() -> Void)? = nil, dimmed: Bool = false) -> some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingMD) {
            HStack(spacing: HideTheme.spacingSM) {
                if let toggle {
                    Button(action: toggle) {
                        HStack(spacing: HideTheme.spacingSM) {
                            Image(systemName: collapsed ? "chevron.right" : "chevron.down")
                            Text(title)
                            Text("\(cards.count)\(overflow ? "+" : "")").foregroundStyle(HideTheme.muted)
                        }
                    }.buttonStyle(.plain)
                } else {
                    Text(title)
                    Text("\(cards.count)\(overflow ? "+" : "")").foregroundStyle(HideTheme.muted)
                }
                Spacer(minLength: HideTheme.spacingNone)
            }
            .hideFont(size: HideTheme.Typography.body, weight: .semibold)
            if !collapsed {
                ForEach(cards) { card in
                    ProjectHomeCardView(card: card)
                        .opacity(dimmed || card.stage == .merged ? HideTheme.Opacity.secondary : 1)
                }
            }
        }
        .padding(HideTheme.spacingMD)
        .frame(width: collapsed ? HideTheme.Home.collapsedWidth : HideTheme.Home.columnWidth, alignment: .topLeading)
        .background(HideTheme.sidebar, in: RoundedRectangle(cornerRadius: HideTheme.radiusMedium))
    }
}

private struct ProjectHomeCardView: View {
    @EnvironmentObject private var model: ShellModel
    let card: ProjectHomeCard
    @State private var linkPresented = false
    @State private var issueInput = ""
    @State private var inputError: String?
    @State private var submitted = false

    private var halo: Color { card.error ? HideTheme.danger : HideTheme.warning }
    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingSM) {
            if !card.agentsView, !card.backlog, let checkout = card.checkout {
                HStack(spacing: HideTheme.spacingXS) {
                    Text(checkout.branch ?? checkout.label)
                        .hideFont(size: HideTheme.Typography.body, design: .monospaced)
                        .lineLimit(1).truncationMode(.tail)
                        .hideTooltip(checkout.branch ?? checkout.label)
                    if !checkout.exists { HideBadge(label: "missing", color: HideTheme.warning) }
                    Spacer(minLength: HideTheme.spacingNone)
                    if card.requestCount >= 2 {
                        Text("\(card.requestCount) 요청").hideFont(size: HideTheme.Typography.micro)
                            .foregroundStyle(HideTheme.muted)
                    }
                }
            }
            if !card.agentsView, (card.stage != nil || card.backlog), let title = card.title, !title.isEmpty {
                Text(title).hideFont(size: HideTheme.Typography.title, weight: .medium)
                    .foregroundStyle(card.backlog ? HideTheme.muted : HideTheme.primary)
                    .lineLimit(2).fixedSize(horizontal: false, vertical: true).hideTooltip(title)
            }
            ForEach(card.rows) { row in
                agentRow(row)
            }
            if card.agentsView || card.stage != nil {
                footer
            }
        }
        .padding(HideTheme.spacingMD)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(card.backlog ? HideTheme.sidebar : HideTheme.elevated,
                    in: RoundedRectangle(cornerRadius: HideTheme.radiusMedium))
        .overlay {
            RoundedRectangle(cornerRadius: HideTheme.radiusMedium)
                .stroke(card.needsYou ? halo : HideTheme.divider, lineWidth: HideTheme.Layout.hairlineWidth)
        }
        .shadow(color: card.needsYou ? halo.opacity(HideTheme.Opacity.emphasisFill) : .clear,
                radius: HideTheme.Home.haloRadius)
        .contentShape(Rectangle())
        .onTapGesture(perform: select)
        .accessibilityAction { select() }
        .accessibilityIdentifier("project-home-card-\(card.id)")
        .contextMenu {
            if card.backlog { Button("이슈 열기", action: openIssue) }
            else {
                if card.needsYou, card.issue != nil { Button("이슈 열기", action: openIssue) }
                Button("pane 포커스", action: focusPane).disabled(card.root == nil)
                if !card.needsYou, card.issue != nil { Button("이슈 열기", action: openIssue) }
                if card.checkout?.isWorktree == true {
                    Button("이슈 연결…") { issueInput = ""; inputError = nil; linkPresented = true }
                    if card.issue != nil { Button("이슈 연결 해제") { saveIssue("") } }
                }
            }
        }
        .popover(isPresented: $linkPresented) {
            VStack(alignment: .leading, spacing: HideTheme.spacingSM) {
                HideSettingsField(placeholder: "owner/repo#번호 또는 이슈 URL", text: $issueInput, onSubmit: submitIssue)
                    .accessibilityIdentifier("project-home-issue-input")
                if let inputError { Text(inputError).foregroundStyle(HideTheme.danger).hideFont(size: HideTheme.Typography.caption) }
                Button(submitted ? "저장 중…" : "저장", action: submitIssue)
                    .buttonStyle(HideTextButtonStyle(appearance: .prominent))
                    .disabled(submitted || model.core.snapshot?.taskOperation?.phase == "working")
            }
            .padding(HideTheme.spacingMD).frame(width: HideTheme.Home.columnWidth)
        }
        .onChange(of: linkPresented) { _, presented in model.projectHomeIssuePopover = presented }
        .onDisappear { if linkPresented { model.projectHomeIssuePopover = false } }
        .onChange(of: model.projectHomeIssueResult?.id) { _, _ in
            guard submitted else { return }
            submitted = false
            if model.projectHomeIssueResult?.phase == "ready" { linkPresented = false }
        }
    }

    private func agentRow(_ row: ProjectHomeRow) -> some View {
        let agent = row.agent
        let status = AgentStatusPresentation(agent: agent, connected: model.agentsConnected)
        let leadingInset = CGFloat(row.depth) * HideTheme.Home.childIndent
        return Button { model.selectAgent(agent); model.closeProjectHome() } label: {
            HStack(alignment: .top, spacing: HideTheme.spacingXS) {
                AgentStatusMark(symbol: status.symbol, color: status.color)
                AgentBadge(agentKind: agent.agentKind, stateColor: status.color, size: HideTheme.compactAgentBadgeSize)
                VStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
                    HStack(spacing: HideTheme.spacingXS) {
                        Text(agent.identityLabel).lineLimit(1).truncationMode(.tail)
                        Spacer(minLength: HideTheme.spacingNone)
                        Text(agent.elapsed).foregroundStyle(HideTheme.muted)
                    }
                    if (agent.demand == "question" || agent.demand == "error" || agent.group == "done"), let detail = agent.detail {
                        Text(detail).foregroundStyle(HideTheme.secondary).lineLimit(2)
                    }
                    if let branch = row.foreignBranch {
                        Text("↳ \(branch)").foregroundStyle(HideTheme.muted).lineLimit(1).hideTooltip(branch)
                    }
                }
                .hideFont(size: HideTheme.Typography.body)
            }
            .padding(.leading, leadingInset)
            .overlay(alignment: .leading) {
                if row.depth > 0 { Rectangle().fill(HideTheme.divider).frame(width: HideTheme.Layout.hairlineWidth) }
            }
        }
        .buttonStyle(.plain)
        .foregroundStyle(HideTheme.primary)
        .opacity(model.agentsConnected ? 1 : HideTheme.Opacity.disabled)
        .hideTooltip([agent.identityLabel, status.label, agent.detail].compactMap { $0 }.joined(separator: " · "))
        .accessibilityIdentifier("project-home-agent-\(agent.paneID)")
    }

    private var footer: some View {
        HStack(spacing: HideTheme.spacingXS) {
                if card.agentsView, let checkout = card.checkout {
                    Text(checkout.branch ?? checkout.label).lineLimit(1).truncationMode(.tail)
                        .hideFont(size: HideTheme.Typography.micro, design: .monospaced)
                        .foregroundStyle(HideTheme.muted).hideTooltip(checkout.branch ?? checkout.label)
                }
                if let stage = card.stageLabel { HideBadge(label: stage, color: HideTheme.secondary) }
                if card.stage == .review, let checks = card.checkout?.pullRequest?.checks {
                    switch checks {
                    case .pending: HideBadge(label: "pending", color: HideTheme.warning)
                    case .failed: HideBadge(label: "failed", color: HideTheme.danger)
                    case .passing: HideBadge(label: "passing", color: HideTheme.success)
                    default: EmptyView()
                    }
                }
            if let issue = card.issue?.issue {
                    Button(action: openIssue) {
                        HideBadge(label: issue.state == "CLOSED" ? "#\(issue.reference.number) 닫힘" : "⊙ #\(issue.reference.number)",
                                  color: issue.state == "CLOSED" ? HideTheme.PullRequest.merged : HideTheme.secondary)
                    }.buttonStyle(.plain).hideTooltip(card.issueHelp)
                    if let mismatch = card.mismatch {
                        HideBadge(label: "≠ \(mismatch)", color: HideTheme.warning).hideTooltip(card.mismatchHelp)
                    }
            }
        }
    }
    private func select() {
        if card.backlog { openIssue() }
        else if card.agentsView { focusPane() }
        else if let checkout = card.checkout { model.selectCheckout(checkout); model.closeProjectHome() }
    }
    private func focusPane() { if let root = card.root { model.selectAgent(root); model.closeProjectHome() } }
    private func openIssue() {
        guard let issue = card.issue?.issue, let url = URL(string: issue.url) else { return }
        ExternalBrowser.open(url) { model.interactionNotice = $0 }
    }
    private func submitIssue() {
        let input = issueInput.trimmingCharacters(in: .whitespacesAndNewlines)
        let repository = model.focusedWorkspace?.homeIssues.repository
        switch ProjectHomeIssueInput.parse(input, repository: repository) {
        case .success(let token): inputError = nil; saveIssue(token)
        case .failure(let error): inputError = error.localizedDescription
        }
    }
    private func saveIssue(_ value: String) {
        guard let checkout = card.checkout else { return }
        submitted = model.saveProjectHomeIssue(checkoutID: checkout.id, value: value)
    }
}

struct ProjectHomeIssueInput {
    struct Invalid: LocalizedError { var errorDescription: String? { "owner/repo#번호, #번호 또는 GitHub 이슈 URL을 입력하세요." } }
    static func parse(_ input: String, repository: String?) -> Result<String, Invalid> {
        var token = input
        if input.hasPrefix("https://github.com/") {
            let parts = String(input.dropFirst("https://github.com/".count)).trimmingCharacters(in: CharacterSet(charactersIn: "/")).split(separator: "/", omittingEmptySubsequences: false)
            guard parts.count == 4, parts[2] == "issues" else { return .failure(Invalid()) }
            token = "\(parts[0])/\(parts[1])#\(parts[3])"
        } else if input.hasPrefix("#"), let repository { token = repository + input }
        let parts = token.split(separator: "#", omittingEmptySubsequences: false)
        guard parts.count == 2, let number = UInt32(parts[1]), number > 0, token.count <= 80 else { return .failure(Invalid()) }
        let repo = parts[0].split(separator: "/", omittingEmptySubsequences: false)
        let allowed = CharacterSet(charactersIn: "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_.")
        guard repo.count == 2, repo.allSatisfy({ !$0.isEmpty && $0 != "." && $0 != ".." && $0.unicodeScalars.allSatisfy(allowed.contains) }) else { return .failure(Invalid()) }
        return .success("\(parts[0])#\(number)")
    }
}
