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
    }
}

private struct AgentsPanel: View {
    @EnvironmentObject private var model: ShellModel

    private var agents: [SidebarAgent] { model.core.snapshot?.navigator.agents ?? [] }

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
                    Text("Connect herdr to load workspaces and agent activity.")
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

    var body: some View {
        VStack(spacing: 0) {
            PanelHeader(
                title: "Terminal",
                systemImage: "terminal",
                trailing: "No pane selected"
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
    var body: some View {
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
                Text("No agent needs attention")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer(minLength: 0)
        }
        .padding(ShellMetrics.panelPadding)
        .accessibilityIdentifier("pet-status")
    }
}

private struct StatusBar: View {
    @EnvironmentObject private var model: ShellModel

    private var statusMessage: String {
        if let error = model.core.bridgeError { return error }
        if let environment = model.core.snapshot?.status.environment.first,
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
            Text("Chromux runtime parked")
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
