import AppKit
import Foundation
import SwiftUI

enum HideTheme {
    static let background = Color(red: 0.035, green: 0.043, blue: 0.055)
    static let sidebar = Color(red: 0.055, green: 0.063, blue: 0.078)
    static let panel = Color(red: 0.070, green: 0.080, blue: 0.098)
    static let elevated = Color(red: 0.105, green: 0.118, blue: 0.142)
    static let divider = Color.white.opacity(0.09)
    static let primary = Color.white.opacity(0.92)
    static let secondary = Color.white.opacity(0.52)
    static let muted = Color.white.opacity(0.32)
    static let accent = Color(red: 0.725, green: 1.0, blue: 0.40)
    static let danger = Color(red: 1.0, green: 0.35, blue: 0.36)
    static let warning = Color(red: 1.0, green: 0.72, blue: 0.28)
    static let success = Color(red: 0.37, green: 0.90, blue: 0.62)

    enum Layout {
        static let hairlineWidth: CGFloat = 1
        static let panelCollapseControlSize: CGFloat = 18
        static let sidebarMinWidth: CGFloat = 220
        static let sidebarIdealWidth: CGFloat = 292
        static let sidebarMaxWidth: CGFloat = 440
        static let terminalMinWidth: CGFloat = 540
        static let terminalIdealWidth: CGFloat = 760
        static let workbenchMinWidth: CGFloat = 260
        static let workbenchIdealWidth: CGFloat = 355
        static let workbenchMaxWidth: CGFloat = 560
    }

    static func color(for hex: String) -> Color {
        let value = hex.trimmingCharacters(in: CharacterSet.alphanumerics.inverted)
        guard value.count == 6, let number = UInt64(value, radix: 16) else { return accent }
        return Color(
            red: Double((number >> 16) & 0xff) / 255,
            green: Double((number >> 8) & 0xff) / 255,
            blue: Double(number & 0xff) / 255
        )
    }
}

private struct HideAccentKey: EnvironmentKey {
    static let defaultValue = HideTheme.accent
}

private struct HideFontScaleKey: EnvironmentKey {
    static let defaultValue = CGFloat(1)
}

private extension EnvironmentValues {
    var hideAccent: Color {
        get { self[HideAccentKey.self] }
        set { self[HideAccentKey.self] = newValue }
    }

    var hideFontScale: CGFloat {
        get { self[HideFontScaleKey.self] }
        set { self[HideFontScaleKey.self] = newValue }
    }
}

private struct HideScaledFontModifier: ViewModifier {
    let size: CGFloat
    let weight: Font.Weight
    let design: Font.Design
    @Environment(\.hideFontScale) private var scale

    func body(content: Content) -> some View {
        content.font(.system(size: size * scale, weight: weight, design: design))
    }
}

extension View {
    func hideFont(
        size: CGFloat,
        weight: Font.Weight = .regular,
        design: Font.Design = .default
    ) -> some View {
        modifier(HideScaledFontModifier(size: size, weight: weight, design: design))
    }
}

struct ShellView: View {
    @EnvironmentObject private var model: ShellModel

    var body: some View {
        ZStack {
            HSplitView {
                if model.leftSidebarVisible {
                    HideSidebar()
                        .frame(
                            minWidth: HideTheme.Layout.sidebarMinWidth,
                            idealWidth: HideTheme.Layout.sidebarIdealWidth,
                            maxWidth: HideTheme.Layout.sidebarMaxWidth
                        )
                        .frame(maxHeight: .infinity, alignment: .topLeading)
                }
                HideMainView()
                    .frame(
                        minWidth: HideTheme.Layout.terminalMinWidth,
                        idealWidth: HideTheme.Layout.terminalIdealWidth,
                        maxWidth: .infinity,
                        maxHeight: .infinity,
                        alignment: .topLeading
                    )
                if model.rightWorkbenchVisible {
                    WorkbenchPanel()
                        .frame(
                            minWidth: HideTheme.Layout.workbenchMinWidth,
                            idealWidth: HideTheme.Layout.workbenchIdealWidth,
                            maxWidth: HideTheme.Layout.workbenchMaxWidth,
                            maxHeight: .infinity
                        )
                        .accessibilityIdentifier("workbench-panel")
                }
            }
            if let cycle = model.agentSwitcherCycle {
                AgentSwitcherOverlay(cycle: cycle, agents: model.agents)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .background(HideTheme.background)
        .preferredColorScheme(.dark)
        .environment(\.colorScheme, .dark)
        .environment(\.hideAccent, HideTheme.color(for: model.core.snapshot?.uiState.accentHex ?? "#B9FF66"))
        .environment(\.hideFontScale, CGFloat((model.core.snapshot?.uiState.fontSize ?? 13) / 13))
        .tint(HideTheme.color(for: model.core.snapshot?.uiState.accentHex ?? "#B9FF66"))
        .sheet(isPresented: $model.showNewWorkspace) {
            NewWorkspaceSheet()
                .environmentObject(model)
        }
        .sheet(isPresented: $model.showNewAgent) {
            NewAgentSheet()
                .environmentObject(model)
        }
        .sheet(isPresented: $model.showSearch) {
            HideSearchSheet()
                .environmentObject(model)
        }
        .sheet(isPresented: $model.showSettings) {
            HideSettingsView(model: model)
        }
        .sheet(isPresented: $model.showPetDashboard) {
            PetDashboardView()
                .environmentObject(model)
        }
        .alert(item: $model.workspaceToRemove) { workspace in
            Alert(
                title: Text("Remove \(workspace.label) from Hide?"),
                message: Text("Hide will remove only its registration. The folder, repository, worktrees, and running processes stay untouched."),
                primaryButton: .destructive(Text("Remove registration"), action: model.confirmRemoveWorkspace),
                secondaryButton: .cancel()
            )
        }
        .alert(
            "Hide",
            isPresented: Binding(
                get: { model.interactionNotice != nil },
                set: { if !$0 { model.clearInteractionNotice() } }
            )
        ) {
            Button("OK", action: model.clearInteractionNotice)
        } message: {
            Text(model.interactionNotice ?? "")
        }
    }
}

private struct AgentSwitcherOverlay: View {
    let cycle: AgentSwitcherCycle
    let agents: [SidebarAgent]

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("RECENT AGENTS")
                .hideFont(size: 10, weight: .bold)
                .foregroundStyle(HideTheme.muted)
            ForEach(cycle.paneIDs, id: \.self) { paneID in
                if let agent = agents.first(where: { $0.paneID == paneID }) {
                    HStack(spacing: 10) {
                        if let mark = AgentMark.image(for: agent.agentKind) {
                            Image(nsImage: mark)
                                .resizable()
                                .aspectRatio(contentMode: .fit)
                                .frame(width: 19, height: 19)
                        }
                        VStack(alignment: .leading, spacing: 2) {
                            Text(agent.summary)
                                .hideFont(size: 12, weight: .semibold)
                            Text("\(agent.workspaceLabel) · \(paneID)")
                                .hideFont(size: 9, design: .monospaced)
                                .foregroundStyle(HideTheme.secondary)
                        }
                        Spacer()
                    }
                    .padding(.horizontal, 10)
                    .frame(height: 44)
                    .background(
                        paneID == cycle.selectedPaneID ? HideTheme.accent.opacity(0.16) : Color.clear,
                        in: RoundedRectangle(cornerRadius: 7)
                    )
                }
            }
        }
        .padding(12)
        .frame(width: 360)
        .background(HideTheme.elevated, in: RoundedRectangle(cornerRadius: 10))
        .overlay {
            RoundedRectangle(cornerRadius: 10).stroke(HideTheme.divider)
        }
        .shadow(color: .black.opacity(0.45), radius: 22, y: 12)
        .accessibilityIdentifier("agent-mru-switcher")
    }
}

private struct PetDashboardView: View {
    @EnvironmentObject private var model: ShellModel
    @Environment(\.dismiss) private var dismiss

    private var projection: PetDashboardProjection { model.petDashboard }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 10) {
                Image(systemName: "pawprint.fill")
                    .foregroundStyle(HideTheme.accent)
                VStack(alignment: .leading, spacing: 2) {
                    Text("Agent dashboard")
                        .hideFont(size: 17, weight: .bold)
                    Text("Live state from the current Herdr snapshot")
                        .hideFont(size: 10)
                        .foregroundStyle(HideTheme.secondary)
                }
                Spacer()
                Text(projection.connection)
                    .hideFont(size: 10, weight: .semibold, design: .monospaced)
                    .foregroundStyle(projection.connection == "connected" ? HideTheme.success : HideTheme.warning)
                Button("Done", action: { dismiss() })
                    .keyboardShortcut(.cancelAction)
            }
            .padding(18)

            HStack(spacing: 8) {
                PetCountTile(label: "TOTAL", value: projection.counts.total, color: HideTheme.primary)
                PetCountTile(label: "WORKING", value: projection.counts.working, color: HideTheme.accent)
                PetCountTile(label: "DONE", value: projection.counts.done, color: HideTheme.success)
                PetCountTile(label: "IDLE", value: projection.counts.idle, color: HideTheme.secondary)
                PetCountTile(label: "ERROR", value: projection.counts.error, color: HideTheme.danger)
                PetCountTile(label: "DISCONNECTED", value: projection.counts.disconnected, color: HideTheme.warning)
            }
            .padding(.horizontal, 18)
            .padding(.bottom, 14)

            if projection.connection != "connected" {
                HStack(alignment: .top, spacing: 8) {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(HideTheme.warning)
                    Text(projection.connectionMessage ?? "Herdr is disconnected. Rows show the last known agents as disconnected.")
                        .hideFont(size: 10)
                        .foregroundStyle(HideTheme.secondary)
                }
                .padding(10)
                .frame(maxWidth: .infinity, alignment: .leading)
                .background(HideTheme.warning.opacity(0.08), in: RoundedRectangle(cornerRadius: 7))
                .padding(.horizontal, 18)
                .padding(.bottom, 10)
            }

            Divider().overlay(HideTheme.divider)

            if projection.groups.isEmpty {
                ContentUnavailableView(
                    "No agents",
                    systemImage: "sparkles",
                    description: Text("Start a Claude or Codex agent to see its live status here.")
                )
                .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else {
                ScrollView {
                    LazyVStack(alignment: .leading, spacing: 12) {
                        ForEach(projection.groups) { group in
                            VStack(alignment: .leading, spacing: 4) {
                                HStack {
                                    Text(group.label.uppercased())
                                        .hideFont(size: 10, weight: .bold)
                                        .tracking(1)
                                        .foregroundStyle(HideTheme.muted)
                                    Spacer()
                                    Text("\(group.agents.count)")
                                        .hideFont(size: 10, design: .monospaced)
                                        .foregroundStyle(HideTheme.muted)
                                }
                                ForEach(group.agents) { agent in
                                    PetDashboardAgentRow(agent: agent) {
                                        dismiss()
                                        model.selectAgent(paneID: agent.paneID)
                                    }
                                }
                            }
                        }
                    }
                    .padding(18)
                }
            }
        }
        .frame(width: 760, height: 560)
        .background(HideTheme.panel)
        .preferredColorScheme(.dark)
        .accessibilityIdentifier("pet-agent-dashboard")
    }
}

private struct PetCountTile: View {
    let label: String
    let value: Int
    let color: Color

    var body: some View {
        VStack(alignment: .leading, spacing: 3) {
            Text(label)
                .hideFont(size: 8, weight: .bold)
                .tracking(0.7)
                .foregroundStyle(HideTheme.muted)
            Text("\(value)")
                .hideFont(size: 18, weight: .bold, design: .rounded)
                .foregroundStyle(color)
        }
        .padding(.horizontal, 10)
        .frame(maxWidth: .infinity, minHeight: 54, alignment: .leading)
        .background(HideTheme.elevated, in: RoundedRectangle(cornerRadius: 7))
    }
}

private struct PetDashboardAgentRow: View {
    let agent: PetDashboardRow
    let action: () -> Void

    private var statusColor: Color {
        switch agent.status {
        case "working": HideTheme.accent
        case "done", "unseen_completion": HideTheme.success
        case "error": HideTheme.danger
        case "question", "approval": HideTheme.warning
        case "disconnected": HideTheme.warning
        default: HideTheme.secondary
        }
    }

    var body: some View {
        Button(action: action) {
            HStack(spacing: 11) {
                AgentBadge(agentKind: agent.agentKind, stateColor: statusColor, size: 19)
                VStack(alignment: .leading, spacing: 3) {
                    Text(agent.summary)
                        .hideFont(size: 12, weight: .semibold)
                        .foregroundStyle(HideTheme.primary)
                        .lineLimit(1)
                    HStack(spacing: 7) {
                        Text(agent.agentKind.capitalized)
                        Text(agent.paneID)
                        Text(agent.elapsed)
                    }
                    .hideFont(size: 9, design: .monospaced)
                    .foregroundStyle(HideTheme.muted)
                }
                Spacer(minLength: 8)
                if let ambient = agent.ambient {
                    VStack(alignment: .trailing, spacing: 2) {
                        Text("sub \(ambient.subagentsActive) · bg \(ambient.backgroundRunning)")
                        if ambient.backgroundFailed > 0 {
                            Text("failed \(ambient.backgroundFailed)")
                                .foregroundStyle(HideTheme.danger)
                        }
                    }
                    .hideFont(size: 8, design: .monospaced)
                    .foregroundStyle(HideTheme.secondary)
                }
                if agent.unseen {
                    Text("UNSEEN")
                        .hideFont(size: 8, weight: .bold)
                        .foregroundStyle(HideTheme.warning)
                }
                VStack(alignment: .trailing, spacing: 2) {
                    Text(agent.status.uppercased())
                        .hideFont(size: 9, weight: .bold)
                        .foregroundStyle(statusColor)
                    Text(agent.connection)
                        .hideFont(size: 8, design: .monospaced)
                        .foregroundStyle(HideTheme.muted)
                }
            }
            .padding(.horizontal, 11)
            .frame(minHeight: 56)
            .contentShape(Rectangle())
            .background(HideTheme.elevated.opacity(0.75), in: RoundedRectangle(cornerRadius: 7))
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("pet-dashboard-agent-\(agent.paneID)")
    }
}

private struct HideSidebar: View {
    @EnvironmentObject private var model: ShellModel

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HideBrandHeader()
            VStack(spacing: 5) {
                HideActionButton(title: "New Workspace", systemImage: "plus.square", shortcut: "⌘⇧N") {
                    model.openNewWorkspace()
                }
                HideActionButton(title: "New Agent", systemImage: "sparkles", shortcut: "⌘N") {
                    model.openNewAgent()
                }
                HideActionButton(title: "Search", systemImage: "magnifyingglass", shortcut: "⌘K") {
                    model.openSearch()
                }
            }
            .padding(.horizontal, 14)
            .padding(.bottom, 16)

            Rectangle()
                .fill(HideTheme.divider)
                .frame(height: 1)

            ScrollView {
                VStack(alignment: .leading, spacing: 0) {
                    // What is blocked comes before where things live, because
                    // it is the only part the user has to act on.
                    let waiting = model.agentsNeedingAttention
                    if !waiting.isEmpty {
                        HideSectionLabel(title: "NEEDS YOU", count: waiting.count)
                        ForEach(waiting) { agent in
                            AgentNavigatorRow(agent: agent, showsWorkspace: true)
                        }
                    }

                    HideSectionLabel(title: "WORKSPACES", count: model.workspaces.count)
                    if model.workspaces.isEmpty {
                        EmptySidebarRow(
                            systemImage: "square.stack.3d.up",
                            title: "No workspaces yet",
                            detail: "Add a folder to create your first workspace."
                        )
                    } else {
                        ForEach(model.workspaces) { workspace in
                            WorkspaceNavigatorRow(workspace: workspace)
                        }
                    }
                }
                .padding(.bottom, 14)
            }

            Rectangle()
                .fill(HideTheme.divider)
                .frame(height: 1)

            VStack(alignment: .leading, spacing: 4) {
                HideSectionLabel(title: "DEVICES", count: nil)
                ForEach(model.devices, id: \.id) { device in
                    DeviceNavigatorRow(device: device)
                }
                Button {
                    model.showSettings = true
                } label: {
                    Label("Settings", systemImage: "gearshape")
                        .hideFont(size: 12, weight: .medium)
                        .foregroundStyle(HideTheme.secondary)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .padding(.horizontal, 12)
                        .padding(.vertical, 8)
                }
                .buttonStyle(.plain)
            }
            .padding(.horizontal, 10)
            .padding(.top, 9)
            .padding(.bottom, 10)
        }
        .background(HideTheme.sidebar)
        .overlay(alignment: .trailing) {
            Rectangle()
                .fill(HideTheme.divider)
                .frame(width: 1)
        }
        .accessibilityIdentifier("hide-sidebar")
    }
}

private struct HideBrandHeader: View {
    @EnvironmentObject private var model: ShellModel

    var body: some View {
        HStack(spacing: 9) {
            Text("hide")
                .hideFont(size: 22, weight: .bold, design: .rounded)
                .tracking(-0.8)
                .foregroundStyle(HideTheme.primary)
            Circle()
                .fill(model.herdrIsConnected ? HideTheme.success : HideTheme.warning)
                .frame(width: 7, height: 7)
                .shadow(color: (model.herdrIsConnected ? HideTheme.success : HideTheme.warning).opacity(0.7), radius: 5)
            Spacer()
            Text(model.isRemoteContext ? model.remote.targetLabel : (model.core.runtimeSelection?.version ?? "offline"))
                .hideFont(size: 10, weight: .medium, design: .monospaced)
                .foregroundStyle(HideTheme.muted)
            Button {
                model.toggleLeftSidebar()
            } label: {
                Image(systemName: "sidebar.left")
                    .frame(
                        width: HideTheme.Layout.panelCollapseControlSize,
                        height: HideTheme.Layout.panelCollapseControlSize
                    )
            }
            .buttonStyle(.plain)
            .foregroundStyle(HideTheme.secondary)
            .help("Hide left sidebar (⌘B)")
            .accessibilityLabel("Hide left sidebar")
            .accessibilityIdentifier("hide-toggle-left-sidebar")
        }
        .padding(.horizontal, 18)
        .padding(.top, 19)
        .padding(.bottom, 18)
        .accessibilityIdentifier("hide-brand")
    }
}

private struct HideActionButton: View {
    let title: String
    let systemImage: String
    let shortcut: String
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 9) {
                Image(systemName: systemImage)
                    .hideFont(size: 12, weight: .semibold)
                    .frame(width: 16)
                Text(title)
                    .hideFont(size: 12, weight: .medium)
                Spacer()
                Text(shortcut)
                    .hideFont(size: 10, design: .monospaced)
                    .foregroundStyle(HideTheme.muted)
            }
            .foregroundStyle(HideTheme.primary)
            .padding(.horizontal, 11)
            .padding(.vertical, 9)
            .background(HideTheme.elevated.opacity(0.75), in: RoundedRectangle(cornerRadius: 7))
        }
        .buttonStyle(.plain)
    }
}

private struct HideSectionLabel: View {
    let title: String
    let count: Int?

    var body: some View {
        HStack(spacing: 7) {
            Text(title)
                .hideFont(size: 10, weight: .bold)
                .tracking(1.2)
                .foregroundStyle(HideTheme.muted)
            if let count {
                Text("\(count)")
                    .hideFont(size: 10, weight: .medium, design: .monospaced)
                    .foregroundStyle(HideTheme.muted)
            }
            Spacer()
        }
        .padding(.horizontal, 17)
        .padding(.top, 17)
        .padding(.bottom, 7)
    }
}

private struct EmptySidebarRow: View {
    let systemImage: String
    let title: String
    let detail: String

    var body: some View {
        HStack(alignment: .top, spacing: 9) {
            Image(systemName: systemImage)
                .foregroundStyle(HideTheme.muted)
                .frame(width: 16)
            VStack(alignment: .leading, spacing: 3) {
                Text(title)
                    .hideFont(size: 12, weight: .medium)
                    .foregroundStyle(HideTheme.secondary)
                Text(detail)
                    .hideFont(size: 10)
                    .foregroundStyle(HideTheme.muted)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .padding(.horizontal, 17)
        .padding(.vertical, 10)
    }
}

private struct WorkspaceNavigatorRow: View {
    @EnvironmentObject private var model: ShellModel
    @Environment(\.hideAccent) private var accent
    let workspace: CoreWorkspaceSnapshot

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            HStack(spacing: 7) {
                Button {
                    model.toggleWorkspace(workspace)
                } label: {
                    Image(systemName: workspace.expanded ? "chevron.down" : "chevron.right")
                        .hideFont(size: 9, weight: .bold)
                        .foregroundStyle(HideTheme.muted)
                        .frame(width: 12, height: 20)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityLabel(workspace.expanded ? "Collapse \(workspace.label)" : "Expand \(workspace.label)")
                .accessibilityIdentifier("hide-workspace-disclosure-\(workspace.id)")
                Image(systemName: workspace.isGit ? "folder.badge.gearshape" : "folder")
                    .hideFont(size: 12, weight: .semibold)
                    .foregroundStyle(workspace.temporary ? HideTheme.warning : accent)
                    .frame(width: 16)
                Text(workspace.label)
                    .hideFont(size: 12, weight: .semibold)
                    .foregroundStyle(HideTheme.primary)
                    .lineLimit(1)
                Spacer(minLength: 0)
                Menu {
                    Button("New Agent") {
                        model.openNewAgent(checkoutID: workspace.checkouts.first?.id)
                    }
                    Divider()
                    Button("Remove registration", role: .destructive) {
                        model.requestRemoveWorkspace(workspace)
                    }
                } label: {
                    Image(systemName: "ellipsis")
                        .hideFont(size: 11, weight: .bold)
                        .foregroundStyle(HideTheme.muted)
                        .frame(width: 22, height: 22)
                        .contentShape(Rectangle())
                }
                .menuStyle(.borderlessButton)
            }
            .padding(.horizontal, 15)
            .padding(.top, 7)
            .padding(.bottom, 2)

            if workspace.expanded {
                ForEach(workspace.checkouts) { checkout in
                    CheckoutNavigatorRow(
                        workspace: workspace,
                        checkout: checkout,
                        isFocused: model.focusedCheckout?.id == checkout.id
                    )
                    // The agents running on this branch, under the branch. Seeing
                    // what a workspace is doing is the reason to open it.
                    ForEach(model.agents(in: checkout)) { agent in
                        AgentNavigatorRow(agent: agent, showsWorkspace: false)
                    }
                }
            }
        }
        .padding(.bottom, 4)
    }
}

private struct CheckoutNavigatorRow: View {
    @EnvironmentObject private var model: ShellModel
    @Environment(\.hideAccent) private var accent
    let workspace: CoreWorkspaceSnapshot
    let checkout: CoreCheckoutSnapshot
    let isFocused: Bool

    /// Panes, not tabs: a checkout row exists because panes are in it, and a
    /// pane count is what tells the user how much is running there.
    private var paneSummary: String {
        let panes = checkout.tabs.reduce(0) { $0 + $1.panes.count }
        switch panes {
        case 0: return "no panes"
        case 1: return "1 pane"
        default: return "\(panes) panes"
        }
    }

    var body: some View {
        Button {
            model.selectCheckout(checkout)
        } label: {
            HStack(spacing: 7) {
                Image(systemName: checkout.isWorktree ? "arrow.triangle.branch" : "rectangle.stack")
                    .hideFont(size: 10, weight: .semibold)
                    .foregroundStyle(isFocused ? accent : HideTheme.secondary)
                    .frame(width: 16)
                VStack(alignment: .leading, spacing: 1) {
                    Text(checkout.label)
                        .hideFont(size: 11, weight: isFocused ? .semibold : .regular)
                        .foregroundStyle(isFocused ? HideTheme.primary : HideTheme.secondary)
                        .lineLimit(1)
                    Text(paneSummary)
                        .hideFont(size: 9, design: .monospaced)
                        .foregroundStyle(HideTheme.muted)
                        .lineLimit(1)
                }
                Spacer(minLength: 0)
                if isFocused {
                    Circle()
                        .fill(accent)
                        .frame(width: 5, height: 5)
                }
            }
            .padding(.leading, 37)
            .padding(.trailing, 14)
            .padding(.vertical, 6)
            .background(isFocused ? accent.opacity(0.10) : .clear, in: RoundedRectangle(cornerRadius: 6))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("hide-checkout-\(checkout.id)")
        .accessibilityLabel("\(workspace.repoName), \(checkout.label)")
        .accessibilityValue(isFocused ? "Selected" : "Not selected")
        .contextMenu {
            Button("Start agent here") { model.openNewAgent(checkoutID: checkout.id) }
        }
    }
}

private struct AgentNavigatorRow: View {
    @EnvironmentObject private var model: ShellModel
    @Environment(\.hideAccent) private var accent
    let agent: SidebarAgent
    /// Under a space the workspace name is the heading above the row, so
    /// repeating it wastes the line the summary needs.
    let showsWorkspace: Bool

    private var stateColor: Color {
        switch agent.state {
        case "working": accent
        case "question", "approval": HideTheme.warning
        case "error": HideTheme.danger
        case "done", "unseen_completion": HideTheme.success
        default: HideTheme.secondary
        }
    }

    private var isFocused: Bool {
        model.focusedPaneID == agent.paneID
    }

    var body: some View {
        Button { model.selectAgent(agent) } label: {
            HStack(alignment: .top, spacing: 8) {
                AgentBadge(
                    agentKind: agent.agentKind,
                    stateColor: stateColor,
                    size: showsWorkspace ? 19 : 16
                )
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 5) {
                        Text(showsWorkspace ? agent.workspaceLabel : agent.summary)
                            .hideFont(size: 11, weight: showsWorkspace ? .semibold : .regular)
                            .foregroundStyle(showsWorkspace ? HideTheme.primary : HideTheme.secondary)
                            .lineLimit(1)
                        Spacer(minLength: 0)
                        Text(agent.elapsed)
                            .hideFont(size: 9, design: .monospaced)
                            .foregroundStyle(HideTheme.muted)
                    }
                    if showsWorkspace {
                        Text(agent.summary)
                            .hideFont(size: 10)
                            .foregroundStyle(HideTheme.secondary)
                            .lineLimit(2)
                        Text(agent.state.replacingOccurrences(of: "_", with: " "))
                            .hideFont(size: 9, weight: .medium)
                            .foregroundStyle(stateColor)
                    }
                }
            }
            .padding(.leading, showsWorkspace ? 15 : 52)
            .padding(.trailing, 15)
            .padding(.vertical, showsWorkspace ? 7 : 4)
            .background(isFocused ? accent.opacity(0.12) : .clear, in: RoundedRectangle(cornerRadius: 6))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .accessibilityIdentifier("hide-agent-\(agent.id)")
        .accessibilityLabel("\(agent.workspaceLabel), \(agent.agentKind), \(agent.state)")
        .accessibilityValue(isFocused ? "Selected" : "Not selected")
    }
}

private struct DeviceNavigatorRow: View {
    @EnvironmentObject private var model: ShellModel
    @Environment(\.hideAccent) private var accent
    let device: CoreDeviceSnapshot

    private var isSelected: Bool { model.selectedDeviceID == device.id }

    var body: some View {
        Button {
            model.selectDevice(device)
        } label: {
            HStack(spacing: 8) {
                Circle()
                    .fill(device.state == "ready" || device.state == "available" ? HideTheme.success : HideTheme.warning)
                    .frame(width: 6, height: 6)
                Text(device.label)
                    .hideFont(size: 11, weight: isSelected ? .semibold : .medium)
                    .foregroundStyle(isSelected ? HideTheme.primary : HideTheme.secondary)
                Spacer()
                if device.agentCount > 0 {
                    Text("\(device.agentCount)")
                        .hideFont(size: 9, design: .monospaced)
                        .foregroundStyle(HideTheme.muted)
                }
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 7)
            .background(isSelected ? accent.opacity(0.10) : .clear, in: RoundedRectangle(cornerRadius: 6))
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        // Stable row identifiers keep repeated sidebar labels independently
        // targetable by assistive technology and verification automation.
        .accessibilityIdentifier("hide-device-\(device.id)")
        .accessibilityValue(isSelected ? "Selected" : "Not selected")
    }
}

private struct HideMainView: View {
    @EnvironmentObject private var model: ShellModel

    var body: some View {
        VStack(spacing: 0) {
            HideTerminalHeader()
            Rectangle()
                .fill(HideTheme.divider)
                .frame(height: 1)
            ZStack {
                HideTerminalSurface()
                if !model.isRemoteContext,
                   model.core.snapshot?.editor.viewerVisible == true {
                    WorkbenchViewerOverlay()
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

private struct HideTerminalHeader: View {
    @EnvironmentObject private var model: ShellModel
    @Environment(\.hideAccent) private var accent

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                if !model.leftSidebarVisible {
                    Button {
                        model.toggleLeftSidebar()
                    } label: {
                        Image(systemName: "rectangle.leftthird.inset.filled")
                    }
                    .buttonStyle(HideToolbarButtonStyle(isProminent: false))
                    .help("Show left sidebar (⌘B)")
                    .accessibilityLabel("Show left sidebar")
                    .accessibilityIdentifier("hide-restore-left-sidebar")
                }
                Image(systemName: "rectangle.3.group")
                    .foregroundStyle(accent)
                VStack(alignment: .leading, spacing: 1) {
                    // The same name the sidebar uses, so the header names the
                    // space the user clicked rather than a directory.
                    Text(model.focusedWorkspace?.label ?? (model.isRemoteContext ? model.remote.targetLabel : "No workspace"))
                        .hideFont(size: 13, weight: .semibold)
                        .foregroundStyle(HideTheme.primary)
                    Text(model.focusedCheckout.map { checkout in
                        checkout.branch.map { "\(checkout.label)  ·  \($0)" } ?? checkout.path
                    } ?? (model.isRemoteContext ? model.remote.message : "Register a workspace to begin"))
                        .hideFont(size: 10, design: .monospaced)
                        .foregroundStyle(HideTheme.secondary)
                        .lineLimit(1)
                }
                Spacer()
                if model.isRemoteContext {
                    Label("mini \(model.remote.phase.rawValue)", systemImage: "externaldrive.connected.to.line.below")
                        .hideFont(size: 10, weight: .medium)
                        .foregroundStyle(model.herdrIsConnected ? HideTheme.success : HideTheme.warning)
                } else if let selection = model.core.runtimeSelection {
                    Label("herdr \(selection.version)", systemImage: "bolt.horizontal.circle")
                        .hideFont(size: 10, weight: .medium)
                        .foregroundStyle(HideTheme.secondary)
                } else {
                    Label("herdr unavailable", systemImage: "bolt.horizontal.circle")
                        .hideFont(size: 10, weight: .medium)
                        .foregroundStyle(HideTheme.warning)
                }
                if !model.rightWorkbenchVisible {
                    Button {
                        model.toggleRightWorkbench()
                    } label: {
                        Image(systemName: "rectangle.rightthird.inset.filled")
                    }
                    .buttonStyle(HideToolbarButtonStyle(isProminent: false))
                    .help("Show Workbench (⌘⌥B)")
                    .accessibilityLabel("Show Workbench")
                    .accessibilityIdentifier("hide-restore-right-workbench")
                }
            }
            .padding(.horizontal, 18)
            .frame(height: 54)

            if !model.focusedTabs.isEmpty {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 3) {
                        ForEach(model.focusedTabs, id: \.stableID) { tab in
                            Button {
                                model.focusTab(tab)
                            } label: {
                                HStack(spacing: 6) {
                                    Circle()
                                        .fill(tab.empty ? HideTheme.muted : accent)
                                        .frame(width: 5, height: 5)
                                    Text(tab.label ?? "Tab")
                                        .hideFont(size: 10, weight: .medium)
                                        .foregroundStyle(HideTheme.secondary)
                                }
                                .padding(.horizontal, 10)
                                .padding(.vertical, 6)
                                .background(HideTheme.elevated.opacity(0.5), in: RoundedRectangle(cornerRadius: 5))
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(.horizontal, 18)
                    .padding(.bottom, 8)
                }
            }
        }
        .background(HideTheme.panel)
    }
}

private struct HideToolbarButtonStyle: ButtonStyle {
    let isProminent: Bool
    @Environment(\.hideAccent) private var accent

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .hideFont(size: 11, weight: .semibold)
            .foregroundStyle(isProminent ? HideTheme.background : HideTheme.primary)
            .padding(.horizontal, isProminent ? 10 : 8)
            .padding(.vertical, 7)
            .background(isProminent ? accent : HideTheme.elevated, in: RoundedRectangle(cornerRadius: 6))
            .opacity(configuration.isPressed ? 0.72 : 1)
    }
}

private struct HideTerminalSurface: View {
    @EnvironmentObject private var model: ShellModel

    private var panes: [CorePaneSnapshot] {
        model.focusedPanes
    }

    var body: some View {
        VStack(spacing: 0) {
            if let notice = model.paneProjectionNotice {
                PaneProjectionUnavailableState(notice: notice)
            } else if panes.isEmpty {
                HideEmptyCheckoutState()
            } else {
                PaneLayoutCanvas(items: model.focusedPaneGridItems) { item in
                    if let pane = model.paneMetadata(for: item.paneID) {
                        PaneTerminalCell(
                            pane: pane,
                            status: model.paneStatus(for: pane.id),
                            statusMessage: model.paneTransportMessage(for: pane.id),
                            isFocused: item.isFocused,
                            onFocus: { model.focusPane(pane.id) },
                            onReconnect: { model.reconnectPane(pane.id) }
                        ) {
                            if model.isRemoteContext {
                                RemoteTerminalHost(
                                    remote: model.remote,
                                    sshAlias: model.remote.sshAlias,
                                    paneID: pane.id,
                                    onFocus: { model.focusPane(pane.id) },
                                    onOpenLink: { model.openTerminalLink($0, paneID: pane.id) }
                                )
                                .accessibilityLabel("Remote SwiftTerm terminal for \(pane.id)")
                            } else {
                                TerminalHost(
                                    bridge: model.core,
                                    paneID: pane.id,
                                    onOpenLink: { model.openTerminalLink($0, paneID: pane.id) }
                                )
                                    .accessibilityLabel("SwiftTerm terminal for \(pane.id)")
                            }
                        }
                    } else {
                        MissingTerminalPaneCell(paneID: item.paneID)
                    }
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(HideTheme.background)
        .accessibilityIdentifier("hide-terminal-surface")
    }
}

private struct PaneProjectionUnavailableState: View {
    let notice: String
    @Environment(\.hideAccent) private var accent

    var body: some View {
        ContentUnavailableView {
            Label("Pane layout unavailable", systemImage: "exclamationmark.triangle")
        } description: {
            Text(notice)
        }
        .foregroundStyle(accent)
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(HideTheme.background)
        .accessibilityIdentifier("pane-layout-unavailable")
    }
}

private struct MissingTerminalPaneCell: View {
    let paneID: String

    var body: some View {
        ContentUnavailableView {
            Label("Terminal pane unavailable", systemImage: "exclamationmark.triangle")
        } description: {
            Text("Hide received layout for \(paneID) without matching pane metadata.")
        }
        .foregroundStyle(HideTheme.danger)
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
                    .hideFont(size: 30, weight: .light)
                    .foregroundStyle(accent.opacity(0.75))
                Text(model.remote.phase == .loading ? "Connecting to \(model.remote.targetLabel)" : "Remote context")
                    .hideFont(size: 17, weight: .semibold)
                    .foregroundStyle(HideTheme.primary)
                Text(model.remote.statusMessage)
                    .hideFont(size: 12)
                    .foregroundStyle(HideTheme.secondary)
                    .multilineTextAlignment(.center)
                    .frame(maxWidth: 420)
                if model.remote.phase != .loading {
                    Button("Retry mini") { model.retryRemote() }
                        .buttonStyle(HideToolbarButtonStyle(isProminent: true))
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding(40)
        } else {
            VStack(spacing: 13) {
                Image(systemName: model.localProjectionNotice != nil ? "exclamationmark.triangle" : (model.focusedCheckout == nil ? "square.stack.3d.up" : "terminal"))
                    .hideFont(size: 30, weight: .light)
                    .foregroundStyle(accent.opacity(0.75))
                if let notice = model.localProjectionNotice {
                    Text("Waiting for selected checkout")
                        .hideFont(size: 17, weight: .semibold)
                        .foregroundStyle(HideTheme.primary)
                    Text(notice)
                        .hideFont(size: 12)
                        .foregroundStyle(HideTheme.secondary)
                        .multilineTextAlignment(.center)
                        .frame(maxWidth: 420)
                } else if model.focusedCheckout == nil {
                    Text("Start with a workspace")
                        .hideFont(size: 17, weight: .semibold)
                        .foregroundStyle(HideTheme.primary)
                    Text("Register a local folder, then choose a checkout from the sidebar.")
                        .hideFont(size: 12)
                        .foregroundStyle(HideTheme.secondary)
                        .multilineTextAlignment(.center)
                        .frame(maxWidth: 360)
                    Button("New Workspace") { model.openNewWorkspace() }
                        .buttonStyle(HideToolbarButtonStyle(isProminent: true))
                } else {
                    switch model.checkoutStartState {
                    case .starting:
                        Text("Starting terminal")
                            .hideFont(size: 17, weight: .semibold)
                            .foregroundStyle(HideTheme.primary)
                        Text("Opening a new Herdr tab and terminal pane at this checkout.")
                            .hideFont(size: 12)
                            .foregroundStyle(HideTheme.secondary)
                            .multilineTextAlignment(.center)
                            .frame(maxWidth: 360)
                        ProgressView()
                            .controlSize(.small)
                    case .started:
                        Text("Terminal is starting")
                            .hideFont(size: 17, weight: .semibold)
                            .foregroundStyle(HideTheme.primary)
                        Text("Waiting for Herdr to attach the new pane to this checkout.")
                            .hideFont(size: 12)
                            .foregroundStyle(HideTheme.secondary)
                            .multilineTextAlignment(.center)
                            .frame(maxWidth: 360)
                    case let .failed(message):
                        Text("Couldn't start terminal")
                            .hideFont(size: 17, weight: .semibold)
                            .foregroundStyle(HideTheme.primary)
                        Text(message)
                            .hideFont(size: 12)
                            .foregroundStyle(HideTheme.secondary)
                            .multilineTextAlignment(.center)
                            .frame(maxWidth: 420)
                        Button("Retry terminal") {
                            if let checkout = model.focusedCheckout {
                                model.selectCheckout(checkout)
                            }
                        }
                            .buttonStyle(HideToolbarButtonStyle(isProminent: true))
                    case .idle:
                        Text("Preparing terminal")
                            .hideFont(size: 17, weight: .semibold)
                            .foregroundStyle(HideTheme.primary)
                        Text("Hide will start a new Herdr tab and terminal pane at this checkout.")
                            .hideFont(size: 12)
                            .foregroundStyle(HideTheme.secondary)
                            .multilineTextAlignment(.center)
                            .frame(maxWidth: 360)
                    }
                }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding(40)
        }
    }
}

private struct HideStatusBar: View {
    @EnvironmentObject private var model: ShellModel

    var body: some View {
        HStack(spacing: 12) {
            Circle()
                .fill(model.herdrIsConnected ? HideTheme.success : HideTheme.warning)
                .frame(width: 6, height: 6)
            Text(model.isRemoteContext
                ? model.remote.statusMessage
                : (model.core.bridgeError ?? model.core.snapshot?.status.herdr.message ?? "Waiting for Herdr"))
                .lineLimit(1)
            Spacer()
            Text("\(model.agents.count) agents")
            Text("•")
                .foregroundStyle(HideTheme.muted)
            Text("hide")
        }
        .hideFont(size: 10, weight: .medium)
        .foregroundStyle(HideTheme.secondary)
        .padding(.horizontal, 14)
        .frame(height: 27)
        .background(HideTheme.panel)
        .accessibilityIdentifier("hide-status-bar")
    }
}

private struct NewWorkspaceSheet: View {
    @EnvironmentObject private var model: ShellModel
    @Environment(\.dismiss) private var dismiss
    @State private var label = ""
    @State private var initializeGit = true

    private var selectedURL: URL? {
        model.pendingWorkspaceURL
    }

    private var isExistingDirectory: Bool {
        guard let selectedURL else { return false }
        var isDirectory: ObjCBool = false
        return FileManager.default.fileExists(atPath: selectedURL.path, isDirectory: &isDirectory) && isDirectory.boolValue
    }

    private var hasGitMetadata: Bool {
        guard let selectedURL else { return false }
        return FileManager.default.fileExists(atPath: selectedURL.appendingPathComponent(".git").path)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            SheetHeader(title: "New Workspace", subtitle: "Register a folder without moving or copying files.")
            Form {
                Section("Folder") {
                    Text(selectedURL?.path ?? "No folder selected")
                        .hideFont(size: 11, design: .monospaced)
                        .foregroundStyle(HideTheme.secondary)
                        .textSelection(.enabled)
                    TextField("Display name", text: $label, prompt: Text(selectedURL?.lastPathComponent ?? "Project"))
                        .textFieldStyle(.roundedBorder)
                }
                Section("Git") {
                    Toggle("Initialize Git when this folder is not a repository", isOn: $initializeGit)
                        .disabled(hasGitMetadata)
                    Text(hasGitMetadata
                        ? "Git metadata already exists. Hide will only discover it."
                        : "Default is on, but git init runs only after you press Add workspace.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            .formStyle(.grouped)
            .scrollContentBackground(.hidden)
            HStack {
                Spacer()
                Button("Cancel") {
                    model.cancelNewWorkspaceConfirmation()
                    dismiss()
                }
                Button("Add workspace") {
                    guard let selectedURL else { return }
                    model.addWorkspace(
                        path: selectedURL,
                        label: label.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                            ? (selectedURL.lastPathComponent.isEmpty ? "Workspace" : selectedURL.lastPathComponent)
                            : label,
                        initializeGit: initializeGit && !hasGitMetadata
                    )
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
                .disabled(!isExistingDirectory)
            }
            .padding(18)
        }
        .frame(width: 570, height: 390)
        .background(HideTheme.panel)
        .preferredColorScheme(.dark)
        .onAppear {
            if label.isEmpty {
                label = selectedURL?.lastPathComponent ?? "Workspace"
            }
        }
        .onDisappear {
            model.cancelNewWorkspaceConfirmation()
        }
    }
}

private struct NewAgentSheet: View {
    @EnvironmentObject private var model: ShellModel
    @Environment(\.dismiss) private var dismiss
    @State private var draft = NewAgentDraft.empty

    private var checkouts: [(workspace: CoreWorkspaceSnapshot, checkout: CoreCheckoutSnapshot)] {
        model.workspaces.flatMap { workspace in
            workspace.checkouts.map { (workspace: workspace, checkout: $0) }
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            SheetHeader(title: "New Agent", subtitle: "Run the selected CLI inside a checkout through Herdr.")
            Form {
                Section("Device") {
                    Picker("Run on", selection: $draft.selectedDeviceID) {
                        ForEach(model.devices) { device in
                            Text(device.label).tag(device.id)
                        }
                    }
                }
                Section("Agent") {
                    HStack(spacing: 9) {
                        ForEach(NewAgentProvider.allCases, id: \.rawValue) { provider in
                            AgentChoiceTile(
                                kind: provider.rawValue,
                                selected: draft.selectedKind == provider.rawValue
                            ) {
                                draft.selectedKind = provider.rawValue
                            }
                        }
                    }
                    if !AgentCLIAvailability.isUsable(draft.selectedKind) {
                        VStack(alignment: .leading, spacing: 5) {
                            Label("\(draft.selectedKind) is not on the login-shell PATH. Install it, then reopen this dialog.", systemImage: "exclamationmark.triangle")
                                .font(.caption)
                                .foregroundStyle(HideTheme.warning)
                            Link("Install \(draft.selectedKind)", destination: draft.selectedKind == "claude"
                                ? URL(string: "https://docs.anthropic.com/en/docs/claude-code/overview")!
                                : URL(string: "https://developers.openai.com/codex/")!)
                                .font(.caption)
                        }
                    }
                }
                Section("Workspace") {
                    Picker("Workspace / checkout", selection: $draft.selectedCheckoutID) {
                        Text("Choose a checkout").tag("")
                        ForEach(checkouts, id: \.checkout.id) { item in
                            Text("\(item.workspace.repoName) / \(item.checkout.label)").tag(item.checkout.id)
                        }
                    }
                }
                Section("Options") {
                    Toggle("Pass the CLI bypass flag", isOn: $draft.bypassWarnings)
                    if draft.bypassWarnings {
                        Label("This passes a provider-specific bypass flag to \(draft.selectedKind). Review its consequences before starting.", systemImage: "exclamationmark.triangle.fill")
                            .font(.caption)
                            .foregroundStyle(HideTheme.warning)
                    } else {
                        Text("Off by default. Hide never hides this choice behind a global setting.")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
            }
            .formStyle(.grouped)
            .scrollContentBackground(.hidden)
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }
                Button("Start agent") {
                    let bypassWarnings = draft.consumeBypassWarnings()
                    model.selectedAgentKind = draft.selectedKind
                    model.selectedAgentCheckoutID = draft.selectedCheckoutID.isEmpty ? nil : draft.selectedCheckoutID
                    model.selectedAgentDeviceID = draft.selectedDeviceID
                    model.startAgent(bypassWarnings: bypassWarnings)
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
                .disabled(draft.selectedCheckoutID.isEmpty || !AgentCLIAvailability.isUsable(draft.selectedKind))
            }
            .padding(18)
        }
        .frame(width: 590, height: 620)
        .background(HideTheme.panel)
        .preferredColorScheme(.dark)
        .onAppear {
            draft = NewAgentDraft.fresh(
                selectedKind: model.selectedAgentKind,
                selectedCheckoutID: model.selectedAgentCheckoutID,
                focusedCheckoutID: model.focusedCheckout?.id,
                selectedDeviceID: model.selectedAgentDeviceID
            )
        }
    }
}

private struct AgentChoiceTile: View {
    @Environment(\.hideAccent) private var accent
    let kind: String
    let selected: Bool
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            VStack(spacing: 4) {
                if let mark = AgentMark.image(for: kind) {
                    Image(nsImage: mark)
                        .resizable()
                        .interpolation(.high)
                        .aspectRatio(contentMode: .fit)
                        .frame(width: 24, height: 24)
                }
                Text(kind.capitalized)
                    .hideFont(size: 11, weight: .semibold)
                Text(AgentCLIAvailability.isUsable(kind) ? "installed" : "not found")
                    .hideFont(size: 9, design: .monospaced)
                    .foregroundStyle(AgentCLIAvailability.isUsable(kind) ? HideTheme.success : HideTheme.warning)
            }
            .frame(maxWidth: .infinity)
            .padding(.vertical, 12)
            .foregroundStyle(selected ? HideTheme.background : HideTheme.primary)
            .background(selected ? accent : HideTheme.elevated, in: RoundedRectangle(cornerRadius: 8))
        }
        .buttonStyle(.plain)
    }
}

private struct HideSearchSheet: View {
    @EnvironmentObject private var model: ShellModel
    @Environment(\.dismiss) private var dismiss
    @Environment(\.hideAccent) private var accent
    @State private var query = ""

    private var agentGroups: [HideSearchAgentGroup] {
        HideSearchPresentation.agentGroups(
            workspaces: model.workspaces,
            agents: model.agents,
            query: query
        )
    }

    private var checkoutEntries: [HideSearchEntry] {
        let entries = model.workspaces.flatMap { workspace in
            workspace.checkouts.map { checkout in
                HideSearchEntry(
                    id: "checkout-\(checkout.id)",
                    title: "\(workspace.repoName) / \(checkout.label)",
                    subtitle: checkout.path,
                    kind: .checkout(workspace, checkout)
                )
            }
        }
        return HideSearchEntry.filtered(entries, query: query)
    }

    private var entries: [HideSearchEntry] {
        agentGroups.flatMap(\.entries) + checkoutEntries
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 9) {
                Image(systemName: "magnifyingglass")
                    .foregroundStyle(accent)
                TextField("Search agents and workspaces", text: $query)
                    .textFieldStyle(.plain)
                    .hideFont(size: 16)
                    .onSubmit { if let first = entries.first { route(first) } }
                Text("ESC")
                    .hideFont(size: 10, design: .monospaced)
                    .foregroundStyle(HideTheme.muted)
            }
            .padding(14)
            .background(HideTheme.elevated, in: RoundedRectangle(cornerRadius: 9))
            .padding(14)
            ScrollView {
                LazyVStack(spacing: 2) {
                    ForEach(agentGroups) { group in
                        Text("\(group.workspace) > AGENTS")
                            .hideFont(size: 10, weight: .bold)
                            .foregroundStyle(HideTheme.muted)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(.horizontal, 12)
                            .padding(.top, 8)
                        ForEach(group.entries) { entry in
                            searchButton(entry)
                        }
                    }
                    if !checkoutEntries.isEmpty {
                        Text("WORKSPACES > CHECKOUTS")
                            .hideFont(size: 10, weight: .bold)
                            .foregroundStyle(HideTheme.muted)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .padding(.horizontal, 12)
                            .padding(.top, 8)
                        ForEach(checkoutEntries) { entry in
                            searchButton(entry)
                        }
                    }
                    if entries.isEmpty {
                        Text("No matching agents or workspaces")
                            .hideFont(size: 12)
                            .foregroundStyle(HideTheme.secondary)
                            .padding(28)
                    }
                }
                .padding(.horizontal, 14)
            }
        }
        .frame(width: 570, height: 430)
        .background(HideTheme.panel)
        .preferredColorScheme(.dark)
    }

    private func searchButton(_ entry: HideSearchEntry) -> some View {
        Button { route(entry) } label: {
            HStack(spacing: 10) {
                Image(systemName: entry.kind.systemImage)
                    .foregroundStyle(accent)
                    .frame(width: 18)
                VStack(alignment: .leading, spacing: 2) {
                    Text(entry.title)
                        .hideFont(size: 12, weight: .semibold)
                        .foregroundStyle(HideTheme.primary)
                    Text(entry.subtitle)
                        .hideFont(size: 10, design: .monospaced)
                        .foregroundStyle(HideTheme.secondary)
                        .lineLimit(1)
                }
                Spacer()
                Text("↵").foregroundStyle(HideTheme.muted)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 9)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
    }

    private func route(_ entry: HideSearchEntry) {
        switch entry.kind {
        case let .agent(agent): model.selectAgent(agent)
        case let .checkout(_, checkout): model.selectCheckout(checkout)
        }
        dismiss()
    }
}

struct HideSearchAgentGroup: Identifiable {
    let id: String
    let workspace: String
    let entries: [HideSearchEntry]
}

enum HideSearchPresentation {
    static func agentGroups(
        workspaces: [CoreWorkspaceSnapshot],
        agents: [SidebarAgent],
        query: String
    ) -> [HideSearchAgentGroup] {
        workspaces.compactMap { workspace in
            let paneIDs = Set(workspace.checkouts.flatMap(\.tabs).flatMap(\.panes).map(\.id))
            let entries = agents
                .filter { paneIDs.contains($0.paneID) }
                .map {
                    HideSearchEntry(
                        id: "agent-\($0.paneID)",
                        title: $0.summary,
                        subtitle: $0.paneID,
                        kind: .agent($0)
                    )
                }
            let filtered = HideSearchEntry.filtered(entries, query: query)
            return filtered.isEmpty
                ? nil
                : HideSearchAgentGroup(
                    id: workspace.id,
                    workspace: workspace.label,
                    entries: filtered
                )
        }
    }
}

struct HideSearchEntry: Identifiable {
    enum Kind {
        case agent(SidebarAgent)
        case checkout(CoreWorkspaceSnapshot, CoreCheckoutSnapshot)

        var systemImage: String {
            switch self {
            case .agent: "sparkles"
            case .checkout: "rectangle.stack"
            }
        }
    }

    enum Route: Equatable {
        case agent(paneID: String)
        case checkout(workspaceID: String, checkoutID: String)
    }

    let id: String
    let title: String
    let subtitle: String
    let kind: Kind

    var route: Route {
        switch kind {
        case let .agent(agent):
            .agent(paneID: agent.paneID)
        case let .checkout(workspace, checkout):
            .checkout(workspaceID: workspace.id, checkoutID: checkout.id)
        }
    }

    static func filtered(_ entries: [HideSearchEntry], query: String) -> [HideSearchEntry] {
        let normalized = query.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !normalized.isEmpty else { return entries }
        return entries.filter { entry in
            entry.title.lowercased().contains(normalized) || entry.subtitle.lowercased().contains(normalized)
        }
    }
}

struct HideSettingsView: View {
    @ObservedObject var model: ShellModel
    @Environment(\.dismiss) private var dismiss
    @State private var accentHex = "#B9FF66"
    @State private var fontSize = 13.0

    var body: some View {
        TabView {
            HideGeneralSettings(model: model)
                .tabItem { Label("General", systemImage: "slider.horizontal.3") }
            HideAppearanceSettings(model: model, accentHex: $accentHex, fontSize: $fontSize)
                .tabItem { Label("Appearance", systemImage: "paintbrush") }
            HideAgentSettings(model: model)
                .tabItem { Label("Agents", systemImage: "sparkles") }
            Form {
                PetSettingsSection(model: model)
            }
            .formStyle(.grouped)
            .scrollContentBackground(.hidden)
            .tabItem { Label("Pet", systemImage: "pawprint") }
            HideDeviceSettings(model: model)
                .tabItem { Label("Devices", systemImage: "externaldrive.connected.to.line.below") }
            AppSettingsView(model: model)
                .tabItem { Label("Shortcuts", systemImage: "command") }
        }
        .padding(20)
        .frame(width: 710, height: 520)
        .background(HideTheme.panel)
        .preferredColorScheme(.dark)
        .onAppear {
            accentHex = model.core.snapshot?.uiState.accentHex ?? "#B9FF66"
            fontSize = model.core.snapshot?.uiState.fontSize ?? 13
        }
    }
}

private struct HideGeneralSettings: View {
    @ObservedObject var model: ShellModel

    var body: some View {
        Form {
            Section("Hide") {
                LabeledContent("Bundle", value: "me.grab.hide")
                LabeledContent("State", value: model.herdrIsConnected ? "Connected" : "Waiting for Herdr")
                LabeledContent("State file", value: model.core.snapshot?.uiState.selectedPath == nil ? "Application Support / hide" : "Application Support / hide")
            }
            Section("Herdr runtime") {
                if let selection = model.core.runtimeSelection {
                    LabeledContent("Selected", value: selection.path)
                    LabeledContent("Version", value: selection.version)
                    LabeledContent("Source", value: selection.source)
                    LabeledContent("SHA-256", value: selection.sha256 ?? "not recorded")
                } else {
                    Text("No verified Herdr runtime is available for this launch.")
                        .foregroundStyle(HideTheme.warning)
                }
                if let guidance = model.core.runtimeSelection?.guidance {
                    Text(guidance)
                        .font(.caption)
                        .foregroundStyle(HideTheme.warning)
                }
                LabeledContent("Login PATH", value: HideRuntimeEnvironment.loginShellPath() ?? "unavailable")
                    .lineLimit(2)
            }
            Section("Authentication") {
                Label("Hide delegates authentication to Herdr and the selected agent CLI.", systemImage: "lock.shield")
                Text("No credential form, secret storage, token field, or passphrase handling is provided by Hide.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .scrollContentBackground(.hidden)
    }
}

private struct HideAppearanceSettings: View {
    @ObservedObject var model: ShellModel
    @Binding var accentHex: String
    @Binding var fontSize: Double
    @Environment(\.hideAccent) private var accent
    private let accents = ["#B9FF66", "#7DD3FC", "#C4B5FD", "#FDBA74"]

    var body: some View {
        Form {
            Section("Theme") {
                Text("Dark is the only product theme in this release. Accent changes remain restrained so status carries the color.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
                HStack(spacing: 10) {
                    ForEach(accents, id: \.self) { hex in
                        Button {
                            accentHex = hex
                            model.updatePreferences(accentHex: hex)
                        } label: {
                            Circle()
                                .fill(HideTheme.color(for: hex))
                                .frame(width: 23, height: 23)
                                .overlay {
                                    Circle().stroke(accentHex == hex ? accent : .clear, lineWidth: 2)
                                }
                        }
                        .buttonStyle(.plain)
                    }
                    Text(accentHex)
                        .hideFont(size: 11, design: .monospaced)
                        .foregroundStyle(.secondary)
                }
            }
            Section("Density") {
                HStack {
                    Text("Interface font")
                    Slider(value: $fontSize, in: 11 ... 17, step: 1) {
                        Text("Font size")
                    }
                    .onChange(of: fontSize) { _, value in
                        model.updatePreferences(fontSize: value)
                    }
                    Text("\(Int(fontSize)) pt")
                        .hideFont(size: 11, design: .monospaced)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .formStyle(.grouped)
        .scrollContentBackground(.hidden)
    }
}

private struct HideAgentSettings: View {
    @ObservedObject var model: ShellModel

    var body: some View {
        Form {
            Section("Installed CLIs") {
                HideCLIStatus(name: "claude")
                HideCLIStatus(name: "codex")
            }
            Section("Launch safety") {
                Text("Permission bypass is always off when a New Agent dialog opens and applies only to that one launch.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .formStyle(.grouped)
        .scrollContentBackground(.hidden)
    }
}

private struct HideCLIStatus: View {
    let name: String

    var body: some View {
        HStack {
            Image(systemName: AgentCLIAvailability.isUsable(name) ? "checkmark.circle.fill" : "exclamationmark.circle")
                .foregroundStyle(AgentCLIAvailability.isUsable(name) ? HideTheme.success : HideTheme.warning)
            Text(name)
                .hideFont(size: 12, weight: .semibold)
            Spacer()
            Text(AgentCLIAvailability.executable(for: name) ?? "not found on login PATH")
                .hideFont(size: 10, design: .monospaced)
                .foregroundStyle(.secondary)
                .lineLimit(1)
        }
    }
}

private struct HideDeviceSettings: View {
    @ObservedObject var model: ShellModel
    @State private var showAddDevice = false
    @Environment(\.hideAccent) private var accent

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Text("SSH targets")
                    .font(.headline)
                Spacer()
                Button("Add device") { showAddDevice = true }
            }
            Text("Hide stores only a label and SSH alias. Authentication stays in your SSH environment; no password or token is collected.")
                .font(.caption)
                .foregroundStyle(.secondary)
            if model.remote.phase != .idle {
                VStack(alignment: .leading, spacing: 7) {
                    HStack(spacing: 7) {
                        Image(systemName: remoteStatusSymbol)
                            .foregroundStyle(remoteStatusColor)
                        Text("Remote status")
                            .hideFont(size: 12, weight: .semibold)
                        Spacer()
                        Text(model.remote.checkedAt)
                            .hideFont(size: 10, design: .monospaced)
                            .foregroundStyle(.secondary)
                    }
                    Text(model.remote.message)
                        .hideFont(size: 11)
                        .foregroundStyle(.secondary)
                    if model.remote.phase == .failed || model.remote.phase == .unavailable || model.remote.phase == .stale {
                        Button("Retry") { model.retryRemote() }
                            .buttonStyle(.borderless)
                    }
                    ForEach(model.remote.workspaces) { workspace in
                        HStack {
                            Text(workspace.label)
                                .hideFont(size: 11, weight: .semibold)
                            Spacer()
                            Text("\(workspace.paneCount) panes")
                                .hideFont(size: 10, design: .monospaced)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
                .padding(10)
                .background(HideTheme.elevated, in: RoundedRectangle(cornerRadius: 8))
            }
            List {
                ForEach(model.devices) { device in
                    HStack(spacing: 9) {
                        Circle()
                            .fill(device.kind == "remote" ? HideTheme.success : accent)
                            .frame(width: 7, height: 7)
                        VStack(alignment: .leading, spacing: 2) {
                            Text(device.label)
                                .hideFont(size: 12, weight: .semibold)
                            Text(device.sshAlias ?? "This Mac")
                                .hideFont(size: 10, design: .monospaced)
                                .foregroundStyle(.secondary)
                        }
                        Spacer()
                        if device.kind == "remote" {
                            Button("Test") { model.testDevice(device) }
                                .buttonStyle(.borderless)
                            Button("Remove", role: .destructive) { model.removeDevice(device) }
                                .buttonStyle(.borderless)
                        }
                    }
                    .padding(.vertical, 3)
                }
            }
            .listStyle(.inset)
        }
        .sheet(isPresented: $showAddDevice) {
            AddDeviceSheet(model: model)
        }
    }

    private var remoteStatusSymbol: String {
        switch model.remote.phase {
        case .ready: "checkmark.circle.fill"
        case .loading: "arrow.triangle.2.circlepath"
        case .failed, .unavailable, .stale: "exclamationmark.triangle.fill"
        case .idle: "circle"
        }
    }

    private var remoteStatusColor: Color {
        switch model.remote.phase {
        case .ready: HideTheme.success
        case .failed, .unavailable, .stale: HideTheme.warning
        case .loading, .idle: accent
        }
    }
}

private struct AddDeviceSheet: View {
    @ObservedObject var model: ShellModel
    @Environment(\.dismiss) private var dismiss
    @State private var label = ""
    @State private var alias = ""

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            SheetHeader(title: "Add device", subtitle: "Use an existing SSH alias. Hide never asks for credentials.")
            Form {
                TextField("Label", text: $label)
                TextField("SSH alias", text: $alias)
                    .textFieldStyle(.roundedBorder)
            }
            .formStyle(.grouped)
            .scrollContentBackground(.hidden)
            HStack {
                Spacer()
                Button("Cancel") { dismiss() }
                Button("Add") {
                    model.addDevice(label: label, alias: alias)
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
                .disabled(label.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || alias.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
            .padding(.horizontal, 18)
            .padding(.bottom, 18)
        }
        .frame(width: 470, height: 290)
        .background(HideTheme.panel)
        .preferredColorScheme(.dark)
    }
}

private struct SheetHeader: View {
    let title: String
    let subtitle: String

    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            Text(title)
                .hideFont(size: 19, weight: .semibold)
                .foregroundStyle(HideTheme.primary)
            Text(subtitle)
                .hideFont(size: 11)
                .foregroundStyle(HideTheme.secondary)
        }
        .padding(.horizontal, 20)
        .padding(.top, 20)
        .padding(.bottom, 7)
    }
}
