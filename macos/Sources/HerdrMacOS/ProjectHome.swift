import SwiftUI

/// The board's cache: one built board per snapshot revision, so an unchanged
/// section never rebuilds the lanes. A class rather than view state because
/// filling it during `body` must not schedule another render (PRD rule 8).
final class ProjectHomeMemo {
    struct Key: Equatable {
        let revision: UInt64
        let workspaceID: String?
        let connected: Bool
    }

    private var key: Key?
    private var board: ProjectHomeBoard?

    func board(for key: Key, build: () -> ProjectHomeBoard) -> ProjectHomeBoard {
        if self.key == key, let board { return board }
        let built = build()
        self.key = key
        board = built
        return built
    }
}

/// Project Home: one lane per checkout, agents as cards, progress as a track.
///
/// It is drawn where a checkout has no tab, and over the pane canvas when the
/// operator raises it. Nothing here decides a label, an order or a state; the
/// board is built by `ProjectHomeBoard` and this composes it, then dispatches
/// the existing pane, checkout, tab and pull-request actions.
struct ProjectHome: View {
    @EnvironmentObject private var model: ShellModel
    let mode: ProjectHomeMode
    @State private var memo = ProjectHomeMemo()
    @State private var query = ""
    @State private var searchSelection = HideSearchSelection()
    @State private var selectedPaneID: String?
    @State private var hoveredPaneID: String?
    @State private var scrollTarget: String?

    private var workspace: CoreWorkspaceSnapshot? { model.focusedWorkspace }

    private var input: ProjectHomeInput? {
        guard let workspace else { return nil }
        let facts = Dictionary(
            (model.core.snapshot?.gitWorktrees?.worktrees ?? []).map {
                ($0.path, ProjectHomeWorktreeFact(merged: $0.merged, agentLine: $0.agentLine))
            },
            uniquingKeysWith: { first, _ in first }
        )
        return ProjectHomeInput(
            projectLabel: workspace.label,
            projectPath: workspace.path,
            isGit: workspace.isGit,
            checkouts: workspace.checkouts,
            agents: model.agents,
            connected: model.agentsConnected,
            worktreeFacts: facts
        )
    }

    private var board: ProjectHomeBoard? {
        guard let input else { return nil }
        let key = ProjectHomeMemo.Key(
            revision: model.core.snapshot?.navigationRevision ?? 0,
            workspaceID: workspace?.id,
            connected: input.connected
        )
        return memo.board(for: key) { ProjectHomeBoard.build(input) }.filtered(query: query)
    }

    var body: some View {
        Group {
            if model.core.snapshot == nil {
                HideEmptyState("Loading Project Home", systemImage: "clock",
                               description: Text("Waiting for the first runtime snapshot."))
            } else if let board {
                page(board)
            } else {
                HideEmptyState("No project", systemImage: "square.stack.3d.up",
                               description: Text("Register a local folder, then choose a checkout from the sidebar."))
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(HideTheme.background)
        .onChange(of: workspace?.id) { _, _ in
            query = ""
            selectedPaneID = nil
            hoveredPaneID = nil
        }
        .accessibilityIdentifier("project-home")
    }

    private func page(_ board: ProjectHomeBoard) -> some View {
        GeometryReader { proxy in
            let folded = proxy.size.width < HideTheme.Home.inspectorFoldWidth
            VStack(spacing: HideTheme.spacingNone) {
                header(board)
                Rectangle().fill(HideTheme.divider).frame(height: HideTheme.Layout.hairlineWidth)
                if !board.connected {
                    Label(ProjectHomeBoard.disconnectedNotice, systemImage: "bolt.slash")
                        .hideFont(size: HideTheme.Typography.subhead)
                        .foregroundStyle(HideTheme.warning)
                        .padding(.horizontal, HideTheme.spacingLG)
                        .padding(.vertical, HideTheme.spacingSM)
                        .accessibilityIdentifier("project-home-disconnected")
                }
                if folded {
                    VStack(spacing: HideTheme.spacingNone) {
                        lanes(board)
                        if let card = selectedPaneID.flatMap(board.card) {
                            Rectangle().fill(HideTheme.divider).frame(height: HideTheme.Layout.hairlineWidth)
                            inspector(card, board: board)
                        }
                    }
                } else {
                    HStack(spacing: HideTheme.spacingNone) {
                        lanes(board)
                        if let card = selectedPaneID.flatMap(board.card) {
                            Rectangle().fill(HideTheme.divider).frame(width: HideTheme.Layout.hairlineWidth)
                            inspector(card, board: board)
                                .frame(width: HideTheme.Home.inspectorWidth)
                        }
                    }
                }
            }
        }
    }

    // MARK: Header

    /// Title row, then the counts, the filter and the start control, which
    /// wrap under each other when the page is narrow rather than squeezing
    /// the counts into broken words.
    private func header(_ board: ProjectHomeBoard) -> some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingSM) {
            HStack(spacing: HideTheme.spacingMD) {
                Text(board.projectLabel)
                    .hideFont(size: HideTheme.Typography.headline, weight: .semibold)
                    .foregroundStyle(HideTheme.primary)
                    .lineLimit(1)
                Spacer(minLength: HideTheme.spacingSM)
                if mode == .overlay {
                    HideIconButton(
                        systemImage: "xmark",
                        help: "Close Project Home",
                        variant: .toolbar,
                        command: .menu(.projectHome),
                        action: model.closeProjectHome
                    )
                    .accessibilityIdentifier("project-home-close")
                }
            }
            ProjectHomeWrapLayout(horizontalSpacing: HideTheme.spacingMD, verticalSpacing: HideTheme.spacingSM) {
                counts(board.counts)
                HideSearchField(
                    placeholder: "Filter agents and checkouts…",
                    text: $query,
                    selection: $searchSelection,
                    resultIDs: board.lanes.flatMap(\.cards).map(\.id),
                    activate: {
                        if let id = searchSelection.selectedID, board.card(id) != nil { select(id, in: board) }
                    },
                    dismiss: { query = "" }
                )
                .frame(width: HideTheme.Home.inspectorWidth)
                .accessibilityIdentifier("project-home-search")
                Button("Start new terminal") { model.addTab() }
                    .buttonStyle(HideTextButtonStyle(appearance: .prominent, density: .regular))
                    .accessibilityIdentifier("project-home-start-terminal")
            }
        }
        .padding(.horizontal, HideTheme.spacingLG)
        .padding(.vertical, HideTheme.spacingMD)
        .background(HideTheme.panel)
    }

    /// The four groups in the pet badge order and colours. A zero keeps its
    /// slot, muted, so the row never shifts as counts move (PRD rule 6).
    private func counts(_ counts: ProjectHomeCounts) -> some View {
        HStack(spacing: HideTheme.spacingSM) {
            countLabel("Needs You", counts.needsYou, HideTheme.warning)
            countLabel("Done", counts.done, HideTheme.success)
            countLabel("Working", counts.working, HideTheme.agentWorking)
            countLabel("Seen", counts.seen, HideTheme.secondary)
        }
        .accessibilityElement(children: .combine)
        .accessibilityLabel("Needs You \(counts.needsYou), Done \(counts.done), Working \(counts.working), Seen \(counts.seen)")
    }

    private func countLabel(_ name: String, _ value: Int, _ color: Color) -> some View {
        HStack(spacing: HideTheme.spacingXS) {
            Text("\(value)")
                .hideFont(size: HideTheme.Typography.subhead, weight: .semibold, design: .monospaced)
            Text(name)
                .hideFont(size: HideTheme.Typography.caption, weight: .medium)
        }
        .foregroundStyle(value > 0 ? color : HideTheme.muted)
        .fixedSize()
    }

    // MARK: Lanes

    private func lanes(_ board: ProjectHomeBoard) -> some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: HideTheme.spacingNone) {
                    attentionStrip(board)
                    if board.isEmpty {
                        emptyLane(query.isEmpty ? ProjectHomeBoard.noCheckoutsSentence : "No agent or checkout matches “\(query)”")
                    }
                    ForEach(board.lanes) { lane in
                        ProjectHomeLaneView(
                            lane: lane,
                            selectedPaneID: selectedPaneID,
                            hoveredPaneID: hoveredPaneID,
                            shownPaneID: model.focusedPaneID,
                            onHover: { hoveredPaneID = $0 },
                            onSelect: { select($0, in: board) },
                            onOpen: { model.selectAgent(paneID: $0) },
                            onOpenCheckout: { open(lane) },
                            onOpenPullRequest: { model.openPullRequest($0) }
                        )
                        .id(lane.id)
                    }
                }
                .padding(.horizontal, HideTheme.spacingLG)
                .padding(.vertical, HideTheme.spacingMD)
                .opacity(board.connected ? 1 : HideTheme.Opacity.dimmed)
            }
            .onChange(of: scrollTarget) { _, target in
                guard let target else { return }
                withAnimation { proxy.scrollTo(target, anchor: .top) }
                scrollTarget = nil
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }

    /// The glance answer, first on the page: every Needs You and Done card
    /// across the lanes. One click finds the card in its lane, a second
    /// opens it. With nothing waiting it is one muted line (PRD rule 4).
    @ViewBuilder
    private func attentionStrip(_ board: ProjectHomeBoard) -> some View {
        if board.attention.isEmpty {
            Text(ProjectHomeBoard.nothingNeedsYouSentence)
                .hideFont(size: HideTheme.Typography.subhead)
                .foregroundStyle(HideTheme.muted)
                .padding(.bottom, HideTheme.spacingMD)
                .accessibilityIdentifier("project-home-attention-empty")
        } else {
            VStack(alignment: .leading, spacing: HideTheme.spacingSM) {
                Text("Needs You · \(board.attention.count)")
                    .hideFont(size: HideTheme.Typography.caption, weight: .semibold)
                    .foregroundStyle(HideTheme.warning)
                // Wrapped, not scrolled sideways: at twelve lanes a
                // horizontal strip hid two thirds of the answer with no
                // sign that more was there.
                ProjectHomeWrapLayout {
                    ForEach(board.attention) { card in
                        ProjectHomeCardView(
                            card: card,
                            width: HideTheme.Home.attentionCardWidth,
                            compact: true,
                            selected: selectedPaneID == card.id,
                            dimmed: false,
                            shown: model.focusedPaneID == card.id,
                            onHover: { _ in },
                            onSelect: {
                                if selectedPaneID == card.id {
                                    model.selectAgent(paneID: card.id)
                                } else {
                                    select(card.id, in: board)
                                    scrollTarget = card.laneID
                                }
                            },
                            onOpen: { model.selectAgent(paneID: card.id) }
                        )
                    }
                }
            }
            .padding(.bottom, HideTheme.spacingLG)
            .accessibilityIdentifier("project-home-attention")
        }
    }

    private func emptyLane(_ sentence: String) -> some View {
        HStack(spacing: HideTheme.spacingSM) {
            Image(systemName: "arrow.triangle.branch")
                .hideFont(size: HideTheme.Typography.caption, weight: .semibold)
                .foregroundStyle(HideTheme.muted)
                .frame(width: HideTheme.checkoutIconWidth)
            Text(sentence)
                .hideFont(size: HideTheme.Typography.subhead)
                .foregroundStyle(HideTheme.secondary)
            if !query.isEmpty {
                Button("Clear filter") { query = "" }
                    .buttonStyle(HideTextButtonStyle(appearance: .quiet))
            }
        }
        .frame(height: HideTheme.checkoutRowHeight)
        .accessibilityIdentifier("project-home-empty-lane")
    }

    private func select(_ paneID: String, in board: ProjectHomeBoard) {
        selectedPaneID = paneID
    }

    private func open(_ lane: ProjectHomeLane) {
        guard let checkout = workspace?.checkouts.first(where: { $0.id == lane.id }) else { return }
        model.selectCheckout(checkout)
    }

    // MARK: Inspector

    private func inspector(_ card: ProjectHomeCard, board: ProjectHomeBoard) -> some View {
        ProjectHomeInspector(
            card: card,
            lane: board.lane(containing: card.id),
            lineage: board.lineage(of: card.id),
            checkout: workspace?.checkouts.first { $0.id == card.laneID },
            shown: model.focusedPaneID == card.id,
            connected: board.connected,
            onOpen: { model.selectAgent(paneID: card.id) },
            onOpenPullRequest: { model.openPullRequest($0) },
            onClose: { selectedPaneID = nil }
        )
    }
}
