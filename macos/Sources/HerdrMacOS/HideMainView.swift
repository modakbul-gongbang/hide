import AppKit
import SwiftUI

struct HideMainView: View {
    @EnvironmentObject private var model: ShellModel

    var body: some View {
        VStack(spacing: HideTheme.spacingNone) {
            HideTabStrip()
            Rectangle()
                .fill(HideTheme.divider)
                .frame(height: HideTheme.Layout.hairlineWidth)
            ZStack {
                HideTerminalSurface()
                if !model.isRemoteContext,
                   model.core.snapshot?.editor.activeTabID != nil {
                    EditorViewerOverlay()
                }
                if !model.isRemoteContext, model.projectHomeVisible {
                    ProjectHome()
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .layoutPriority(1)
            .background(HideTheme.background)
            HideStatusBar()
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(HideTheme.background)
        .accessibilityIdentifier("hide-main")
    }
}
/// The tab strip, which is the window's first row.
///
/// Nothing sits above it: the system titlebar and the workspace header that
/// used to repeat the sidebar's name, branch, and herdr version are both gone.
/// What the header carried and this strip keeps are the two panel-restore
/// controls, one at each end, each shown only while its panel is hidden. The
/// connection warning and the remote target's state are the status bar's to
/// report, and the herdr version the sidebar's brand header's.
private struct HideTabStrip: View {
    @EnvironmentObject private var model: ShellModel
    @Environment(\.hideAccent) private var accent
    /// What the pointer is carrying right now. This is the only piece of the
    /// strip the shell holds: the order itself belongs to the core, so a drop
    /// is reported rather than applied here.
    @State private var draggingTabID: String?
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var dragTranslation: CGFloat = 0

    /// The traffic lights sit over whichever surface reaches the window's top
    /// left corner. With the sidebar open that is the brand header and the
    /// strip starts after it; with the sidebar collapsed the strip is that
    /// surface and keeps its own first control clear of them.
    private var leadingInset: CGFloat {
        AdaptiveTabStripPresentation.leadingInset(leftSidebarVisible: model.leftSidebarVisible)
    }

    var body: some View {
        HStack(spacing: HideTheme.spacingSM) {
            if !model.leftSidebarVisible {
                HideIconButton(
                    systemImage: "rectangle.leftthird.inset.filled",
                    help: "Show left sidebar",
                    variant: .toolbar,
                    command: .menu(.toggleLeftSidebar),
                    action: model.toggleLeftSidebar
                )
                .accessibilityIdentifier("hide-restore-left-sidebar")
            }

            if model.focusedWorkspace != nil {
                GeometryReader { proxy in
                    let tabs = model.unifiedTabs
                    let presentation = AdaptiveTabStripPresentation(
                        availableWidth: max(0, proxy.size.width - HideTheme.IconButton.toolbarSize.width),
                        tabIDs: tabs.map(\.id),
                        activeTabID: tabs.first(where: \.active)?.id
                    )
                    ZStack(alignment: .leading) {
                        WindowDragArea()
                        HStack(spacing: HideTheme.spacingNone) {
                            ForEach(presentation.slots, id: \.tabIndex) { slot in
                                tabCell(tabs[slot.tabIndex], slot: slot, presentation: presentation)
                            }
                            if presentation.showsOverflow {
                                overflowMenu(tabs: tabs, presentation: presentation)
                            }
                            HideIconButton(
                                systemImage: "plus",
                                help: "New Tab",
                                accessibilityLabel: "New Herdr tab",
                                variant: .toolbar,
                                command: .menu(.newTab),
                                action: model.addTab
                            )
                            .accessibilityIdentifier("hide-new-tab")
                        }
                        .animation(
                            .easeOut(duration: HideTooltipState.fadeDuration(reduceMotion: reduceMotion)),
                            value: model.shortcutHintState.revealed
                                && model.shortcutHintState.modifiers == [.command]
                        )
                    }
                }
                .layoutPriority(1)
            } else {
                WindowDragArea()
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            }

            if model.focusedWorkspace != nil, !model.isRemoteContext {
                HideIconButton(systemImage: "square.grid.2x2", help: "Project Home",
                    variant: .toolbar, isSelected: model.projectHomeVisible,
                    command: .menu(.projectHome), action: model.toggleProjectHome)
                    .accessibilityIdentifier("hide-project-home-toggle")
            }

            if let notice = model.reopenTabNotice ?? model.pendingCloseNotice ?? model.asyncTabNotice {
                HStack(spacing: HideTheme.spacingXS) {
                    if model.core.snapshot?.recentClosed.restoring == true || model.pendingCloseStatusChecking {
                        ProgressView()
                            .controlSize(.small)
                    } else {
                        Image(systemName: "exclamationmark.triangle")
                            .hideFont(size: HideTheme.Typography.caption, weight: .semibold)
                    }
                    Text(notice)
                        .hideFont(size: HideTheme.Typography.caption, weight: .medium)
                        .lineLimit(1)
                    if model.pendingCloseNeedsStatusCheck {
                        Button("Check status", action: model.checkLatestCloseStatus)
                            .buttonStyle(HideTextButtonStyle(appearance: .quiet))
                            .disabled(model.pendingCloseStatusChecking)
                            .accessibilityIdentifier("hide-check-close-status")
                    }
                }
                .foregroundStyle(HideTheme.warning)
                .accessibilityIdentifier("hide-reopen-notice")
            }

            if !model.rightPanelVisible {
                HideIconButton(
                    systemImage: "rectangle.rightthird.inset.filled",
                    help: "Show Right Panel",
                    variant: .toolbar,
                    command: .menu(.toggleRightPanel),
                    action: model.toggleRightPanel
                )
                .accessibilityIdentifier("hide-restore-right-panel")
            }
        }
        .padding(.leading, leadingInset)
        .padding(.trailing, HideTheme.spacingSM)
        .frame(height: HideTheme.Layout.tabStripHeight)
        .background(HideTheme.panel)
        .accessibilityIdentifier("hide-tab-strip")
    }

    private func tabCell(
        _ tab: ShellTabItem,
        slot: AdaptiveTabStripPresentation.Slot,
        presentation: AdaptiveTabStripPresentation
    ) -> some View {
        HStack(spacing: HideTheme.spacingNone) {
            Button {
                model.focusUnifiedTab(tab)
            } label: {
                tabSelectionContent(tab, density: presentation.density)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .contentShape(Rectangle())
            }
            .buttonStyle(HideInteractiveButtonStyle())
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            // A double-click on the title keeps a preview tab; the button's
            // own click still focuses it on the first click.
            .simultaneousGesture(TapGesture(count: 2).onEnded { model.keepUnifiedTabOpen(tab) })
            .accessibilityLabel(tabAccessibilityLabel(tab))
            .hideTooltip(
                tabAccessibilityLabel(tab),
                command: model.tabShortcutNumber(tabID: tab.id).map(HideCommand.tab),
                inline: true
            )

            if tab.active {
                HideIconButton(
                    systemImage: "xmark",
                    help: "Close \(tab.label)",
                    variant: .toolbar,
                    command: .menu(.closeTab),
                    tabID: tab.id,
                    action: { model.closeUnifiedTab(tab) }
                )
            }
        }
        .frame(width: slot.width, height: HideTheme.Layout.tabStripHeight)
        // A carried tab climbs to the top of the surface ladder, which is how
        // this system says "closer" without a drop shadow.
        .background(
            tab.active || draggingTabID == tab.id
                ? HideTheme.elevated
                : HideTheme.panel
        )
        .overlay(alignment: .trailing) {
            Rectangle()
                .fill(HideTheme.divider)
                .frame(width: HideTheme.Layout.hairlineWidth)
        }
        .overlay {
            if draggingTabID == tab.id {
                Rectangle()
                    .strokeBorder(
                        HideTheme.divider,
                        lineWidth: HideTheme.Layout.hairlineWidth
                    )
            }
        }
        .offset(x: draggingTabID == tab.id ? dragTranslation : 0)
        .zIndex(draggingTabID == tab.id ? 1 : 0)
        .accessibilityIdentifier("hide-tab-\(tab.id)")
        .gesture(tabDragGesture(for: tab, presentation: presentation))
    }

    @ViewBuilder
    private func tabSelectionContent(
        _ tab: ShellTabItem,
        density: AdaptiveTabStripPresentation.Density
    ) -> some View {
        switch density {
        case .icon:
            compactTabIdentity(tab, includesAgentStatusMark: true)
                .frame(maxWidth: .infinity, alignment: .leading)
                .foregroundStyle(tab.active ? HideTheme.primary : HideTheme.secondary)
        case .standard, .compressed:
            let compressed = density == .compressed
            HStack(spacing: compressed ? HideTheme.spacingXS : HideTheme.spacingSM) {
                if compressed {
                    compactTabIdentity(tab, includesAgentStatusMark: false)
                } else if let agent = tab.focusedAgent {
                    let status = AgentStatusPresentation(agent: agent, connected: model.agentsConnected)
                    AgentStatusMark(symbol: status.symbol, color: status.color)
                    AgentBadge(
                        agentKind: agent.agentKind,
                        stateColor: status.color,
                        size: HideTheme.lineageChevronWidth
                    )
                } else {
                    Image(systemName: tabIcon(tab))
                        .hideFont(size: HideTheme.Typography.caption, weight: .medium)
                }
                Text(tab.label + model.tabActivity(for: tab))
                    .hideFont(
                        size: HideTheme.Typography.body,
                        weight: tab.active ? .semibold : .medium,
                        italic: EditorTabTitlePresentation.italic(preview: tab.preview)
                    )
                    .lineLimit(1)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .layoutPriority(1)
                if !compressed, let notice = model.tabNotice(for: tab) {
                    Image(systemName: "exclamationmark.triangle")
                        .hideFont(size: HideTheme.Typography.caption, weight: .semibold)
                        .foregroundStyle(HideTheme.warning)
                        .accessibilityHidden(true)
                        .hideTooltip(notice)
                }
                if !compressed, tab.dirty {
                    Circle()
                        .fill(HideTheme.secondary)
                        .frame(
                            width: HideTheme.Layout.tabStatusDotSize,
                            height: HideTheme.Layout.tabStatusDotSize
                        )
                        .accessibilityHidden(true)
                }
                // The keycap holds its slot while its density shows titles,
                // so revealing hints never resizes a tab under the pointer.
                if let shortcutNumber = model.tabShortcutNumber(tabID: tab.id) {
                    HideKeycap(command: .tab(shortcutNumber))
                        .opacity(
                            model.shortcutHintState.revealed
                                && model.shortcutHintState.modifiers == [.command] ? 1 : 0
                        )
                }
            }
            .foregroundStyle(tab.active ? HideTheme.primary : HideTheme.secondary)
            .padding(.leading, compressed ? HideTheme.spacingSM : HideTheme.spacingMD)
            .padding(.trailing, HideTheme.spacingXS)
        }
    }

    private func compactTabIdentity(
        _ tab: ShellTabItem,
        includesAgentStatusMark: Bool
    ) -> some View {
        ZStack {
            HStack(spacing: HideTheme.spacingXXS) {
                if let agent = tab.focusedAgent {
                    let status = AgentStatusPresentation(agent: agent, connected: model.agentsConnected)
                    if includesAgentStatusMark {
                        AgentStatusMark(symbol: status.symbol, color: status.color)
                    }
                    AgentBadge(
                        agentKind: agent.agentKind,
                        stateColor: status.color,
                        size: HideTheme.lineageChevronWidth
                    )
                } else {
                    Image(systemName: compactTabIcon(tab))
                        .hideFont(size: HideTheme.Typography.caption, weight: .semibold)
                        .accessibilityHidden(true)
                }
            }
            if model.tabNotice(for: tab) != nil {
                Image(systemName: "exclamationmark.triangle.fill")
                    .hideFont(size: HideTheme.Typography.micro, weight: .bold)
                    .foregroundStyle(HideTheme.warning)
                    .offset(x: HideTheme.spacingSM, y: -HideTheme.spacingSM)
                    .accessibilityHidden(true)
            }
            if tab.dirty {
                Circle()
                    .fill(HideTheme.secondary)
                    .frame(
                        width: HideTheme.Layout.tabStatusDotSize,
                        height: HideTheme.Layout.tabStatusDotSize
                    )
                    .offset(x: HideTheme.spacingSM, y: HideTheme.spacingSM)
                    .accessibilityHidden(true)
            }
        }
        .frame(
            width: includesAgentStatusMark ? HideTheme.spacingXXXL : HideTheme.spacingXL,
            height: HideTheme.IconButton.toolbarSize.height
        )
    }

    private func overflowMenu(
        tabs: [ShellTabItem],
        presentation: AdaptiveTabStripPresentation
    ) -> some View {
        Menu {
            ForEach(presentation.hiddenIndices, id: \.self) { index in
                let tab = tabs[index]
                Button {
                    model.focusUnifiedTab(tab)
                } label: {
                    Label(tabAccessibilityLabel(tab), systemImage: compactTabIcon(tab))
                }
            }
        } label: {
            Image(systemName: "ellipsis")
                .hideFont(size: HideTheme.Typography.body, weight: .bold)
                .foregroundStyle(HideTheme.secondary)
                .frame(
                    width: HideTheme.Layout.tabOverflowControlWidth,
                    height: HideTheme.Layout.tabStripHeight
                )
                .contentShape(Rectangle())
        }
        .menuStyle(.borderlessButton)
        .menuIndicator(.hidden)
        .fixedSize()
        .hideTooltip("More tabs")
        .accessibilityLabel("More tabs")
        .accessibilityIdentifier("hide-tab-overflow")
    }

    private func tabAccessibilityLabel(_ tab: ShellTabItem) -> String {
        let spoken = EditorTabTitlePresentation.spoken(label: tab.label, preview: tab.preview)
        var details = [tab.contextLabel ?? spoken]
        if let agent = tab.focusedAgent {
            details.append(AgentStatusPresentation(agent: agent, connected: model.agentsConnected).label)
        }
        if tab.dirty {
            details.append("Unsaved changes")
        }
        if let notice = model.tabNotice(for: tab) {
            details.append(notice)
        }
        let activity = model.tabActivity(for: tab).trimmingCharacters(in: .whitespacesAndNewlines)
        if !activity.isEmpty {
            details.append(activity)
        }
        return details.joined(separator: " · ")
    }

    /// Carries a tab under the pointer and reports where it was let go.
    ///
    /// The gesture only starts after the activation distance, so a click
    /// still reaches the tab's own button, and a drag that starts on a tab is
    /// always a reorder rather than anything the surface behind it does. The
    /// order is not changed here: the drop is dispatched and the strip
    /// redraws from the next snapshot.
    private func tabDragGesture(
        for tab: ShellTabItem,
        presentation: AdaptiveTabStripPresentation
    ) -> some Gesture {
        DragGesture(minimumDistance: HideTheme.Layout.tabDragActivationDistance)
            .onChanged { value in
                draggingTabID = tab.id
                dragTranslation = value.translation.width
            }
            .onEnded { value in
                let tabs = model.unifiedTabs
                draggingTabID = nil
                dragTranslation = 0
                guard presentation.tabIDs == tabs.map(\.id) else { return }
                guard let from = tabs.firstIndex(where: { $0.id == tab.id }) else { return }
                let destination = presentation.destinationIndex(
                    from: from,
                    translation: value.translation.width
                )
                model.reorderUnifiedTab(tab, to: destination)
            }
    }

    private func tabIcon(_ tab: ShellTabItem) -> String {
        switch tab.kind {
        case .herdr: "rectangle.split.2x1"
        case .editor(let tab): tab.kind == .diff ? "doc.text.magnifyingglass" : "doc.text"
        }
    }

    private func compactTabIcon(_ tab: ShellTabItem) -> String {
        guard tab.preview else { return tabIcon(tab) }
        switch tab.kind {
        case .herdr: return tabIcon(tab)
        case .editor(let editor):
            return editor.kind == .diff ? "doc.text.magnifyingglass" : "doc.text.fill"
        }
    }
}

struct HideStatusBar: View {
    @EnvironmentObject private var model: ShellModel

    var body: some View {
        HStack(spacing: HideTheme.spacingMD) {
            Circle()
                .fill(model.herdrIsConnected ? HideTheme.success : HideTheme.warning)
                .frame(width: 6, height: 6)
            Text(model.isRemoteContext
                ? model.remote.statusMessage
                : HerdrStatusPresentation.localMessage(
                    startupDiagnostic: model.core.startupDiagnostic,
                    bridgeError: model.core.bridgeError,
                    state: model.core.snapshot?.status.herdr.state,
                    providerMessage: model.core.snapshot?.status.herdr.message
                ))
                .lineLimit(1)
            Spacer()
            Text("\(model.agents.count) agents")
            Text("•")
                .foregroundStyle(HideTheme.muted)
            Text("hide")
        }
        .hideFont(size: HideTheme.Typography.caption, weight: .medium)
        .foregroundStyle(HideTheme.secondary)
        .padding(.horizontal, HideTheme.spacingLG)
        .frame(height: 27)
        .background(HideTheme.panel)
        .accessibilityIdentifier("hide-status-bar")
    }
}
