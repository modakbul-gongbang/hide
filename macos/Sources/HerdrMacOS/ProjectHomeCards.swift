import SwiftUI

/// One checkout as a full-width band: the header reads branch, then track,
/// then the open control; the cards flow beneath it and wrap.
struct ProjectHomeLaneView: View {
    let lane: ProjectHomeLane
    let selectedPaneID: String?
    /// The family the page raised across every lane (hover or selection);
    /// nil means nothing is raised anywhere.
    let raisedPaneIDs: Set<String>?
    /// The checkout entered Needs You while its slot was frozen; the header
    /// carries the mark until the lanes are ranked again.
    let enteredNeedsYou: Bool
    let shownPaneID: String?
    let onHover: (String?) -> Void
    let onSelect: (String) -> Void
    let onOpen: (String) -> Void
    let onOpenCheckout: () -> Void
    let onOpenPullRequest: (CorePullRequest) -> Void

    /// The cards raised in this lane. Empty means none of the raised family
    /// lives here, so nothing here is dimmed (PRD rule 5).
    private var raised: Set<String> {
        guard let raisedPaneIDs else { return [] }
        return raisedPaneIDs.intersection(lane.cards.map(\.id))
    }

    private var roots: [ProjectHomeCard] { lane.cards.filter { $0.depth == 0 } }

    var body: some View {
        // A lane with nobody in it is its header row alone: the name, the
        // badges and the track already say what there is to say.
        VStack(alignment: .leading, spacing: HideTheme.spacingSM) {
            header
            if !lane.cards.isEmpty {
                ProjectHomeWrapLayout {
                    ForEach(roots) { root in
                        cardGroup(root)
                    }
                }
            }
        }
        .padding(.vertical, HideTheme.spacingMD)
        .opacity(lane.isMissing ? HideTheme.Opacity.dimmed : 1)
        .overlay(alignment: .bottom) {
            Rectangle().fill(HideTheme.divider).frame(height: HideTheme.Layout.hairlineWidth)
        }
        .accessibilityElement(children: .contain)
        .accessibilityLabel("Checkout \(lane.label), \(lane.cards.count) agents\(enteredNeedsYou ? ", entered Needs You" : "")")
        .accessibilityIdentifier("project-home-lane-\(lane.id)")
    }

    /// Label first, track beside it while both fit and under it when the
    /// lane is narrow; the branch name is what the operator scans for, so
    /// it truncates last.
    private var header: some View {
        ProjectHomeWrapLayout(horizontalSpacing: HideTheme.spacingSM, verticalSpacing: HideTheme.spacingXS) {
            Button(action: onOpenCheckout) {
                HStack(spacing: HideTheme.spacingSM) {
                    Image(systemName: lane.kindIcon)
                        .hideFont(size: HideTheme.Typography.caption, weight: .semibold)
                        .foregroundStyle(HideTheme.secondary)
                        .frame(width: HideTheme.checkoutIconWidth)
                    Text(lane.label)
                        .hideFont(size: HideTheme.Typography.title, weight: .semibold)
                        .foregroundStyle(HideTheme.primary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                    if enteredNeedsYou {
                        AgentStatusMark(symbol: "?", color: HideTheme.warning)
                            .hideTooltip("Entered Needs You since the lanes were sorted")
                            .accessibilityLabel("Entered Needs You since the lanes were sorted")
                    }
                    if lane.summary.agentCount > 0 {
                        WorkspaceAgentSummary(presentation: lane.summary)
                            .hideTooltip(lane.summary.detailTooltip)
                    }
                    if lane.isDetached { HideBadge(label: "detached", color: HideTheme.secondary) }
                    if lane.isMissing { HideBadge(label: "missing", color: HideTheme.danger) }
                    if lane.isPrimary { HideBadge(label: "primary", color: HideTheme.secondary) }
                    if lane.cards.isEmpty, let reason = lane.uninstrumentedReason {
                        // The third of the uninstrumented mark's positions:
                        // "nobody is here" and "Hide cannot see in" must not
                        // read the same (docs/status-model.md).
                        Image(systemName: "questionmark.circle")
                            .hideFont(size: HideTheme.Typography.micro, weight: .semibold)
                            .foregroundStyle(HideTheme.muted)
                            .hideTooltip(reason)
                            .accessibilityLabel(lane.uninstrumentedLabel ?? reason)
                    }
                }
                .padding(.vertical, HideTheme.spacingXS)
                .padding(.trailing, HideTheme.spacingXS)
                .contentShape(Rectangle())
            }
            .buttonStyle(HideInteractiveButtonStyle())
            .disabled(!lane.canOpen)
            .hideTooltip(lane.canOpen ? "Open \(lane.label)\n\(lane.path)" : "Worktree folder is missing\n\(lane.path)")
            .accessibilityLabel("Open checkout \(lane.label)")
            .accessibilityIdentifier("project-home-open-checkout-\(lane.id)")
            ProjectHomeTrackView(stages: lane.track, pullRequest: lane.pullRequest, onOpenPullRequest: onOpenPullRequest)
        }
        // The row itself is the open control too, so a click between the
        // name and the track, or on a hollow chip, opens the checkout; the
        // PR chip is a button of its own and keeps its click.
        .contentShape(Rectangle())
        .onTapGesture { if lane.canOpen { onOpenCheckout() } }
    }

    /// A root with its descendants stacked under it, each child one lineage
    /// column in and hanging from a hairline, so the parent and its work
    /// wrap as one slot.
    private func cardGroup(_ root: ProjectHomeCard) -> some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
            card(root, width: HideTheme.Home.cardWidth)
            ForEach(descendants(of: root)) { child in
                card(child, width: HideTheme.Home.childCardWidth)
                    .padding(.leading, HideTheme.lineageInset(depth: child.depth))
                    .overlay(alignment: .leading) {
                        Rectangle()
                            .fill(HideTheme.divider)
                            .frame(width: HideTheme.Layout.hairlineWidth)
                            .padding(.leading, HideTheme.lineageInset(depth: child.depth) - HideTheme.Home.childRailInset)
                    }
            }
        }
    }

    private func descendants(of root: ProjectHomeCard) -> [ProjectHomeCard] {
        let ids = lane.descendants(of: root)
        return lane.cards.filter { ids.contains($0.id) }
    }

    private func card(_ card: ProjectHomeCard, width: CGFloat) -> some View {
        ProjectHomeCardView(
            card: card,
            width: width,
            compact: false,
            selected: selectedPaneID == card.id,
            dimmed: !raised.isEmpty && !raised.contains(card.id),
            shown: shownPaneID == card.id,
            onHover: { onHover($0 ? card.id : nil) },
            onSelect: { onSelect(card.id) },
            onOpen: { onOpen(card.id) }
        )
    }
}

/// The lane's progress track: five fixed segments joined by rules. A filled
/// segment carries its colour; a hollow one is outlined and says in its
/// tooltip why the data could not fill it (PRD rule 2).
struct ProjectHomeTrackView: View {
    let stages: [ProjectHomeTrackStage]
    let pullRequest: CorePullRequest?
    let onOpenPullRequest: (CorePullRequest) -> Void

    var body: some View {
        HStack(spacing: HideTheme.spacingNone) {
            ForEach(Array(stages.enumerated()), id: \.element.id) { index, stage in
                if index > 0 {
                    Rectangle()
                        .fill(HideTheme.divider)
                        .frame(width: HideTheme.Home.trackConnectorWidth, height: HideTheme.Layout.hairlineWidth)
                }
                if stage.kind == .pullRequest, let pullRequest, !stage.isHollow {
                    Button { onOpenPullRequest(pullRequest) } label: { segment(stage) }
                        .buttonStyle(HideInteractiveButtonStyle())
                        .hideTooltip(stage.tooltip + "\nOpen on GitHub")
                        .accessibilityLabel("Open pull request \(pullRequest.number)")
                        .accessibilityIdentifier("project-home-pr-\(pullRequest.number)")
                } else {
                    segment(stage)
                        .hideTooltip(stage.tooltip)
                        .accessibilityLabel(stage.tooltip)
                }
            }
        }
        .accessibilityElement(children: .contain)
    }

    private func segment(_ stage: ProjectHomeTrackStage) -> some View {
        Text(stage.label)
            .hideFont(size: HideTheme.Typography.caption, weight: .medium)
            .foregroundStyle(stage.isHollow ? HideTheme.muted : stage.color)
            .lineLimit(1)
            .padding(.horizontal, HideTheme.spacingSM)
            .frame(minWidth: HideTheme.Home.trackStageMinWidth, minHeight: HideTheme.Home.trackHeight)
            .background(
                stage.isHollow ? Color.clear : stage.color.opacity(HideTheme.Opacity.selectedFill),
                in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
            )
            .overlay {
                RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                    .strokeBorder(
                        stage.isHollow ? HideTheme.divider : stage.color.opacity(HideTheme.Opacity.emphasisFill),
                        style: StrokeStyle(lineWidth: HideTheme.Layout.hairlineWidth, dash: stage.isHollow ? [HideTheme.spacingXXS, HideTheme.spacingXXS] : [])
                    )
            }
            .contentShape(Rectangle())
    }
}

/// One agent card. The mark, badge, identity, second line and elapsed time
/// are the row's own values drawn the way `AgentRow` draws them; the card
/// only adds a fixed width and the lineage caption.
struct ProjectHomeCardView: View {
    let card: ProjectHomeCard
    let width: CGFloat
    /// The Needs You strip's density: identity and sentence, nothing else.
    let compact: Bool
    let selected: Bool
    let dimmed: Bool
    let shown: Bool
    let onHover: (Bool) -> Void
    let onSelect: () -> Void
    let onOpen: () -> Void

    private var agent: SidebarAgent { card.agent }
    private var titleColor: Color { agent.delegated ? HideTheme.muted : HideTheme.primary }

    var body: some View {
        Button(action: onSelect) {
            HStack(alignment: .top, spacing: HideTheme.spacingXS) {
                AgentStatusMark(symbol: card.status.symbol, color: card.status.color)
                AgentBadge(agentKind: agent.agentKind, stateColor: card.status.color, size: HideTheme.compactAgentBadgeSize)
                VStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
                    Text(agent.identityLabel)
                        .hideFont(size: HideTheme.Typography.body, weight: agent.emphasized ? .semibold : .medium)
                        .foregroundStyle(titleColor)
                        .lineLimit(2)
                        .fixedSize(horizontal: false, vertical: true)
                    secondLine
                    if !compact, let caption = card.fromParentCaption {
                        Text(caption)
                            .hideFont(size: HideTheme.Typography.micro)
                            .foregroundStyle(HideTheme.muted)
                            .lineLimit(1)
                    }
                    if !compact, let caption = card.toLanesCaption {
                        Text(caption)
                            .hideFont(size: HideTheme.Typography.micro)
                            .foregroundStyle(HideTheme.muted)
                            .lineLimit(1)
                            .truncationMode(.middle)
                    }
                    if !compact, let notice = card.stallNotice {
                        HStack(spacing: HideTheme.spacingXXS) {
                            Image(systemName: "clock.badge.exclamationmark")
                                .hideFont(size: HideTheme.Typography.micro, weight: .semibold)
                            Text(notice)
                                .hideFont(size: HideTheme.Typography.micro, weight: .medium)
                                .lineLimit(2)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                        .foregroundStyle(HideTheme.warning)
                    }
                }
                Spacer(minLength: HideTheme.spacingNone)
                Text(agent.elapsed)
                    .hideFont(size: HideTheme.Typography.micro, design: .monospaced)
                    .foregroundStyle(HideTheme.muted)
            }
            .padding(HideTheme.spacingSM)
            .frame(width: width, alignment: .leading)
            .background(
                selected ? HideTheme.elevated : HideTheme.panel,
                in: RoundedRectangle(cornerRadius: HideTheme.radiusMedium)
            )
            .overlay {
                RoundedRectangle(cornerRadius: HideTheme.radiusMedium)
                    .strokeBorder(
                        selected ? HideTheme.primary : (shown ? HideTheme.secondary : HideTheme.divider),
                        lineWidth: HideTheme.Layout.hairlineWidth
                    )
            }
            .contentShape(RoundedRectangle(cornerRadius: HideTheme.radiusMedium))
        }
        .buttonStyle(HideInteractiveButtonStyle())
        .simultaneousGesture(TapGesture(count: 2).onEnded(onOpen))
        .onHover(perform: onHover)
        .opacity(dimmed ? HideTheme.Opacity.dimmed : 1)
        .hideTooltip(tooltip)
        .accessibilityLabel(
            [agent.identityLabel, agent.agentKind, card.status.label, agent.detail].compactMap { $0 }.joined(separator: ", ")
        )
        .accessibilityValue(accessibilityValue)
        .accessibilityAddTraits(selected ? .isSelected : [])
        .accessibilityIdentifier("project-home-card-\(card.id)")
    }

    /// The core's second line, drawn as `AgentRow` draws it: the status
    /// word in the mark's colour when the row has no sentence, the sentence
    /// in the row's emphasis otherwise (docs/status-model.md).
    @ViewBuilder
    private var secondLine: some View {
        HStack(alignment: .firstTextBaseline, spacing: HideTheme.spacingXS) {
            if agent.statusWordVisible {
                Text(card.status.label)
                    .hideFont(size: HideTheme.Typography.caption, weight: .medium)
                    .foregroundStyle(card.status.color)
            }
            if let detail = agent.detail {
                Text(detail)
                    .hideFont(size: HideTheme.Typography.caption)
                    .foregroundStyle(agent.delegated ? HideTheme.muted : (agent.emphasized ? HideTheme.primary : HideTheme.secondary))
                    .lineLimit(1)
                    .truncationMode(.tail)
            }
        }
    }

    private var tooltip: String {
        var lines = ["\(agent.identityLabel) · \(card.status.label)"]
        if let detail = agent.detail { lines.append(detail) }
        lines.append(shown ? "Shown · double-click to return" : "Click to inspect · double-click to open")
        return lines.joined(separator: "\n")
    }

    private var accessibilityValue: String {
        var values = [selected ? "Selected" : "Not selected"]
        if shown { values.append("Shown") }
        if agent.delegated { values.append("Delegated") }
        if let caption = card.fromParentCaption { values.append(caption) }
        if let caption = card.toLanesCaption { values.append(caption) }
        if let notice = card.stallNotice { values.append(notice) }
        if !agent.elapsed.isEmpty { values.append(agent.elapsed) }
        return values.joined(separator: ", ")
    }
}

/// The persistent column a selection opens. Inspecting never moves
/// terminal focus; only `Open` does (PRD rule 5).
struct ProjectHomeInspector: View {
    let card: ProjectHomeCard
    let lane: ProjectHomeLane?
    let lineage: [ProjectHomeLineageStep]
    let checkout: CoreCheckoutSnapshot?
    let shown: Bool
    let connected: Bool
    let onOpen: () -> Void
    let onOpenPullRequest: (CorePullRequest) -> Void
    let onClose: () -> Void

    private var agent: SidebarAgent { card.agent }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: HideTheme.spacingMD) {
                HStack {
                    Text("Inspected agent").foregroundStyle(HideTheme.secondary)
                    Spacer(minLength: HideTheme.spacingXS)
                    HideIconButton(systemImage: "xmark", help: "Close inspector", variant: .toolbar, action: onClose)
                }
                .hideFont(size: HideTheme.Typography.subhead)

                HStack(alignment: .top, spacing: HideTheme.spacingSM) {
                    AgentBadge(agentKind: agent.agentKind, stateColor: card.status.color, size: HideTheme.compactAgentBadgeSize)
                    VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
                        Text(agent.identityLabel)
                            .hideFont(size: HideTheme.Typography.title, weight: .semibold)
                            .foregroundStyle(HideTheme.primary)
                            .fixedSize(horizontal: false, vertical: true)
                        Text("\(card.status.symbol) \(card.status.label)")
                            .foregroundStyle(card.status.color)
                        if let detail = agent.detail {
                            Text(detail)
                                .foregroundStyle(HideTheme.secondary)
                                .fixedSize(horizontal: false, vertical: true)
                        }
                    }
                }
                if let notice = card.stallNotice {
                    Label(notice, systemImage: "clock.badge.exclamationmark").foregroundStyle(HideTheme.warning)
                        .fixedSize(horizontal: false, vertical: true)
                }
                if agent.lineageOrphan {
                    Label(agent.lineageHint ?? "Parent unavailable; shown as operator-owned work", systemImage: "questionmark.circle")
                        .foregroundStyle(HideTheme.warning)
                        .fixedSize(horizontal: false, vertical: true)
                }
                if !connected {
                    Label("Live status unavailable while Herdr reconnects", systemImage: "bolt.slash")
                        .foregroundStyle(HideTheme.warning)
                }

                Button(shown ? "Viewing" : "Open") { onOpen() }
                    .buttonStyle(HideTextButtonStyle(appearance: .prominent))
                    .disabled(shown)
                    .accessibilityIdentifier("project-home-open-agent")

                if let lane {
                    section("Checkout") {
                        Text(lane.label).foregroundStyle(HideTheme.primary)
                        Text(lane.path)
                            .foregroundStyle(HideTheme.muted)
                            .fixedSize(horizontal: false, vertical: true)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .textSelection(.enabled)
                    }
                }
                if lineage.count > 1 {
                    section("Lineage") {
                        ForEach(Array(lineage.enumerated()), id: \.element.id) { index, step in
                            HStack(spacing: HideTheme.spacingXS) {
                                Text(step.paneID == card.id ? "●" : "○")
                                    .hideFont(size: HideTheme.Typography.micro, design: .monospaced)
                                    .foregroundStyle(HideTheme.muted)
                                Text(step.label).lineLimit(1)
                                    .foregroundStyle(step.paneID == card.id ? HideTheme.primary : HideTheme.secondary)
                                if step.laneLabel != lane?.label {
                                    Text(step.laneLabel).hideFont(size: HideTheme.Typography.micro).foregroundStyle(HideTheme.muted)
                                }
                            }
                            .padding(.leading, HideTheme.lineageInset(depth: index))
                        }
                    }
                }
                if let pullRequest = checkout?.pullRequest {
                    section("Pull request") {
                        HStack {
                            Text("#\(pullRequest.number) · \(CheckoutCardPresentation.pullRequestState(pullRequest))")
                                .foregroundStyle(CheckoutCardPresentation.pullRequestColor(pullRequest))
                            Spacer(minLength: HideTheme.spacingXS)
                            Button("Open PR") { onOpenPullRequest(pullRequest) }
                                .buttonStyle(HideTextButtonStyle(appearance: .quiet))
                        }
                    }
                }
            }
            .hideFont(size: HideTheme.Typography.subhead)
            .padding(HideTheme.spacingMD)
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .background(HideTheme.panel)
        .accessibilityIdentifier("project-home-inspector")
    }

    private func section<Content: View>(_ title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingXS) {
            Text(title)
                .hideFont(size: HideTheme.Typography.caption, weight: .semibold)
                .foregroundStyle(HideTheme.secondary)
            content()
        }
    }
}
