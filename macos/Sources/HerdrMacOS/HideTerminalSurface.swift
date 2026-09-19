import AppKit
import SwiftUI

struct HideTerminalSurface: View {
    @EnvironmentObject private var model: ShellModel

    private var panes: [CorePaneSnapshot] {
        model.focusedPanes
    }

    var body: some View {
        VStack(spacing: HideTheme.spacingNone) {
            if let notice = model.paneProjectionNotice {
                PaneProjectionUnavailableState(notice: notice)
            } else if panes.isEmpty {
                HideEmptyCheckoutState()
            } else if model.isRemoteContext {
                HideTabCanvas(
                    items: model.remotePaneGridItems,
                    dividers: [],
                    isZoomed: false
                )
                .id(model.focusedTab?.stableID ?? "no-herdr-tab")
            } else {
                // Every tab the operator has opened keeps its canvas, and
                // with it the terminal views holding its scrollback. Only
                // which one is on top changes, so a switch neither rebuilds a
                // view nor reports a new size to Herdr.
                ZStack {
                    ForEach(model.retainedTabCanvases) { canvas in
                        HideTabCanvas(
                            items: canvas.items,
                            dividers: canvas.dividers,
                            isZoomed: canvas.isZoomed
                        )
                        .id(canvas.tabID)
                        .opacity(canvas.isVisible ? 1 : 0)
                        // Opacity alone leaves AppKit drawing every hidden
                        // terminal at full cost whenever it is fed; the
                        // terminal host hides its NSView on this value.
                        .environment(\.hideCanvasVisible, canvas.isVisible)
                        .allowsHitTesting(canvas.isVisible)
                        .accessibilityHidden(!canvas.isVisible)
                        .zIndex(canvas.isVisible ? 1 : 0)
                    }
                }
                .transaction { transaction in
                    transaction.animation = nil
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(HideTheme.background)
        .overlay(alignment: .top) {
            if let operation = model.paneSelectionOperation {
                PaneSelectionOutcomeNotice(
                    operation: operation,
                    onRetry: model.retryPaneSelection
                )
                .padding(HideTheme.spacingSM)
            }
        }
        .accessibilityIdentifier("hide-terminal-surface")
    }
}

/// Keeps a relationship Open/Return outcome visible after the core moves the
/// selected pane and the source header is no longer on the visible canvas.
/// It overlays the canvas, so pending and failure feedback do not resize the
/// terminal or mutate Herdr-owned geometry (PRD B23, B24).
struct PaneSelectionOutcomeNotice: View {
    let operation: PaneSelectionOperation
    let onRetry: () -> Void

    var body: some View {
        HStack(alignment: .top, spacing: HideTheme.spacingSM) {
            switch operation.phase {
            case .pending:
                ProgressView().controlSize(.small)
                Text("Opening \(operation.targetLabel)…")
                    .hideFont(size: HideTheme.Typography.caption, weight: .semibold)
            case .failed(let reason, let retryable):
                Image(systemName: "exclamationmark.triangle.fill")
                    .foregroundStyle(HideTheme.warning)
                VStack(alignment: .leading, spacing: HideTheme.spacingXXS) {
                    Text("Could not open \(operation.targetLabel)")
                        .hideFont(size: HideTheme.Typography.caption, weight: .semibold)
                    Text(reason)
                        .hideFont(size: HideTheme.Typography.micro)
                        .foregroundStyle(HideTheme.warning)
                        .fixedSize(horizontal: false, vertical: true)
                }
                if retryable {
                    Button("Retry", action: onRetry)
                        .buttonStyle(HideInteractiveButtonStyle())
                }
            }
        }
        .padding(.horizontal, HideTheme.spacingMD)
        .padding(.vertical, HideTheme.spacingSM)
        .foregroundStyle(HideTheme.primary)
        .background(
            HideTheme.elevated,
            in: RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
        )
        .overlay(
            RoundedRectangle(cornerRadius: HideTheme.radiusSmall)
                .stroke(HideTheme.divider, lineWidth: HideTheme.Layout.hairlineWidth)
        )
        .accessibilityElement(children: .contain)
        .accessibilityIdentifier("pane-selection-outcome")
    }
}

/// One tab's panes on the canvas. Kept separate from the surface so every
/// retained tab builds the same way and only its visibility differs.
private struct HideTabCanvas: View {
    @EnvironmentObject private var model: ShellModel
    let items: [PaneGridItem]
    let dividers: [PaneGridDivider]
    let isZoomed: Bool

    var body: some View {
        PaneLayoutCanvas(
            items: items,
            dividers: dividers,
            onResize: model.resizePane
        ) { item in
            if let pane = model.paneMetadata(for: item.paneID) {
                switch pane.content {
                case .browser(let binding):
                    BrowserPaneView(
                        pane: pane, binding: binding,
                        isFocused: item.isFocused,
                        isKeyboardFocused: item.isFocused && model.activeSurface == .terminal,
                        isZoomed: isZoomed,
                        activity: model.paneActivity(for: pane.id),
                        notice: model.paneNotice(for: pane.id),
                        onFocus: { model.focusPane(pane.id) },
                        onToggleZoom: { model.togglePaneZoom(pane.id) },
                        onClose: { model.closePaneFromHeader(pane.id) }
                    )
                case .unavailable(let reason):
                    HideTerminalPaneCard(
                        paneID: pane.id, kind: "unavailable", title: pane.herdrLabel ?? "Pane unavailable",
                        status: "ready", isFocused: item.isFocused,
                        isKeyboardFocused: item.isFocused && model.activeSurface == .terminal,
                        isZoomed: isZoomed,
                        activity: model.paneActivity(for: pane.id),
                        notice: model.paneNotice(for: pane.id),
                        onFocus: { model.focusPane(pane.id) },
                        onClose: { model.closePaneFromHeader(pane.id) },
                        onToggleZoom: { model.togglePaneZoom(pane.id) }
                    ) {
                        HideEmptyState("Pane unavailable", systemImage: "exclamationmark.triangle", description: Text(reason))
                    }
                case .terminal:
                    PaneTerminalCell(
                        pane: pane,
                        lineagePath: model.resolvedLineagePath(for: pane),
                        agent: model.agents.first { $0.paneID == pane.id },
                        status: model.paneStatus(for: pane.id),
                        statusMessage: model.paneTransportMessage(for: pane.id),
                        isFocused: item.isFocused,
                        isKeyboardFocused: item.isFocused && model.activeSurface == .terminal,
                        isZoomed: isZoomed,
                        isConversation: model.isConversation(for: pane.id),
                        canShowConversation: model.canShowConversation(for: pane.id),
                        showsFork: model.canForkPane(pane),
                        activity: model.paneActivity(for: pane.id),
                        notice: model.paneNotice(for: pane.id),
                        connected: model.agentsConnected,
                        paneSelectionOperation: model.paneSelectionOperation,
                        onFocus: { model.focusPane(pane.id) },
                        onReconnect: { model.reconnectPane(pane.id) },
                        onClose: { model.closePaneFromHeader(pane.id) },
                        onToggleZoom: { model.togglePaneZoom(pane.id) },
                        onToggleConversation: { model.toggleConversation(pane.id) },
                        onFork: { model.forkPaneFromHeader(pane.id) },
                        onOpenPort: { model.openPanePort($0) },
                        // A child chip, a breadcrumb step and a sibling are
                        // the same intent: show that pane instead of this one.
                        // The core moves the visible tab to whichever tab
                        // holds it, so the screen is replaced rather than
                        // split (PRD B7, D-16).
                        onSelectPane: { model.requestPaneSelection(from: pane.id, to: $0) }
                    ) {
                        if model.isConversation(for: pane.id),
                           model.canShowConversation(for: pane.id),
                           let agent = model.agents.first(where: { $0.paneID == pane.id }),
                           let provider = ConversationProvider(agentKind: agent.agentKind)
                        {
                            ConversationPaneView(
                                provider: provider,
                                sessionID: model.conversationSessionID(for: pane.id),
                                cwd: pane.cwd,
                                agent: agent,
                                textScale: model.textScale(for: pane.id),
                                isKeyboardFocused: item.isFocused && model.activeSurface == .terminal,
                                onShowTerminal: { model.toggleConversation(pane.id) },
                                openLink: { model.openTerminalLink($0, paneID: pane.id) }
                            ) {
                                TerminalHost(
                                    bridge: model.core,
                                    paneID: pane.id,
                                    textScale: model.textScale(for: pane.id),
                                    onFocus: { model.focusPane(pane.id) },
                                    onOpenLink: { model.openTerminalLink($0, paneID: pane.id) },
                                    allowsInput: ConversationInputPolicy.terminalInputAllowed(isConversation: true)
                                )
                                .accessibilityLabel("SwiftTerm terminal for \(pane.id)")
                            }
                        } else {
                            TerminalHost(
                                bridge: model.core,
                                paneID: pane.id,
                                textScale: model.textScale(for: pane.id),
                                onFocus: { model.focusPane(pane.id) },
                                onOpenLink: { model.openTerminalLink($0, paneID: pane.id) }
                            )
                            .accessibilityLabel("SwiftTerm terminal for \(pane.id)")
                        }
                    }
                }
            } else {
                MissingTerminalPaneCell(paneID: item.paneID)
            }
        }
        .clipped()
        .transaction { transaction in
            transaction.animation = nil
        }
    }
}

private struct PaneProjectionUnavailableState: View {
    let notice: String
    @Environment(\.hideAccent) private var accent

    var body: some View {
        HideEmptyState(emphasis: accent) {
            Label("Pane layout unavailable", systemImage: "exclamationmark.triangle")
        } description: {
            Text(notice)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(HideTheme.background)
        .accessibilityIdentifier("pane-layout-unavailable")
    }
}

private struct MissingTerminalPaneCell: View {
    let paneID: String

    var body: some View {
        HideEmptyState(emphasis: HideTheme.danger) {
            Label("Terminal pane unavailable", systemImage: "exclamationmark.triangle")
        } description: {
            Text("Hide received layout for \(paneID) without matching pane metadata.")
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(HideTheme.panel)
        .accessibilityIdentifier("missing-terminal-pane-\(paneID)")
    }
}

private struct HideEmptyCheckoutState: View {
    @EnvironmentObject private var model: ShellModel
    @Environment(\.hideAccent) private var accent

    var body: some View {
        if model.isRemoteContext {
            VStack(spacing: 13) {
                Image(systemName: model.remote.phase == .loading ? "arrow.triangle.2.circlepath" : "externaldrive.connected.to.line.below")
                    .hideFont(size: HideTheme.Typography.display, weight: .light)
                    .foregroundStyle(accent.opacity(HideTheme.Opacity.secondary))
                Text(model.remote.phase == .loading ? "Connecting to \(model.remote.targetLabel)" : "Remote context")
                    .hideFont(size: HideTheme.Typography.headline, weight: .semibold)
                    .foregroundStyle(HideTheme.primary)
                Text(model.remote.statusMessage)
                    .hideFont(size: HideTheme.Typography.subhead)
                    .foregroundStyle(HideTheme.secondary)
                    .multilineTextAlignment(.center)
                    .frame(maxWidth: 420)
                if model.remote.phase != .loading {
                    Button("Retry mini") { model.retryRemote() }
                        .buttonStyle(HideTextButtonStyle(appearance: .prominent))
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding(HideTheme.spacingXXXL)
        } else if ProjectHomeEntryPolicy.drawsHome(
            startState: model.checkoutStartState,
            hasCheckout: model.focusedCheckout != nil,
            projectionNotice: model.localProjectionNotice
        ) {
            // A checkout with no tab used to say "No terminal open" and
            // nothing else. Project Home takes that state: the whole project
            // at a glance, with Start new terminal still in its header.
            ProjectHome()
        } else {
            VStack(spacing: 13) {
                Image(systemName: model.localProjectionNotice != nil ? "exclamationmark.triangle" : (model.focusedCheckout == nil ? "square.stack.3d.up" : "terminal"))
                    .hideFont(size: HideTheme.Typography.display, weight: .light)
                    .foregroundStyle(accent.opacity(HideTheme.Opacity.secondary))
                if let notice = model.localProjectionNotice {
                    Text("Waiting for selected checkout")
                        .hideFont(size: HideTheme.Typography.headline, weight: .semibold)
                        .foregroundStyle(HideTheme.primary)
                    Text(notice)
                        .hideFont(size: HideTheme.Typography.subhead)
                        .foregroundStyle(HideTheme.secondary)
                        .multilineTextAlignment(.center)
                        .frame(maxWidth: 420)
                } else if model.focusedCheckout == nil {
                    Text("Start with a workspace")
                        .hideFont(size: HideTheme.Typography.headline, weight: .semibold)
                        .foregroundStyle(HideTheme.primary)
                    Text("Register a local folder, then choose a checkout from the sidebar.")
                        .hideFont(size: HideTheme.Typography.subhead)
                        .foregroundStyle(HideTheme.secondary)
                        .multilineTextAlignment(.center)
                        .frame(maxWidth: 360)
                    Button("New Workspace") { model.openNewWorkspace() }
                        .buttonStyle(HideTextButtonStyle(appearance: .prominent))
                        .accessibilityIdentifier("hide-empty-state-new-workspace")
                } else {
                    // `.idle` never reaches here: `ProjectHomeEntryPolicy`
                    // hands that state to Project Home above.
                    if case .starting = model.checkoutStartState {
                        Text("Starting terminal")
                            .hideFont(size: HideTheme.Typography.headline, weight: .semibold)
                            .foregroundStyle(HideTheme.primary)
                        Text("Opening a new Herdr tab and terminal pane at this checkout.")
                            .hideFont(size: HideTheme.Typography.subhead)
                            .foregroundStyle(HideTheme.secondary)
                            .multilineTextAlignment(.center)
                            .frame(maxWidth: 360)
                        ProgressView()
                            .controlSize(.small)
                    } else if case .started = model.checkoutStartState {
                        Text("Terminal is starting")
                            .hideFont(size: HideTheme.Typography.headline, weight: .semibold)
                            .foregroundStyle(HideTheme.primary)
                        Text("Waiting for Herdr to attach the new pane to this checkout.")
                            .hideFont(size: HideTheme.Typography.subhead)
                            .foregroundStyle(HideTheme.secondary)
                            .multilineTextAlignment(.center)
                            .frame(maxWidth: 360)
                    } else if case let .failed(message) = model.checkoutStartState {
                        Text("Couldn't start terminal")
                            .hideFont(size: HideTheme.Typography.headline, weight: .semibold)
                            .foregroundStyle(HideTheme.primary)
                        Text(message)
                            .hideFont(size: HideTheme.Typography.subhead)
                            .foregroundStyle(HideTheme.secondary)
                            .multilineTextAlignment(.center)
                            .frame(maxWidth: 420)
                        Button("Retry terminal") {
                            if let checkout = model.focusedCheckout {
                                model.selectCheckout(checkout)
                            }
                        }
                            .buttonStyle(HideTextButtonStyle(appearance: .prominent))
                    }
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding(HideTheme.spacingXXXL)
        }
    }
}
