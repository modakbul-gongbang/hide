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
    /// Lane slots for the open page; settled from rank when the page opens,
    /// on `Sort`, and when the project changes (PRD rule 1, round 2).
    @State private var laneOrder: ProjectHomeLaneOrder?
    /// Whether the Needs You strip shows past its two rows.
    @State private var attentionExpanded = false

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

    /// The whole board in rank order, before the filter.
    private var builtBoard: ProjectHomeBoard? {
        guard let input else { return nil }
        let key = ProjectHomeMemo.Key(
            revision: model.core.snapshot?.navigationRevision ?? 0,
            workspaceID: workspace?.id,
            connected: input.connected
        )
        return memo.board(for: key) { ProjectHomeBoard.build(input) }
    }

    private var board: ProjectHomeBoard? { builtBoard?.filtered(query: query) }

    /// The family a hover or, failing that, the selection raises across
    /// every lane; nil when nothing is raised.
    private func raisedFamily(in board: ProjectHomeBoard) -> Set<String>? {
        (hoveredPaneID ?? selectedPaneID).map(board.family)
    }

    private func settleLaneOrder(from sort: Bool = false) {
        guard let workspace, let built = builtBoard else { return }
        laneOrder = ProjectHomeLaneOrder.settle(
            sort ? nil : laneOrder, projectPath: workspace.path, rankedLanes: built.lanes
        )
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
            attentionExpanded = false
        }
        // The order settles when the page opens and whenever the set of
        // lanes changes (a new checkout appends, a gone one drops); rank
        // changes alone never move a lane while the page is open.
        .onChange(of: builtBoard?.lanes.map(\.id) ?? [], initial: true) { _, _ in settleLaneOrder() }
        .accessibilityIdentifier("project-home")
    }

    private func page(_ board: ProjectHomeBoard) -> some View {
        GeometryReader { proxy in
            let folded = proxy.size.width < HideTheme.Home.inspectorFoldWidth
            let inspecting = selectedPaneID.flatMap(board.card) != nil
            let lanesWidth = proxy.size.width
                - (inspecting && !folded ? HideTheme.Home.inspectorWidth + HideTheme.Layout.hairlineWidth : 0)
                - HideTheme.spacingLG * 2
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
                        lanes(board, width: lanesWidth)
                        if let card = selectedPaneID.flatMap(board.card) {
                            Rectangle().fill(HideTheme.divider).frame(height: HideTheme.Layout.hairlineWidth)
                            inspector(card, board: board)
                        }
                    }
                } else {
                    HStack(spacing: HideTheme.spacingNone) {
                        lanes(board, width: lanesWidth)
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

    /// One row while it fits, with the filter field giving way first; below
    /// that the title keeps its row and the counts, the filter and the
    /// start control wrap under it. A count never breaks inside its word.
    private func header(_ board: ProjectHomeBoard) -> some View {
        ViewThatFits(in: .horizontal) {
            HStack(spacing: HideTheme.spacingMD) {
                title(board)
                counts(board.counts)
                sortControl
                Spacer(minLength: HideTheme.spacingSM)
                searchField(board)
                    .frame(minWidth: HideTheme.Home.searchMinWidth, idealWidth: HideTheme.Home.searchMinWidth,
                           maxWidth: HideTheme.Home.inspectorWidth)
                startControl
                closeControl
            }
            VStack(alignment: .leading, spacing: HideTheme.spacingSM) {
                HStack(spacing: HideTheme.spacingMD) {
                    title(board)
                    Spacer(minLength: HideTheme.spacingSM)
                    closeControl
                }
                ProjectHomeWrapLayout(horizontalSpacing: HideTheme.spacingMD, verticalSpacing: HideTheme.spacingSM) {
                    counts(board.counts)
                    sortControl
                    searchField(board)
                        .frame(width: HideTheme.Home.inspectorWidth)
                    startControl
                }
            }
        }
        .padding(.horizontal, HideTheme.spacingLG)
        .padding(.vertical, HideTheme.spacingMD)
        .background(HideTheme.panel)
    }

    private func title(_ board: ProjectHomeBoard) -> some View {
        Text(board.projectLabel)
            .hideFont(size: HideTheme.Typography.headline, weight: .semibold)
            .foregroundStyle(HideTheme.primary)
            .lineLimit(1)
    }

    private func searchField(_ board: ProjectHomeBoard) -> some View {
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
        .accessibilityIdentifier("project-home-search")
    }

    private var startControl: some View {
        Button("Start new terminal") { model.addTab() }
            .buttonStyle(HideTextButtonStyle(appearance: .prominent, density: .regular))
            .accessibilityIdentifier("project-home-start-terminal")
    }

    /// Re-ranks the lanes on demand; disabled while they already sit in
    /// rank order, so the control says whether anything moved.
    private var sortControl: some View {
        let ranked = laneOrder.map { order in builtBoard.map { order.isRanked(against: $0.lanes) } ?? true } ?? true
        return Button("Sort") { settleLaneOrder(from: true) }
            .buttonStyle(HideTextButtonStyle(appearance: .quiet))
            .disabled(ranked)
            .hideTooltip(ranked
                ? "Lanes are in attention order"
                : "Reorder lanes by attention: Needs You, Done, Working, then the rest")
            .accessibilityIdentifier("project-home-sort")
    }

    @ViewBuilder
    private var closeControl: some View {
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

    private func lanes(_ board: ProjectHomeBoard, width: CGFloat) -> some View {
        let raised = raisedFamily(in: board)
        let lanes = laneOrder?.apply(to: board.lanes) ?? board.lanes
        let entered = laneOrder?.enteredNeedsYou(in: board.lanes) ?? []
        return ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: HideTheme.spacingNone) {
                    attentionStrip(board, width: width)
                    if board.isEmpty {
                        emptyLane(query.isEmpty ? ProjectHomeBoard.noCheckoutsSentence : "No agent or checkout matches “\(query)”")
                    }
                    ForEach(lanes) { lane in
                        ProjectHomeLaneView(
                            lane: lane,
                            selectedPaneID: selectedPaneID,
                            raisedPaneIDs: raised,
                            enteredNeedsYou: entered.contains(lane.id),
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
    private func attentionStrip(_ board: ProjectHomeBoard, width: CGFloat) -> some View {
        let perRow = ProjectHomeAttentionFold.perRow(
            width: width, cardWidth: HideTheme.Home.attentionCardWidth, spacing: HideTheme.spacingSM
        )
        let visible = ProjectHomeAttentionFold.visibleCount(
            total: board.attention.count, perRow: perRow, expanded: attentionExpanded
        )
        let hidden = board.attention.count - visible
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
                // Wrapped, not scrolled sideways, and capped at two rows
                // with the rest one click away: at twelve lanes a sideways
                // strip hid two thirds of the answer with no sign that more
                // was there, and an uncapped one filled the first screen.
                ProjectHomeWrapLayout {
                    ForEach(board.attention.prefix(visible)) { card in
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
                    if hidden > 0 {
                        Button(ProjectHomeAttentionFold.moreLabel(hidden: hidden)) { attentionExpanded = true }
                            .buttonStyle(HideTextButtonStyle(appearance: .quiet))
                            .hideTooltip("Show every card that needs you")
                            .accessibilityIdentifier("project-home-attention-more")
                    } else if attentionExpanded, board.attention.count > ProjectHomeAttentionFold.rows * perRow {
                        Button(ProjectHomeAttentionFold.fewerLabel) { attentionExpanded = false }
                            .buttonStyle(HideTextButtonStyle(appearance: .quiet))
                            .hideTooltip("Fold the strip back to two rows")
                            .accessibilityIdentifier("project-home-attention-fewer")
                    }
                }
            }
            .padding(.bottom, HideTheme.spacingLG)
            .accessibilityElement(children: .contain)
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
