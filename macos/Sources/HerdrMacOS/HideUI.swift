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
        HStack(spacing: 0) {
            HideSidebar()
                .frame(width: 292)
                .frame(maxHeight: .infinity, alignment: .topLeading)
            HideMainView()
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
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
        .accessibilityIdentifier("hide-shell")
    }
}

private struct HideSidebar: View {
    @EnvironmentObject private var model: ShellModel

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HideBrandHeader()
            VStack(spacing: 5) {
                HideActionButton(title: "New Workspace", systemImage: "plus.square", shortcut: "⌘N") {
                    model.openNewWorkspace()
                }
                HideActionButton(title: "New Agent", systemImage: "sparkles", shortcut: "⌘⇧N") {
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
                    HideSectionLabel(title: "WORKSPACES", count: model.workspaces.count)
                    if model.workspaces.isEmpty {
                        EmptySidebarRow(
                            systemImage: "square.stack.3d.up",
                            title: "No workspaces yet",
                            detail: "Register a folder to see its repo and checkouts here."
                        )
                    } else {
                        ForEach(model.workspaces) { workspace in
                            WorkspaceNavigatorRow(workspace: workspace)
                        }
                    }

                    HideSectionLabel(title: "AGENTS", count: model.agents.count)
                    if model.agents.isEmpty {
                        EmptySidebarRow(
                            systemImage: "person.2",
                            title: "No active agents",
                            detail: "Start Claude or Codex from a checkout."
                        )
                    } else {
                        ForEach(model.agents) { agent in
                            AgentNavigatorRow(agent: agent)
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
                Image(systemName: workspace.isGit ? "folder.badge.gearshape" : "folder")
                    .hideFont(size: 12, weight: .semibold)
                    .foregroundStyle(workspace.temporary ? HideTheme.warning : accent)
                    .frame(width: 16)
                Text(workspace.repoName)
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

            ForEach(workspace.checkouts) { checkout in
                CheckoutNavigatorRow(
                    workspace: workspace,
                    checkout: checkout,
                    isFocused: model.focusedCheckout?.id == checkout.id
                )
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
                    Text(checkout.branch.map { "\($0) · \(checkout.tabs.count) tabs" } ?? "Folder · \(checkout.tabs.count) tabs")
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
        .contextMenu {
            Button("Start agent here") { model.openNewAgent(checkoutID: checkout.id) }
        }
    }
}

private struct AgentNavigatorRow: View {
    @EnvironmentObject private var model: ShellModel
    @Environment(\.hideAccent) private var accent
    let agent: SidebarAgent

    private var stateColor: Color {
        switch agent.state {
        case "working": accent
        case "question", "approval": HideTheme.warning
        case "error": HideTheme.danger
        case "done", "unseen_completion": HideTheme.success
        default: HideTheme.secondary
        }
    }

    var body: some View {
        Button { model.selectAgent(agent) } label: {
            HStack(alignment: .top, spacing: 8) {
                Text(agent.agentKind == "claude" ? "C" : agent.agentKind == "codex" ? "O" : "?" )
                    .hideFont(size: 10, weight: .bold, design: .rounded)
                    .foregroundStyle(stateColor)
                    .frame(width: 19, height: 19)
                    .background(stateColor.opacity(0.13), in: RoundedRectangle(cornerRadius: 5))
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 5) {
                        Text(agent.workspaceLabel)
                            .hideFont(size: 11, weight: .semibold)
                            .foregroundStyle(HideTheme.primary)
                            .lineLimit(1)
                        Spacer(minLength: 0)
                        Text(agent.elapsed)
                            .hideFont(size: 9, design: .monospaced)
                            .foregroundStyle(HideTheme.muted)
                    }
                    Text(agent.summary)
                        .hideFont(size: 10)
                        .foregroundStyle(HideTheme.secondary)
                        .lineLimit(2)
                    Text(agent.state.replacingOccurrences(of: "_", with: " "))
                        .hideFont(size: 9, weight: .medium)
                        .foregroundStyle(stateColor)
                }
            }
            .padding(.horizontal, 15)
            .padding(.vertical, 7)
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
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
        // The shell-level identifier is inherited by direct SwiftUI controls
        // on macOS. A per-device identifier keeps accessibility automation
        // from resolving every device button to the first `hide-shell` match.
        .accessibilityIdentifier("hide-device-\(device.id)")
        .accessibilityValue(isSelected ? "Selected" : "Not selected")
    }
}

private struct HideMainView: View {
    @EnvironmentObject private var model: ShellModel

    var body: some View {
        VStack(spacing: 0) {
            HideToolbar()
            Rectangle()
                .fill(HideTheme.divider)
                .frame(height: 1)
            HSplitView {
                HideTerminalSurface()
                    .frame(minWidth: 540, maxWidth: .infinity, maxHeight: .infinity)
                WorkbenchPanel()
                    .frame(minWidth: 285, idealWidth: 355, maxWidth: 430, maxHeight: .infinity)
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

private struct HideToolbar: View {
    @EnvironmentObject private var model: ShellModel
    @Environment(\.hideAccent) private var accent

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 10) {
                Image(systemName: "rectangle.3.group")
                    .foregroundStyle(accent)
                VStack(alignment: .leading, spacing: 1) {
                    Text(model.focusedWorkspace?.repoName ?? (model.isRemoteContext ? model.remote.targetLabel : "No workspace"))
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
                Button {
                    model.openNewAgent()
                } label: {
                    Label("New Agent", systemImage: "sparkles")
                }
                .buttonStyle(HideToolbarButtonStyle(isProminent: true))
                .disabled(model.focusedCheckout == nil)
                Button {
                    model.addTab()
                } label: {
                    Image(systemName: "plus")
                }
                .buttonStyle(HideToolbarButtonStyle(isProminent: false))
                .help("New tab")
            }
            .padding(.horizontal, 18)
            .frame(height: 54)

            if !model.focusedTabs.isEmpty {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 3) {
                        ForEach(model.focusedTabs, id: \.stableID) { tab in
                            Button {
                                guard let tabID = tab.id,
                                      let workspace = model.focusedWorkspace,
                                      let checkout = model.focusedCheckout
                                else { return }
                                if model.isRemoteContext {
                                    model.remote.focus(
                                        workspaceID: workspace.id,
                                        checkoutID: checkout.id,
                                        paneID: tab.panes.first?.id
                                    )
                                } else {
                                    model.core.focusTab(
                                        workspaceID: workspace.id,
                                        checkoutID: checkout.id,
                                        tabID: tabID
                                    )
                                }
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
            if model.isRemoteContext {
                if panes.isEmpty {
                    HideEmptyCheckoutState()
                } else if let workspace = model.focusedWorkspace,
                          let checkout = model.focusedCheckout {
                    HideTerminalGrid(
                        items: PaneGridPresentation.uniformItems(
                            paneIDs: panes.map(\.id),
                            focusedPaneID: model.focusedPaneID
                        )
                    ) { item in
                        if let pane = panes.first(where: { $0.id == item.paneID }) {
                            RemoteTerminalCell(
                                pane: pane,
                                model: model,
                                workspaceID: workspace.id,
                                checkoutID: checkout.id
                            )
                        } else {
                            MissingTerminalPaneCell(paneID: item.paneID)
                        }
                    }
                }
            } else if let layout = model.focusedPaneLayout {
                PaneLayoutCanvas(layout: layout, bridge: model.core)
            } else if panes.isEmpty {
                HideEmptyCheckoutState()
            } else {
                HideTerminalGrid(
                    items: PaneGridPresentation.uniformItems(
                        paneIDs: panes.map(\.id),
                        focusedPaneID: model.core.snapshot?.terminal.paneID
                    )
                ) { item in
                    PaneTerminalCell(
                        paneID: item.paneID,
                        focusedPaneID: model.core.snapshot?.terminal.paneID,
                        bridge: model.core
                    )
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .background(HideTheme.background)
        .accessibilityIdentifier("hide-terminal-surface")
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

private struct RemoteTerminalCell: View {
    let pane: CorePaneSnapshot
    @ObservedObject var model: ShellModel
    let workspaceID: String
    let checkoutID: String

    var body: some View {
        HideTerminalPaneCard(
            paneID: pane.id,
            title: pane.label.isEmpty ? pane.id : pane.label,
            cwd: pane.cwd,
            status: pane.state,
            isFocused: model.focusedPaneID == pane.id,
            onFocus: {
                model.remote.focus(
                    workspaceID: workspaceID,
                    checkoutID: checkoutID,
                    paneID: pane.id
                )
            }
        ) {
            RemoteTerminalHost(
                remote: model.remote,
                sshAlias: model.remote.sshAlias,
                paneID: pane.id
            )
            .accessibilityLabel("Remote SwiftTerm terminal for \(pane.id)")
        }
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
                Text(model.remote.attachError ?? model.remote.message)
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
                ? (model.remote.attachError ?? model.remote.message)
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
    @State private var path = ""
    @State private var label = ""
    @State private var initializeGit = true

    private var selectedURL: URL? {
        guard !path.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return nil }
        return URL(fileURLWithPath: path, isDirectory: true)
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
                    HStack {
                        TextField("/path/to/project", text: $path)
                            .textFieldStyle(.roundedBorder)
                        Button("Choose…", action: chooseFolder)
                    }
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
                Button("Cancel") { dismiss() }
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
    }

    private func chooseFolder() {
        let panel = NSOpenPanel()
        panel.canChooseFiles = false
        panel.canChooseDirectories = true
        panel.allowsMultipleSelection = false
        if panel.runModal() == .OK, let url = panel.url {
            path = url.path
            if label.isEmpty { label = url.lastPathComponent }
        }
    }
}

private struct NewAgentSheet: View {
    @EnvironmentObject private var model: ShellModel
    @Environment(\.dismiss) private var dismiss
    @State private var selectedKind = "claude"
    @State private var selectedCheckoutID = ""
    @State private var selectedDeviceID = "local"
    @State private var bypass = false

    private var checkouts: [(workspace: CoreWorkspaceSnapshot, checkout: CoreCheckoutSnapshot)] {
        model.workspaces.flatMap { workspace in
            workspace.checkouts.map { (workspace: workspace, checkout: $0) }
        }
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            SheetHeader(title: "New Agent", subtitle: "Run the selected CLI inside a checkout through Herdr.")
            Form {
                Section("Agent") {
                    HStack(spacing: 9) {
                        AgentChoiceTile(kind: "claude", selected: selectedKind == "claude") { selectedKind = "claude" }
                        AgentChoiceTile(kind: "codex", selected: selectedKind == "codex") { selectedKind = "codex" }
                    }
                    if !AgentCLIAvailability.isUsable(selectedKind) {
                        VStack(alignment: .leading, spacing: 5) {
                            Label("\(selectedKind) is not on the login-shell PATH. Install it, then reopen this dialog.", systemImage: "exclamationmark.triangle")
                                .font(.caption)
                                .foregroundStyle(HideTheme.warning)
                            Link("Install \(selectedKind)", destination: selectedKind == "claude"
                                ? URL(string: "https://docs.anthropic.com/en/docs/claude-code/overview")!
                                : URL(string: "https://developers.openai.com/codex/")!)
                                .font(.caption)
                        }
                    }
                }
                Section("Context") {
                    Picker("Device", selection: $selectedDeviceID) {
                        ForEach(model.devices) { device in
                            Text(device.label).tag(device.id)
                        }
                    }
                    Picker("Checkout", selection: $selectedCheckoutID) {
                        Text("Choose a checkout").tag("")
                        ForEach(checkouts, id: \.checkout.id) { item in
                            Text("\(item.workspace.repoName) / \(item.checkout.label)").tag(item.checkout.id)
                        }
                    }
                }
                Section("Safety") {
                    Toggle("Pass the CLI bypass flag", isOn: $bypass)
                    if bypass {
                        Label("This passes a provider-specific bypass flag to \(selectedKind). Review its consequences before starting.", systemImage: "exclamationmark.triangle.fill")
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
                    model.selectedAgentKind = selectedKind
                    model.selectedAgentCheckoutID = selectedCheckoutID.isEmpty ? nil : selectedCheckoutID
                    model.selectedAgentDeviceID = selectedDeviceID
                    model.agentBypassWarnings = bypass
                    model.startAgent()
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
                .disabled(selectedCheckoutID.isEmpty || !AgentCLIAvailability.isUsable(selectedKind))
            }
            .padding(18)
        }
        .frame(width: 590, height: 500)
        .background(HideTheme.panel)
        .preferredColorScheme(.dark)
        .onAppear {
            selectedKind = model.selectedAgentKind
            selectedCheckoutID = model.selectedAgentCheckoutID ?? model.focusedCheckout?.id ?? ""
            selectedDeviceID = model.selectedAgentDeviceID
            bypass = model.agentBypassWarnings
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
                Text(kind == "claude" ? "C" : "O")
                    .hideFont(size: 19, weight: .bold, design: .rounded)
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

    private var entries: [HideSearchEntry] {
        var result = model.agents.map {
            HideSearchEntry(id: "agent-\($0.id)", title: $0.summary, subtitle: "Agent · \($0.workspaceLabel)", kind: .agent($0))
        }
        result += model.workspaces.flatMap { workspace in
            workspace.checkouts.map { checkout in
                HideSearchEntry(
                    id: "checkout-\(checkout.id)",
                    title: "\(workspace.repoName) / \(checkout.label)",
                    subtitle: checkout.path,
                    kind: .checkout(workspace, checkout)
                )
            }
        }
        return HideSearchEntry.filtered(result, query: query)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 9) {
                Image(systemName: "magnifyingglass")
                    .foregroundStyle(accent)
                TextField("Search agents and spaces", text: $query)
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
                    ForEach(entries) { entry in
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
                                Text("↵")
                                    .foregroundStyle(HideTheme.muted)
                            }
                            .padding(.horizontal, 12)
                            .padding(.vertical, 9)
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                    }
                    if entries.isEmpty {
                        Text("No matching agents or spaces")
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

    private func route(_ entry: HideSearchEntry) {
        switch entry.kind {
        case let .agent(agent): model.selectAgent(agent)
        case let .checkout(_, checkout): model.selectCheckout(checkout)
        }
        dismiss()
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
    @State private var bypassWarnings = false

    var body: some View {
        TabView {
            HideGeneralSettings(model: model)
                .tabItem { Label("General", systemImage: "slider.horizontal.3") }
            HideAppearanceSettings(model: model, accentHex: $accentHex, fontSize: $fontSize)
                .tabItem { Label("Appearance", systemImage: "paintbrush") }
            HideAgentSettings(model: model, bypassWarnings: $bypassWarnings)
                .tabItem { Label("Agents", systemImage: "sparkles") }
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
            bypassWarnings = model.core.snapshot?.uiState.bypassWarnings ?? false
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
    @Binding var bypassWarnings: Bool

    var body: some View {
        Form {
            Section("Installed CLIs") {
                HideCLIStatus(name: "claude")
                HideCLIStatus(name: "codex")
            }
            Section("Defaults") {
                Toggle("Remember bypass choice for new agent dialogs", isOn: $bypassWarnings)
                    .onChange(of: bypassWarnings) { _, value in
                        model.updatePreferences(bypassWarnings: value)
                    }
                Text("This preference never suppresses the per-agent warning. Hide does not impose a CLI version lower bound.")
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
