import SwiftUI

/// What the pane header draws about this pane's place in the tree.
///
/// The decisions live here rather than in the views so a chip row, a
/// breadcrumb step and the Overview line cannot disagree about what a child
/// is called or how many were left out.
enum PaneLineagePresentation {
    /// The chips that fit, and how many did not.
    ///
    /// Overflow folds into the same `+N` the Workspace summary chip uses
    /// rather than a second pattern (PRD B6, D-20).
    static func chipRow(_ chips: [CoreAgentChip], limit: Int) -> (visible: [CoreAgentChip], overflow: Int) {
        guard limit > 0 else { return ([], chips.count) }
        guard chips.count > limit else { return (chips, 0) }
        // One slot is spent on the `+N` itself, so the count it shows is
        // honest rather than one short.
        let kept = Array(chips.prefix(limit - 1))
        return (kept, chips.count - kept.count)
    }

    /// How many chips fit in the width the row was given.
    static func chipLimit(width: CGFloat) -> Int {
        let slot = HideTheme.Layout.paneChildChipMaxWidth + HideTheme.spacingXS
        return max(1, Int((width / slot).rounded(.down)))
    }

    /// The count badge for the in-process subagents, or nothing to draw.
    ///
    /// An unknown count is written as a dash, never as a zero: a zero claims
    /// the session is working alone, which is exactly what Hide does not know
    /// (PRD B24, B32, D-53).
    static func subagentBadge(_ counts: CoreSubagentCounts) -> (text: String, accessibility: String)? {
        if counts.isSilent { return nil }
        let mark = { (value: UInt32?) in value.map(String.init) ?? "-" }
        let text = "\(mark(counts.working))/\(mark(counts.done))"
        let spoken = { (value: UInt32?, noun: String) in
            value.map { "\($0) \(noun)" } ?? "an unknown number \(noun)"
        }
        return (
            text,
            "Subagents in this session: \(spoken(counts.working, "working")), \(spoken(counts.done, "done"))"
        )
    }

    /// Whether this pane's header shows a second row at all.
    ///
    /// A pane with no agent has no children snapshot and therefore no row; a
    /// pane whose instrumented session has spawned nothing has a snapshot and
    /// nothing to say, which is a confirmed answer and also no row (PRD B22,
    /// B23, D-30).
    static func showsChildRow(_ children: CorePaneChildren?) -> Bool {
        guard let children else { return false }
        if !children.instrumented { return true }
        return !children.chips.isEmpty || subagentBadge(children.subagents) != nil
    }
}

/// One child of this pane, as a chip that replaces the screen when clicked.
struct PaneChildChip: View {
    let chip: CoreAgentChip
    let connected: Bool
    let onSelect: () -> Void

    private var status: AgentStatusPresentation {
        AgentStatusPresentation(
            demand: chip.demand,
            activity: chip.activity,
            emphasized: chip.emphasized,
            symbol: chip.symbol,
            label: chip.statusLabel,
            connected: connected
        )
    }

    var body: some View {
        Button(action: onSelect) {
            HStack(spacing: HideTheme.spacingXXS) {
                Text(status.symbol)
                    .hideFont(size: HideTheme.Typography.micro, weight: .bold)
                    .foregroundStyle(status.color)
                    .frame(width: HideTheme.agentMarkWidth)
                Text(chip.label)
                    .hideFont(size: HideTheme.Typography.micro, weight: .semibold)
                    .foregroundStyle(chip.delegated ? HideTheme.secondary : HideTheme.primary)
                    .lineLimit(1)
                    .truncationMode(.middle)
            }
            .padding(.horizontal, HideTheme.spacingXS)
            .frame(height: HideTheme.Layout.panelCollapseControlSize)
            .frame(maxWidth: HideTheme.Layout.paneChildChipMaxWidth, alignment: .leading)
            .background(
                RoundedRectangle(cornerRadius: HideTheme.radiusExtraSmall)
                    .fill(HideTheme.elevated)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(HideInteractiveButtonStyle())
        .hideTooltip("\(chip.label): \(status.label). \(chip.detail)")
        .accessibilityLabel("\(chip.label), \(status.label). \(chip.detail)")
        .accessibilityHint("Shows this child in place of the current pane")
    }
}

/// The mark shown where the chips would be when Hide cannot see into a pane.
///
/// It is a mark plus an accessible name, never a color on its own, and the
/// whole explanation is on the tooltip (PRD B21, B37).
struct PaneUninstrumentedMark: View {
    let reason: String
    let accessibilityName: String

    var body: some View {
        HStack(spacing: HideTheme.spacingXXS) {
            Image(systemName: "questionmark.circle")
                .hideFont(size: HideTheme.Typography.micro, weight: .semibold)
            Text("Children unknown")
                .hideFont(size: HideTheme.Typography.micro, weight: .semibold)
                .lineLimit(1)
                .truncationMode(.tail)
        }
        .foregroundStyle(HideTheme.secondary)
        .padding(.horizontal, HideTheme.spacingXS)
        .frame(height: HideTheme.Layout.panelCollapseControlSize)
        .background(
            RoundedRectangle(cornerRadius: HideTheme.radiusExtraSmall)
                .fill(HideTheme.elevated)
        )
        .hideTooltip("\(reason) Open Settings to see the hook diagnosis.")
        .accessibilityLabel(accessibilityName)
        .accessibilityHint(reason)
    }
}

/// The in-process subagents, as one badge at the end of the chip row.
struct PaneSubagentBadge: View {
    let text: String
    let accessibilityName: String

    var body: some View {
        HStack(spacing: HideTheme.spacingXXS) {
            Image(systemName: "circle.grid.2x2")
                .hideFont(size: HideTheme.Typography.micro, weight: .semibold)
            Text(text)
                .hideFont(size: HideTheme.Typography.micro, weight: .semibold)
        }
        .foregroundStyle(HideTheme.secondary)
        .padding(.horizontal, HideTheme.spacingXS)
        .frame(height: HideTheme.Layout.panelCollapseControlSize)
        .background(
            RoundedRectangle(cornerRadius: HideTheme.radiusExtraSmall)
                .fill(HideTheme.elevated)
        )
        .hideTooltip(accessibilityName)
        .accessibilityLabel(accessibilityName)
    }
}

/// The pane header's second row: this pane's children, or the reason there is
/// nothing to say about them.
///
/// It exists only when there is something to draw, which is what keeps a pane
/// with no children at the header's own 28pt (DESIGN.md, user decision).
struct PaneChildRow: View {
    let children: CorePaneChildren
    let connected: Bool
    let onSelect: (String) -> Void

    var body: some View {
        GeometryReader { proxy in
            let limit = PaneLineagePresentation.chipLimit(width: proxy.size.width)
            let row = PaneLineagePresentation.chipRow(children.chips, limit: limit)
            HStack(spacing: HideTheme.spacingXS) {
                if !children.instrumented,
                    let reason = children.uninstrumentedReason
                {
                    PaneUninstrumentedMark(
                        reason: reason,
                        accessibilityName: children.uninstrumentedLabel ?? reason
                    )
                }

                ForEach(row.visible) { chip in
                    PaneChildChip(chip: chip, connected: connected) { onSelect(chip.paneID) }
                }

                if row.overflow > 0 {
                    Text("+\(row.overflow)")
                        .hideFont(size: HideTheme.Typography.micro, weight: .semibold)
                        .foregroundStyle(HideTheme.secondary)
                        .padding(.horizontal, HideTheme.spacingXS)
                        .frame(height: HideTheme.Layout.panelCollapseControlSize)
                        .background(
                            RoundedRectangle(cornerRadius: HideTheme.radiusExtraSmall)
                                .fill(HideTheme.elevated)
                        )
                        .hideTooltip("\(row.overflow) more children. The sidebar lists them all.")
                        .accessibilityLabel("\(row.overflow) more children")
                }

                Spacer(minLength: HideTheme.spacingNone)

                if let badge = PaneLineagePresentation.subagentBadge(children.subagents) {
                    PaneSubagentBadge(text: badge.text, accessibilityName: badge.accessibility)
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
        }
        .padding(.horizontal, HideTheme.spacingSM)
        .frame(
            maxWidth: .infinity,
            minHeight: HideTheme.Layout.paneChildRowHeight,
            maxHeight: HideTheme.Layout.paneChildRowHeight,
            alignment: .leading
        )
    }
}

/// The pane's ancestors, root first, each step carrying that layer's siblings.
///
/// The path is derived from the lineage every time rather than stored, so it
/// cannot drift after a restart, a tab switch or a child exiting (PRD B9,
/// D-18). A step with no siblings draws no chevron, following the existing
/// rule that a control with nothing to disclose is not drawn (PRD B10, D-12).
struct PaneBreadcrumb: View {
    let steps: [CoreLineageStep]
    let onSelect: (String) -> Void

    var body: some View {
        HStack(spacing: HideTheme.spacingXXS) {
            ForEach(steps) { step in
                Button { onSelect(step.paneID) } label: {
                    Text(step.label)
                        .hideFont(size: HideTheme.Typography.micro, weight: .semibold)
                        .foregroundStyle(HideTheme.secondary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .contentShape(Rectangle())
                }
                .buttonStyle(HideInteractiveButtonStyle())
                .hideTooltip("Go back to \(step.label)")
                .accessibilityLabel("Go back to \(step.label)")

                if !step.siblings.isEmpty {
                    Menu {
                        ForEach(step.siblings) { sibling in
                            Button(sibling.label) { onSelect(sibling.paneID) }
                        }
                    } label: {
                        Image(systemName: "chevron.down")
                            .hideFont(size: HideTheme.Typography.micro, weight: .semibold)
                            .foregroundStyle(HideTheme.muted)
                    }
                    .menuStyle(.borderlessButton)
                    .menuIndicator(.hidden)
                    .fixedSize()
                    .hideTooltip("Switch to another agent at this step")
                    .accessibilityLabel("Siblings of \(step.label)")
                }

                Text("/")
                    .hideFont(size: HideTheme.Typography.micro)
                    .foregroundStyle(HideTheme.muted)
                    .accessibilityHidden(true)
            }
        }
        .frame(maxWidth: HideTheme.Layout.paneChildChipMaxWidth * 2, alignment: .leading)
    }
}

/// The Overview worktree row's one agent line.
///
/// Overview's value is width, so this is a line rather than a new area
/// (PRD B34, D-32, D-55). An empty line with no mark is a worktree nobody is
/// working in; an empty line with the mark is one Hide cannot see into, and
/// the difference is what the third mark position exists for (PRD B35, D-60).
struct WorktreeAgentLine: View {
    let line: CoreWorktreeAgentLine
    let connected: Bool
    let onSelect: (String) -> Void

    var body: some View {
        if line.agents.isEmpty && line.uninstrumentedReason == nil {
            // Nobody is working here, and that is an answer rather than a
            // gap, so the row says nothing rather than apologising.
            EmptyView()
        } else {
            GeometryReader { proxy in
                // The Git panel is narrow, so the line folds the way the pane
                // header's chip row does rather than running off the edge.
                // The mark, when there is one, costs a slot of its own.
                let slots = PaneLineagePresentation.chipLimit(width: proxy.size.width)
                let limit = line.uninstrumentedReason == nil ? slots : slots - 1
                let row = PaneLineagePresentation.chipRow(line.agents, limit: limit)
                HStack(spacing: HideTheme.spacingXS) {
                    if let reason = line.uninstrumentedReason {
                        PaneUninstrumentedMark(
                            reason: reason,
                            accessibilityName: line.uninstrumentedLabel ?? reason
                        )
                    }
                    ForEach(row.visible) { agent in
                        PaneChildChip(chip: agent, connected: connected) { onSelect(agent.paneID) }
                    }
                    if row.overflow > 0 {
                        Text("+\(row.overflow)")
                            .hideFont(size: HideTheme.Typography.micro, weight: .semibold)
                            .foregroundStyle(HideTheme.secondary)
                            .padding(.horizontal, HideTheme.spacingXS)
                            .frame(height: HideTheme.Layout.panelCollapseControlSize)
                            .background(
                                RoundedRectangle(cornerRadius: HideTheme.radiusExtraSmall)
                                    .fill(HideTheme.elevated)
                            )
                            .hideTooltip("\(row.overflow) more agents. The sidebar lists them all.")
                            .accessibilityLabel("\(row.overflow) more agents")
                    }
                    Spacer(minLength: HideTheme.spacingNone)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .leading)
            }
            .frame(
                maxWidth: .infinity,
                minHeight: HideTheme.Layout.paneChildRowHeight,
                maxHeight: HideTheme.Layout.paneChildRowHeight,
                alignment: .leading
            )
            .accessibilityElement(children: .contain)
            .accessibilityLabel("Agents in this worktree")
        }
    }
}
