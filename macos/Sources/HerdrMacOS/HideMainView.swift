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
/// Each tab's drawn width, gathered so a drag knows what it is passing over.
/// Tabs are as wide as their labels, so the destination of a drop cannot be
/// worked out from an index alone.
private struct TabWidthPreferenceKey: PreferenceKey {
    static let defaultValue: [String: CGFloat] = [:]

    static func reduce(value: inout [String: CGFloat], nextValue: () -> [String: CGFloat]) {
        value.merge(nextValue(), uniquingKeysWith: { _, next in next })
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
    @State private var tabWidths: [String: CGFloat] = [:]

    /// The traffic lights sit over whichever surface reaches the window's top
    /// left corner. With the sidebar open that is the brand header and the
    /// strip starts after it; with the sidebar collapsed the strip is that
    /// surface and keeps its own first control clear of them.
    private var leadingInset: CGFloat {
        model.leftSidebarVisible ? HideTheme.spacingSM : HideTheme.Layout.trafficLightInset
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
                HStack(spacing: HideTheme.spacingNone) {
                    ScrollViewReader { scroll in
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: HideTheme.spacingNone) {
                            ForEach(model.unifiedTabs) { tab in
                                HStack(spacing: HideTheme.spacingNone) {
                                    Button {
                                        model.focusUnifiedTab(tab)
                                    } label: {
                                        HStack(spacing: HideTheme.spacingSM) {
                                            if let agent = tab.focusedAgent {
                                                let status = AgentStatusPresentation(agent: agent, connected: model.agentsConnected)
                                                AgentStatusMark(symbol: status.symbol, color: status.color)
                                                AgentBadge(agentKind: agent.agentKind, stateColor: status.color, size: HideTheme.lineageChevronWidth)
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
                                                .frame(maxWidth: HideTheme.tabTitleMaxWidth, alignment: .leading)
                                            if let notice = model.tabNotice(for: tab) {
                                                Image(systemName: "exclamationmark.triangle")
                                                    .hideFont(size: HideTheme.Typography.caption, weight: .semibold)
                                                    .foregroundStyle(HideTheme.warning)
                                                    .accessibilityLabel(notice)
                                            }
                                            if tab.dirty {
                                                Circle()
                                                    .fill(HideTheme.secondary)
                                                    .frame(width: 5, height: 5)
                                            }
                                            // The keycap holds its slot whether
                                            // or not it is shown, so revealing
                                            // the hints fades them in without
                                            // resizing the tab under the
                                            // pointer.
                                            if let shortcutNumber = model.tabShortcutNumber(tabID: tab.id) {
                                                HideKeycap(command: .tab(shortcutNumber))
                                                .opacity((model.shortcutHintState.revealed && model.shortcutHintState.modifiers == [.command]) ? 1 : 0)
                                            }
                                        }
                                        .foregroundStyle(tab.active ? HideTheme.primary : HideTheme.secondary)
                                        .padding(.leading, HideTheme.spacingMD)
                                        .padding(.trailing, HideTheme.spacingSM)
                                        .frame(height: HideTheme.Layout.tabStripHeight)
                                        .contentShape(Rectangle())
                                    }
                                    .buttonStyle(HideInteractiveButtonStyle())
                                    // A double-click on the title keeps a
                                    // preview tab; the button's own click
                                    // still focuses it on the first click.
                                    .simultaneousGesture(TapGesture(count: 2).onEnded { model.keepUnifiedTabOpen(tab) })
                                    .accessibilityLabel(EditorTabTitlePresentation.spoken(label: tab.label, preview: tab.preview))
                                    .hideTooltip([tab.contextLabel ?? EditorTabTitlePresentation.spoken(label: tab.label, preview: tab.preview), tab.focusedAgent.map { AgentStatusPresentation(agent: $0, connected: model.agentsConnected).label }].compactMap { $0 }.joined(separator: " · "), command: model.tabShortcutNumber(tabID: tab.id).map(HideCommand.tab), inline: true)

                                    HideIconButton(
                                        systemImage: "xmark",
                                        help: "Close \(tab.label)",
                                        variant: .toolbar,
                                        command: .menu(.closeTab),
                                        tabID: tab.id,
                                        action: { model.closeUnifiedTab(tab) }
                                    )
                                }
                                .padding(.trailing, HideTheme.spacingXS)
                                // A carried tab climbs to the top of the
                                // surface ladder, which is how this system
                                // says "closer" without a drop shadow.
                                .background(
                                    tab.active || draggingTabID == tab.id
                                        ? HideTheme.elevated
                                        : HideTheme.panel
                                )
                                .background(
                                    GeometryReader { proxy in
                                        Color.clear.preference(
                                            key: TabWidthPreferenceKey.self,
                                            value: [tab.id: proxy.size.width]
                                        )
                                    }
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
                                .gesture(tabDragGesture(for: tab))
                            }
                        }
                        .animation(.easeOut(duration: HideTooltipState.fadeDuration(reduceMotion: reduceMotion)), value: (model.shortcutHintState.revealed && model.shortcutHintState.modifiers == [.command]))
                        // SwiftUI hands preference changes to a Sendable
                        // closure, so the hop back to the main actor is what
                        // lets the widths land in view state. It only fires
                        // when a tab's drawn width actually changes.
                        .onPreferenceChange(TabWidthPreferenceKey.self) { widths in
                            Task { @MainActor in tabWidths = widths }
                        }
                    }
                    // The active tab is always in view. With more tabs than
                    // the strip can show, the one the operator just chose
                    // sat past the edge with no indicator that it existed.
                    .onChange(of: model.unifiedTabs.first(where: \.active)?.id, initial: true) { _, activeID in
                        guard let activeID else { return }
                        scroll.scrollTo(activeID)
                    }
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
                // The strip takes the row before the drag area does. Sharing
                // the row equally cut the strip to four tabs while the rest
                // of the row stayed empty.
                .layoutPriority(1)
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

            WindowDragArea()
                .frame(maxWidth: .infinity, maxHeight: .infinity)

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

    /// Carries a tab under the pointer and reports where it was let go.
    ///
    /// The gesture only starts after the activation distance, so a click
    /// still reaches the tab's own button, and a drag that starts on a tab is
    /// always a reorder rather than anything the surface behind it does. The
    /// order is not changed here: the drop is dispatched and the strip
    /// redraws from the next snapshot.
    private func tabDragGesture(for tab: ShellTabItem) -> some Gesture {
        DragGesture(minimumDistance: HideTheme.Layout.tabDragActivationDistance)
            .onChanged { value in
                draggingTabID = tab.id
                dragTranslation = value.translation.width
            }
            .onEnded { value in
                let tabs = model.unifiedTabs
                draggingTabID = nil
                dragTranslation = 0
                guard let from = tabs.firstIndex(where: { $0.id == tab.id }) else { return }
                let destination = TabDragPlacement.destinationIndex(
                    from: from,
                    translation: value.translation.width,
                    widths: tabs.map { tabWidths[$0.id] ?? 0 }
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
