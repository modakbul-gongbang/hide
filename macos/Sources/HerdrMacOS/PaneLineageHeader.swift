import SwiftUI

/// What the pane header draws about this pane's place in the tree.
///
/// The decisions live here rather than in the views so a chip row, a
/// breadcrumb step and the Overview line cannot disagree about what a child
/// is called or how many were left out.
enum PaneLineagePresentation {
    /// Replaces transport-level breadcrumb labels with the identity Hide is
    /// already drawing for each live pane. Herdr lineage remains the authority
    /// for the relationship and pane id; the live pane supplies only the name
    /// the operator recognizes on screen (PRD B8, B15, B23).
    static func resolvingLivePaneLabels(
        in steps: [CoreLineageStep],
        labelForPane: (String) -> String?
    ) -> [CoreLineageStep] {
        steps.map { step in
            guard let label = labelForPane(step.paneID)?.trimmingCharacters(in: .whitespacesAndNewlines),
                  !label.isEmpty
            else { return step }
            return CoreLineageStep(paneID: step.paneID, label: label, siblings: step.siblings)
        }
    }

    /// The closest live ancestor, excluding the pane currently being shown.
    /// Production breadcrumbs include the current pane as their final layer
    /// so that layer can offer its siblings. It must never become the return
    /// destination or label (PRD B15, B23).
    static func directParent(in steps: [CoreLineageStep], currentPaneID: String) -> CoreLineageStep? {
        steps.last { $0.paneID != currentPaneID }
    }

    /// One recognizable child and an exact count of its remaining siblings.
    static func chipRow(_ chips: [CoreAgentChip]) -> (visible: [CoreAgentChip], overflow: Int) {
        (Array(chips.prefix(1)), max(0, chips.count - 1))
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
    /// Only known child work earns the child row. Instrumentation uncertainty
    /// belongs to the identity row's help icon, so an agent with no known
    /// child remains a single 28pt row (PRD B19, D-07).
    static func showsChildRow(_ children: CorePaneChildren?) -> Bool {
        guard let children else { return false }
        return !children.chips.isEmpty || subagentBadge(children.subagents) != nil
    }

    /// Whether the identity row needs the instrumentation help icon.
    ///
    /// This is deliberately independent of child-row visibility: partial
    /// data can show known children while still stating that the total is not
    /// authoritative (PRD B19).
    static func showsInstrumentationHelp(_ children: CorePaneChildren?) -> Bool {
        children?.instrumented == false
    }
}

/// One child of this pane, as a chip that replaces the screen when clicked.
struct PaneChildChip: View {
    let chip: CoreAgentChip
    let connected: Bool
    var pending = false
    var navigationPending = false
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
            HStack(spacing: HideTheme.spacingXS) {
                if pending {
                    ProgressView().controlSize(.mini)
                } else {
                    AgentStatusMark(symbol: status.symbol, color: status.color)
                }
                AgentBadge(agentKind: chip.agentKind, stateColor: status.color, size: HideTheme.compactAgentBadgeSize)
                Text(chip.label)
                    .hideFont(size: HideTheme.Typography.body, weight: .medium)
                    .foregroundStyle(chip.delegated ? HideTheme.secondary : HideTheme.primary)
                    .lineLimit(1)
                    .truncationMode(.tail)
            }
            .padding(.horizontal, HideTheme.spacingXS)
            .frame(height: HideTheme.Control.compactHeight)
            .frame(maxWidth: HideTheme.Layout.paneChildChipMaxWidth, alignment: .leading)
            .background(
                RoundedRectangle(cornerRadius: HideTheme.radiusExtraSmall)
                    .fill(HideTheme.elevated)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(HideInteractiveButtonStyle())
        .disabled(navigationPending)
        .hideTooltip("\(pending ? "Opening " : "")\(chip.label): \(chip.description(status: status))")
        .accessibilityLabel("\(chip.label), \(chip.description(status: status))")
        .accessibilityHint("Shows this child in place of the current pane")
    }
}

/// The first-line help mark shown when Hide cannot measure child work.
///
/// It does not consume a child row or spell out a second title. The full
/// explanation remains available on hover and to assistive technology (PRD
/// B19, D-07).
struct PaneInstrumentationHelp: View {
    let reason: String
    let accessibilityName: String

    var body: some View {
        Image(systemName: "questionmark.circle")
            .hideFont(size: HideTheme.Typography.micro, weight: .semibold)
        .foregroundStyle(HideTheme.secondary)
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
    let paneID: String
    let children: CorePaneChildren
    let connected: Bool
    let operation: PaneSelectionOperation?
    let onSelect: (String) -> Void
    @State private var showsRelationship = false
    @State private var inspectedPaneID: String?

    var body: some View {
        // One child slot plus the honest +N overflow slot.
        let row = PaneLineagePresentation.chipRow(children.chips)
        HStack(spacing: HideTheme.spacingXS) {
            ForEach(row.visible) { chip in
                PaneChildChip(
                    chip: chip, connected: connected,
                    pending: operation?.isPending == true && operation?.targetPaneID == chip.paneID,
                    navigationPending: operation?.isPending == true
                ) { onSelect(chip.paneID) }
            }

            if !children.chips.isEmpty {
                Button {
                    inspectedPaneID = inspectedPaneID ?? children.chips.first?.paneID
                    showsRelationship = true
                } label: {
                    HStack(spacing: HideTheme.spacingXS) {
                        Image(systemName: "arrow.triangle.branch")
                        if row.overflow > 0 { Text("+\(row.overflow)") }
                    }
                    .hideFont(size: HideTheme.Typography.caption, weight: .medium)
                    .padding(.horizontal, HideTheme.spacingXS)
                    .frame(height: HideTheme.Control.compactHeight)
                    .contentShape(Rectangle())
                }
                .buttonStyle(HideInteractiveButtonStyle())
                .fixedSize()
                .hideTooltip("Inspect this pane's \(children.chips.count) direct children")
                .accessibilityLabel("Inspect child relationships")
            }

            Spacer(minLength: HideTheme.spacingNone)

            if let badge = PaneLineagePresentation.subagentBadge(children.subagents) {
                PaneSubagentBadge(text: badge.text, accessibilityName: badge.accessibility)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .padding(.horizontal, HideTheme.spacingSM)
        .frame(
            maxWidth: .infinity,
            minHeight: HideTheme.Layout.paneChildRowHeight,
            maxHeight: HideTheme.Layout.paneChildRowHeight,
            alignment: .leading
        )
        .sheet(isPresented: $showsRelationship) {
            PaneRelationshipSheet(
                sourcePaneID: paneID,
                children: children.chips,
                connected: connected,
                inspectedPaneID: $inspectedPaneID,
                operation: operation,
                onOpen: onSelect
            )
        }
    }
}

struct PaneParentReturn: View {
    let steps: [CoreLineageStep]
    let currentPaneID: String
    let operation: PaneSelectionOperation?
    let onSelect: (String) -> Void
    @State private var showsFailure = false

    var body: some View {
        if let parent = PaneLineagePresentation.directParent(in: steps, currentPaneID: currentPaneID) {
            ViewThatFits(in: .horizontal) {
                parentButton(parent, showsName: true)
                parentButton(parent, showsName: false)
            }
        }
    }

    private func parentButton(_ parent: CoreLineageStep, showsName: Bool) -> some View {
        let matchingOperation = operation.flatMap {
            $0.isFor(sourcePaneID: currentPaneID, targetPaneID: parent.paneID) ? $0 : nil
        }
        let pending = matchingOperation?.isPending == true
        let failure = matchingOperation.flatMap { operation -> String? in
            guard case .failed(let reason, _) = operation.phase else { return nil }
            return reason
        }
        let canRetry = matchingOperation.map { operation in
            guard case .failed(_, let retryable) = operation.phase else { return false }
            return retryable
        } ?? false
        return Button {
            if failure != nil { showsFailure = true }
            else { onSelect(parent.paneID) }
        } label: {
            HStack(spacing: HideTheme.spacingXXS) {
                if pending {
                    ProgressView().controlSize(.mini)
                } else {
                    Image(systemName: failure == nil ? "arrow.turn.up.left" : "exclamationmark.triangle")
                        .hideFont(size: HideTheme.Typography.micro)
                }
                if showsName {
                    Text(parent.label)
                        .lineLimit(1)
                        .truncationMode(.middle)
                }
            }
            .hideFont(size: HideTheme.Typography.micro, weight: .semibold)
            .foregroundStyle(failure == nil ? HideTheme.secondary : HideTheme.warning)
            .contentShape(Rectangle())
        }
        .buttonStyle(HideInteractiveButtonStyle())
        .disabled(operation?.isPending == true)
        .hideTooltip(failure ?? (pending ? "Opening parent \(parent.label)" : "Return to parent \(parent.label)"))
        .accessibilityLabel(failure ?? (pending ? "Opening parent \(parent.label)" : "Return to parent \(parent.label)"))
        .accessibilityIdentifier("pane-parent-return-\(parent.paneID)")
        .onAppear { if failure != nil { showsFailure = true } }
        .onChange(of: failure) { _, reason in showsFailure = reason != nil }
        .popover(isPresented: $showsFailure) {
            if let failure {
                VStack(alignment: .leading, spacing: HideTheme.spacingSM) {
                    Text("Could not return to \(parent.label)")
                        .hideFont(size: HideTheme.Typography.subhead, weight: .semibold)
                    Text(failure)
                        .hideFont(size: HideTheme.Typography.body)
                        .foregroundStyle(HideTheme.warning)
                        .fixedSize(horizontal: false, vertical: true)
                    if canRetry {
                        HStack {
                            Spacer()
                            Button("Retry") {
                                showsFailure = false
                                onSelect(parent.paneID)
                            }
                        }
                    }
                }
                .padding(HideTheme.spacingMD)
                .frame(width: HideTheme.Layout.pullRequestPopoverWidth)
                .foregroundStyle(HideTheme.primary)
                .background(HideTheme.panel)
                .preferredColorScheme(.dark)
            }
        }
    }
}

private struct PaneRelationshipSheet: View {
    let sourcePaneID: String
    let children: [CoreAgentChip]
    let connected: Bool
    @Binding var inspectedPaneID: String?
    let operation: PaneSelectionOperation?
    let onOpen: (String) -> Void
    @Environment(\.dismiss) private var dismiss
    @State private var requestedPaneID: String?

    var body: some View {
        VStack(alignment: .leading, spacing: HideTheme.spacingMD) {
            HStack {
                Text("Direct children")
                    .hideFont(size: HideTheme.Typography.headline, weight: .semibold)
                Spacer()
                HideIconButton(
                    systemImage: "xmark",
                    help: "Close relationships",
                    accessibilityLabel: "Close relationships",
                    variant: .toolbar
                ) { dismiss() }
            }
            ScrollView {
                VStack(spacing: HideTheme.spacingXS) {
                    ForEach(children) { child in
                        let status = AgentStatusPresentation(
                            demand: child.demand,
                            activity: child.activity,
                            emphasized: child.emphasized,
                            symbol: child.symbol,
                            label: child.statusLabel,
                            connected: connected
                        )
                        Button {
                            inspectedPaneID = child.paneID
                        } label: {
                            HStack(spacing: HideTheme.spacingSM) {
                                AgentStatusMark(symbol: status.symbol, color: status.color)
                                AgentBadge(
                                    agentKind: child.agentKind,
                                    stateColor: status.color,
                                    size: HideTheme.compactAgentBadgeSize
                                )
                                VStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
                                    Text(child.label)
                                        .hideFont(size: HideTheme.Typography.body, weight: .medium)
                                        .lineLimit(2)
                                        .fixedSize(horizontal: false, vertical: true)
                                    Text(child.description(status: status))
                                        .hideFont(size: HideTheme.Typography.caption)
                                        .foregroundStyle(HideTheme.secondary)
                                        .lineLimit(1)
                                }
                                Spacer()
                                if inspectedPaneID == child.paneID {
                                    Image(systemName: "checkmark")
                                }
                            }
                            .padding(HideTheme.spacingSM)
                            .background(
                                inspectedPaneID == child.paneID ? HideTheme.elevated : Color.clear,
                                in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                            )
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(HideInteractiveButtonStyle())
                        .disabled(operation?.isPending == true)
                        .hideTooltip("\(child.label): \(child.description(status: status))")
                    }
                }
            }
            .frame(maxHeight: HideTheme.Layout.relationshipListMaxHeight)
            if let failureReason {
                HStack(alignment: .top, spacing: HideTheme.spacingSM) {
                    Image(systemName: "exclamationmark.triangle.fill")
                    Text(failureReason)
                        .fixedSize(horizontal: false, vertical: true)
                    Spacer(minLength: HideTheme.spacingSM)
                    if failureRetryable {
                        Button("Retry") {
                            guard let inspectedPaneID else { return }
                            requestedPaneID = inspectedPaneID
                            onOpen(inspectedPaneID)
                        }
                    }
                }
                .hideFont(size: HideTheme.Typography.micro)
                .foregroundStyle(HideTheme.warning)
                .accessibilityIdentifier("pane-relationship-failure")
            }
            HStack {
                Spacer()
                Button(isPending ? "Opening…" : "Open") {
                    if let inspectedPaneID {
                        requestedPaneID = inspectedPaneID
                        onOpen(inspectedPaneID)
                    }
                }
                .buttonStyle(HideTextButtonStyle(appearance: .prominent))
                .disabled(inspectedPaneID == nil || operation?.isPending == true)
                .keyboardShortcut(.defaultAction)
            }
        }
        .padding(HideTheme.spacingLG)
        .frame(width: HideTheme.Layout.pullRequestPopoverWidth)
        .foregroundStyle(HideTheme.primary)
        .background(HideTheme.panel)
        .preferredColorScheme(.dark)
        .onChange(of: operation) { oldValue, newValue in
            guard oldValue?.isPending == true,
                  newValue == nil,
                  requestedPaneID != nil
            else { return }
            dismiss()
        }
    }

    private var matchingOperation: PaneSelectionOperation? {
        guard let operation,
              operation.sourcePaneID == sourcePaneID,
              inspectedPaneID == operation.targetPaneID
        else { return nil }
        return operation
    }

    private var isPending: Bool { matchingOperation?.isPending == true }

    private var failureReason: String? {
        guard let matchingOperation,
              case .failed(let reason, _) = matchingOperation.phase
        else { return nil }
        return reason
    }

    private var failureRetryable: Bool {
        guard let matchingOperation,
              case .failed(_, let retryable) = matchingOperation.phase
        else { return false }
        return retryable
    }
}
