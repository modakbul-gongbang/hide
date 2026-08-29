import AppKit
import SwiftUI

enum ShellMetrics {
    static let windowMinWidth: CGFloat = 960
    static let windowMinHeight: CGFloat = 620
    static let windowDefaultWidth: CGFloat = 1_440
    static let windowDefaultHeight: CGFloat = 900

    static let agentsMinWidth: CGFloat = 220
    static let agentsIdealWidth: CGFloat = 270
    static let terminalMinWidth: CGFloat = 420
    static let terminalIdealWidth: CGFloat = 760
    static let workbenchMinWidth: CGFloat = 260
    static let workbenchIdealWidth: CGFloat = 360

    static let panelPadding: CGFloat = 14
    static let compactSpacing: CGFloat = 8
    static let cardRadius: CGFloat = 10
}

struct ShellView: View {
    @EnvironmentObject private var model: ShellModel
    @FocusState private var focusedSurface: ShellSurface?

    var body: some View {
        VStack(spacing: 0) {
            HSplitView {
                AgentsPanel()
                    .frame(
                        minWidth: ShellMetrics.agentsMinWidth,
                        idealWidth: ShellMetrics.agentsIdealWidth
                    )
                    .focusable()
                    .focused($focusedSurface, equals: .agents)
                    .onTapGesture { model.focus(.agents) }
                    .accessibilityIdentifier("agents-panel")

                TerminalPanel()
                    .frame(
                        minWidth: ShellMetrics.terminalMinWidth,
                        idealWidth: ShellMetrics.terminalIdealWidth
                    )
                    .focusable()
                    .focused($focusedSurface, equals: .terminal)
                    .onTapGesture { model.focus(.terminal) }
                    .accessibilityIdentifier("terminal-panel")

                WorkbenchPanel()
                    .frame(
                        minWidth: ShellMetrics.workbenchMinWidth,
                        idealWidth: ShellMetrics.workbenchIdealWidth
                    )
                    .focusable()
                    .focused($focusedSurface, equals: .workbench)
                    .onTapGesture { model.focus(.workbench) }
                    .accessibilityIdentifier("workbench-panel")
            }

            Divider()
            StatusBar()
        }
        .background(Color(nsColor: .windowBackgroundColor))
        .onAppear {
            focusedSurface = model.activeSurface
        }
        .onChange(of: model.activeSurface) { _, surface in
            focusedSurface = surface
        }
        .alert(item: $model.consequenceNotice) { notice in
            Alert(
                title: Text(notice.title),
                message: Text(notice.consequence + affectedSummary(notice.affected)),
                primaryButton: .destructive(Text("Continue"), action: model.confirmConsequencePreview),
                secondaryButton: .cancel(Text("Cancel"), action: model.cancelConsequencePreview)
            )
        }
    }

    private func affectedSummary(_ targets: [DestructiveTarget]) -> String {
        guard !targets.isEmpty else { return "" }
        return "\n\nAffected work:\n" + targets
            .map { "• \($0.label): \($0.summary)" }
            .joined(separator: "\n")
    }
}

private struct AgentsPanel: View {
    @EnvironmentObject private var model: ShellModel

    private var agents: [SidebarAgent] { model.core.snapshot?.navigator.agents ?? [] }

    /// The empty sidebar states the herdr connection status instead of a
    /// generic prompt: a missing socket, an unreachable server, and a
    /// connected-but-empty session are different situations.
    private var emptyAgentsDescription: String {
        guard let herdr = model.core.snapshot?.status.herdr else {
            return "Connect herdr to load workspaces and agent activity."
        }
        switch herdr.state {
        case "connected":
            return "Herdr is connected but no agent panes are running."
        case "unconfigured":
            return "No herdr socket is configured for this launch."
        default:
            return herdr.message ?? "Herdr is not reachable."
        }
    }

    var body: some View {
        VStack(spacing: 0) {
            PanelHeader(
                title: "Agents",
                systemImage: "person.2.fill",
                trailing: "\(agents.count)"
            )

            Divider()

            if agents.isEmpty {
                ContentUnavailableView {
                    Label("No agents yet", systemImage: "rectangle.stack.badge.person.crop")
                } description: {
                    Text(emptyAgentsDescription)
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .padding(ShellMetrics.panelPadding)
            } else {
                ScrollView {
                    LazyVStack(spacing: 6) {
                        ForEach(agents) { agent in
                            AgentRow(agent: agent) {
                                model.core.focusPane(agent.paneID)
                                model.focus(.terminal)
                            }
                        }
                    }
                    .padding(10)
                }
                .accessibilityIdentifier("agent-list")
            }

            Divider()
            RuntimeStatusCards()
            Divider()
            PetStatus()
        }
        .background(Color(nsColor: .controlBackgroundColor))
    }
}

private struct AgentRow: View {
    let agent: SidebarAgent
    let select: () -> Void

    var body: some View {
        Button(action: select) {
            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 7) {
                    Text(agent.symbol)
                        .font(.system(.body, design: .rounded, weight: .bold))
                        .foregroundStyle(stateColor)
                        .frame(width: 16)
                    Text(agent.workspaceLabel)
                        .font(.callout.weight(.semibold))
                        .lineLimit(1)
                    Spacer(minLength: 4)
                    Text(agent.agentKind)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
                HStack(spacing: 6) {
                    Text(agent.summary)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                    Spacer(minLength: 4)
                    Text(agent.elapsed)
                        .font(.caption.monospacedDigit())
                        .foregroundStyle(.tertiary)
                }
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 8)
            .background(stateColor.opacity(0.08), in: RoundedRectangle(cornerRadius: 8))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityLabel("\(agent.state), \(agent.workspaceLabel), \(agent.agentKind), \(agent.summary), \(agent.elapsed)")
        .accessibilityIdentifier("agent-\(agent.id)")
    }

    private var stateColor: Color {
        switch agent.state {
        case "error": .red
        case "question": .yellow
        case "approval": .orange
        case "working": .blue
        case "unseen_completion": .green
        case "idle": .secondary
        default: .gray
        }
    }
}

private struct TerminalPanel: View {
    @EnvironmentObject private var model: ShellModel

    private var attachedPaneDescription: String {
        guard let terminal = model.core.snapshot?.terminal, let paneID = terminal.paneID else {
            return "No pane selected"
        }
        let workspace = model.core.snapshot?.navigator.agents
            .first { $0.paneID == paneID }
            .map { " · \($0.workspaceLabel)" } ?? ""
        let zoom = model.core.snapshot?.zoomed == paneID ? " · zoomed" : ""
        return terminal.closed ? "\(paneID)\(workspace) · closed\(zoom)" : "\(paneID)\(workspace)\(zoom)"
    }

    var body: some View {
        VStack(spacing: 0) {
            PanelHeader(
                title: "Terminal",
                systemImage: "terminal",
                trailing: attachedPaneDescription
            )

            Divider()

            TerminalHost(bridge: model.core)
                .accessibilityLabel("SwiftTerm terminal connected to herdr-core")
        }
        .background(Color(nsColor: .textBackgroundColor))
    }
}

struct PanelHeader: View {
    let title: String
    let systemImage: String
    let trailing: String

    var body: some View {
        HStack(spacing: ShellMetrics.compactSpacing) {
            Label(title, systemImage: systemImage)
                .font(.headline)
            Spacer(minLength: ShellMetrics.compactSpacing)
            Text(trailing)
                .font(.caption)
                .foregroundStyle(.secondary)
                .lineLimit(1)
        }
        .padding(.horizontal, ShellMetrics.panelPadding)
        .frame(height: 44)
    }
}

private struct PetStatus: View {
    @EnvironmentObject private var model: ShellModel

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack(spacing: 10) {
                ZStack {
                    RoundedRectangle(cornerRadius: ShellMetrics.cardRadius)
                        .fill(Color.accentColor.opacity(0.12))
                    Image(systemName: "pawprint.fill")
                        .foregroundStyle(Color.accentColor)
                }
                .frame(width: 34, height: 34)

                VStack(alignment: .leading, spacing: 2) {
                    Text("Pet is quiet")
                        .font(.callout.weight(.medium))
                    Text("The overlay appears while this window is behind.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }
                Spacer(minLength: 0)
            }

            Menu("Safety previews") {
                Button("Close working pane…") { model.previewConsequence(.pane) }
                Button("Close tab…") { model.previewConsequence(.tab) }
                Button("Close workspace…") { model.previewConsequence(.workspace) }
                Button("Remove worktree checkout…") { model.previewConsequence(.worktree) }
            }
            .menuStyle(.borderlessButton)
            .accessibilityIdentifier("safety-preview-menu")

            if let result = model.consequenceResult {
                Text(result)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(ShellMetrics.panelPadding)
        .accessibilityIdentifier("pet-status")
    }
}

private struct RuntimeStatusCards: View {
    @EnvironmentObject private var model: ShellModel

    var body: some View {
        VStack(spacing: 8) {
            BrowserStatusCard()
            RemoteStatusCard()
        }
        .padding(10)
        .background(Color(nsColor: .windowBackgroundColor))
    }
}

private struct BrowserStatusCard: View {
    @EnvironmentObject private var model: ShellModel
    private var receipt: BrowserRuntimeReceipt { model.browser.receipt }

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Label("Chrome", systemImage: "globe")
                    .font(.caption.weight(.semibold))
                Spacer()
                Text(receipt.phase.rawValue)
                    .font(.caption2.monospaced())
                    .foregroundStyle(phaseColor(receipt.phase))
            }
            Text(receipt.currentTitle ?? receipt.message)
                .font(.caption)
                .lineLimit(2)
            if let url = receipt.currentURL {
                Text(url)
                    .font(.caption2.monospaced())
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            HStack {
                Button("Open Browser") { model.browser.openOrFocus() }
                    .disabled(receipt.phase == .loading || model.browser.profile != "default")
                    .accessibilityIdentifier("chromux-open-default")
                Button("Retry") { model.browser.refresh() }
                    .disabled(receipt.phase == .loading)
            }
            .controlSize(.small)
            if receipt.phase == .stale || receipt.phase == .failed || receipt.phase == .unavailable {
                Text(receipt.message + " Last checked: " + receipt.checkedAt)
                    .font(.caption2)
                    .foregroundStyle(.orange)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(9)
        .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 8))
        .accessibilityIdentifier("chromux-status-card")
    }
}

private struct RemoteStatusCard: View {
    @EnvironmentObject private var model: ShellModel

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack {
                Label("mini", systemImage: "externaldrive.connected.to.line.below")
                    .font(.caption.weight(.semibold))
                Spacer()
                Text(model.remote.phase.rawValue)
                    .font(.caption2.monospaced())
                    .foregroundStyle(phaseColor(model.remote.phase))
            }
            if model.remote.phase == .loading {
                ProgressView("Connecting through SSH…")
                    .controlSize(.small)
            } else {
                Text(model.remote.message)
                    .font(.caption)
                    .lineLimit(3)
            }
            if let workspace = model.remote.workspaces.first {
                Label("\(workspace.label) · \(workspace.paneCount) pane(s)", systemImage: "rectangle.3.group")
                    .font(.caption2)
                    .lineLimit(1)
                Text("Remote inline editing is disabled. Use the attached remote terminal to modify files.")
                    .font(.caption2)
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            Button("Refresh mini") { model.remote.refreshMini() }
                .controlSize(.small)
                .disabled(model.remote.phase == .loading)
                .accessibilityIdentifier("remote-mini-refresh")
        }
        .padding(9)
        .background(Color(nsColor: .controlBackgroundColor), in: RoundedRectangle(cornerRadius: 8))
        .accessibilityIdentifier("remote-mini-status-card")
    }
}

private func phaseColor(_ phase: RuntimePhase) -> Color {
    switch phase {
    case .ready: .green
    case .loading: .blue
    case .stale, .unavailable: .orange
    case .failed: .red
    case .idle: .secondary
    }
}

private struct StatusBar: View {
    @EnvironmentObject private var model: ShellModel

    private var statusMessage: String {
        if let error = model.core.bridgeError { return error }
        if model.browser.receipt.phase == .failed || model.browser.receipt.phase == .stale {
            return model.browser.receipt.message
        }
        if let environment = model.core.snapshot?.status.environment.first(where: { $0.key == "SSH_AUTH_SOCK" }),
           environment.state != "available" {
            return environment.message
        }
        return "Local features ready"
    }

    var body: some View {
        HStack(spacing: 14) {
            Label(statusMessage, systemImage: "circle.fill")
                .symbolRenderingMode(.palette)
                .foregroundStyle(.orange, .orange)
            Spacer()
            Text("Chromux \(model.browser.receipt.phase.rawValue)")
            Text("mini \(model.remote.phase.rawValue)")
            Text("Core schema v1")
        }
        .font(.caption)
        .foregroundStyle(.secondary)
        .padding(.horizontal, 12)
        .frame(height: 28)
        .background(Color(nsColor: .controlBackgroundColor))
        .accessibilityIdentifier("status-bar")
    }
}
